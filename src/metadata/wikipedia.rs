use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::cache;

const USER_AGENT: &str = concat!(
    "mopytui/",
    env!("CARGO_PKG_VERSION"),
    " ( https://github.com/anthropics/claude-code )"
);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiSummary {
    pub title: String,
    pub extract: String,
    pub thumbnail_url: Option<String>,
    pub original_image_url: Option<String>,
    pub page_url: String,
    pub language: String,
}

pub struct Wikipedia {
    client: tokio::sync::OnceCell<reqwest::Client>,
}

impl Wikipedia {
    pub fn new() -> Self {
        Self { client: tokio::sync::OnceCell::new() }
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

    /// Fetch summary by slug. Tries English, then Spanish.
    pub async fn fetch_summary(&self, slug: &str) -> Option<WikiSummary> {
        if let Some(s) = self.fetch_from_wiki("en", slug).await {
            return Some(s);
        }
        self.fetch_from_wiki("es", slug).await
    }

    pub async fn search_artist(&self, name: &str) -> Option<WikiSummary> {
        for q in artist_search_queries(name) {
            if let Some(r) = self.search(&q, "en").await {
                return Some(r);
            }
        }
        for q in artist_search_queries(name) {
            if let Some(r) = self.search(&q, "es").await {
                return Some(r);
            }
        }
        None
    }

    pub async fn search(&self, query: &str, lang: &str) -> Option<WikiSummary> {
        let cache_key = format!("wiki_search_{lang}_{}", query.to_lowercase());
        if let Some(bytes) = cache::get_default(&cache_key) {
            let cached = String::from_utf8_lossy(&bytes).to_string();
            if cached == "(none)" {
                return None;
            }
            return self.fetch_from_wiki(lang, &cached).await;
        }

        let slug = query.replace(' ', "_");
        if let Some(r) = self.fetch_from_wiki(lang, &slug).await {
            cache::set(&cache_key, slug.as_bytes());
            return Some(r);
        }
        if let Some(title) = self.search_title(query, lang).await {
            let fixed = title.replace(' ', "_");
            if let Some(r) = self.fetch_from_wiki(lang, &fixed).await {
                cache::set(&cache_key, fixed.as_bytes());
                return Some(r);
            }
        }
        cache::set(&cache_key, b"(none)");
        None
    }

    async fn search_title(&self, query: &str, lang: &str) -> Option<String> {
        let cache_key = format!("wiki_title_{lang}_{}", query.to_lowercase());
        if let Some(bytes) = cache::get_default(&cache_key) {
            let cached = String::from_utf8_lossy(&bytes).to_string();
            return if cached == "(none)" { None } else { Some(cached) };
        }
        let encoded = urlencode(query);
        let url = format!(
            "https://{lang}.wikipedia.org/w/api.php?action=query&list=search&srsearch={encoded}&format=json&srlimit=1"
        );
        let resp = self.http().await.get(&url).send().await.ok()?;
        if !resp.status().is_success() {
            cache::set(&cache_key, b"(none)");
            return None;
        }
        let json: serde_json::Value = resp.json().await.ok()?;
        let title = json
            .get("query")
            .and_then(|v| v.get("search"))
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|r| r.get("title"))
            .and_then(|v| v.as_str())
            .map(String::from);
        match title {
            Some(t) => {
                cache::set(&cache_key, t.as_bytes());
                Some(t)
            }
            None => {
                cache::set(&cache_key, b"(none)");
                None
            }
        }
    }

    async fn fetch_from_wiki(&self, lang: &str, slug: &str) -> Option<WikiSummary> {
        let decoded = urldecode(slug);
        let clean = decoded.replace(' ', "_");
        let encoded_path = urlencode_path(&clean);
        let url = format!("https://{lang}.wikipedia.org/api/rest_v1/page/summary/{encoded_path}");

        let cache_key = format!("wiki_{lang}_{url}");
        if let Some(bytes) = cache::get_default(&cache_key)
            && let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes)
        {
            return parse_summary(&json, lang);
        }

        let resp = self.http().await.get(&url).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let bytes = resp.bytes().await.ok()?;
        let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        let summary = parse_summary(&json, lang)?;
        cache::set(&cache_key, &bytes);
        Some(summary)
    }
}

impl Default for Wikipedia {
    fn default() -> Self { Self::new() }
}

fn parse_summary(json: &serde_json::Value, lang: &str) -> Option<WikiSummary> {
    if json.get("type").and_then(|v| v.as_str()) == Some("disambiguation") {
        return None;
    }
    let title = json.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let extract = json.get("extract").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if extract.is_empty() {
        return None;
    }
    let page_url = json
        .get("content_urls")
        .and_then(|v| v.get("desktop"))
        .and_then(|v| v.get("page"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let thumbnail_url = json
        .get("thumbnail")
        .and_then(|v| v.get("source"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let original_image_url = json
        .get("originalimage")
        .and_then(|v| v.get("source"))
        .and_then(|v| v.as_str())
        .map(String::from);

    Some(WikiSummary {
        title,
        extract,
        thumbnail_url,
        original_image_url,
        page_url,
        language: lang.to_string(),
    })
}

pub fn artist_search_queries(name: &str) -> Vec<String> {
    let mut out = vec![name.to_string()];
    let separators = [" & ", " and ", " with ", " feat. ", " feat ", " ft. ", " ft ", " + ", " y "];
    let lower = name.to_lowercase();
    for sep in &separators {
        if let Some(idx) = lower.find(&sep.to_lowercase()) {
            let first = name[..idx].trim();
            if !first.is_empty() {
                out.push(first.to_string());
            }
            break;
        }
    }
    if name.contains('/') {
        out.push(name.replace('/', " "));
    }
    out
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

fn urlencode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '~' | '(' | ')') {
            out.push(ch);
        } else {
            let mut buf = [0u8; 4];
            for b in ch.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}
