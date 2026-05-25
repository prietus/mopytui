use serde::{Deserialize, Deserializer, Serialize};

/// `#[serde(default)]` only fills in **missing** fields. Mopidy (especially
/// `mopidy_tidal`) sometimes emits explicit `null` for string fields like
/// `track.name` when a track ref isn't fully resolved — that crashes any
/// `String` field. Apply this on String fields that may come back as null
/// to coerce `null` → `""` instead of erroring the whole call.
fn de_string_default<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Artist {
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default, deserialize_with = "de_string_default")]
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Album {
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default, deserialize_with = "de_string_default")]
    pub name: String,
    #[serde(default)]
    pub artists: Vec<Artist>,
    #[serde(default)]
    pub num_tracks: Option<u32>,
    #[serde(default)]
    pub date: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Track {
    pub uri: String,
    #[serde(default, deserialize_with = "de_string_default")]
    pub name: String,
    #[serde(default)]
    pub artists: Vec<Artist>,
    #[serde(default)]
    pub album: Option<Album>,
    #[serde(default)]
    pub length: Option<u64>,
    #[serde(default)]
    pub track_no: Option<u32>,
    #[serde(default)]
    pub disc_no: Option<u32>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub bitrate: Option<u32>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub composers: Vec<Artist>,
}

impl Track {
    pub fn artists_joined(&self) -> String {
        self.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
    }
    pub fn album_name(&self) -> &str {
        self.album.as_ref().map(|a| a.name.as_str()).unwrap_or("")
    }
}

/// Mopidy serializes library refs with a `__model__: "Ref"` tag.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "__model__")]
pub enum Ref {
    #[serde(rename = "Ref")]
    Ref {
        #[serde(rename = "type")]
        kind: String,
        uri: String,
        #[serde(default, deserialize_with = "de_string_default")]
        name: String,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibRef {
    pub kind: String,
    pub uri: String,
    pub name: String,
}

impl Ref {
    pub fn into_lib(self) -> LibRef {
        match self {
            Ref::Ref { kind, uri, name } => LibRef { kind, uri, name },
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Image {
    pub uri: String,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchResult {
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub albums: Vec<Album>,
    #[serde(default)]
    pub artists: Vec<Artist>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TlTrack {
    pub tlid: u32,
    pub track: Track,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Playlist {
    pub uri: String,
    #[serde(default, deserialize_with = "de_string_default")]
    pub name: String,
    #[serde(default)]
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub last_modified: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AudioFormat {
    pub rate: u32,
    pub bits: u32,
    pub channels: u32,
}

impl AudioFormat {
    pub fn parse(s: &str) -> Option<Self> {
        let mut it = s.split(':');
        let rate = it.next()?.parse().ok()?;
        let bits_s = it.next()?;
        let bits = if bits_s == "f" { 32 } else { bits_s.parse().ok()? };
        let channels = it.next()?.parse().ok()?;
        Some(Self { rate, bits, channels })
    }
}

#[derive(Debug, Clone, Default)]
pub struct PlaybackSnapshot {
    pub state: PlayState,
    pub current: Option<Track>,
    pub current_tlid: Option<u32>,
    pub elapsed_ms: i64,
    pub volume: i32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlayState {
    Playing,
    Paused,
    #[default]
    Stopped,
}

impl PlayState {
    pub fn from_str(s: &str) -> Self {
        match s {
            "playing" => PlayState::Playing,
            "paused" => PlayState::Paused,
            _ => PlayState::Stopped,
        }
    }
    #[allow(dead_code)]
    pub fn glyph(self) -> &'static str {
        match self {
            PlayState::Playing => "▶",
            PlayState::Paused => "⏸",
            PlayState::Stopped => "■",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Modes {
    pub random: bool,
    pub repeat: bool,
    pub single: bool,
    pub consume: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: mopidy_tidal returns playlist tracks with `name: null`
    /// (and a sentinel `tidal:track:0:0:0` uri) for unresolved refs. The old
    /// `name: String` with `#[serde(default)]` failed because `default` only
    /// kicks in for *missing* fields — `null` was an outright type error,
    /// aborting `tracklist.add` with "json: invalid type: null, expected a
    /// string".
    #[test]
    fn track_tolerates_null_name_from_tidal() {
        let raw = r#"{
            "__model__":"Track","uri":"tidal:track:0:0:0","name":null,
            "artists":[],"album":null,"composers":[],"performers":[],
            "genre":null,"track_no":null,"disc_no":null,"date":null,
            "length":null,"bitrate":null,"comment":null,
            "musicbrainz_id":null,"last_modified":null
        }"#;
        let t: Track = serde_json::from_str(raw).expect("null name should deserialize");
        assert_eq!(t.name, "");
        assert_eq!(t.uri, "tidal:track:0:0:0");
    }

    #[test]
    fn tl_track_list_with_null_names_round_trips() {
        let raw = r#"[
            {"__model__":"TlTrack","tlid":1,"track":{"uri":"a","name":null}},
            {"__model__":"TlTrack","tlid":2,"track":{"uri":"b","name":"ok"}}
        ]"#;
        let v: Vec<TlTrack> = serde_json::from_str(raw).expect("mixed null/string names");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].track.name, "");
        assert_eq!(v[1].track.name, "ok");
    }

    #[test]
    fn artist_and_album_tolerate_null_name() {
        let a: Artist = serde_json::from_str(r#"{"uri":"x","name":null}"#).unwrap();
        assert_eq!(a.name, "");
        let al: Album = serde_json::from_str(r#"{"uri":"x","name":null}"#).unwrap();
        assert_eq!(al.name, "");
    }
}
