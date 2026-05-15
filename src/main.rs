use std::io;
use std::sync::Arc;

use anyhow::{Context, Result};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tracing_subscriber::EnvFilter;

mod app;
mod audio;
mod cmd;
mod config;
mod events;
mod fanart;
mod images;
mod input;
mod lyrics;
mod metadata;
mod mopidy;
mod ui;

use app::{App, Cmd};
use events::{AppEvent, Events};
use mopidy::{Client, MpdEvent, spawn_mpd_idle};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = match parse_cli() {
        CliAction::Run(c) => c,
        CliAction::Help => { print_help(); return Ok(()); }
        CliAction::Version => { println!("mopytui {}", env!("CARGO_PKG_VERSION")); return Ok(()); }
    };

    init_tracing();
    let mut cfg = config::load_or_template();
    cli.apply(&mut cfg);
    tracing::info!(
        "connecting to mopidy at http://{}:{} (mpd:{})",
        cfg.host, cfg.http_port, cfg.mpd_port
    );

    let client = Client::new(&cfg.host, cfg.http_port);
    let images = Arc::new(images::ImageCache::new());
    let mpd_rx = spawn_mpd_idle(cfg.host.clone(), cfg.mpd_port);

    let mut terminal = setup_terminal().context("terminal setup")?;
    install_panic_hook();

    // Picker queries the terminal for capabilities (Kitty/iTerm2/Sixel) via
    // escape sequences. Must run AFTER raw mode is on so the response isn't
    // line-buffered or echoed. CLI override forces a specific protocol when
    // auto-detection picks one the terminal doesn't actually render.
    let picker = images::make_picker(cli.image_protocol.as_deref());
    tracing::info!(
        "image protocol: {:?}  cell_size: {:?}  (override={:?})",
        picker.protocol_type(),
        picker.font_size(),
        cli.image_protocol,
    );

    // Audio source for real FFT spectrum (optional). TCP wins if both are
    // set. The reader thread keeps retrying connect()/open() if the source
    // isn't ready yet, so it handles mopidy starting after us too.
    let audio_source = cfg
        .audio_udp
        .as_ref()
        .map(|a| audio::AudioSource::Udp(a.clone()))
        .or_else(|| {
            cfg.audio_tcp
                .as_ref()
                .map(|a| audio::AudioSource::Tcp(a.clone()))
        })
        .or_else(|| {
            cfg.audio_pipe
                .as_ref()
                .map(|p| audio::AudioSource::Fifo(std::path::PathBuf::from(p)))
        });
    let audio_reader = audio_source.map(audio::spawn_audio_reader);

    let mut app = App::new(cfg, client, images, picker, audio_reader);

    // Initial fetches before the first frame so we don't render a blank UI.
    cmd::refresh_all(&mut app).await;

    // 250ms tick keeps the progress bar fluid enough at 4 fps while
    // reducing terminal-paint pressure — iTerm2 flickers embedded images
    // when there's constant escape traffic from other cells (the waveform
    // gradient, playhead) at 10 Hz.
    let mut events = Events::new(mpd_rx, 250);

    let res = run(&mut terminal, &mut app, &mut events).await;

    restore_terminal()?;
    res
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,mopytui=debug"));
    let log_path = config::log_path();
    let layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_ansi(false);
    if let Some(p) = log_path
        && let Some(parent) = p.parent()
    {
        let _ = std::fs::create_dir_all(parent);
        if let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
        {
            tracing_subscriber::registry()
                .with(filter)
                .with(layer.with_writer(file))
                .init();
            return;
        }
    }
    // Fallback: no file logging available. Use a minimal silent subscriber
    // so writes to stdout/stderr don't corrupt the TUI.
    tracing_subscriber::registry()
        .with(EnvFilter::new("off"))
        .init();
}

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    Ok(terminal)
}

fn restore_terminal() -> io::Result<()> {
    let mut stdout = io::stdout();
    execute!(stdout, LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}

fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        original(info);
    }));
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    events: &mut Events,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui::render(f, app))?;
        let ev = events.next().await;
        handle(app, ev).await?;
        if app.quit { break; }
    }
    Ok(())
}

async fn handle(app: &mut App, ev: AppEvent) -> Result<()> {
    match ev {
        AppEvent::Tick => {
            app.tick_local_elapsed();
        }
        AppEvent::Key(k) => {
            let cmd = input::handle_key(app, k);
            cmd::apply(app, cmd).await?;
        }
        AppEvent::Resize(_, _) => {
            // Ratatui auto-handles via draw; just trigger one redraw.
        }
        AppEvent::Mpd(ev) => apply_mpd(app, ev).await,
    }
    Ok(())
}

async fn apply_mpd(app: &mut App, ev: MpdEvent) {
    match ev {
        MpdEvent::Connecting => {
            app.connected = false;
            tracing::debug!("mpd: connecting");
        }
        MpdEvent::Connected => {
            app.connected = true;
            tracing::info!("mpd: connected");
            // Refresh everything on (re)connect.
            cmd::refresh_all(app).await;
        }
        MpdEvent::Disconnected => {
            app.connected = false;
            app.status.flash("mpd disconnected", crate::app::StatusKind::Warn);
        }
        MpdEvent::Error(e) => {
            app.connected = false;
            app.status.flash(format!("mpd: {e}"), crate::app::StatusKind::Err);
        }
        MpdEvent::Changed(subs) => {
            tracing::debug!(?subs, "mpd changed");
            let mut want_playback = false;
            let mut want_queue = false;
            let mut want_modes = false;
            let mut want_playlists = false;
            for s in subs {
                match s.as_str() {
                    "player" | "mixer" => want_playback = true,
                    "playlist" => { want_queue = true; }
                    "stored_playlist" => { want_playlists = true; }
                    "options" => { want_modes = true; }
                    "update" | "database" => { /* library changed — defer to user refresh */ }
                    _ => {}
                }
            }
            if want_playback { cmd::refresh_playback(app).await; }
            if want_queue { cmd::refresh_queue(app).await; }
            if want_modes { cmd::refresh_modes(app).await; }
            if want_playlists { cmd::load_playlists(app).await; }
        }
        MpdEvent::Audio { audio, bitrate } => {
            // Only overwrite when MPD actually has the info. Mopidy 4.0.0a2 +
            // mopidy-mpd 3.3.0 leave `audio:` empty and report `bitrate: 0`
            // for every track — accepting those would clobber the live chain
            // we populate from the `tidal_goodies` `/audio/active` endpoint.
            if let Some(a) = audio.filter(|a| a.rate > 0) {
                app.audio = Some(a);
            }
            if let Some(b) = bitrate.filter(|b| *b > 0) {
                app.bitrate = Some(b);
            }
        }
    }
}

// `crate::app::Cmd` is reachable through `cmd::apply`; this re-export keeps
// the binary's surface explicit for tooling.
#[allow(dead_code)]
type _Cmd = Cmd;

// Used implicitly by `init_tracing` via SubscriberInitExt / SubscriberExt
// imported above.
#[allow(unused_imports)]
use tracing_appender as _;

// ─── CLI ────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct CliOverrides {
    host: Option<String>,
    http_port: Option<u16>,
    mpd_port: Option<u16>,
    theme: Option<String>,
    image_protocol: Option<String>,
    audio_pipe: Option<String>,
    audio_tcp: Option<String>,
    audio_udp: Option<String>,
}

impl CliOverrides {
    fn apply(&self, cfg: &mut config::AppConfig) {
        if let Some(h) = &self.host { cfg.host = h.clone(); }
        if let Some(p) = self.http_port { cfg.http_port = p; }
        if let Some(p) = self.mpd_port { cfg.mpd_port = p; }
        if let Some(t) = &self.theme { cfg.theme = t.clone(); }
        if let Some(p) = &self.audio_pipe { cfg.audio_pipe = Some(p.clone()); }
        if let Some(a) = &self.audio_tcp { cfg.audio_tcp = Some(a.clone()); }
        if let Some(a) = &self.audio_udp { cfg.audio_udp = Some(a.clone()); }
    }
}

enum CliAction {
    Run(CliOverrides),
    Help,
    Version,
}

fn parse_cli() -> CliAction {
    let mut out = CliOverrides::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => return CliAction::Help,
            "-V" | "--version" => return CliAction::Version,
            "--host" | "-H" => {
                if let Some(v) = args.next() { out.host = Some(v); }
            }
            "--port" | "--http-port" => {
                if let Some(v) = args.next().and_then(|s| s.parse().ok()) {
                    out.http_port = Some(v);
                }
            }
            "--mpd-port" => {
                if let Some(v) = args.next().and_then(|s| s.parse().ok()) {
                    out.mpd_port = Some(v);
                }
            }
            "--theme" => {
                if let Some(v) = args.next() { out.theme = Some(v); }
            }
            "--image-protocol" => {
                if let Some(v) = args.next() { out.image_protocol = Some(v); }
            }
            "--audio-pipe" => {
                if let Some(v) = args.next() { out.audio_pipe = Some(v); }
            }
            "--audio-tcp" => {
                if let Some(v) = args.next() { out.audio_tcp = Some(v); }
            }
            "--audio-udp" => {
                if let Some(v) = args.next() { out.audio_udp = Some(v); }
            }
            _ => {
                // host:port shorthand: a single positional `host:port` arg.
                if let Some((h, p)) = a.split_once(':')
                    && let Ok(port) = p.parse::<u16>()
                {
                    out.host = Some(h.to_string());
                    out.http_port = Some(port);
                }
            }
        }
    }
    CliAction::Run(out)
}

fn print_help() {
    let path = config::config_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(no config dir)".into());
    println!(
        "mopytui {} — a terminal client for Mopidy

USAGE:
    mopytui [OPTIONS] [HOST:PORT]

OPTIONS:
    -H, --host <HOST>         Mopidy host (default 127.0.0.1)
    --port, --http-port <P>   Mopidy HTTP port (default 6680)
    --mpd-port <PORT>         Mopidy MPD port (default 6600)
    --theme <NAME>            midnight | soft-dark | daylight | solar
    --image-protocol <P>      Force image protocol: auto (default) |
                              kitty | iterm2 | sixel | halfblocks
                              Use 'halfblocks' if covers don't render in
                              your terminal — it's the safe fallback.
    --audio-udp <BIND>        Bind a UDP socket (host:port) to receive PCM
                              from GStreamer `udpsink`. Powers the real-FFT
                              spectrum visualizer (see config template).
    --audio-tcp <HOST:PORT>   Connect to a GStreamer `tcpserversink` instead
                              of UDP (less reliable — can stall mopidy).
    --audio-pipe <PATH>       Read PCM audio from a named pipe instead of UDP
                              (legacy; requires `mkfifo` + `filesink`).
    -h, --help                Show this help
    -V, --version             Show version

EXAMPLES:
    mopytui                                # use config defaults
    mopytui --host 192.168.1.10            # remote server
    mopytui 192.168.1.10:6680              # shorthand
    mopytui --theme solar

CONFIG FILE (TOML):
    {path}

    host = \"127.0.0.1\"
    http_port = 6680
    mpd_port = 6600
    theme = \"midnight\"

CLI flags override config values for this run.",
        env!("CARGO_PKG_VERSION"),
    );
}
