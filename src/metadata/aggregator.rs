use serde::{Deserialize, Serialize};

use super::musicbrainz::{MbArtistInfo, MbRelease, MusicBrainz};
use super::wikipedia::{WikiSummary, Wikipedia};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumMeta {
    pub release: Option<MbRelease>,
    pub wiki: Option<WikiSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistMeta {
    pub info: Option<MbArtistInfo>,
    pub wiki: Option<WikiSummary>,
}

pub struct MetadataState {
    pub mb: MusicBrainz,
    pub wiki: Wikipedia,
}

impl MetadataState {
    pub fn new() -> Self {
        Self { mb: MusicBrainz::new(), wiki: Wikipedia::new() }
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
        AlbumMeta { release, wiki }
    }

    pub async fn artist(&self, name: &str) -> ArtistMeta {
        let info = self.mb.search_artist(name).await;
        let wiki = match info.as_ref().and_then(|i| i.wikipedia_slug.clone()) {
            Some(slug) => self.wiki.fetch_summary(&slug).await,
            None => self.wiki.search_artist(name).await,
        };
        ArtistMeta { info, wiki }
    }
}
