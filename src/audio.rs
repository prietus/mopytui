// The spectrum panel gets real audio from mopidy-goodies' WebSocket feed when
// the server offers one, or from `audio_udp` / `audio_tcp` / `audio_pipe` in
// config.toml. Without any of them it falls back to a synthesised spectrum.
#![allow(dead_code)]

//! Audio reader for the live spectrum visualizer.
//!
//! Mopidy doesn't expose audio samples to JSON-RPC clients. The usual source
//! is mopidy-goodies' `WS /goodies/audio/visualizer` feed (raw S16LE stereo
//! PCM at 44.1 kHz, one binary message per chunk), which is picked up
//! automatically when `/goodies/_health` reports `"visualizer": true`. The
//! PCM is stashed in a ring buffer for the UI to FFT.
//!
//! Without goodies, Mopidy's GStreamer `[audio] output` can `tee` the stream
//! straight to us (UDP, TCP or a named pipe). `mopidy.conf` snippet for UDP:
//!
//! ```text
//! [audio]
//! output = tee name=t allow-not-linked=true
//!   t. ! queue leaky=2 max-size-buffers=200 ! autoaudiosink
//!   t. ! queue leaky=2 max-size-buffers=200
//!      ! audioresample ! audioconvert
//!      ! audio/x-raw,format=S16LE,rate=44100,channels=2
//!      ! udpsink host=<mopytui-host> port=5555 sync=false
//! ```
//!
//! Then in `~/.config/mopytui/config.toml`:
//!
//! ```text
//! audio_udp = "0.0.0.0:5555"
//! ```
//!
//! UDP is preferred over TCP because `tcpserversink` does async preroll and
//! computes durations that fail (`gst_util_uint64_scale: denom != 0`) when
//! no client is connected, which can stall the whole pipeline. `udpsink` is
//! pure fire-and-forget: drops packets if no one listens, no preroll.
//!
//! `leaky=2` on the visualizer branch means buffers are dropped (instead of
//! blocking the pipeline) if we read slowly or are absent — so the DAC branch
//! stays bit-perfect.  `allow-not-linked=true` lets Mopidy start before
//! mopytui is connected.

use std::collections::VecDeque;
use std::io::Read;
use std::net::{TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const SAMPLE_RATE: u32 = 44_100;
/// Ring buffer length: one second of mono samples. An FFT window is 2048
/// samples; the rest is room for jitter and for `visualizer_delay_ms`.
const RING_CAP: usize = 44_100;
/// The UI is considered to be showing the visualizer if it asked for samples
/// this recently. Sources that cost something (the WebSocket feed) only run
/// while that holds, and the event loop only redraws fast meanwhile.
const WANTED_FOR: Duration = Duration::from_secs(1);
/// Bytes per stereo frame (s16le × 2 channels).
const STEREO_FRAME_BYTES: usize = 4;

pub enum AudioSource {
    /// Named-pipe / regular file. mopytui only reads; the producer
    /// (`filesink location=...`) must create or open the path itself.
    Fifo(PathBuf),
    /// `host:port` of a GStreamer `tcpserversink`. We connect as client.
    Tcp(String),
    /// `host:port` to bind a UDP socket on. GStreamer `udpsink` fires PCM
    /// packets at us. Preferred over TCP — no preroll, no stalls.
    Udp(String),
}

pub struct AudioReader {
    samples: Mutex<VecDeque<f32>>,
    last_data_at: Mutex<Option<Instant>>,
    has_writer: AtomicBool,
    /// When the UI last asked for samples (see [`WANTED_FOR`]).
    wanted_at: Mutex<Option<Instant>>,
    /// How far behind the newest sample the UI reads, to line the picture up
    /// with what the DAC is playing (`visualizer_delay_ms`).
    delay_samples: AtomicUsize,
    /// Display smoothing state: per-band levels and the running peak.
    smooth: Mutex<(Vec<f32>, f32)>,
    /// The same audio as `samples`, left and right kept apart for the VU meter.
    stereo: Mutex<VecDeque<[f32; 2]>>,
    vu: Mutex<VuState>,
}

/// Bottom of the VU scale, in dBFS. The top is 0 dBFS (digital full scale).
pub const VU_MIN_DB: f32 = -48.0;

/// What the VU meter shows for [left, right], in dBFS.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vu {
    /// RMS level with meter ballistics (instant rise, steady fall).
    pub level: [f32; 2],
    /// Sample peak, held briefly and then released.
    pub peak: [f32; 2],
}

struct VuState {
    vu: Vu,
    /// Seconds each peak marker has been held.
    held: [f32; 2],
    last: Option<Instant>,
}

impl Default for VuState {
    fn default() -> Self {
        Self {
            vu: Vu { level: [VU_MIN_DB; 2], peak: [VU_MIN_DB; 2] },
            held: [0.0; 2],
            last: None,
        }
    }
}

/// Advance the meter by `dt` seconds given the newly measured RMS and sample
/// peak (dBFS) of each channel. Levels jump up at once and fall at a steady
/// rate; peak markers hold for a moment, then drop.
fn vu_step(state: &mut VuState, rms_db: [f32; 2], peak_db: [f32; 2], dt: f32) -> Vu {
    const FALL_DB_PER_S: f32 = 30.0;
    const PEAK_HOLD_S: f32 = 0.8;
    const PEAK_FALL_DB_PER_S: f32 = 24.0;
    for ch in 0..2 {
        let level = &mut state.vu.level[ch];
        *level = rms_db[ch].max(*level - FALL_DB_PER_S * dt).clamp(VU_MIN_DB, 0.0);
        let peak = &mut state.vu.peak[ch];
        if peak_db[ch] >= *peak {
            *peak = peak_db[ch];
            state.held[ch] = 0.0;
        } else {
            state.held[ch] += dt;
            if state.held[ch] > PEAK_HOLD_S {
                *peak -= PEAK_FALL_DB_PER_S * dt;
            }
        }
        *peak = peak.max(state.vu.level[ch]).clamp(VU_MIN_DB, 0.0);
    }
    state.vu
}

fn to_db(amplitude: f32) -> f32 {
    if amplitude <= 0.0 { VU_MIN_DB } else { (20.0 * amplitude.log10()).max(VU_MIN_DB) }
}

impl AudioReader {
    fn new() -> Self {
        Self {
            samples: Mutex::new(VecDeque::with_capacity(RING_CAP)),
            last_data_at: Mutex::new(None),
            has_writer: AtomicBool::new(false),
            wanted_at: Mutex::new(None),
            delay_samples: AtomicUsize::new(0),
            smooth: Mutex::new((Vec::new(), 0.0)),
            stereo: Mutex::new(VecDeque::with_capacity(RING_CAP)),
            vu: Mutex::new(VuState::default()),
        }
    }

    /// Current VU reading from the last ~50 ms of audio (behind the live edge
    /// by the configured delay). `None` when there is no fresh audio.
    pub fn vu(&self) -> Option<Vu> {
        const WINDOW: usize = SAMPLE_RATE as usize / 20;
        self.mark_wanted();
        if !self.is_fresh() {
            *self.vu.lock().unwrap() = VuState::default();
            return None;
        }
        let delay = self.delay_samples.load(Ordering::Relaxed);
        let (mut sum, mut peak) = ([0.0_f32; 2], [0.0_f32; 2]);
        {
            let buf = self.stereo.lock().unwrap();
            if buf.len() < WINDOW + delay {
                return None;
            }
            for frame in buf.iter().skip(buf.len() - delay - WINDOW).take(WINDOW) {
                for ch in 0..2 {
                    sum[ch] += frame[ch] * frame[ch];
                    peak[ch] = peak[ch].max(frame[ch].abs());
                }
            }
        }
        let rms_db = [to_db((sum[0] / WINDOW as f32).sqrt()), to_db((sum[1] / WINDOW as f32).sqrt())];
        let peak_db = [to_db(peak[0]), to_db(peak[1])];
        let mut state = self.vu.lock().unwrap();
        let now = Instant::now();
        let dt = state.last.map_or(0.0, |t| now.duration_since(t).as_secs_f32()).min(0.5);
        state.last = Some(now);
        Some(vu_step(&mut state, rms_db, peak_db, dt))
    }

    /// Read `ms` behind the live edge (clamped so an FFT window still fits).
    pub fn set_delay_ms(&self, ms: u32) {
        let samples = (ms as usize * SAMPLE_RATE as usize / 1000).min(RING_CAP - 4096);
        self.delay_samples.store(samples, Ordering::Relaxed);
    }

    /// The UI is drawing the visualizer right now.
    pub fn mark_wanted(&self) {
        *self.wanted_at.lock().unwrap() = Some(Instant::now());
    }

    pub fn is_wanted(&self) -> bool {
        self.wanted_at.lock().unwrap().is_some_and(|t| t.elapsed() < WANTED_FOR)
    }

    /// Data arrived within the last 250 ms.
    pub fn is_fresh(&self) -> bool {
        self.last_data_at.lock().unwrap().is_some_and(|t| t.elapsed() < Duration::from_millis(250))
    }

    /// Append raw S16LE stereo frames, downmixed to mono. `bytes` must be
    /// frame-aligned (a trailing partial frame is ignored).
    fn push_pcm(&self, bytes: &[u8]) {
        let mut samples = self.samples.lock().unwrap();
        let mut stereo = self.stereo.lock().unwrap();
        for frame in bytes.chunks_exact(STEREO_FRAME_BYTES) {
            let l = i16::from_le_bytes([frame[0], frame[1]]) as f32 / 32768.0;
            let r = i16::from_le_bytes([frame[2], frame[3]]) as f32 / 32768.0;
            if samples.len() >= RING_CAP {
                samples.pop_front();
                stereo.pop_front();
            }
            samples.push_back(0.5 * (l + r));
            stereo.push_back([l, r]);
        }
        drop(stereo);
        drop(samples);
        *self.last_data_at.lock().unwrap() = Some(Instant::now());
    }

    /// Snapshot `n` samples into `out`, ending `delay_samples` before the
    /// newest one. Returns `false` if not enough samples are buffered yet, or
    /// if no data has arrived recently (Mopidy paused or source disconnected).
    pub fn copy_recent(&self, out: &mut Vec<f32>, n: usize) -> bool {
        self.mark_wanted();
        if !self.is_fresh() {
            return false;
        }
        let delay = self.delay_samples.load(Ordering::Relaxed);
        let buf = self.samples.lock().unwrap();
        if buf.len() < n + delay {
            return false;
        }
        out.clear();
        out.extend(buf.iter().skip(buf.len() - delay - n).take(n).copied());
        true
    }

    pub fn sample_rate(&self) -> u32 { SAMPLE_RATE }
    pub fn is_live(&self) -> bool { self.has_writer.load(Ordering::Relaxed) }
}

/// Spawn a dedicated OS thread that opens the source and feeds the ring
/// buffer. Both transports block on `open()`/`connect()` until a producer is
/// available, so this runs off the tokio executor.
pub fn spawn_audio_reader(source: AudioSource) -> Arc<AudioReader> {
    let reader = Arc::new(AudioReader::new());
    let r = reader.clone();
    let name = match &source {
        AudioSource::Fifo(_) => "audio-fifo",
        AudioSource::Tcp(_) => "audio-tcp",
        AudioSource::Udp(_) => "audio-udp",
    };
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || match source {
            AudioSource::Fifo(p) => fifo_loop(r, p),
            AudioSource::Tcp(a) => tcp_loop(r, a),
            AudioSource::Udp(a) => udp_loop(r, a),
        })
        .expect("spawn audio thread");
    reader
}

fn fifo_loop(reader: Arc<AudioReader>, path: PathBuf) {
    loop {
        let file = match std::fs::OpenOptions::new().read(true).open(&path) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("audio fifo {}: {}", path.display(), e);
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
        };
        tracing::info!("audio fifo opened: {}", path.display());
        reader.has_writer.store(true, Ordering::Relaxed);
        read_until_eof(file, &reader);
        reader.has_writer.store(false, Ordering::Relaxed);
        tracing::info!("audio fifo writer disconnected — reopening");
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn udp_loop(reader: Arc<AudioReader>, addr: String) {
    loop {
        let sock = match UdpSocket::bind(&addr) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("audio udp bind {}: {}", addr, e);
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
        };
        // Read timeout so the `has_writer` flag flips back to false when
        // mopidy stops sending (paused / stopped), which makes the UI swap
        // back to "no live audio" instead of showing a frozen spectrum.
        let _ = sock.set_read_timeout(Some(Duration::from_millis(300)));
        tracing::info!("audio udp listening: {}", addr);
        let mut buf = [0u8; 4096];
        loop {
            match sock.recv(&mut buf) {
                Ok(n) => {
                    let usable = n - (n % STEREO_FRAME_BYTES);
                    if usable == 0 { continue; }
                    reader.push_pcm(&buf[..usable]);
                    reader.has_writer.store(true, Ordering::Relaxed);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    // No packet in the window — assume mopidy is paused or
                    // not streaming. UI will switch to "no live audio".
                    reader.has_writer.store(false, Ordering::Relaxed);
                    continue;
                }
                Err(e) => {
                    tracing::warn!("audio udp recv: {e}");
                    break;
                }
            }
        }
        reader.has_writer.store(false, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn tcp_loop(reader: Arc<AudioReader>, addr: String) {
    loop {
        let stream = match TcpStream::connect(&addr) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("audio tcp {}: {}", addr, e);
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
        };
        // Small read timeout so a dead/stalled server doesn't park us forever
        // and we cycle back to reconnect.
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        tracing::info!("audio tcp connected: {}", addr);
        reader.has_writer.store(true, Ordering::Relaxed);
        read_until_eof(stream, &reader);
        reader.has_writer.store(false, Ordering::Relaxed);
        tracing::info!("audio tcp disconnected — reconnecting");
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Feed the ring buffer from mopidy-goodies' WebSocket visualizer feed
/// (`ws://host:port/goodies/audio/visualizer`). Runs as a tokio task that is
/// only connected while the UI is drawing the visualizer — the feed is ~176
/// kB/s of PCM, not worth pulling for a panel nobody is looking at — and
/// reconnects on its own after errors or a Mopidy restart.
pub fn spawn_ws_reader(url: String) -> Arc<AudioReader> {
    let reader = Arc::new(AudioReader::new());
    let r = reader.clone();
    tokio::spawn(async move { ws_loop(r, url).await });
    reader
}

async fn ws_loop(reader: Arc<AudioReader>, url: String) {
    use futures::StreamExt;
    use tokio_tungstenite::tungstenite::Message;
    loop {
        if !reader.is_wanted() {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        }
        let conn = tokio::time::timeout(
            Duration::from_secs(5),
            tokio_tungstenite::connect_async(url.as_str()),
        )
        .await;
        let mut ws = match conn {
            Ok(Ok((ws, _))) => ws,
            Ok(Err(e)) => {
                tracing::debug!("visualizer ws {url}: {e}");
                tokio::time::sleep(Duration::from_secs(3)).await;
                continue;
            }
            Err(_) => {
                tracing::debug!("visualizer ws {url}: connect timeout");
                tokio::time::sleep(Duration::from_secs(3)).await;
                continue;
            }
        };
        tracing::info!("visualizer ws connected: {url}");
        reader.has_writer.store(true, Ordering::Relaxed);
        // Chunks aren't guaranteed to end on a frame boundary: carry the
        // remainder over so the channels never swap or go out of phase.
        let mut carry: Vec<u8> = Vec::new();
        while reader.is_wanted() {
            // Wake up regularly even when paused (no messages) to notice the
            // UI moved away.
            match tokio::time::timeout(Duration::from_millis(500), ws.next()).await {
                Err(_) => continue,
                Ok(Some(Ok(Message::Binary(data)))) => {
                    carry.extend_from_slice(&data);
                    let usable = carry.len() - carry.len() % STEREO_FRAME_BYTES;
                    reader.push_pcm(&carry[..usable]);
                    carry.drain(..usable);
                }
                Ok(Some(Ok(Message::Close(_)))) | Ok(None) => break,
                Ok(Some(Ok(_))) => {}
                Ok(Some(Err(e))) => {
                    tracing::debug!("visualizer ws read: {e}");
                    break;
                }
            }
        }
        reader.has_writer.store(false, Ordering::Relaxed);
        let _ = ws.close(None).await;
        tracing::info!("visualizer ws closed");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

fn read_until_eof<R: Read>(mut src: R, reader: &Arc<AudioReader>) {
    // Read in chunks aligned to stereo frames.
    let mut buf = [0u8; 4096];
    loop {
        match src.read(&mut buf) {
            Ok(0) => return, // writer closed
            Ok(n) => {
                let usable = n - (n % STEREO_FRAME_BYTES);
                if usable == 0 { continue; }
                reader.push_pcm(&buf[..usable]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                // Read timeout fired but the connection is alive — keep
                // looping; the reader UI will just see no fresh data.
                continue;
            }
            Err(e) => {
                tracing::warn!("audio source read: {e}");
                return;
            }
        }
    }
}

/// Real FFT spectrum from the latest samples in the ring buffer.
/// Returns `bands` magnitudes in [0.0, 1.0].
pub fn compute_fft_bands(reader: &AudioReader, bands: usize) -> Option<Vec<f32>> {
    use rustfft::FftPlanner;
    use rustfft::num_complex::Complex;

    const N: usize = 2048;
    let mut samples: Vec<f32> = Vec::with_capacity(N);
    if !reader.copy_recent(&mut samples, N) {
        return None;
    }
    let sr = reader.sample_rate() as f32;

    // Hann window to reduce spectral leakage.
    let mut buf: Vec<Complex<f32>> = samples
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let w = 0.5
                - 0.5
                    * (2.0 * std::f32::consts::PI * i as f32 / (N - 1) as f32).cos();
            Complex::new(s * w, 0.0)
        })
        .collect();

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(N);
    fft.process(&mut buf);

    // Magnitude of the first N/2 bins. Each bin spans `sr / N` Hz.
    let mag: Vec<f32> = buf[..N / 2].iter().map(|c| c.norm()).collect();

    // Group bins into log-spaced bands (30 Hz .. 16 kHz).
    let min_freq = 30.0f32;
    let max_freq = (sr * 0.5).min(16_000.0);
    let mut out = Vec::with_capacity(bands);
    for b in 0..bands {
        let t0 = b as f32 / bands as f32;
        let t1 = (b + 1) as f32 / bands as f32;
        let f0 = min_freq * (max_freq / min_freq).powf(t0);
        let f1 = min_freq * (max_freq / min_freq).powf(t1);
        let lo = (f0 / sr * N as f32) as usize;
        let hi = (f1 / sr * N as f32) as usize;
        let lo = lo.min(N / 2 - 1);
        let hi = hi.max(lo + 1).min(N / 2);
        let sum: f32 = mag[lo..hi].iter().sum();
        out.push(sum / (hi - lo) as f32);
    }

    // Per-band log compression.
    for v in out.iter_mut() {
        *v = (1.0 + *v).ln();
    }
    let mut state = reader.smooth.lock().unwrap();
    smooth_bands(&mut out, &mut state);
    Some(out)
}

/// Normalise `bands` to `[0, 1]` for display, in place.
///
/// The scale follows a running peak that decays slowly instead of each
/// frame's own maximum, so quiet passages look quiet rather than being blown
/// up to full height. Bars rise at once and fall gradually, which reads as
/// "on the beat" at 25 fps without flickering.
fn smooth_bands(bands: &mut [f32], state: &mut (Vec<f32>, f32)) {
    const PEAK_DECAY: f32 = 0.995;
    const PEAK_FLOOR: f32 = 0.5;
    const FALL: f32 = 0.80;
    let (levels, peak) = state;
    if levels.len() != bands.len() {
        *levels = vec![0.0; bands.len()];
    }
    let frame_max = bands.iter().fold(0.0_f32, |a, &b| a.max(b));
    *peak = frame_max.max(*peak * PEAK_DECAY).max(PEAK_FLOOR);
    for (v, level) in bands.iter_mut().zip(levels.iter_mut()) {
        let now = (*v / *peak).clamp(0.0, 1.0);
        *level = now.max(*level * FALL);
        *v = *level;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(pairs: &[(i16, i16)]) -> Vec<u8> {
        pairs.iter().flat_map(|(l, r)| [l.to_le_bytes(), r.to_le_bytes()].concat()).collect()
    }

    #[test]
    fn push_pcm_downmixes_and_caps_the_ring() {
        let r = AudioReader::new();
        r.push_pcm(&frames(&[(16384, 16384), (32767, -32767), (0, 0)]));
        let buf: Vec<f32> = r.samples.lock().unwrap().iter().copied().collect();
        assert_eq!(buf, vec![0.5, 0.0, 0.0]);
        // A trailing partial frame is ignored; the ring never outgrows its cap.
        r.push_pcm(&[1, 2, 3]);
        assert_eq!(r.samples.lock().unwrap().len(), 3);
        r.push_pcm(&vec![0u8; (RING_CAP + 10) * STEREO_FRAME_BYTES]);
        assert_eq!(r.samples.lock().unwrap().len(), RING_CAP);
    }

    #[test]
    fn copy_recent_reads_behind_the_live_edge_by_the_delay() {
        let r = AudioReader::new();
        let mut out = Vec::new();
        assert!(!r.copy_recent(&mut out, 2), "no data yet");
        assert!(r.is_wanted(), "asking for samples marks the visualizer as shown");
        // Samples 1..=10 (scaled); the newest is 10.
        let pcm = frames(&(1..=10).map(|i| (i * 3276, i * 3276)).collect::<Vec<_>>());
        r.push_pcm(&pcm);
        assert!(r.copy_recent(&mut out, 2));
        assert!((out[1] - 10.0 * 3276.0 / 32768.0).abs() < 1e-6);
        r.delay_samples.store(3, Ordering::Relaxed);
        assert!(r.copy_recent(&mut out, 2));
        assert!((out[1] - 7.0 * 3276.0 / 32768.0).abs() < 1e-6, "ends 3 samples earlier");
        // Not enough history for window + delay.
        r.delay_samples.store(9, Ordering::Relaxed);
        assert!(!r.copy_recent(&mut out, 2));
    }

    #[test]
    fn delay_is_clamped_so_a_window_still_fits() {
        let r = AudioReader::new();
        r.set_delay_ms(150);
        assert_eq!(r.delay_samples.load(Ordering::Relaxed), 6615);
        r.set_delay_ms(60_000);
        assert!(r.delay_samples.load(Ordering::Relaxed) + 4096 <= RING_CAP);
    }

    /// Manual check against a real server that is playing something:
    /// `MOPYTUI_WS_URL=ws://host:6680/goodies/audio/visualizer cargo test live_ws -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_ws_feed_fills_the_ring_only_while_wanted() {
        let url = std::env::var("MOPYTUI_WS_URL").expect("set MOPYTUI_WS_URL");
        let r = spawn_ws_reader(url);
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert!(!r.is_live(), "must not connect before the UI asks for samples");
        let mut got = None;
        for _ in 0..50 {
            got = compute_fft_bands(&r, 32).or(got);
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
        let bands = got.expect("no audio arrived in 2 s (is something playing?)");
        println!("live={} buffered={} bands={:.2?}", r.is_live(), r.samples.lock().unwrap().len(), bands);
        assert!(r.is_live() && bands.iter().any(|v| *v > 0.05));
        // Stop asking: the reader must disconnect by itself.
        tokio::time::sleep(Duration::from_millis(2000)).await;
        assert!(!r.is_live(), "still connected after the UI stopped asking");
    }

    #[test]
    fn vu_reads_each_channel_in_dbfs() {
        let r = AudioReader::new();
        // Left: full-scale square wave (0 dBFS RMS). Right: half scale (-6 dB).
        let pcm = frames(&(0..4410).map(|i| if i % 2 == 0 { (32767, 16384) } else { (-32767, -16384) }).collect::<Vec<_>>());
        r.push_pcm(&pcm);
        let vu = r.vu().expect("fresh audio");
        assert!(vu.level[0] > -0.1, "{vu:?}");
        assert!((vu.level[1] + 6.02).abs() < 0.1, "{vu:?}");
        assert!(vu.peak[0] >= vu.level[0] && vu.peak[1] >= vu.level[1]);
    }

    #[test]
    fn vu_ballistics_rise_at_once_fall_steadily_and_hold_the_peak() {
        let mut st = VuState::default();
        let quiet = [VU_MIN_DB; 2];
        let vu = vu_step(&mut st, [-10.0, -20.0], [-4.0, -12.0], 0.04);
        assert_eq!(vu.level, [-10.0, -20.0]);
        assert_eq!(vu.peak, [-4.0, -12.0]);
        // 0.1 s of silence: the level has fallen 3 dB, the peak is still held.
        let vu = vu_step(&mut st, quiet, quiet, 0.1);
        assert!((vu.level[0] + 13.0).abs() < 1e-3, "{vu:?}");
        assert_eq!(vu.peak[0], -4.0);
        // After the hold time the peak starts to drop, but never below the level.
        let vu = vu_step(&mut st, quiet, quiet, 1.0);
        assert!(vu.peak[0] < -4.0 && vu.peak[0] >= vu.level[0], "{vu:?}");
        // Long silence bottoms out at the scale minimum.
        let vu = vu_step(&mut st, quiet, quiet, 10.0);
        assert_eq!(vu.level, quiet);
    }

    #[test]
    fn smoothing_keeps_quiet_passages_quiet_and_lets_bars_fall_gradually() {
        let mut state = (Vec::new(), 0.0);
        let mut loud = [4.0_f32, 2.0];
        smooth_bands(&mut loud, &mut state);
        assert_eq!(loud, [1.0, 0.5]);
        // A much quieter frame right after: scaled by the remembered peak,
        // not blown up to full height, and no lower than the decayed bar.
        let mut quiet = [0.4_f32, 0.2];
        smooth_bands(&mut quiet, &mut state);
        assert!(quiet[0] < 0.85 && quiet[0] >= 0.79, "{quiet:?}");
        // Silence decays towards zero instead of dropping at once.
        let mut silence = [0.0_f32, 0.0];
        smooth_bands(&mut silence, &mut state);
        assert!(silence[0] > 0.5 && silence[0] < quiet[0]);
    }
}
