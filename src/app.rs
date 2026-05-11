use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ratatui::widgets::{ListState, TableState};
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;

use crate::audio::AudioReader;
use crate::config::AppConfig;
use crate::images::ImageCache;
use crate::lyrics::{LyricsCache, ParsedLyrics};
use crate::metadata::{AlbumMeta, ArtistMeta, MetadataState};
use crate::mopidy::Client;
use crate::mopidy::models::{
    AudioFormat, LibRef, Modes, PlayState, PlaybackSnapshot, Playlist, TlTrack, Track,
};
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverFitMode {
    /// Preserve aspect ratio — may leave empty space on one axis.
    Fit,
    /// Fill the area, cropping the image as needed.
    Crop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Library,
    Albums,
    Queue,
    NowPlaying,
    Search,
    Playlists,
    Goodies,
    Info,
    Help,
}

impl View {
    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            View::Library => "Library",
            View::Albums => "Albums",
            View::Queue => "Queue",
            View::NowPlaying => "Now Playing",
            View::Search => "Search",
            View::Playlists => "Playlists",
            View::Goodies => "Stats",
            View::Info => "Info",
            View::Help => "Help",
        }
    }
}

#[derive(Default)]
pub struct LibraryState {
    /// Current browse path: each entry is (uri, display name).
    pub crumbs: Vec<(Option<String>, String)>,
    pub entries: Vec<LibRef>,
    pub entries_state: ListState,
    /// When entries[selected].kind == "album" we eagerly load tracks here.
    pub album_tracks: Option<Vec<Track>>,
    pub album_tracks_state: ListState,
    pub focus: LibraryFocus,
    pub loading: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LibraryFocus {
    #[default]
    Entries,
    Tracks,
}

#[derive(Default)]
pub struct QueueState {
    pub table: TableState,
}

#[derive(Default)]
pub struct SearchState {
    pub input: String,
    pub editing: bool,
    pub results: Vec<crate::mopidy::models::SearchResult>,
    pub flat: Vec<SearchHit>,
    pub state: ListState,
    pub last_query: Option<String>,
}

#[derive(Debug, Clone)]
pub enum SearchHit {
    Track(Track),
    Album(crate::mopidy::models::Album),
    Artist(crate::mopidy::models::Artist),
}

#[derive(Default)]
pub struct PlaylistsState {
    pub items: Vec<LibRef>,
    pub state: ListState,
    pub current: Option<Playlist>,
    pub tracks_state: ListState,
    pub focus: PlaylistsFocus,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlaylistsFocus {
    #[default]
    List,
    Tracks,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AlbumsMode {
    #[default]
    Grid,
    Detail,
}

#[derive(Debug, Clone)]
pub struct AlbumCard {
    pub uri: String,
    pub name: String,
    pub artist: String,
    pub year: Option<String>,
    pub source: AlbumSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlbumSource { Local, Tidal, Other }

impl AlbumSource {
    pub fn from_uri(uri: &str) -> Self {
        if uri.starts_with("tidal:") { AlbumSource::Tidal }
        else if uri.starts_with("local:") || uri.starts_with("file:") || uri.starts_with("m3u:") {
            AlbumSource::Local
        } else { AlbumSource::Other }
    }
}

#[derive(Debug, Clone)]
pub struct AlbumDetail {
    pub card: AlbumCard,
    pub tracks: Vec<Track>,
    pub track_index: usize,
}

pub struct AlbumsState {
    pub mode: AlbumsMode,
    pub items: Vec<AlbumCard>,
    pub grid_index: usize,
    pub grid_offset_row: usize,
    pub cover_protocols: HashMap<String, ratatui_image::protocol::StatefulProtocol>,
    pub cover_protocol_sizes: HashMap<String, (u16, u16)>,
    pub detail: Option<AlbumDetail>,
    pub loading: bool,
    pub loaded: bool,
    /// Tracks which album URIs we've already started a cover fetch for, so
    /// we don't re-spawn tasks on every render.
    pub cover_requested: std::collections::HashSet<String>,
}

impl Default for AlbumsState {
    fn default() -> Self {
        Self {
            mode: AlbumsMode::Grid,
            items: Vec::new(),
            grid_index: 0,
            grid_offset_row: 0,
            cover_protocols: HashMap::new(),
            cover_protocol_sizes: HashMap::new(),
            detail: None,
            loading: false,
            loaded: false,
            cover_requested: std::collections::HashSet::new(),
        }
    }
}

#[derive(Default)]
pub struct GoodiesState {
    pub available: bool,
    pub recent: Vec<GoodiesItem>,
    pub most: Vec<GoodiesItem>,
    pub state: ListState,
    pub tab: GoodiesTab,
    /// 24 entries; counts per hour-of-day.
    pub heatmap_hours: Vec<u64>,
    /// 7 entries; counts per day-of-week (Mon..Sun).
    pub heatmap_dow: Vec<u64>,
    pub genres: Vec<(String, u64)>,
    pub totals: Option<serde_json::Value>,
    /// Tidal album IDs the user has favorited via goodies.
    pub favorites: std::collections::HashSet<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GoodiesTab {
    #[default]
    Recent,
    MostPlayed,
    TopArtists,
    TopAlbums,
    Heatmap,
    Genres,
    Totals,
}

impl GoodiesTab {
    pub fn label(self) -> &'static str {
        match self {
            GoodiesTab::Recent => "Recent",
            GoodiesTab::MostPlayed => "Most Played",
            GoodiesTab::TopArtists => "Top Artists",
            GoodiesTab::TopAlbums => "Top Albums",
            GoodiesTab::Heatmap => "When",
            GoodiesTab::Genres => "Genres",
            GoodiesTab::Totals => "Totals",
        }
    }
    pub fn next(self) -> Self {
        match self {
            GoodiesTab::Recent => GoodiesTab::MostPlayed,
            GoodiesTab::MostPlayed => GoodiesTab::TopArtists,
            GoodiesTab::TopArtists => GoodiesTab::TopAlbums,
            GoodiesTab::TopAlbums => GoodiesTab::Heatmap,
            GoodiesTab::Heatmap => GoodiesTab::Genres,
            GoodiesTab::Genres => GoodiesTab::Totals,
            GoodiesTab::Totals => GoodiesTab::Recent,
        }
    }
    pub fn prev(self) -> Self {
        match self {
            GoodiesTab::Recent => GoodiesTab::Totals,
            GoodiesTab::MostPlayed => GoodiesTab::Recent,
            GoodiesTab::TopArtists => GoodiesTab::MostPlayed,
            GoodiesTab::TopAlbums => GoodiesTab::TopArtists,
            GoodiesTab::Heatmap => GoodiesTab::TopAlbums,
            GoodiesTab::Genres => GoodiesTab::Heatmap,
            GoodiesTab::Totals => GoodiesTab::Genres,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct GoodiesItem {
    pub uri: String,
    pub title: String,
    pub subtitle: String,
    pub count: Option<u32>,
}

/// Strip the Mopidy URI scheme to get the bare Tidal album ID (the form the
/// `tidal_goodies` HTTP endpoint expects). `tidal:album:12345` → `12345`.
pub fn tidal_album_id(uri: &str) -> Option<&str> {
    uri.strip_prefix("tidal:album:")
}

#[derive(Default)]
pub struct StatusBar {
    pub message: String,
    pub kind: StatusKind,
    pub expires: Option<Instant>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StatusKind {
    #[default]
    Info,
    Ok,
    Warn,
    Err,
}

impl StatusBar {
    pub fn flash(&mut self, msg: impl Into<String>, kind: StatusKind) {
        self.message = msg.into();
        self.kind = kind;
        self.expires = Some(Instant::now() + Duration::from_secs(4));
    }
    pub fn maybe_clear(&mut self) {
        if let Some(t) = self.expires
            && Instant::now() >= t
        {
            self.message.clear();
            self.expires = None;
        }
    }
}

pub struct App {
    pub cfg: AppConfig,
    pub theme: Theme,
    pub client: Client,
    pub images: Arc<ImageCache>,

    pub view: View,
    pub prev_view: View,

    pub playback: PlaybackSnapshot,
    pub modes: Modes,
    pub queue: Vec<TlTrack>,
    pub audio: Option<AudioFormat>,
    pub bitrate: Option<u32>,
    pub connected: bool,

    pub library: LibraryState,
    pub albums: AlbumsState,
    pub queue_state: QueueState,
    pub search: SearchState,
    pub playlists: PlaylistsState,
    pub goodies: GoodiesState,

    pub picker: Picker,
    pub cover_protocol: Option<StatefulProtocol>,
    pub cover_protocol_uri: Option<String>,
    pub cover_protocol_size: Option<(u16, u16)>,
    pub cover_uri_for_current: Option<String>,
    pub cover_fit_mode: CoverFitMode,
    pub audio_reader: Option<Arc<AudioReader>>,

    /// Cached cover URIs (raw image URI as returned by core.library.get_images)
    /// keyed by track or album URI.
    pub cover_url_by_uri: HashMap<String, String>,

    pub lyrics_cache: LyricsCache,
    pub lyrics: Option<Arc<ParsedLyrics>>,
    pub lyrics_key: Option<String>,
    pub show_lyrics: bool,

    pub metadata: Arc<MetadataState>,
    pub meta_slot: Arc<std::sync::Mutex<MetaSlot>>,
    pub meta_key: Option<String>,
    pub current_album_meta: Option<AlbumMeta>,
    pub current_artist_meta: Option<ArtistMeta>,

    pub status: StatusBar,
    pub quit: bool,
    /// Monotonic ms-since-last-tick used to drive elapsed locally between
    /// MPD player-state updates.
    pub last_tick: Instant,
}

impl App {
    pub fn new(
        cfg: AppConfig,
        client: Client,
        images: Arc<ImageCache>,
        picker: Picker,
        audio_reader: Option<Arc<AudioReader>>,
    ) -> Self {
        let theme = Theme::from_name(&cfg.theme);
        Self {
            cfg,
            theme,
            client,
            images,
            view: View::NowPlaying,
            prev_view: View::NowPlaying,
            playback: PlaybackSnapshot::default(),
            modes: Modes::default(),
            queue: Vec::new(),
            audio: None,
            bitrate: None,
            connected: false,
            library: LibraryState::default(),
            albums: AlbumsState::default(),
            queue_state: QueueState::default(),
            search: SearchState::default(),
            playlists: PlaylistsState::default(),
            goodies: GoodiesState::default(),
            picker,
            cover_protocol: None,
            cover_protocol_uri: None,
            cover_protocol_size: None,
            cover_uri_for_current: None,
            cover_fit_mode: CoverFitMode::Crop,
            audio_reader,
            cover_url_by_uri: HashMap::new(),
            lyrics_cache: LyricsCache::new(),
            lyrics: None,
            lyrics_key: None,
            show_lyrics: true,
            metadata: Arc::new(MetadataState::new()),
            meta_slot: Arc::new(std::sync::Mutex::new(MetaSlot::default())),
            meta_key: None,
            current_album_meta: None,
            current_artist_meta: None,
            status: StatusBar::default(),
            quit: false,
            last_tick: Instant::now(),
        }
    }

    pub fn set_view(&mut self, v: View) {
        if self.view != v {
            self.prev_view = self.view;
            self.view = v;
            // Arriving at Search with no query → drop straight into edit mode
            // so the user can just start typing. Leaving Search always exits
            // edit mode so global keys (`q`, etc.) work again.
            if v == View::Search {
                if self.search.input.is_empty() {
                    self.search.editing = true;
                }
            } else {
                self.search.editing = false;
            }
        }
    }

    /// Local elapsed extrapolation between playback snapshots so the progress
    /// bar keeps moving smoothly even when we only refresh playback state on
    /// MPD player events.
    pub fn tick_local_elapsed(&mut self) {
        let now = Instant::now();
        let delta = now.saturating_duration_since(self.last_tick);
        self.last_tick = now;
        if self.playback.state == PlayState::Playing {
            self.playback.elapsed_ms = self
                .playback
                .elapsed_ms
                .saturating_add(delta.as_millis() as i64);
        }
        self.status.maybe_clear();
        self.poll_lyrics_cache();
        self.poll_meta_slot();
    }

    /// If a lyrics fetch was scheduled for the current track, check whether
    /// it landed in the cache and adopt it. Cheap mutex-lookup per tick.
    fn poll_lyrics_cache(&mut self) {
        let Some(key) = self.lyrics_key.clone() else { return; };
        if self.lyrics.is_some() { return; }
        if let Some(entry) = self.lyrics_cache.get(&key)
            && let Some(parsed) = entry
        {
            self.lyrics = Some(parsed);
        }
    }

    /// Per-tick check for background metadata results.
    pub fn poll_meta_slot(&mut self) {
        let Some(key) = self.meta_key.clone() else { return; };
        let mut slot = self.meta_slot.lock().unwrap();
        if slot.key.as_deref() != Some(key.as_str()) {
            return;
        }
        if self.current_album_meta.is_none()
            && let Some(a) = slot.album.take()
        {
            self.current_album_meta = Some(a);
        }
        if self.current_artist_meta.is_none()
            && let Some(a) = slot.artist.take()
        {
            self.current_artist_meta = Some(a);
        }
    }
}

#[derive(Default)]
pub struct MetaSlot {
    pub key: Option<String>,
    pub album: Option<AlbumMeta>,
    pub artist: Option<ArtistMeta>,
}

/// Inputs that mutate state asynchronously via the client. Returning a Cmd
/// instead of awaiting inline keeps the render loop responsive.
#[allow(dead_code)]
pub enum Cmd {
    Quit,
    None,
    RefreshAll,
    RefreshPlayback,
    RefreshQueue,
    RefreshModes,
    BrowseInto(Option<String>, String),
    BrowseUp,
    OpenAlbum(String),
    LoadPlaylists,
    OpenPlaylist(String),
    Play(Option<u32>),
    TogglePlayPause,
    Stop,
    Next,
    Prev,
    Seek(i64),
    SeekRelative(i64),
    Volume(i32),
    ToggleMute,
    ToggleRandom,
    ToggleRepeat,
    ToggleSingle,
    ToggleConsume,
    Add(Vec<String>),
    RemoveTlid(u32),
    MoveQueue { start: u32, end: u32, to: u32 },
    ClearQueue,
    ShuffleQueue,
    SaveQueueAs(String),
    DeletePlaylist(String),
    RefreshLibrary(Option<String>),
    Search(String),
    LoadGoodies,
    FetchCover(String),
    ToggleFavoriteAlbum(String),
    LoadAlbums,
    OpenAlbumDetail(String),
    BackToAlbumsGrid,
    PlayAlbum(String),
    QueueAlbum(String),
}

