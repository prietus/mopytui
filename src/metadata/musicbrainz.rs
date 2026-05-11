use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::time::{Instant, sleep_until};

use super::cache;

const BASE_URL: &str = "https://musicbrainz.org/ws/2";
const USER_AGENT: &str = concat!(
    "mopytui/",
    env!("CARGO_PKG_VERSION"),
    " ( https://github.com/anthropics/claude-code )"
);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credit {
    pub name: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MbRelease {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub date: String,
    pub country: String,
    pub label: String,
    pub catalog_number: String,
    pub barcode: String,
    pub status: String,
    pub credits: Vec<Credit>,
    pub wikipedia_slug: Option<String>,
    pub genres: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub name: String,
    pub role: String,
    pub period: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MbArtistInfo {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub begin_date: String,
    pub end_date: String,
    pub area: String,
    pub wikipedia_slug: Option<String>,
    pub members: Vec<Member>,
}

/// MusicBrainz client with 1 req/sec rate limiting (per their TOS).
pub struct MusicBrainz {
    client: tokio::sync::OnceCell<reqwest::Client>,
    next_slot: Mutex<Instant>,
}

impl MusicBrainz {
    pub fn new() -> Self {
        Self {
            client: tokio::sync::OnceCell::new(),
            next_slot: Mutex::new(Instant::now()),
        }
    }

    async fn http(&self) -> &reqwest::Client {
        self.client
            .get_or_init(|| async {
                reqwest::Client::builder()
                    .user_agent(USER_AGENT)
                    .timeout(Duration::from_secs(15))
                    .hickory_dns(true)
                    .build()
                    .expect("reqwest client")
            })
            .await
    }

    async fn throttle(&self) {
        let mut slot = self.next_slot.lock().await;
        let now = Instant::now();
        if *slot > now {
            let until = *slot;
            drop(slot);
            sleep_until(until).await;
            slot = self.next_slot.lock().await;
        }
        *slot = Instant::now() + Duration::from_millis(1100);
    }

    async fn fetch_json(&self, url: &str) -> Option<serde_json::Value> {
        self.throttle().await;
        let resp = self.http().await.get(url).header("Accept", "application/json").send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        resp.json::<serde_json::Value>().await.ok()
    }

    pub async fn search_release(
        &self,
        artist: &str,
        album: &str,
        format: Option<&str>,
        country: Option<&str>,
        label: Option<&str>,
    ) -> Option<MbRelease> {
        let hint_suffix: String = [
            format.map(|f| format!("fmt_{f}")),
            country.map(|c| format!("cc_{c}")),
            label.map(|l| format!("lbl_{l}")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("_")
        .to_lowercase();
        let search_key = if hint_suffix.is_empty() {
            format!("mb_search_release_{}_{}", artist.to_lowercase(), album.to_lowercase())
        } else {
            format!(
                "mb_search_release_{}_{}_{}",
                artist.to_lowercase(),
                album.to_lowercase(),
                hint_suffix
            )
        };

        if let Some(bytes) = cache::get_default(&search_key) {
            let cached = String::from_utf8_lossy(&bytes).to_string();
            if cached == "(none)" {
                return None;
            }
            return self.fetch_release(&cached).await;
        }

        let mut query = format!("release:{album} AND artist:{artist}");
        if let Some(l) = label
            && !l.is_empty()
        {
            query.push_str(&format!(" AND label:{l}"));
        }
        let encoded = urlencode(&query);
        let has_hints = format.is_some() || country.is_some();
        let limit = if has_hints { 25 } else { 5 };
        let url = format!("{BASE_URL}/release/?query={encoded}&fmt=json&limit={limit}");

        let json = self.fetch_json(&url).await?;
        let releases = json.get("releases").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        if releases.is_empty() {
            cache::set(&search_key, b"(none)");
            return None;
        }

        let default_id = releases.first().and_then(|r| r.get("id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let mut release_id = default_id.clone();

        if has_hints {
            let format_lower = format.map(|s| s.to_lowercase());
            let country_upper = country.map(|s| s.to_uppercase());
            for rel in &releases {
                let rel_country = rel.get("country").and_then(|v| v.as_str()).unwrap_or("").to_uppercase();
                let media = rel.get("media").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                let media_formats: Vec<String> = media
                    .iter()
                    .filter_map(|m| m.get("format").and_then(|v| v.as_str()).map(|s| s.to_lowercase()))
                    .collect();
                let format_match = match &format_lower {
                    Some(f) => media_formats.iter().any(|m| m.contains(f)),
                    None => true,
                };
                let country_match = match &country_upper {
                    Some(c) => &rel_country == c,
                    None => true,
                };
                if format_match && country_match
                    && let Some(id) = rel.get("id").and_then(|v| v.as_str())
                {
                    release_id = id.to_string();
                    break;
                }
            }
        }

        cache::set(&search_key, release_id.as_bytes());
        self.fetch_release(&release_id).await
    }

    pub async fn fetch_release(&self, id: &str) -> Option<MbRelease> {
        let cache_key = format!("mb_release_{id}");
        if let Some(bytes) = cache::get_default(&cache_key)
            && let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes)
        {
            return parse_release(id, &json);
        }
        let url = format!(
            "{BASE_URL}/release/{id}?inc=artist-credits+labels+recordings+release-groups+url-rels+artist-rels+recording-level-rels+genres&fmt=json"
        );
        let json = self.fetch_json(&url).await?;
        if let Ok(bytes) = serde_json::to_vec(&json) {
            cache::set(&cache_key, &bytes);
        }
        parse_release(id, &json)
    }

    pub async fn search_artist(&self, name: &str) -> Option<MbArtistInfo> {
        let search_key = format!("mb_search_artist_{}", name.to_lowercase());
        if let Some(bytes) = cache::get_default(&search_key) {
            let cached = String::from_utf8_lossy(&bytes).to_string();
            if cached == "(none)" {
                return None;
            }
            return self.fetch_artist(&cached).await;
        }
        let encoded = urlencode(name);
        let url = format!("{BASE_URL}/artist/?query=artist:{encoded}&fmt=json&limit=3");
        let json = self.fetch_json(&url).await?;
        let artist_id = json
            .get("artists")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|a| a.get("id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        match artist_id {
            Some(id) => {
                cache::set(&search_key, id.as_bytes());
                self.fetch_artist(&id).await
            }
            None => {
                cache::set(&search_key, b"(none)");
                None
            }
        }
    }

    pub async fn fetch_artist(&self, id: &str) -> Option<MbArtistInfo> {
        let cache_key = format!("mb_artist_{id}");
        if let Some(bytes) = cache::get_default(&cache_key)
            && let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes)
        {
            return parse_artist(id, &json);
        }
        let url = format!("{BASE_URL}/artist/{id}?inc=url-rels+artist-rels+genres&fmt=json");
        let json = self.fetch_json(&url).await?;
        if let Ok(bytes) = serde_json::to_vec(&json) {
            cache::set(&cache_key, &bytes);
        }
        parse_artist(id, &json)
    }
}

impl Default for MusicBrainz {
    fn default() -> Self { Self::new() }
}

fn parse_release(id: &str, json: &serde_json::Value) -> Option<MbRelease> {
    let title = str_field(json, "title");
    let date = str_field(json, "date");
    let country = str_field(json, "country");
    let status = str_field(json, "status");
    let barcode = str_field(json, "barcode");

    let artist = json
        .get("artist-credit")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.get("name").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();

    let mut label = String::new();
    let mut catalog_number = String::new();
    if let Some(first) = json.get("label-info").and_then(|v| v.as_array()).and_then(|a| a.first()) {
        if let Some(lbl) = first.get("label").and_then(|v| v.as_object())
            && let Some(n) = lbl.get("name").and_then(|v| v.as_str())
        {
            label = n.to_string();
        }
        if let Some(c) = first.get("catalog-number").and_then(|v| v.as_str()) {
            catalog_number = c.to_string();
        }
    }

    let mut credits: Vec<Credit> = Vec::new();
    collect_credits_from_relations(json.get("relations"), &mut credits);
    if let Some(media) = json.get("media").and_then(|v| v.as_array()) {
        for medium in media {
            if let Some(tracks) = medium.get("tracks").and_then(|v| v.as_array()) {
                for track in tracks {
                    if let Some(recording) = track.get("recording") {
                        collect_credits_from_relations(recording.get("relations"), &mut credits);
                    }
                }
            }
        }
    }

    let mut wiki_slug = None;
    if let Some(rg) = json.get("release-group") {
        wiki_slug = wiki_slug_from_relations(rg.get("relations"));
    }
    if wiki_slug.is_none() {
        wiki_slug = wiki_slug_from_relations(json.get("relations"));
    }

    let genres = json
        .get("release-group")
        .and_then(|v| v.get("genres"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|g| g.get("name").and_then(|v| v.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Some(MbRelease {
        id: id.to_string(),
        title,
        artist,
        date,
        country,
        label,
        catalog_number,
        barcode,
        status,
        credits,
        wikipedia_slug: wiki_slug,
        genres,
    })
}

fn collect_credits_from_relations(relations: Option<&serde_json::Value>, out: &mut Vec<Credit>) {
    let Some(arr) = relations.and_then(|v| v.as_array()) else { return };
    for rel in arr {
        let kind = rel.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let Some(artist_obj) = rel.get("artist") else { continue };
        let Some(name) = artist_obj.get("name").and_then(|v| v.as_str()) else { continue };
        let attrs: Vec<String> = rel
            .get("attributes")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str())
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        let role = if attrs.is_empty() {
            kind.to_string()
        } else {
            format!("{kind} ({})", attrs.join(", "))
        };
        if !out.iter().any(|c| c.name == name && c.role == role) {
            out.push(Credit { name: name.to_string(), role });
        }
    }
}

fn wiki_slug_from_relations(relations: Option<&serde_json::Value>) -> Option<String> {
    let arr = relations?.as_array()?;
    for rel in arr {
        if let Some(resource) = rel.get("url").and_then(|u| u.get("resource")).and_then(|v| v.as_str())
            && resource.contains("wikipedia.org")
            && let Some((_, slug)) = resource.split_once("/wiki/")
        {
            return Some(slug.to_string());
        }
    }
    None
}

fn parse_artist(id: &str, json: &serde_json::Value) -> Option<MbArtistInfo> {
    let name = str_field(json, "name");
    let kind = str_field(json, "type");
    let begin_date = json.get("life-span").and_then(|v| v.get("begin")).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let end_date = json.get("life-span").and_then(|v| v.get("end")).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let area = json.get("area").and_then(|v| v.get("name")).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let wikipedia_slug = wiki_slug_from_relations(json.get("relations"));

    let mut members: Vec<Member> = Vec::new();
    let mut keys = std::collections::HashSet::new();
    if let Some(arr) = json.get("relations").and_then(|v| v.as_array()) {
        for rel in arr {
            if rel.get("type").and_then(|v| v.as_str()) != Some("member of band") { continue; }
            let Some(name) = rel.get("artist").and_then(|a| a.get("name")).and_then(|v| v.as_str()) else { continue };
            let attrs = rel
                .get("attributes")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "))
                .unwrap_or_default();
            let begin = rel.get("begin").and_then(|v| v.as_str()).unwrap_or("");
            let end = rel.get("end").and_then(|v| v.as_str()).unwrap_or("");
            let period = if begin.is_empty() {
                String::new()
            } else if end.is_empty() {
                format!("{begin}–present")
            } else {
                format!("{begin}–{end}")
            };
            let key = format!("{name}|{period}");
            if keys.insert(key) {
                members.push(Member { name: name.to_string(), role: attrs, period });
            }
        }
    }

    Some(MbArtistInfo {
        id: id.to_string(),
        name,
        kind,
        begin_date,
        end_date,
        area,
        wikipedia_slug,
        members,
    })
}

fn str_field(json: &serde_json::Value, key: &str) -> String {
    json.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
