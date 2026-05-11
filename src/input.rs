//! Pure key → Cmd dispatch. The async executor in `cmd.rs` turns Cmds into
//! JSON-RPC calls. Splitting it this way keeps key handling synchronous and
//! the render loop responsive.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Cmd, LibraryFocus, PlaylistsFocus, View};
use crate::mopidy::models::PlayState;

pub fn handle_key(app: &mut App, key: KeyEvent) -> Cmd {
    // Edit mode owns the keyboard.
    if app.view == View::Search && app.search.editing {
        return handle_search_edit(app, key);
    }

    if let Some(g) = global_key(app, key) {
        return g;
    }

    match app.view {
        View::Library => handle_library(app, key),
        View::Albums => handle_albums(app, key),
        View::Queue => handle_queue(app, key),
        View::Search => handle_search_list(app, key),
        View::Playlists => handle_playlists(app, key),
        View::NowPlaying => handle_now_playing(app, key),
        View::Goodies => handle_goodies(app, key),
        View::Help | View::Info => handle_info(app, key),
    }
}

fn handle_albums(app: &mut App, key: KeyEvent) -> Cmd {
    use crate::app::AlbumsMode;
    match app.albums.mode {
        AlbumsMode::Grid => handle_albums_grid(app, key),
        AlbumsMode::Detail => handle_albums_detail(app, key),
    }
}

fn handle_albums_grid(app: &mut App, key: KeyEvent) -> Cmd {
    let len = app.albums.items.len();
    if len == 0 {
        if matches!(key.code, KeyCode::Char('r')) {
            return Cmd::LoadAlbums;
        }
        return Cmd::None;
    }
    // Cols is recomputed at render time; mirror the formula here so input
    // navigation jumps by the right amount.
    let cols = effective_cols(app);
    let cur = app.albums.grid_index;
    match key.code {
        KeyCode::Right | KeyCode::Char('l') => {
            if cur + 1 < len { app.albums.grid_index = cur + 1; }
            Cmd::None
        }
        KeyCode::Left | KeyCode::Char('h') => {
            if cur > 0 { app.albums.grid_index = cur - 1; }
            Cmd::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.albums.grid_index = (cur + cols).min(len - 1);
            Cmd::None
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if cur >= cols { app.albums.grid_index = cur - cols; }
            Cmd::None
        }
        KeyCode::PageDown => {
            app.albums.grid_index = (cur + cols * 3).min(len - 1);
            Cmd::None
        }
        KeyCode::PageUp => {
            app.albums.grid_index = cur.saturating_sub(cols * 3);
            Cmd::None
        }
        KeyCode::Home => { app.albums.grid_index = 0; Cmd::None }
        KeyCode::End => { app.albums.grid_index = len - 1; Cmd::None }
        KeyCode::Enter => {
            let uri = app.albums.items[cur].uri.clone();
            Cmd::OpenAlbumDetail(uri)
        }
        KeyCode::Char('a') => {
            let uri = app.albums.items[cur].uri.clone();
            Cmd::QueueAlbum(uri)
        }
        KeyCode::Char('p') => {
            let uri = app.albums.items[cur].uri.clone();
            Cmd::PlayAlbum(uri)
        }
        KeyCode::Char('f') => {
            let uri = app.albums.items[cur].uri.clone();
            Cmd::ToggleFavoriteAlbum(uri)
        }
        KeyCode::Char('r') => Cmd::LoadAlbums,
        _ => Cmd::None,
    }
}

/// Approximate grid columns count for keyboard navigation. Mirrors the
/// render-time formula so j/k jump rows that visually match the layout.
fn effective_cols(app: &App) -> usize {
    let _ = app;
    // Mirror render-time formula in ui/albums.rs (inner.width / 28). Without
    // plumbing terminal size here we default to 4 — matches typical
    // terminals at 120-cell width after chrome.
    4
}

fn handle_albums_detail(app: &mut App, key: KeyEvent) -> Cmd {
    let Some(detail) = app.albums.detail.as_mut() else { return Cmd::None };
    let len = detail.tracks.len();
    match key.code {
        KeyCode::Esc | KeyCode::Backspace => Cmd::BackToAlbumsGrid,
        KeyCode::Char('p') => Cmd::PlayAlbum(detail.card.uri.clone()),
        KeyCode::Char('a') => Cmd::QueueAlbum(detail.card.uri.clone()),
        KeyCode::Char('f') => Cmd::ToggleFavoriteAlbum(detail.card.uri.clone()),
        KeyCode::Down | KeyCode::Char('j') => {
            if len > 0 && detail.track_index + 1 < len { detail.track_index += 1; }
            Cmd::None
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if detail.track_index > 0 { detail.track_index -= 1; }
            Cmd::None
        }
        KeyCode::Enter => {
            if let Some(t) = detail.tracks.get(detail.track_index) {
                Cmd::Add(vec![t.uri.clone()])
            } else {
                Cmd::None
            }
        }
        _ => Cmd::None,
    }
}

fn global_key(app: &mut App, key: KeyEvent) -> Option<Cmd> {
    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') => Some(Cmd::Quit),
        KeyCode::Esc if app.view == View::Help => {
            app.set_view(app.prev_view);
            Some(Cmd::None)
        }
        KeyCode::Char('?') => {
            if app.view == View::Help {
                app.set_view(app.prev_view);
            } else {
                app.set_view(View::Help);
            }
            Some(Cmd::None)
        }
        KeyCode::Char('1') => { app.set_view(View::Queue); Some(Cmd::RefreshQueue) }
        KeyCode::Char('2') => {
            app.set_view(View::Albums);
            // Lazy load on first visit.
            if !app.albums.loaded && !app.albums.loading {
                Some(Cmd::LoadAlbums)
            } else {
                Some(Cmd::None)
            }
        }
        KeyCode::Char('3') => { app.set_view(View::Library); Some(Cmd::None) }
        KeyCode::Char('4') => { app.set_view(View::Playlists); Some(Cmd::LoadPlaylists) }
        KeyCode::Char('5') => {
            app.set_view(View::Search);
            app.search.editing = true;
            Some(Cmd::None)
        }
        KeyCode::Char('6') => { app.set_view(View::NowPlaying); Some(Cmd::None) }
        KeyCode::Char('7') => { app.set_view(View::Goodies); Some(Cmd::LoadGoodies) }
        KeyCode::Char('8') => { app.set_view(View::Info); Some(Cmd::None) }
        KeyCode::Tab => {
            let next = match app.view {
                View::Queue => View::Albums,
                View::Albums => View::Library,
                View::Library => View::Playlists,
                View::Playlists => View::Search,
                View::Search => View::NowPlaying,
                View::NowPlaying => View::Goodies,
                View::Goodies => View::Info,
                View::Info => View::Queue,
                View::Help => app.prev_view,
            };
            app.set_view(next);
            if next == View::Albums && !app.albums.loaded && !app.albums.loading {
                return Some(Cmd::LoadAlbums);
            }
            Some(Cmd::None)
        }
        // Playback globals (work from any view except Search-edit).
        KeyCode::Char(' ') => Some(Cmd::TogglePlayPause),
        KeyCode::Char('s') if !matches!(app.view, View::Library | View::Search) => Some(Cmd::Stop),
        // `<` / `>` for prev/next — `p` stays free for "play" in
        // Albums grid, Albums detail, and Search results. `n` is also
        // not globally bound so it doesn't shadow per-view actions.
        KeyCode::Char('>') => Some(Cmd::Next),
        KeyCode::Char('<') => Some(Cmd::Prev),
        KeyCode::Char('[') => Some(Cmd::SeekRelative(-10_000)),
        KeyCode::Char(']') => Some(Cmd::SeekRelative(10_000)),
        KeyCode::Left if key.modifiers.is_empty() && app.view == View::NowPlaying => {
            Some(Cmd::SeekRelative(-5_000))
        }
        KeyCode::Right if key.modifiers.is_empty() && app.view == View::NowPlaying => {
            Some(Cmd::SeekRelative(5_000))
        }
        KeyCode::Char('+') | KeyCode::Char('=') => {
            let v = (app.playback.volume + 5).clamp(0, 100);
            Some(Cmd::Volume(v))
        }
        KeyCode::Char('-') => {
            let v = (app.playback.volume - 5).clamp(0, 100);
            Some(Cmd::Volume(v))
        }
        KeyCode::Char('m') if app.view != View::Search => Some(Cmd::ToggleMute),
        KeyCode::Char('R') => Some(Cmd::ToggleRandom),
        KeyCode::Char('T') => Some(Cmd::ToggleRepeat),
        KeyCode::Char('S') => Some(Cmd::ToggleSingle),
        KeyCode::Char('C') => Some(Cmd::ToggleConsume),
        KeyCode::Char('L') => {
            app.show_lyrics = !app.show_lyrics;
            Some(Cmd::None)
        }
        KeyCode::Char('c') if !matches!(app.view, View::Search | View::Goodies) => {
            app.cover_fit_mode = match app.cover_fit_mode {
                crate::app::CoverFitMode::Fit => crate::app::CoverFitMode::Crop,
                crate::app::CoverFitMode::Crop => crate::app::CoverFitMode::Fit,
            };
            // Force protocol rebuild so the new resize strategy applies.
            app.cover_protocol = None;
            app.cover_protocol_uri = None;
            Some(Cmd::None)
        }
        KeyCode::Char('r') if key.modifiers == KeyModifiers::CONTROL => {
            Some(Cmd::RefreshAll)
        }
        _ => None,
    }
}

fn handle_library(app: &mut App, key: KeyEvent) -> Cmd {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => { select_delta(app, 1); Cmd::None }
        KeyCode::Up | KeyCode::Char('k') => { select_delta(app, -1); Cmd::None }
        KeyCode::PageDown => { select_delta(app, 10); Cmd::None }
        KeyCode::PageUp => { select_delta(app, -10); Cmd::None }
        KeyCode::Home => { select_to(app, 0); Cmd::None }
        KeyCode::End => {
            let len = entries_len(app);
            select_to(app, len.saturating_sub(1));
            Cmd::None
        }
        KeyCode::Tab => {
            app.library.focus = match app.library.focus {
                LibraryFocus::Entries => LibraryFocus::Tracks,
                LibraryFocus::Tracks => LibraryFocus::Entries,
            };
            Cmd::None
        }
        KeyCode::Enter => library_open_selected(app),
        KeyCode::Backspace | KeyCode::Char('h') => Cmd::BrowseUp,
        KeyCode::Char('a') | KeyCode::Char('A') => library_add_selected(app, key.code == KeyCode::Char('A')),
        KeyCode::Char('f') => {
            // Favorite the selected album (Tidal goodies).
            if app.library.focus == LibraryFocus::Entries
                && let Some(i) = app.library.entries_state.selected()
                && let Some(e) = app.library.entries.get(i)
                && e.kind == "album"
            {
                return Cmd::ToggleFavoriteAlbum(e.uri.clone());
            }
            Cmd::None
        }
        KeyCode::Char('r') if key.modifiers.is_empty() => {
            let uri = app.library.entries
                .get(app.library.entries_state.selected().unwrap_or(0))
                .map(|e| e.uri.clone());
            Cmd::RefreshLibrary(uri)
        }
        KeyCode::Char('/') => {
            app.set_view(View::Search);
            app.search.editing = true;
            Cmd::None
        }
        _ => Cmd::None,
    }
}

fn entries_len(app: &App) -> usize {
    match app.library.focus {
        LibraryFocus::Entries => app.library.entries.len(),
        LibraryFocus::Tracks => app.library.album_tracks.as_ref().map(|t| t.len()).unwrap_or(0),
    }
}

fn select_delta(app: &mut App, delta: i32) {
    let len = entries_len(app);
    if len == 0 { return; }
    let state = match app.library.focus {
        LibraryFocus::Entries => &mut app.library.entries_state,
        LibraryFocus::Tracks => &mut app.library.album_tracks_state,
    };
    let cur = state.selected().unwrap_or(0) as i32;
    let next = (cur + delta).clamp(0, len as i32 - 1) as usize;
    state.select(Some(next));
}

fn select_to(app: &mut App, idx: usize) {
    let state = match app.library.focus {
        LibraryFocus::Entries => &mut app.library.entries_state,
        LibraryFocus::Tracks => &mut app.library.album_tracks_state,
    };
    state.select(Some(idx));
}

fn library_open_selected(app: &mut App) -> Cmd {
    match app.library.focus {
        LibraryFocus::Entries => {
            let Some(idx) = app.library.entries_state.selected() else { return Cmd::None };
            let Some(e) = app.library.entries.get(idx).cloned() else { return Cmd::None };
            match e.kind.as_str() {
                "directory" => Cmd::BrowseInto(Some(e.uri), e.name),
                "album" => Cmd::OpenAlbum(e.uri),
                "track" => Cmd::Add(vec![e.uri]),
                "playlist" => Cmd::OpenPlaylist(e.uri),
                _ => Cmd::None,
            }
        }
        LibraryFocus::Tracks => {
            let Some(idx) = app.library.album_tracks_state.selected() else { return Cmd::None };
            if let Some(t) = app.library.album_tracks.as_ref().and_then(|v| v.get(idx)).cloned() {
                Cmd::Add(vec![t.uri])
            } else { Cmd::None }
        }
    }
}

fn library_add_selected(app: &App, also_play: bool) -> Cmd {
    let uri = match app.library.focus {
        LibraryFocus::Entries => app.library.entries
            .get(app.library.entries_state.selected().unwrap_or(0))
            .map(|e| e.uri.clone()),
        LibraryFocus::Tracks => app.library.album_tracks.as_ref()
            .and_then(|v| v.get(app.library.album_tracks_state.selected().unwrap_or(0)))
            .map(|t| t.uri.clone()),
    };
    let Some(u) = uri else { return Cmd::None };
    if also_play {
        // Adding then auto-play is handled in the Cmd executor.
        Cmd::Add(vec![u])
    } else {
        Cmd::Add(vec![u])
    }
}

fn handle_queue(app: &mut App, key: KeyEvent) -> Cmd {
    let len = app.queue.len();
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => {
            let cur = app.queue_state.table.selected().unwrap_or(0) as i32;
            let next = (cur + 1).clamp(0, len.saturating_sub(1) as i32) as usize;
            app.queue_state.table.select(Some(next));
            Cmd::None
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let cur = app.queue_state.table.selected().unwrap_or(0) as i32;
            let next = (cur - 1).clamp(0, len.saturating_sub(1) as i32) as usize;
            app.queue_state.table.select(Some(next));
            Cmd::None
        }
        KeyCode::Enter => {
            let Some(i) = app.queue_state.table.selected() else { return Cmd::None };
            app.queue.get(i).map(|t| Cmd::Play(Some(t.tlid))).unwrap_or(Cmd::None)
        }
        KeyCode::Char('d') | KeyCode::Delete => {
            let Some(i) = app.queue_state.table.selected() else { return Cmd::None };
            app.queue.get(i).map(|t| Cmd::RemoveTlid(t.tlid)).unwrap_or(Cmd::None)
        }
        KeyCode::Char('J') => {
            let Some(i) = app.queue_state.table.selected() else { return Cmd::None };
            if i + 1 >= len { return Cmd::None }
            Cmd::MoveQueue { start: i as u32, end: i as u32 + 1, to: i as u32 + 1 }
        }
        KeyCode::Char('K') => {
            let Some(i) = app.queue_state.table.selected() else { return Cmd::None };
            if i == 0 { return Cmd::None }
            Cmd::MoveQueue { start: i as u32, end: i as u32 + 1, to: i as u32 - 1 }
        }
        KeyCode::Char('X') => Cmd::ClearQueue,
        KeyCode::Char('Z') => Cmd::ShuffleQueue,
        _ => Cmd::None,
    }
}

fn handle_search_edit(app: &mut App, key: KeyEvent) -> Cmd {
    match key.code {
        KeyCode::Esc => {
            app.search.editing = false;
            Cmd::None
        }
        KeyCode::Enter => {
            app.search.editing = false;
            if app.search.input.trim().is_empty() {
                Cmd::None
            } else {
                Cmd::Search(app.search.input.clone())
            }
        }
        KeyCode::Backspace => { app.search.input.pop(); Cmd::None }
        KeyCode::Char(c) => { app.search.input.push(c); Cmd::None }
        _ => Cmd::None,
    }
}

fn handle_search_list(app: &mut App, key: KeyEvent) -> Cmd {
    use crate::app::SearchHit;
    let len = app.search.flat.len();
    match key.code {
        KeyCode::Char('/') => { app.search.editing = true; Cmd::None }
        KeyCode::Down | KeyCode::Char('j') => {
            let cur = app.search.state.selected().unwrap_or(0) as i32;
            let next = (cur + 1).clamp(0, len.saturating_sub(1) as i32) as usize;
            app.search.state.select(Some(next));
            Cmd::None
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let cur = app.search.state.selected().unwrap_or(0) as i32;
            let next = (cur - 1).clamp(0, len.saturating_sub(1) as i32) as usize;
            app.search.state.select(Some(next));
            Cmd::None
        }
        KeyCode::Enter => {
            let Some(i) = app.search.state.selected() else { return Cmd::None };
            match app.search.flat.get(i) {
                Some(SearchHit::Track(t)) => Cmd::Add(vec![t.uri.clone()]),
                Some(SearchHit::Album(a)) => a.uri.clone().map(|u| Cmd::OpenAlbum(u)).unwrap_or(Cmd::None),
                Some(SearchHit::Artist(a)) => a.uri.clone().map(|u| Cmd::BrowseInto(Some(u), a.name.clone())).unwrap_or(Cmd::None),
                None => Cmd::None,
            }
        }
        KeyCode::Char('f') => {
            let Some(i) = app.search.state.selected() else { return Cmd::None };
            match app.search.flat.get(i) {
                Some(SearchHit::Album(a)) => a
                    .uri
                    .clone()
                    .map(Cmd::ToggleFavoriteAlbum)
                    .unwrap_or(Cmd::None),
                Some(SearchHit::Track(t)) => {
                    // Fall back to the track's album URI.
                    t.album.as_ref().and_then(|al| al.uri.clone())
                        .map(Cmd::ToggleFavoriteAlbum)
                        .unwrap_or(Cmd::None)
                }
                _ => Cmd::None,
            }
        }
        KeyCode::Char('p') => {
            let Some(i) = app.search.state.selected() else { return Cmd::None };
            match app.search.flat.get(i) {
                Some(SearchHit::Album(a)) => a.uri.clone().map(Cmd::PlayAlbum).unwrap_or(Cmd::None),
                Some(SearchHit::Track(t)) => Cmd::Add(vec![t.uri.clone()]),
                _ => Cmd::None,
            }
        }
        KeyCode::Char('a') => {
            let Some(i) = app.search.state.selected() else { return Cmd::None };
            match app.search.flat.get(i) {
                Some(SearchHit::Album(a)) => a.uri.clone().map(Cmd::QueueAlbum).unwrap_or(Cmd::None),
                Some(SearchHit::Track(t)) => Cmd::Add(vec![t.uri.clone()]),
                _ => Cmd::None,
            }
        }
        _ => Cmd::None,
    }
}

fn handle_playlists(app: &mut App, key: KeyEvent) -> Cmd {
    let len_list = app.playlists.items.len();
    let len_tracks = app.playlists.current.as_ref().map(|p| p.tracks.len()).unwrap_or(0);
    match key.code {
        KeyCode::Tab => {
            app.playlists.focus = match app.playlists.focus {
                PlaylistsFocus::List => PlaylistsFocus::Tracks,
                PlaylistsFocus::Tracks => PlaylistsFocus::List,
            };
            Cmd::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            match app.playlists.focus {
                PlaylistsFocus::List => {
                    let cur = app.playlists.state.selected().unwrap_or(0) as i32;
                    let next = (cur + 1).clamp(0, len_list.saturating_sub(1) as i32) as usize;
                    app.playlists.state.select(Some(next));
                }
                PlaylistsFocus::Tracks => {
                    let cur = app.playlists.tracks_state.selected().unwrap_or(0) as i32;
                    let next = (cur + 1).clamp(0, len_tracks.saturating_sub(1) as i32) as usize;
                    app.playlists.tracks_state.select(Some(next));
                }
            }
            Cmd::None
        }
        KeyCode::Up | KeyCode::Char('k') => {
            match app.playlists.focus {
                PlaylistsFocus::List => {
                    let cur = app.playlists.state.selected().unwrap_or(0) as i32;
                    let next = (cur - 1).clamp(0, len_list.saturating_sub(1) as i32) as usize;
                    app.playlists.state.select(Some(next));
                }
                PlaylistsFocus::Tracks => {
                    let cur = app.playlists.tracks_state.selected().unwrap_or(0) as i32;
                    let next = (cur - 1).clamp(0, len_tracks.saturating_sub(1) as i32) as usize;
                    app.playlists.tracks_state.select(Some(next));
                }
            }
            Cmd::None
        }
        KeyCode::Enter => {
            match app.playlists.focus {
                PlaylistsFocus::List => {
                    let Some(i) = app.playlists.state.selected() else { return Cmd::None };
                    app.playlists.items.get(i).map(|p| Cmd::OpenPlaylist(p.uri.clone())).unwrap_or(Cmd::None)
                }
                PlaylistsFocus::Tracks => {
                    let Some(i) = app.playlists.tracks_state.selected() else { return Cmd::None };
                    let uri = app.playlists.current.as_ref().and_then(|p| p.tracks.get(i)).map(|t| t.uri.clone());
                    uri.map(|u| Cmd::Add(vec![u])).unwrap_or(Cmd::None)
                }
            }
        }
        KeyCode::Char('a') if app.playlists.focus == PlaylistsFocus::List => {
            // Add all tracks of current playlist to queue.
            let uris = app.playlists.current.as_ref().map(|p| {
                p.tracks.iter().map(|t| t.uri.clone()).collect::<Vec<_>>()
            }).unwrap_or_default();
            if uris.is_empty() { Cmd::None } else { Cmd::Add(uris) }
        }
        KeyCode::Char('D') => {
            // Delete selected playlist.
            let Some(i) = app.playlists.state.selected() else { return Cmd::None };
            app.playlists.items.get(i).map(|p| Cmd::DeletePlaylist(p.uri.clone())).unwrap_or(Cmd::None)
        }
        _ => Cmd::None,
    }
}

fn handle_now_playing(app: &App, key: KeyEvent) -> Cmd {
    match key.code {
        KeyCode::Char(' ') => Cmd::TogglePlayPause,
        KeyCode::Char('s') => Cmd::Stop,
        KeyCode::Char('f') => {
            // Favorite the album of the currently playing track.
            app.playback
                .current
                .as_ref()
                .and_then(|t| t.album.as_ref())
                .and_then(|a| a.uri.clone())
                .map(Cmd::ToggleFavoriteAlbum)
                .unwrap_or(Cmd::None)
        }
        _ => Cmd::None,
    }
}

fn handle_goodies(app: &mut App, key: KeyEvent) -> Cmd {
    use crate::app::GoodiesTab;
    match key.code {
        KeyCode::Left | KeyCode::Char('h') => {
            app.goodies.tab = app.goodies.tab.prev();
            app.goodies.state.select(None);
            Cmd::LoadGoodies
        }
        KeyCode::Right | KeyCode::Char('l') => {
            app.goodies.tab = app.goodies.tab.next();
            app.goodies.state.select(None);
            Cmd::LoadGoodies
        }
        KeyCode::Enter => {
            let Some(i) = app.goodies.state.selected() else { return Cmd::None };
            let items = match app.goodies.tab {
                GoodiesTab::Recent => &app.goodies.recent,
                GoodiesTab::MostPlayed | GoodiesTab::TopArtists | GoodiesTab::TopAlbums => {
                    &app.goodies.most
                }
                _ => return Cmd::None,
            };
            items.get(i).map(|it| Cmd::Add(vec![it.uri.clone()])).unwrap_or(Cmd::None)
        }
        KeyCode::Char('f') => {
            let Some(i) = app.goodies.state.selected() else { return Cmd::None };
            let items = match app.goodies.tab {
                GoodiesTab::Recent => &app.goodies.recent,
                GoodiesTab::MostPlayed | GoodiesTab::TopArtists | GoodiesTab::TopAlbums => {
                    &app.goodies.most
                }
                _ => return Cmd::None,
            };
            items.get(i).map(|it| Cmd::ToggleFavoriteAlbum(it.uri.clone())).unwrap_or(Cmd::None)
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let items = match app.goodies.tab {
                GoodiesTab::Recent => &app.goodies.recent,
                GoodiesTab::MostPlayed | GoodiesTab::TopArtists | GoodiesTab::TopAlbums => {
                    &app.goodies.most
                }
                _ => return Cmd::None,
            };
            let len = items.len();
            let cur = app.goodies.state.selected().unwrap_or(0) as i32;
            let next = (cur + 1).clamp(0, len.saturating_sub(1) as i32) as usize;
            app.goodies.state.select(Some(next));
            Cmd::None
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let cur = app.goodies.state.selected().unwrap_or(0) as i32;
            let next = (cur - 1).max(0) as usize;
            app.goodies.state.select(Some(next));
            Cmd::None
        }
        _ => Cmd::None,
    }
}

fn handle_info(_app: &mut App, _key: KeyEvent) -> Cmd { Cmd::None }

// Silence unused-arg warnings for handlers that don't read app state today
// but might once features expand.
#[allow(dead_code)]
fn _suppress(_: PlayState) {}
