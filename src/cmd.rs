//! Async executor: turns `Cmd`s into JSON-RPC calls and mutates `App` state.
//! Background-fetchable work (e.g. cover bytes) is spawned with `tokio::spawn`.

use anyhow::Result;
use ratatui::widgets::ListState;

use crate::app::{App, Cmd, SearchHit};
use crate::images::fetch_and_decode;
use crate::mopidy::models::{PlayState, Track};

pub async fn apply(app: &mut App, cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::None => {}
        Cmd::Quit => app.quit = true,
        Cmd::RefreshAll => refresh_all(app).await,
        Cmd::RefreshPlayback => refresh_playback(app).await,
        Cmd::RefreshQueue => refresh_queue(app).await,
        Cmd::RefreshModes => refresh_modes(app).await,
        Cmd::BrowseInto(uri, name) => browse_into(app, uri, name).await,
        Cmd::BrowseUp => browse_up(app).await,
        Cmd::OpenAlbum(uri) => open_album(app, uri).await,
        Cmd::LoadPlaylists => load_playlists(app).await,
        Cmd::OpenPlaylist(uri) => open_playlist(app, uri).await,
        Cmd::Play(tlid) => {
            log_err(app, "play", app.client.playback_play(tlid).await);
        }
        Cmd::TogglePlayPause => toggle_play_pause(app).await,
        Cmd::Stop => log_err(app, "stop", app.client.playback_stop().await),
        Cmd::Next => log_err(app, "next", app.client.playback_next().await),
        Cmd::Prev => log_err(app, "prev", app.client.playback_previous().await),
        Cmd::Seek(ms) => log_err(app, "seek", app.client.playback_seek(ms).await),
        Cmd::SeekRelative(delta) => {
            let target = (app.playback.elapsed_ms + delta).max(0);
            log_err(app, "seek", app.client.playback_seek(target).await);
        }
        Cmd::Volume(v) => log_err(app, "volume", app.client.set_volume(v.max(0) as u32).await),
        Cmd::ToggleMute => {
            match app.client.toggle_mute().await {
                Ok(muted) => app.status.flash(
                    if muted { "muted" } else { "unmuted" },
                    crate::app::StatusKind::Info,
                ),
                Err(e) => app.status.flash(e.0, crate::app::StatusKind::Err),
            }
        }
        Cmd::ToggleRandom => {
            let next = !app.modes.random;
            log_err(app, "random", app.client.set_random(next).await);
            app.modes.random = next;
        }
        Cmd::ToggleRepeat => {
            let next = !app.modes.repeat;
            log_err(app, "repeat", app.client.set_repeat(next).await);
            app.modes.repeat = next;
        }
        Cmd::ToggleSingle => {
            let next = !app.modes.single;
            log_err(app, "single", app.client.set_single(next).await);
            app.modes.single = next;
        }
        Cmd::ToggleConsume => {
            let next = !app.modes.consume;
            log_err(app, "consume", app.client.set_consume(next).await);
            app.modes.consume = next;
        }
        Cmd::Add(uris) => add_uris(app, uris).await,
        Cmd::RemoveTlid(tlid) => {
            log_err(
                app,
                "remove",
                app.client.tracklist_remove(vec![tlid]).await.map(|_| ()),
            );
            refresh_queue(app).await;
        }
        Cmd::MoveQueue { start, end, to } => {
            log_err(
                app,
                "move",
                app.client.tracklist_move(start, end, to).await,
            );
            refresh_queue(app).await;
        }
        Cmd::ClearQueue => {
            log_err(app, "clear", app.client.tracklist_clear().await);
            refresh_queue(app).await;
        }
        Cmd::ShuffleQueue => {
            log_err(app, "shuffle", app.client.tracklist_shuffle().await);
            refresh_queue(app).await;
        }
        Cmd::SaveQueueAs(name) => save_queue_as(app, name).await,
        Cmd::DeletePlaylist(uri) => {
            log_err(
                app,
                "delete playlist",
                app.client.playlist_delete(&uri).await.map(|_| ()),
            );
            load_playlists(app).await;
        }
        Cmd::RefreshLibrary(uri) => {
            log_err(
                app,
                "library refresh",
                app.client.library_refresh(uri.as_deref()).await,
            );
            app.status.flash("library refresh requested", crate::app::StatusKind::Ok);
        }
        Cmd::Search => search(app).await,
        Cmd::LoadGoodies => load_goodies(app).await,
        Cmd::FetchCover(uri) => fetch_cover_async(app, uri),
        Cmd::ToggleFavoriteAlbum(uri) => toggle_favorite_album(app, uri).await,
        Cmd::LoadAlbums => load_albums(app).await,
        Cmd::OpenAlbumDetail(uri) => open_album_detail(app, uri).await,
        Cmd::BackToAlbumsGrid => {
            app.albums.mode = crate::app::AlbumsMode::Grid;
            app.albums.detail = None;
        }
        Cmd::PlayAlbum(uri) => play_album(app, uri).await,
        Cmd::QueueAlbum(uri) => queue_album(app, uri).await,
    }
    Ok(())
}

fn log_err(app: &mut App, what: &str, r: crate::mopidy::client::Result<()>) {
    if let Err(e) = r {
        tracing::error!("{what}: {e}");
        app.status.flash(format!("{what}: {}", e.0), crate::app::StatusKind::Err);
    }
}

// ─── refresh helpers ────────────────────────────────────────────────────────

pub async fn refresh_all(app: &mut App) {
    refresh_playback(app).await;
    refresh_queue(app).await;
    refresh_modes(app).await;
    if app.library.crumbs.is_empty() && app.library.entries.is_empty() {
        browse_into(app, None, "Library".into()).await;
    }
    check_goodies(app).await;
}

pub async fn refresh_playback(app: &mut App) {
    match app.client.fetch_playback().await {
        Ok(snap) => {
            let new_uri = snap.current.as_ref().map(|t| t.uri.clone());
            let changed = new_uri != app.playback.current.as_ref().map(|t| t.uri.clone());
            app.playback = snap;
            app.last_tick = std::time::Instant::now();
            if changed {
                app.cover_protocol = None;
                app.cover_protocol_uri = None;
                app.cover_uri_for_current = None;
                app.lyrics = None;
                app.lyrics_key = None;
                app.current_album_meta = None;
                app.current_artist_meta = None;
                app.current_artist_avatar_key = None;
                app.info_protocols.clear();
                app.info_protocol_sizes.clear();
                app.meta_key = None;
                if let Some(t) = app.playback.current.clone() {
                    schedule_cover_for_track(app, &t).await;
                    schedule_lyrics_for_track(app, &t);
                    schedule_metadata_for_track(app, &t);
                }
            }
            // Pull fresh chain info every player event, not just on URI change.
            // The GStreamer pipeline often hasn't negotiated caps yet on the
            // first poll after a track change → `active: false` and no format.
            // Subsequent play/seek events let us catch the real format once
            // mopidy has it. `refresh_audio_active` only overwrites `app.audio`
            // when the plugin returns a non-empty format, so paused/stopped
            // states don't blank the last-known chain.
            refresh_audio_active(app).await;
        }
        Err(e) => tracing::warn!("fetch_playback: {e}"),
    }
}

pub async fn refresh_queue(app: &mut App) {
    match app.client.get_tl_tracks().await {
        Ok(q) => {
            if app.queue_state.table.selected().is_none() && !q.is_empty() {
                app.queue_state.table.select(Some(0));
            }
            app.queue = q;
        }
        Err(e) => tracing::warn!("get_tl_tracks: {e}"),
    }
}

pub async fn refresh_modes(app: &mut App) {
    match app.client.get_modes().await {
        Ok(m) => app.modes = m,
        Err(e) => tracing::warn!("get_modes: {e}"),
    }
}

// ─── library ────────────────────────────────────────────────────────────────

pub async fn browse_into(app: &mut App, uri: Option<String>, name: String) {
    app.library.loading = true;
    match app.client.browse(uri.clone()).await {
        Ok(entries) => {
            app.library.crumbs.push((uri, name));
            app.library.entries = entries;
            app.library.entries_state = ListState::default();
            if !app.library.entries.is_empty() {
                app.library.entries_state.select(Some(0));
            }
            app.library.album_tracks = None;
            app.library.album_tracks_state = ListState::default();
            app.library.focus = crate::app::LibraryFocus::Entries;
        }
        Err(e) => app.status.flash(format!("browse: {}", e.0), crate::app::StatusKind::Err),
    }
    app.library.loading = false;
}

pub async fn browse_up(app: &mut App) {
    if app.library.crumbs.is_empty() { return; }
    app.library.crumbs.pop();
    let (uri, name) = app
        .library
        .crumbs
        .pop()
        .unwrap_or((None, "Library".to_string()));
    browse_into(app, uri, name).await;
}

pub async fn open_album(app: &mut App, uri: String) {
    match app.client.lookup(vec![uri.clone()]).await {
        Ok(mut map) => {
            let tracks = map.remove(&uri).unwrap_or_default();
            app.library.album_tracks_state = ListState::default();
            if !tracks.is_empty() {
                app.library.album_tracks_state.select(Some(0));
            }
            app.library.album_tracks = Some(tracks);
            app.library.focus = crate::app::LibraryFocus::Tracks;
        }
        Err(e) => app.status.flash(format!("lookup: {}", e.0), crate::app::StatusKind::Err),
    }
}

// ─── playlists ──────────────────────────────────────────────────────────────

pub async fn load_playlists(app: &mut App) {
    match app.client.playlists_as_list().await {
        Ok(items) => {
            if app.playlists.state.selected().is_none() && !items.is_empty() {
                app.playlists.state.select(Some(0));
            }
            app.playlists.items = items;
        }
        Err(e) => app.status.flash(format!("playlists: {}", e.0), crate::app::StatusKind::Err),
    }
}

pub async fn open_playlist(app: &mut App, uri: String) {
    match app.client.playlist_lookup(&uri).await {
        Ok(p) => {
            app.playlists.tracks_state = ListState::default();
            if let Some(p) = &p && !p.tracks.is_empty() {
                app.playlists.tracks_state.select(Some(0));
            }
            app.playlists.current = p;
            app.playlists.focus = crate::app::PlaylistsFocus::Tracks;
        }
        Err(e) => app.status.flash(format!("playlist: {}", e.0), crate::app::StatusKind::Err),
    }
}

async fn save_queue_as(app: &mut App, name: String) {
    let schemes = app.client.playlist_uri_schemes().await.unwrap_or_default();
    let scheme = schemes.iter().find(|s| s.as_str() == "m3u").cloned().or_else(|| schemes.first().cloned());
    match app.client.playlist_create(&name, scheme.as_deref()).await {
        Ok(Some(mut pl)) => {
            pl.tracks = app.queue.iter().map(|t| t.track.clone()).collect();
            if let Err(e) = app.client.playlist_save(&pl).await {
                app.status.flash(format!("save playlist: {}", e.0), crate::app::StatusKind::Err);
            } else {
                app.status.flash(format!("saved \"{name}\""), crate::app::StatusKind::Ok);
                load_playlists(app).await;
            }
        }
        Ok(None) => app.status.flash("playlist create returned null", crate::app::StatusKind::Warn),
        Err(e) => app.status.flash(format!("create: {}", e.0), crate::app::StatusKind::Err),
    }
}

// ─── playback ───────────────────────────────────────────────────────────────

pub async fn toggle_play_pause(app: &mut App) {
    let r = match app.playback.state {
        PlayState::Playing => app.client.playback_pause().await,
        PlayState::Paused => app.client.playback_resume().await,
        PlayState::Stopped => app.client.playback_play(None).await,
    };
    log_err(app, "play/pause", r);
}

pub async fn add_uris(app: &mut App, uris: Vec<String>) {
    let was_empty = app.queue.is_empty();
    let auto_play = was_empty && app.playback.state != PlayState::Playing;
    match app.client.tracklist_add(uris.clone(), None).await {
        Ok(added) => {
            app.status.flash(format!("added {} track(s)", added.len()), crate::app::StatusKind::Ok);
            refresh_queue(app).await;
            if auto_play
                && let Some(first) = added.first()
            {
                let _ = app.client.playback_play(Some(first.tlid)).await;
                refresh_playback(app).await;
            }
        }
        Err(e) => app.status.flash(format!("add: {}", e.0), crate::app::StatusKind::Err),
    }
}

// ─── search ─────────────────────────────────────────────────────────────────

pub async fn search(app: &mut App) {
    use crate::app::SearchField;
    use std::collections::HashMap;

    // Build per-field query map. Mopidy expects Vec<String> per key — we
    // pass a single phrase per filled field; users wanting multi-term AND
    // can just type a longer phrase since search is "contains" by default.
    let mut query: HashMap<String, Vec<String>> = HashMap::new();
    let mut label_parts: Vec<String> = Vec::new();
    for f in SearchField::ALL {
        let v = app.search.form.get(f).trim();
        if v.is_empty() { continue; }
        query.insert(f.mopidy_key().to_string(), vec![v.to_string()]);
        label_parts.push(format!("{}:{v}", f.label().to_lowercase()));
    }

    if query.is_empty() {
        app.status.flash("search: fill at least one field", crate::app::StatusKind::Warn);
        return;
    }

    let form = &app.search.form;
    let uris: Option<Vec<String>> = match (form.local, form.tidal) {
        (true, true) => None, // search everywhere
        (true, false) => Some(vec!["local:".into(), "file:".into(), "m3u:".into()]),
        (false, true) => Some(vec!["tidal:".into()]),
        (false, false) => {
            app.status.flash("search: enable Local or Tidal", crate::app::StatusKind::Warn);
            return;
        }
    };

    match app.client.search_query(query, uris, false).await {
        Ok(results) => {
            let mut flat: Vec<SearchHit> = Vec::new();
            for r in &results {
                // Mopidy-Local only fills `tracks` on search hits — derive
                // distinct albums from the tracks so the user sees both. We
                // dedupe by album URI to avoid one entry per matching track.
                if r.albums.is_empty() && !r.tracks.is_empty() {
                    let mut seen: std::collections::HashSet<String> =
                        std::collections::HashSet::new();
                    for t in &r.tracks {
                        let Some(album) = &t.album else { continue };
                        let Some(uri) = album.uri.as_deref() else { continue };
                        if !seen.insert(uri.to_string()) { continue }
                        let mut a = album.clone();
                        // Tracks from local often have an album without
                        // artists; fall back to the track's artists so the
                        // result row isn't blank.
                        if a.artists.is_empty() { a.artists = t.artists.clone(); }
                        flat.push(SearchHit::Album(a));
                    }
                } else {
                    for a in &r.albums {
                        flat.push(SearchHit::Album(a.clone()));
                    }
                }
                for a in &r.artists {
                    flat.push(SearchHit::Artist(a.clone()));
                }
                for t in &r.tracks {
                    flat.push(SearchHit::Track(t.clone()));
                }
            }
            app.search.state = ListState::default();
            if !flat.is_empty() {
                app.search.state.select(Some(0));
            }
            app.search.last_query = Some(label_parts.join(" · "));
            app.search.results = results;
            app.search.flat = flat;
        }
        Err(e) => app.status.flash(format!("search: {}", e.0), crate::app::StatusKind::Err),
    }
}

// ─── covers ─────────────────────────────────────────────────────────────────

/// Look up the cover image URI for the given track and schedule a background
/// fetch+decode into the shared image cache. Sets
/// `app.cover_uri_for_current` so the render loop materialises a protocol
/// on next frame.
pub async fn schedule_cover_for_track(app: &mut App, track: &Track) {
    // Try both the album URI and the track URI — some Mopidy backends only
    // expose cover images keyed by one or the other (Local, M3U, Tidal, etc).
    let mut candidates: Vec<String> = Vec::new();
    if let Some(album_uri) = track.album.as_ref().and_then(|a| a.uri.clone()) {
        candidates.push(album_uri);
    }
    candidates.push(track.uri.clone());
    candidates.sort();
    candidates.dedup();

    // Cache hit on any candidate?
    for c in &candidates {
        if let Some(image_uri) = app.cover_url_by_uri.get(c).cloned() {
            app.cover_uri_for_current = Some(image_uri.clone());
            if !app.images.contains(&image_uri) {
                fetch_cover_async(app, image_uri);
            }
            return;
        }
    }

    match app.client.get_images(candidates.clone()).await {
        Ok(map) => {
            // Pick the largest image across all candidates' results.
            let mut best: Option<(String, crate::mopidy::models::Image)> = None;
            for c in &candidates {
                if let Some(imgs) = map.get(c) {
                    for img in imgs {
                        let area = img.width.unwrap_or(0) as u64
                            * img.height.unwrap_or(0) as u64;
                        let current_area = best
                            .as_ref()
                            .map(|(_, b)| {
                                b.width.unwrap_or(0) as u64
                                    * b.height.unwrap_or(0) as u64
                            })
                            .unwrap_or(0);
                        if best.is_none() || area > current_area {
                            best = Some((c.clone(), img.clone()));
                        }
                    }
                }
            }
            if let Some((src, image)) = best {
                let image_uri = image.uri.clone();
                app.cover_url_by_uri.insert(src, image_uri.clone());
                app.cover_uri_for_current = Some(image_uri.clone());
                fetch_cover_async(app, image_uri);
            } else {
                tracing::info!(
                    "no cover returned by get_images for {:?}",
                    candidates
                );
            }
        }
        Err(e) => {
            tracing::warn!("get_images: {e}");
            app.status.flash(
                format!("cover lookup failed: {}", e.0),
                crate::app::StatusKind::Warn,
            );
        }
    }
}

/// Compute lyrics cache key, set it on the App, and spawn an HTTP fetch in
/// the background that writes the parsed result into the LyricsCache. The
/// per-tick `App::poll_lyrics_cache` adopts the result once it lands.
pub fn schedule_lyrics_for_track(app: &mut App, track: &Track) {
    let artist = track.artists_joined();
    let title = track.name.clone();
    let album = track.album_name().to_string();
    let dur_ms = track.length.unwrap_or(0) as i64;
    if artist.trim().is_empty() || title.trim().is_empty() {
        return;
    }
    let key = crate::lyrics::cache_key(&artist, &title, &album, dur_ms);
    app.lyrics_key = Some(key.clone());
    // Cached?
    if let Some(entry) = app.lyrics_cache.get(&key) {
        app.lyrics = entry;
        return;
    }
    let cache = app.lyrics_cache.clone();
    tokio::spawn(async move {
        let _ = crate::lyrics::fetch_for(&cache, &artist, &title, &album, dur_ms).await;
    });
}

/// Spawn a MusicBrainz + Wikipedia lookup for the playing track's artist and
/// album. Results land in `app.meta_slot`; the per-tick poll adopts them.
pub fn schedule_metadata_for_track(app: &mut App, track: &Track) {
    let artist = track.artists_joined();
    let album = track.album_name().to_string();
    if artist.trim().is_empty() {
        return;
    }
    let key = format!("{}|{}", artist.to_lowercase(), album.to_lowercase());
    app.meta_key = Some(key.clone());
    // Reset the slot for this new key.
    {
        let mut slot = app.meta_slot.lock().unwrap();
        slot.key = Some(key.clone());
        slot.album = None;
        slot.artist = None;
        slot.artist_avatar_key = None;
    }
    let state = app.metadata.clone();
    let slot = app.meta_slot.clone();
    let images = app.images.clone();
    let fanart_api_key = app.cfg.fanart_api_key.clone().unwrap_or_default();
    tokio::spawn(async move {
        let (album_res, artist_res) = tokio::join!(
            async {
                if album.trim().is_empty() {
                    None
                } else {
                    Some(state.album(&artist, &album).await)
                }
            },
            state.artist(&artist),
        );

        // Capture the MBID before moving artist_res into the slot.
        let artist_mbid = artist_res
            .info
            .as_ref()
            .map(|i| i.id.clone())
            .filter(|s| !s.is_empty());

        {
            let mut s = slot.lock().unwrap();
            if s.key.as_deref() == Some(key.as_str()) {
                if let Some(a) = album_res { s.album = Some(a); }
                s.artist = Some(artist_res);
            }
        }

        // Try to pull an artist thumbnail from fanart.tv. Requires both an
        // API key and the MusicBrainz id; otherwise silently skip.
        if let Some(mbid) = artist_mbid
            && !fanart_api_key.is_empty()
            && let Some(url) = state.fanart.artist_image_url(&mbid, &fanart_api_key).await
        {
            let cache_key = format!("fanart:artist:{mbid}");
            // Reuse existing decode if cached from a previous run.
            if !images.contains(&cache_key) {
                match state.fanart.download_bytes(&url).await {
                    Ok(bytes) => match image::load_from_memory(&bytes) {
                        Ok(img) => {
                            images.put(cache_key.clone(), std::sync::Arc::new(img));
                        }
                        Err(e) => tracing::warn!(
                            target: "mopytui::fanart",
                            "decode {mbid}: {e}"
                        ),
                    },
                    Err(e) => tracing::warn!(
                        target: "mopytui::fanart",
                        "download {mbid}: {e:#}"
                    ),
                }
            }
            if images.contains(&cache_key) {
                let mut s = slot.lock().unwrap();
                if s.key.as_deref() == Some(key.as_str()) {
                    s.artist_avatar_key = Some(cache_key);
                }
            }
        }
    });
}

// ─── albums ─────────────────────────────────────────────────────────────────

/// Aggregate album entries from every backend that exposes them via
/// `core.library.browse`. We try a handful of known URIs in parallel — each
/// backend implements its own scheme, so misses are silent.
pub async fn load_albums(app: &mut App) {
    if app.albums.loading { return; }
    app.albums.loading = true;

    let probes: Vec<Option<String>> = vec![
        Some("local:directory?type=album".into()),
        Some("tidal:my_albums".into()),
        Some("tidal:my-albums".into()),
        Some("tidal:favorites:albums".into()),
        // Generic: dump the root and recurse one level for any `album` refs.
        None,
    ];

    let mut all: Vec<crate::app::AlbumCard> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for probe in probes {
        let refs = match app.client.browse(probe.clone()).await {
            Ok(r) => r,
            Err(_) => continue,
        };
        for r in &refs {
            match r.kind.as_str() {
                "album" => {
                    if seen.insert(r.uri.clone()) {
                        all.push(crate::app::AlbumCard {
                            uri: r.uri.clone(),
                            name: r.name.clone(),
                            artist: String::new(),
                            year: None,
                            source: crate::app::AlbumSource::from_uri(&r.uri),
                        });
                    }
                }
                "directory" if probe.is_none() => {
                    // One level of recursion at the root.
                    if let Ok(sub) = app.client.browse(Some(r.uri.clone())).await {
                        for s in sub {
                            if s.kind == "album" && seen.insert(s.uri.clone()) {
                                all.push(crate::app::AlbumCard {
                                    uri: s.uri.clone(),
                                    name: s.name.clone(),
                                    artist: String::new(),
                                    year: None,
                                    source: crate::app::AlbumSource::from_uri(&s.uri),
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Enrich the first ~80 with artist/year via a batched lookup of one
    // track each. That keeps the cards informative without blocking on
    // a huge multi-album RPC.
    let head_uris: Vec<String> = all.iter().take(80).map(|c| c.uri.clone()).collect();
    if !head_uris.is_empty()
        && let Ok(map) = app.client.lookup(head_uris.clone()).await
    {
        for card in all.iter_mut() {
            if let Some(tracks) = map.get(&card.uri)
                && let Some(first) = tracks.first()
            {
                card.artist = first.artists_joined();
                if card.year.is_none() {
                    card.year = first.date.clone().or_else(|| {
                        first.album.as_ref().and_then(|a| a.date.clone())
                    });
                }
            }
        }
    }

    all.sort_by(|a, b| {
        a.artist
            .to_lowercase()
            .cmp(&b.artist.to_lowercase())
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    app.albums.items = all;
    if app.albums.grid_index >= app.albums.items.len() {
        app.albums.grid_index = 0;
    }
    app.albums.loaded = true;
    app.albums.loading = false;

    app.status.flash(
        format!("loaded {} albums", app.albums.items.len()),
        crate::app::StatusKind::Ok,
    );
}

pub async fn open_album_detail(app: &mut App, uri: String) {
    let Some(card) = app.albums.items.iter().find(|c| c.uri == uri).cloned() else { return };
    let tracks: Vec<crate::mopidy::models::Track> = match app.client.lookup(vec![uri.clone()]).await {
        Ok(mut map) => map.remove(&uri).unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    let mut detail = crate::app::AlbumDetail {
        card: card.clone(),
        tracks,
        track_index: 0,
    };

    // If we don't have a cover URI for this album yet, fetch + decode.
    if !app.cover_url_by_uri.contains_key(&uri)
        && let Ok(map) = app.client.get_images(vec![uri.clone()]).await
        && let Some(imgs) = map.get(&uri)
        && let Some(best) =
            imgs.iter().max_by_key(|i| (i.width.unwrap_or(0), i.height.unwrap_or(0)))
    {
        let image_uri = best.uri.clone();
        app.cover_url_by_uri.insert(uri.clone(), image_uri.clone());
        fetch_cover_async(app, image_uri);
    }

    // Enrich card if it was missing artist/year.
    if detail.card.artist.is_empty()
        && let Some(first) = detail.tracks.first()
    {
        detail.card.artist = first.artists_joined();
        if detail.card.year.is_none() {
            detail.card.year = first.date.clone();
        }
    }

    // Schedule MusicBrainz + Wikipedia in the background. We reuse the
    // existing meta_slot keyed by (artist|album) so the per-tick poll picks
    // up the result without a custom channel.
    let key = format!(
        "{}|{}",
        detail.card.artist.to_lowercase(),
        detail.card.name.to_lowercase()
    );
    app.meta_key = Some(key.clone());
    {
        let mut slot = app.meta_slot.lock().unwrap();
        slot.key = Some(key.clone());
        slot.album = None;
        slot.artist = None;
    }
    app.current_album_meta = None;
    app.current_artist_meta = None;
    let state = app.metadata.clone();
    let slot = app.meta_slot.clone();
    let artist = detail.card.artist.clone();
    let album = detail.card.name.clone();
    tokio::spawn(async move {
        let (a_meta, ar_meta) = tokio::join!(
            async {
                if album.trim().is_empty() { None } else { Some(state.album(&artist, &album).await) }
            },
            state.artist(&artist),
        );
        let mut s = slot.lock().unwrap();
        if s.key.as_deref() == Some(key.as_str()) {
            if let Some(a) = a_meta { s.album = Some(a); }
            s.artist = Some(ar_meta);
        }
    });

    app.albums.detail = Some(detail);
    app.albums.mode = crate::app::AlbumsMode::Detail;
}

pub async fn play_album(app: &mut App, uri: String) {
    if let Err(e) = app.client.tracklist_clear().await {
        app.status.flash(format!("clear: {}", e.0), crate::app::StatusKind::Err);
        return;
    }
    match app.client.tracklist_add(vec![uri.clone()], None).await {
        Ok(added) => {
            if let Some(first) = added.first() {
                let _ = app.client.playback_play(Some(first.tlid)).await;
            }
            refresh_queue(app).await;
            refresh_playback(app).await;
            app.status.flash("playing album", crate::app::StatusKind::Ok);
        }
        Err(e) => app.status.flash(format!("add: {}", e.0), crate::app::StatusKind::Err),
    }
}

pub async fn queue_album(app: &mut App, uri: String) {
    match app.client.tracklist_add(vec![uri], None).await {
        Ok(added) => {
            app.status.flash(format!("queued {} tracks", added.len()), crate::app::StatusKind::Ok);
            refresh_queue(app).await;
        }
        Err(e) => app.status.flash(format!("add: {}", e.0), crate::app::StatusKind::Err),
    }
}

/// Fire-and-forget background fetch of a cover for a specific album, dropping
/// the decoded image into the shared cache and tagging it as requested so
/// later renders don't re-spawn.
pub fn schedule_album_cover(app: &mut App, album: &crate::app::AlbumCard) {
    if app.albums.cover_requested.contains(&album.uri) { return; }
    app.albums.cover_requested.insert(album.uri.clone());
    let cache = app.images.clone();
    let client = app.client.clone();
    let album_uri = album.uri.clone();
    tokio::spawn(async move {
        // Resolve image URI once.
        let image_uri = match client.get_images(vec![album_uri.clone()]).await {
            Ok(map) => map
                .get(&album_uri)
                .and_then(|imgs| {
                    imgs.iter()
                        .max_by_key(|i| (i.width.unwrap_or(0), i.height.unwrap_or(0)))
                        .map(|i| i.uri.clone())
                }),
            Err(_) => None,
        };
        let Some(image_uri) = image_uri else { return };
        match fetch_and_decode(&client, &cache, &image_uri).await {
            // Also alias the decoded image under `album_uri` so the grid
            // renderer can find it without owning the album-uri → image-uri
            // map (which lives on the UI thread).
            Ok(arc) => cache.put(album_uri.clone(), arc),
            Err(e) => tracing::warn!("album cover {album_uri}: {e}"),
        }
    });
}

fn fetch_cover_async(app: &App, image_uri: String) {
    let cache = app.images.clone();
    let client = app.client.clone();
    tokio::spawn(async move {
        if let Err(e) = fetch_and_decode(&client, &cache, &image_uri).await {
            tracing::warn!("cover fetch {image_uri}: {e}");
        }
    });
}

// ─── goodies ────────────────────────────────────────────────────────────────

pub async fn check_goodies(app: &mut App) {
    match app.client.goodies_health().await {
        Ok(Some(_)) => {
            app.goodies.available = true;
            // Pull favorites once so the ★ markers light up in library views.
            load_favorites(app).await;
            // Initial audio snapshot — pulls DAC + live format + verdict.
            // Requires tidal_goodies >= 0.4.0; older plugins return 404 → no-op.
            refresh_audio_active(app).await;
        }
        _ => app.goodies.available = false,
    }
}

/// Pull the live chain snapshot from `tidal_goodies /audio/active`. Updates
/// `app.audio` (the live format), `app.dac_label`, and `app.audio_verdict`.
/// Mopidy's MPD frontend doesn't emit `audio:` in `status` against Mopidy
/// 4.0.0a2, so this endpoint is the only source of truth for rate/bits/channels.
pub async fn refresh_audio_active(app: &mut App) {
    if !app.goodies.available { return; }
    match app.client.goodies_audio_active().await {
        Ok(Some(a)) => {
            if a.dac_label.is_some() {
                app.dac_label = a.dac_label;
            }
            // `format` is None when no track is playing — keep last-known so
            // the UI doesn't flash empty between tracks.
            if a.format.is_some() {
                app.audio = a.format;
            }
            app.audio_verdict = a.verdict;
        }
        Ok(None) => { /* older plugin without /audio/active */ }
        Err(e) => tracing::debug!("goodies audio_active: {e}"),
    }
}

pub async fn load_goodies(app: &mut App) {
    use crate::app::GoodiesTab;
    check_goodies(app).await;
    if !app.goodies.available { return; }
    match app.goodies.tab {
        GoodiesTab::Recent => {
            let v = app.client.goodies_stats_recent(100).await.unwrap_or_default();
            app.goodies.recent = parse_goodies(&v);
        }
        GoodiesTab::MostPlayed => {
            let v = app.client.goodies_stats_most_played(100, None).await.unwrap_or_default();
            app.goodies.most = parse_goodies(&v);
        }
        GoodiesTab::TopArtists => {
            let v = app.client.goodies_stats_top_artists(100, None).await.unwrap_or_default();
            app.goodies.most = parse_goodies(&v);
        }
        GoodiesTab::TopAlbums => {
            let v = app.client.goodies_stats_top_albums(100, None).await.unwrap_or_default();
            app.goodies.most = parse_goodies(&v);
        }
        GoodiesTab::Heatmap => {
            let (h, d) = tokio::join!(
                app.client.goodies_stats_by_hour(),
                app.client.goodies_stats_by_day_of_week(),
            );
            app.goodies.heatmap_hours = parse_buckets(h.unwrap_or_default(), 24, "hour");
            app.goodies.heatmap_dow = parse_buckets(d.unwrap_or_default(), 7, "day_of_week");
        }
        GoodiesTab::Genres => {
            let v = app.client.goodies_stats_by_genre(50, None).await.unwrap_or_default();
            app.goodies.genres = parse_genres(&v);
        }
        GoodiesTab::Totals => {
            let (t, h) = tokio::join!(
                app.client.goodies_stats_totals(),
                app.client.goodies_stats_by_hour(),
            );
            app.goodies.totals = t.ok();
            app.goodies.heatmap_hours = parse_buckets(h.unwrap_or_default(), 24, "hour");
        }
    }
    if app.goodies.state.selected().is_none() {
        let has = match app.goodies.tab {
            GoodiesTab::Recent => !app.goodies.recent.is_empty(),
            GoodiesTab::MostPlayed | GoodiesTab::TopArtists | GoodiesTab::TopAlbums => {
                !app.goodies.most.is_empty()
            }
            _ => false,
        };
        if has { app.goodies.state.select(Some(0)); }
    }
}

pub async fn load_favorites(app: &mut App) {
    match app.client.goodies_favorite_album_ids().await {
        Ok(Some(ids)) => {
            app.goodies.favorites = ids.into_iter().collect();
        }
        _ => {}
    }
}

pub async fn toggle_favorite_album(app: &mut App, uri: String) {
    let Some(id) = crate::app::tidal_album_id(&uri) else {
        app.status.flash("not a Tidal album", crate::app::StatusKind::Warn);
        return;
    };
    let id = id.to_string();
    let currently = app.goodies.favorites.contains(&id);
    let next = !currently;
    match app.client.goodies_set_album_favorite(&id, next).await {
        Ok(true) => {
            if next {
                app.goodies.favorites.insert(id);
                app.status.flash("★ favorited", crate::app::StatusKind::Ok);
            } else {
                app.goodies.favorites.remove(&id);
                app.status.flash("removed from favorites", crate::app::StatusKind::Info);
            }
        }
        Ok(false) => app.status.flash(
            "goodies plugin not installed on the server",
            crate::app::StatusKind::Warn,
        ),
        Err(e) => app.status.flash(format!("favorite: {}", e.0), crate::app::StatusKind::Err),
    }
}

/// Common field names goodies uses for "how many plays" across endpoints.
const COUNT_KEYS: &[&str] = &[
    "count",
    "plays",
    "play_count",
    "playcount",
    "n_plays",
    "play_counts",
    "n",
    "total",
    "value",
    "occurrences",
];

fn pick_count(v: &serde_json::Value) -> Option<u64> {
    for k in COUNT_KEYS {
        if let Some(n) = v.get(*k).and_then(|x| x.as_u64()) {
            return Some(n);
        }
    }
    None
}

fn parse_buckets(v: serde_json::Value, n: usize, key_field: &str) -> Vec<u64> {
    // Accept either:
    //  • a plain numeric array [c0, c1, ...]
    //  • a map of "key: count"
    //  • an array of {<key>: i, count: c}
    let mut out = vec![0u64; n];
    if let Some(arr) = v.as_array() {
        if arr.iter().all(|x| x.is_number()) {
            for (i, x) in arr.iter().enumerate().take(n) {
                out[i] = x.as_u64().unwrap_or(0);
            }
            return out;
        }
        for item in arr {
            let idx = item.get(key_field).and_then(|x| x.as_u64()).map(|n| n as usize);
            let count = pick_count(item).unwrap_or(0);
            if let Some(i) = idx
                && i < n
            {
                out[i] = count;
            }
        }
    } else if let Some(obj) = v.as_object() {
        for (k, val) in obj {
            if let Some(c) = val.as_u64() {
                if let Ok(i) = k.parse::<usize>()
                    && i < n
                {
                    out[i] = c;
                }
            } else if let Some(c) = pick_count(val) {
                if let Ok(i) = k.parse::<usize>()
                    && i < n
                {
                    out[i] = c;
                }
            }
        }
    }
    tracing::debug!(target: "mopytui::goodies", "parse_buckets({key_field}): {:?}", out);
    out
}

fn parse_genres(v: &serde_json::Value) -> Vec<(String, u64)> {
    let arr = v.as_array().cloned().unwrap_or_default();
    let mut out: Vec<(String, u64)> = arr
        .into_iter()
        .filter_map(|x| {
            let g = x.get("genre")
                .or_else(|| x.get("name"))
                .and_then(|v| v.as_str())?
                .to_string();
            let c = pick_count(&x).unwrap_or(0);
            Some((g, c))
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1));
    if !out.is_empty() {
        tracing::debug!(target: "mopytui::goodies", "parse_genres: {} entries, top={:?}", out.len(), out.first());
    }
    out
}

fn parse_goodies(v: &serde_json::Value) -> Vec<crate::app::GoodiesItem> {
    let arr = v.as_array().cloned().unwrap_or_default();
    if let Some(sample) = arr.first() {
        tracing::debug!(target: "mopytui::goodies", "parse_goodies sample: {}", sample);
    }
    arr.into_iter()
        .map(|x| {
            // Track which field gave us the title so we can avoid using it
            // again for the subtitle (otherwise endpoints that key by artist
            // end up showing "Artist · Artist").
            let (title, title_src) = ["title", "name", "track", "track_name", "album", "artist"]
                .iter()
                .find_map(|k| {
                    x.get(*k)
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(|s| (s.to_string(), *k))
                })
                .unwrap_or_default();
            let subtitle = ["artist", "album_artist", "album", "year"]
                .iter()
                .filter(|k| **k != title_src)
                .find_map(|k| {
                    x.get(*k)
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                })
                .unwrap_or_default();
            let uri = x.get("uri").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let count = pick_count(&x).map(|n| n as u32);
            crate::app::GoodiesItem { uri, title, subtitle, count }
        })
        .collect()
}

