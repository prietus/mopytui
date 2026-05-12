// The spectrum panel is opt-in via `audio_pipe` / `audio_tcp` in config.toml.
// We keep the audio pipeline + FFT always compiled in so it can be re-enabled
// without rebuilding.
#![allow(dead_code)]

//! Audio reader for the live spectrum visualizer.
//!
//! Mopidy doesn't expose audio samples to JSON-RPC clients. To get a real
//! spectrum we ask the user to configure Mopidy's GStreamer `[audio] output`
//! to `tee` the audio stream into a sink we can read — either a named pipe
//! (FIFO) or a TCP server. The PCM is then read in a background thread and
//! stashed in a ring buffer for the UI to FFT.
//!
//! Recommended `mopidy.conf` snippet (UDP, fire-and-forget):
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const SAMPLE_RATE: u32 = 44_100;
/// Ring buffer length. ~185 ms of mono samples is enough to FFT a 2048-sample
/// window while leaving headroom for jitter.
const RING_CAP: usize = 8192;
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
}

impl AudioReader {
    fn new() -> Self {
        Self {
            samples: Mutex::new(VecDeque::with_capacity(RING_CAP)),
            last_data_at: Mutex::new(None),
            has_writer: AtomicBool::new(false),
        }
    }

    /// Snapshot the last `n` samples into `out`. Returns `false` if not
    /// enough samples are buffered yet, or if no data has arrived recently
    /// (older than 250 ms — Mopidy paused or pipe disconnected).
    pub fn copy_recent(&self, out: &mut Vec<f32>, n: usize) -> bool {
        let fresh = self
            .last_data_at
            .lock()
            .unwrap()
            .map(|t| t.elapsed() < Duration::from_millis(250))
            .unwrap_or(false);
        if !fresh {
            return false;
        }
        let buf = self.samples.lock().unwrap();
        if buf.len() < n {
            return false;
        }
        out.clear();
        out.extend(buf.iter().skip(buf.len() - n).copied());
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
                    let frames = &buf[..usable];
                    let mut samples = reader.samples.lock().unwrap();
                    for frame in frames.chunks_exact(STEREO_FRAME_BYTES) {
                        let l = i16::from_le_bytes([frame[0], frame[1]]) as f32 / 32768.0;
                        let r = i16::from_le_bytes([frame[2], frame[3]]) as f32 / 32768.0;
                        let mono = 0.5 * (l + r);
                        if samples.len() >= RING_CAP {
                            samples.pop_front();
                        }
                        samples.push_back(mono);
                    }
                    drop(samples);
                    *reader.last_data_at.lock().unwrap() = Some(Instant::now());
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

fn read_until_eof<R: Read>(mut src: R, reader: &Arc<AudioReader>) {
    // Read in chunks aligned to stereo frames.
    let mut buf = [0u8; 4096];
    loop {
        match src.read(&mut buf) {
            Ok(0) => return, // writer closed
            Ok(n) => {
                let usable = n - (n % STEREO_FRAME_BYTES);
                if usable == 0 { continue; }
                let frames = &buf[..usable];
                // s16le stereo → f32 mono (downmix average).
                let mut samples = reader.samples.lock().unwrap();
                let mut chunks = frames.chunks_exact(STEREO_FRAME_BYTES);
                for frame in &mut chunks {
                    let l = i16::from_le_bytes([frame[0], frame[1]]) as f32 / 32768.0;
                    let r = i16::from_le_bytes([frame[2], frame[3]]) as f32 / 32768.0;
                    let mono = 0.5 * (l + r);
                    if samples.len() >= RING_CAP {
                        samples.pop_front();
                    }
                    samples.push_back(mono);
                }
                drop(samples);
                *reader.last_data_at.lock().unwrap() = Some(Instant::now());
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

    // Per-band log compression, then normalise to [0, 1].
    for v in out.iter_mut() {
        *v = (1.0 + *v).ln();
    }
    let max = out.iter().fold(0.0_f32, |a, &b| a.max(b));
    if max > 0.0 {
        for v in out.iter_mut() {
            *v /= max;
        }
    }
    Some(out)
}
