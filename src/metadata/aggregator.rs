use serde::{Deserialize, Serialize};

use super::musicbrainz::{MbArtistInfo, MbRelease, MusicBrainz};
use super::wikipedia::{WikiSummary, Wikipedia};
use crate::fanart::Fanart;
use crate::mopidy::client::{TidalCredit, TidalText};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumMeta {
    pub release: Option<MbRelease>,
    pub wiki: Option<WikiSummary>,
    /// Tidal's editorial review (via goodies), filled in after the lookup.
    #[serde(default)]
    pub tidal: Option<TidalText>,
    /// Credits of the playing track (goodies: tags or Tidal).
    #[serde(default)]
    pub track_credits: Vec<TidalCredit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistMeta {
    pub info: Option<MbArtistInfo>,
    pub wiki: Option<WikiSummary>,
    /// Tidal's artist biography (via goodies), filled in after the lookup.
    #[serde(default)]
    pub tidal: Option<TidalText>,
}

pub struct MetadataState {
    pub mb: MusicBrainz,
    pub wiki: Wikipedia,
    pub fanart: Fanart,
}

impl MetadataState {
    pub fn new() -> Self {
        Self {
            mb: MusicBrainz::new(),
            wiki: Wikipedia::new(),
            fanart: Fanart::new(),
        }
    }
}

impl Default for MetadataState {
    fn default() -> Self { Self::new() }
}

impl MetadataState {
    pub async fn album(&self, artist: &str, album: &str) -> AlbumMeta {
        let release = self.mb.search_release(artist, album, None, None, None).await;
        let wiki = match release.as_ref().and_then(|r| r.wikipedia_slug.clone()) {
            Some(slug) => self.wiki.fetch_summary(&slug).await,
            None => self.wiki.search(&format!("{album} ({artist} album)"), "en").await,
        };
        AlbumMeta { release, wiki, tidal: None, track_credits: Vec::new() }
    }

    pub async fn artist(&self, name: &str) -> ArtistMeta {
        let info = self.mb.search_artist(name).await;
        let wiki = match info.as_ref().and_then(|i| i.wikipedia_slug.clone()) {
            Some(slug) => self.wiki.fetch_summary(&slug).await,
            None => self.wiki.search_artist(name).await,
        };
        ArtistMeta { info, wiki, tidal: None }
    }
}
