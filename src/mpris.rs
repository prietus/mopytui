//! MPRIS (Media Player Remote Interfacing) integration. Linux-only; on
//! other platforms the public API becomes a no-op so callers don't need
//! `cfg` blocks at every callsite.
//!
//! Architecture
//! ============
//! The MPRIS server lives on its own OS thread driving a current-thread
//! tokio runtime + `LocalSet` (because `mpris_server::Player` is `!Send`).
//! The main thread talks to it through two channels:
//!
//!   main ──MprisState──▶ mpris thread   (state pushes)
//!   main ◀────Cmd────── mpris thread    (D-Bus invocations from KDE/etc)
//!
//! The mpris thread keeps `Player` alive while it polls `Player::run()` and
//! a receiver loop in parallel via `spawn_local`.

use tokio::sync::mpsc;

use crate::app::Cmd;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MprisPlayState {
    Playing,
    Paused,
    #[default]
    Stopped,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // fields are only consumed in the Linux backend
pub struct MprisState {
    pub status: MprisPlayState,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub album_artists: Vec<String>,
    /// Track length in milliseconds. Zero means "unknown".
    pub length_ms: i64,
    /// Current playback position in milliseconds.
    pub position_ms: i64,
    /// 0.0..=1.0
    pub volume: f64,
    /// D-Bus object path (must start with "/"). Use `MprisState::no_track`
    /// when nothing is playing.
    pub track_id: String,
    pub art_url: Option<String>,
    pub url: Option<String>,
    pub shuffle: bool,
    pub repeat: bool,
    pub single: bool,
}

impl MprisState {
    pub fn no_track() -> String {
        "/org/mpris/MediaPlayer2/TrackList/NoTrack".to_string()
    }
}

/// Snapshot of the app's playback state in MPRIS terms. Always safe to call;
/// pushing the result through `MprisHandle::update` is a no-op off Linux.
pub fn snapshot(app: &crate::app::App) -> MprisState {
    use crate::mopidy::models::PlayState;
    let status = match app.playback.state {
        PlayState::Playing => MprisPlayState::Playing,
        PlayState::Paused => MprisPlayState::Paused,
        PlayState::Stopped => MprisPlayState::Stopped,
    };
    let (title, artists, album, album_artists, length_ms, url, track_id) =
        match &app.playback.current {
            Some(t) => {
                let artists: Vec<String> = t.artists.iter().map(|a| a.name.clone()).collect();
                let album_obj = t.album.as_ref();
                let album_name = album_obj.map(|a| a.name.clone()).unwrap_or_default();
                let album_artists: Vec<String> = album_obj
                    .map(|a| a.artists.iter().map(|x| x.name.clone()).collect())
                    .unwrap_or_default();
                let length_ms = t.length.unwrap_or(0) as i64;
                (
                    t.name.clone(),
                    artists,
                    album_name,
                    album_artists,
                    length_ms,
                    Some(t.uri.clone()),
                    track_id_for_uri(&t.uri),
                )
            }
            None => (
                String::new(),
                Vec::new(),
                String::new(),
                Vec::new(),
                0,
                None,
                MprisState::no_track(),
            ),
        };

    MprisState {
        status,
        title,
        artists,
        album,
        album_artists,
        length_ms,
        position_ms: app.playback.elapsed_ms.max(0),
        // app.playback.volume is -1 when bit-perfect/no mixer; clamp to 0
        // and leave it at zero (KDE's slider will just sit at the bottom).
        volume: (app.playback.volume.max(0) as f64 / 100.0).clamp(0.0, 1.0),
        track_id,
        // Mopidy image URIs are already absolute http(s):// URLs.
        art_url: app.cover_uri_for_current.clone().filter(|s| !s.is_empty()),
        url,
        shuffle: app.modes.random,
        repeat: app.modes.repeat,
        single: app.modes.single,
    }
}

/// Map an arbitrary Mopidy URI to a valid D-Bus object path. Object paths
/// must match `^/[A-Za-z0-9_/]*$`, so we hash the URI for a stable, safe id.
fn track_id_for_uri(uri: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    uri.hash(&mut h);
    format!("/org/mopidy/mopytui/track/{:x}", h.finish())
}

/// Handle held by the main thread. On non-Linux platforms updates are
/// dropped silently; the type still exists so callers stay cfg-free.
#[derive(Clone)]
pub struct MprisHandle {
    #[cfg(target_os = "linux")]
    tx: mpsc::UnboundedSender<MprisState>,
}

impl MprisHandle {
    pub fn update(&self, state: MprisState) {
        #[cfg(target_os = "linux")]
        {
            let _ = self.tx.send(state);
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = state;
        }
    }
}

/// Boot the MPRIS bridge. Returns the handle for state pushes; commands
/// triggered by D-Bus arrive on `cmd_tx`. On non-Linux this is a no-op.
pub fn spawn(cmd_tx: mpsc::UnboundedSender<Cmd>) -> MprisHandle {
    #[cfg(target_os = "linux")]
    {
        let (tx, rx) = mpsc::unbounded_channel();
        let _ = std::thread::Builder::new()
            .name("mpris".into())
            .spawn(move || run_linux(cmd_tx, rx));
        MprisHandle { tx }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = cmd_tx;
        MprisHandle {}
    }
}

#[cfg(target_os = "linux")]
fn run_linux(cmd_tx: mpsc::UnboundedSender<Cmd>, mut rx: mpsc::UnboundedReceiver<MprisState>) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("mpris: failed to build runtime: {e}");
            return;
        }
    };
    let local = tokio::task::LocalSet::new();
    local.block_on(&rt, async move {
        if let Err(e) = serve(cmd_tx, &mut rx).await {
            tracing::warn!("mpris: bridge stopped: {e}");
        }
    });
}

#[cfg(target_os = "linux")]
async fn serve(
    cmd_tx: mpsc::UnboundedSender<Cmd>,
    rx: &mut mpsc::UnboundedReceiver<MprisState>,
) -> anyhow::Result<()> {
    use mpris_server::{LoopStatus, Player};
    use std::rc::Rc;

    let player = Player::builder("org.mopidy.mopytui")
        .identity("mopytui")
        .desktop_entry("mopytui")
        .can_play(true)
        .can_pause(true)
        .can_go_next(true)
        .can_go_previous(true)
        .can_seek(true)
        .can_control(true)
        .build()
        .await?;

    // ─── wire D-Bus → Cmd ───
    let tx = cmd_tx.clone();
    player.connect_play_pause(move |_| { let _ = tx.send(Cmd::TogglePlayPause); });
    let tx = cmd_tx.clone();
    player.connect_play(move |_| { let _ = tx.send(Cmd::TogglePlayPause); });
    let tx = cmd_tx.clone();
    player.connect_pause(move |_| { let _ = tx.send(Cmd::TogglePlayPause); });
    let tx = cmd_tx.clone();
    player.connect_next(move |_| { let _ = tx.send(Cmd::Next); });
    let tx = cmd_tx.clone();
    player.connect_previous(move |_| { let _ = tx.send(Cmd::Prev); });
    let tx = cmd_tx.clone();
    player.connect_stop(move |_| { let _ = tx.send(Cmd::Stop); });
    let tx = cmd_tx.clone();
    player.connect_seek(move |_, offset| {
        // MPRIS Seek is a *relative* offset in microseconds.
        let ms = offset.as_micros() / 1000;
        let _ = tx.send(Cmd::SeekRelative(ms));
    });
    let tx = cmd_tx.clone();
    player.connect_set_position(move |_, _track, pos| {
        // SetPosition is *absolute*, microseconds.
        let ms = pos.as_micros() / 1000;
        let _ = tx.send(Cmd::Seek(ms));
    });
    let tx = cmd_tx.clone();
    player.connect_set_volume(move |_, vol| {
        // Volume is 0.0..=1.0 in MPRIS, 0..=100 in Mopidy.
        let v = (vol * 100.0).round().clamp(0.0, 100.0) as i32;
        let _ = tx.send(Cmd::Volume(v));
    });
    let tx = cmd_tx.clone();
    player.connect_set_shuffle(move |_, _on| {
        // Mopidy only exposes a toggle — accept whatever side the caller
        // asked for by emitting a toggle; the next state push will
        // reconcile if the toggle didn't land on the requested value.
        let _ = tx.send(Cmd::ToggleRandom);
    });
    let tx = cmd_tx.clone();
    player.connect_set_loop_status(move |_, status| {
        // Approximate mapping. MPRIS distinguishes None / Track / Playlist;
        // Mopidy splits `repeat` (playlist) and `single` (one-track loop).
        let cmd = match status {
            LoopStatus::None => Cmd::ToggleRepeat,
            LoopStatus::Track => Cmd::ToggleSingle,
            LoopStatus::Playlist => Cmd::ToggleRepeat,
        };
        let _ = tx.send(cmd);
    });

    // Player::run() borrows &self — share it with Rc so we can also poll
    // the state channel concurrently on the same LocalSet.
    let player = Rc::new(player);
    let runner = player.clone();
    let run_handle = tokio::task::spawn_local(async move {
        runner.run().await;
    });

    while let Some(state) = rx.recv().await {
        apply_state(&player, state).await;
    }
    run_handle.abort();
    Ok(())
}

#[cfg(target_os = "linux")]
async fn apply_state(player: &mpris_server::Player, state: MprisState) {
    use mpris_server::{LoopStatus, Metadata, PlaybackStatus, Time, TrackId};

    let mut meta = Metadata::new();
    if !state.title.is_empty() {
        meta.set_title(Some(state.title.clone()));
    }
    if !state.artists.is_empty() {
        meta.set_artist(Some(state.artists.clone()));
    }
    if !state.album.is_empty() {
        meta.set_album(Some(state.album.clone()));
    }
    if !state.album_artists.is_empty() {
        meta.set_album_artist(Some(state.album_artists.clone()));
    }
    if state.length_ms > 0 {
        meta.set_length(Some(Time::from_millis(state.length_ms)));
    }
    if !state.track_id.is_empty()
        && let Ok(tid) = TrackId::try_from(state.track_id.as_str())
    {
        meta.set_trackid(Some(tid));
    }
    if let Some(art) = state.art_url.as_deref() {
        meta.set_art_url(Some(art.to_string()));
    }
    if let Some(url) = state.url.as_deref() {
        meta.set_url(Some(url.to_string()));
    }
    let _ = player.set_metadata(meta).await;

    let status = match state.status {
        MprisPlayState::Playing => PlaybackStatus::Playing,
        MprisPlayState::Paused => PlaybackStatus::Paused,
        MprisPlayState::Stopped => PlaybackStatus::Stopped,
    };
    let _ = player.set_playback_status(status).await;
    let _ = player.set_volume(state.volume.clamp(0.0, 1.0)).await;
    let _ = player.set_shuffle(state.shuffle).await;
    let loop_status = if state.single {
        LoopStatus::Track
    } else if state.repeat {
        LoopStatus::Playlist
    } else {
        LoopStatus::None
    };
    let _ = player.set_loop_status(loop_status).await;

    // `set_position` is intentionally a sync no-signal setter; MPRIS clients
    // poll Position lazily.
    player.set_position(Time::from_millis(state.position_ms));
}
