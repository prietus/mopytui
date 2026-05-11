use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::time::sleep;

use super::models::AudioFormat;

/// MPD subsystems we listen to via `idle`.
const IDLE_CMD: &[u8] = b"idle player mixer options output update playlist database\n";

#[derive(Debug, Clone)]
pub enum MpdEvent {
    Connecting,
    Connected,
    Disconnected,
    Error(String),
    /// Subsystems changed. The TUI should refresh whichever views depend on
    /// the listed subsystems (e.g. `"player"`, `"mixer"`, `"playlist"`).
    Changed(Vec<String>),
    /// Live audio chain format (sample rate, bit depth, channels, bitrate kbps).
    Audio { audio: Option<AudioFormat>, bitrate: Option<u32> },
}

/// Spawn the MPD idle subscriber. Pure consumer of mopidy-mpd's `idle` push
/// stream — never sends commands back through it. Mutations go via the
/// JSON-RPC client because mopidy-mpd's tracklist view drifts from core when
/// the queue is mutated through the HTTP API.
pub fn spawn_mpd_idle(host: String, port: u16) -> broadcast::Receiver<MpdEvent> {
    let (tx, rx) = broadcast::channel::<MpdEvent>(64);
    tokio::spawn(async move {
        run(host, port, tx).await;
    });
    rx
}

async fn run(host: String, port: u16, tx: broadcast::Sender<MpdEvent>) {
    loop {
        let _ = tx.send(MpdEvent::Connecting);
        match run_session(&host, port, &tx).await {
            Ok(()) => {
                let _ = tx.send(MpdEvent::Disconnected);
            }
            Err(e) => {
                let _ = tx.send(MpdEvent::Error(e));
            }
        }
        sleep(Duration::from_secs(3)).await;
    }
}

async fn run_session(
    host: &str,
    port: u16,
    tx: &broadcast::Sender<MpdEvent>,
) -> Result<(), String> {
    let stream = TcpStream::connect((host, port))
        .await
        .map_err(|e| format!("connect {host}:{port}: {e}"))?;
    let _ = stream.set_nodelay(true);
    let (r, mut w) = stream.into_split();
    let mut r = BufReader::new(r);

    // Greeting: "OK MPD x.y.z\n"
    let mut greet = String::new();
    r.read_line(&mut greet).await.map_err(|e| format!("read greet: {e}"))?;
    if greet.is_empty() {
        return Err("empty greeting".into());
    }
    let _ = tx.send(MpdEvent::Connected);

    refresh_audio(&mut r, &mut w, tx).await?;

    loop {
        w.write_all(IDLE_CMD).await.map_err(|e| format!("idle: {e}"))?;
        let mut changed: Vec<String> = Vec::new();
        loop {
            let mut line = String::new();
            let n = r.read_line(&mut line).await.map_err(|e| format!("read: {e}"))?;
            if n == 0 {
                return Err("eof".into());
            }
            let t = line.trim_end();
            if t == "OK" { break; }
            if let Some(rest) = t.strip_prefix("ACK") {
                return Err(format!("idle ack: {rest}"));
            }
            if let Some(rest) = t.strip_prefix("changed: ") {
                changed.push(rest.to_string());
            }
        }
        if !changed.is_empty() {
            let _ = tx.send(MpdEvent::Changed(changed));
            refresh_audio(&mut r, &mut w, tx).await?;
        }
    }
}

async fn refresh_audio(
    r: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    w: &mut tokio::net::tcp::OwnedWriteHalf,
    tx: &broadcast::Sender<MpdEvent>,
) -> Result<(), String> {
    w.write_all(b"status\n").await.map_err(|e| format!("status: {e}"))?;
    let mut audio: Option<AudioFormat> = None;
    let mut bitrate: Option<u32> = None;
    loop {
        let mut line = String::new();
        let n = r.read_line(&mut line).await.map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            return Err("eof".into());
        }
        let t = line.trim_end();
        if t == "OK" { break; }
        if let Some(rest) = t.strip_prefix("ACK") {
            return Err(format!("status ack: {rest}"));
        }
        if let Some((k, v)) = t.split_once(": ") {
            match k {
                "audio" => audio = AudioFormat::parse(v),
                "bitrate" => bitrate = v.parse().ok(),
                _ => {}
            }
        }
    }
    let _ = tx.send(MpdEvent::Audio { audio, bitrate });
    Ok(())
}
