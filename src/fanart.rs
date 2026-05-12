//! Tiny fanart.tv client. Only used to fetch artist thumbs for the Info view.
//!
//! Endpoint: `GET https://webservice.fanart.tv/v3/music/{mbid}?api_key={key}`
//! Response: { "artistthumb": [{ "url": "..." }, ...], ... }
//! We just take the first thumb if any.

use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::sync::OnceCell;

const BASE_URL: &str = "https://webservice.fanart.tv/v3/music";
const USER_AGENT: &str = concat!("mopytui/", env!("CARGO_PKG_VERSION"));

#[derive(Deserialize)]
struct ArtistResponse {
    #[serde(default)]
    artistthumb: Option<Vec<FanartImage>>,
    #[serde(default)]
    artistbackground: Option<Vec<FanartImage>>,
}

#[derive(Deserialize)]
struct FanartImage {
    url: String,
}

pub struct Fanart {
    client: OnceCell<reqwest::Client>,
}

impl Fanart {
    pub fn new() -> Self {
        Self { client: OnceCell::new() }
    }

    async fn http(&self) -> &reqwest::Client {
        self.client
            .get_or_init(|| async {
                reqwest::Client::builder()
                    .user_agent(USER_AGENT)
                    .timeout(Duration::from_secs(15))
                    .build()
                    .expect("reqwest client")
            })
            .await
    }

    /// Returns the URL of the best artist thumbnail (square portrait) for the
    /// given MusicBrainz artist id. Falls back to background art when there's
    /// no thumb. Returns `None` on any 4xx/5xx or empty payload.
    pub async fn artist_image_url(&self, mbid: &str, api_key: &str) -> Option<String> {
        if mbid.is_empty() || api_key.is_empty() {
            return None;
        }
        let url = format!("{BASE_URL}/{mbid}?api_key={api_key}");
        let resp = self.http().await.get(&url).send().await.ok()?;
        if !resp.status().is_success() {
            tracing::debug!(
                target: "mopytui::fanart",
                "artist {mbid}: HTTP {}",
                resp.status()
            );
            return None;
        }
        let body: ArtistResponse = resp.json().await.ok()?;
        body.artistthumb
            .and_then(|v| v.into_iter().next().map(|x| x.url))
            .or_else(|| {
                body.artistbackground
                    .and_then(|v| v.into_iter().next().map(|x| x.url))
            })
    }

    /// Download raw image bytes from a fanart.tv image URL (or any HTTPS URL).
    pub async fn download_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let resp = self.http().await.get(url).send().await.context("fanart image")?;
        let status = resp.status();
        if !status.is_success() {
            anyhow::bail!("HTTP {status} fetching {url}");
        }
        Ok(resp.bytes().await.context("read image body")?.to_vec())
    }
}

impl Default for Fanart {
    fn default() -> Self { Self::new() }
}
