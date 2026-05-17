use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_mpd_port")]
    pub mpd_port: u16,
    #[serde(default = "default_http_port")]
    pub http_port: u16,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub mpris: bool,
    /// `host:port` to bind a UDP socket on. GStreamer `udpsink` in mopidy
    /// fires raw s16le stereo PCM packets at us at 44.1 kHz. Preferred
    /// transport — no preroll, no stalls, fire-and-forget. Works across
    /// machines.
    #[serde(default)]
    pub audio_udp: Option<String>,
    /// `host:port` of a GStreamer `tcpserversink`. Alternative to
    /// `audio_udp`. Can stall mopidy's pipeline if no client is connected
    /// (`gst_util_uint64_scale: denom != 0` in mopidy logs).
    #[serde(default)]
    pub audio_tcp: Option<String>,
    /// Path to a named pipe / file with raw s16le stereo PCM at 44.1 kHz.
    /// Alternative to `audio_udp` (kept for setups that already use a FIFO).
    /// Requires `mkfifo` and a `filesink` in mopidy's `output`.
    #[serde(default)]
    pub audio_pipe: Option<String>,
    /// Default visualizer style for the spectrum panel: `bars`, `mirror`,
    /// `dots`, or `wave`. Cycled at runtime with `v`.
    #[serde(default)]
    pub visualizer_style: Option<String>,
    #[serde(default)]
    pub lastfm_api_key: Option<String>,
    #[serde(default)]
    pub fanart_api_key: Option<String>,
    #[serde(default)]
    pub discogs_token: Option<String>,
}

fn default_host() -> String { "127.0.0.1".into() }
fn default_mpd_port() -> u16 { 6600 }
fn default_http_port() -> u16 { 6680 }
fn default_theme() -> String { "midnight".into() }

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            mpd_port: default_mpd_port(),
            http_port: default_http_port(),
            theme: default_theme(),
            mpris: false,
            audio_udp: None,
            audio_tcp: None,
            audio_pipe: None,
            visualizer_style: None,
            lastfm_api_key: None,
            fanart_api_key: None,
            discogs_token: None,
        }
    }
}

const TEMPLATE: &str = r#"# mopytui config
host = "127.0.0.1"
http_port = 6680
mpd_port = 6600

# midnight | soft-dark | daylight | solar
theme = "midnight"

# Enable MPRIS integration (Linux only — no-op elsewhere).
mpris = false

# Live FFT spectrum visualizer (optional). Configure mopidy's `[audio] output`
# to `tee` the stream into a sink mopytui can read. Bit-perfect to the DAC is
# preserved because `tee` only duplicates buffers and the visualizer branch
# uses `leaky=2` queues so it never blocks the audio path.
#
# Recommended: UDP (fire-and-forget, no preroll). Add to mopidy.conf:
#
#     [audio]
#     output = tee name=t allow-not-linked=true t. ! queue leaky=2 max-size-buffers=200 ! autoaudiosink t. ! queue leaky=2 max-size-buffers=200 ! audioresample ! audioconvert ! audio/x-raw,format=S16LE,rate=44100,channels=2 ! udpsink host=<mopytui-host> port=5555 sync=false
#
# audio_udp = "0.0.0.0:5555"
#
# Alternatives (less reliable):
# audio_tcp  = "127.0.0.1:5555"     # GStreamer tcpserversink
# audio_pipe = "/tmp/mopidy.fifo"   # filesink + mkfifo

# Default visualizer style on launch. Cycle at runtime with `v`.
# bars   — vertical FFT bars (default)
# mirror — bars mirrored above/below a centre axis
# dots   — FFT bars in braille sub-pixels (2× horizontal × 4× vertical)
# wave   — raw PCM waveform (braille line plot)
# visualizer_style = "bars"

# Optional API keys for richer metadata.
# last.fm:   https://www.last.fm/api/account/create
# fanart.tv: https://fanart.tv/get-an-api-key/
# discogs:   https://www.discogs.com/settings/developers
lastfm_api_key = ""
fanart_api_key = ""
discogs_token = ""
"#;

pub fn config_path() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "mopytui")?;
    Some(dirs.config_dir().join("config.toml"))
}

pub fn cache_dir() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "mopytui")?;
    let dir = dirs.cache_dir().to_path_buf();
    let _ = std::fs::create_dir_all(&dir);
    Some(dir)
}

pub fn log_path() -> Option<PathBuf> {
    Some(cache_dir()?.join("mopytui.log"))
}

pub fn load_or_template() -> AppConfig {
    let Some(path) = config_path() else {
        return AppConfig::default();
    };
    if !path.exists() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, TEMPLATE);
        eprintln!("wrote template config at {}", path.display());
        return AppConfig::default();
    }
    match read_config(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config error: {e} — using defaults");
            AppConfig::default()
        }
    }
}

fn read_config(path: &Path) -> Result<AppConfig> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?;
    Ok(toml::from_str::<AppConfig>(&raw)?)
}
