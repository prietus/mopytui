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
    /// Path to a named pipe / file with raw s16le stereo PCM at 44.1 kHz.
    /// When set and reachable, the spectrum visualizer switches from the
    /// pseudo oscillators to a real FFT of the live audio. Only useful when
    /// mopidy runs on the same machine as mopytui (or the FIFO is bridged
    /// somehow). Leave empty to keep pseudo-spectrum.
    #[serde(default)]
    pub audio_pipe: Option<String>,
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
            audio_pipe: None,
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

# Live FFT spectrum visualizer (optional). Only works when mopidy runs on
# the same machine as mopytui — set the path to a named pipe / file that
# carries raw s16le stereo PCM at 44.1 kHz. Leave commented for pseudo
# spectrum (no audio analysis).
#
# Linux + PipeWire (no mopidy config needed — just tap the sink monitor):
#     mkfifo /tmp/mopidy.fifo
#     pw-record --target @DEFAULT_AUDIO_SINK@.monitor \
#       --format=s16 --rate=44100 --channels=2 - > /tmp/mopidy.fifo &
#
# macOS local + DAC (add a `tee` to mopidy.conf — see README for the safe
# bit-perfect pipeline):
#     mkfifo /tmp/mopidy.fifo
#
# audio_pipe = "/tmp/mopidy.fifo"

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
