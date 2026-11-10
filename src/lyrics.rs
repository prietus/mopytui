use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::Result;
use serde::Deserialize;

const BASE_URL: &str = "https://lrclib.net/api";
const USER_AGENT: &str = concat!("mopytui/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Default)]
pub struct ParsedLyrics {
    pub synced: Vec<(i64, String)>,  // (ms, line)
    pub plain: Option<String>,
    pub instrumental: bool,
    /// Where they came from, for the panel title: "lrclib", "Tidal (MUSIXMATCH)"…
    pub source: Option<String>,
}

/// Where to ask for Tidal's lyrics when lrclib has none (or only unsynced
/// text): mopidy-goodies on the Mopidy server, by the track's Mopidy URI.
pub struct TidalSource {
    pub client: crate::mopidy::Client,
    pub uri: String,
}

/// Lyrics from goodies' `/tidal/lyrics` payload: `subtitles` is LRC (line
/// timestamps), `text` the plain version.
pub fn from_tidal(text: Option<&str>, subtitles: Option<&str>, provider: Option<&str>) -> ParsedLyrics {
    ParsedLyrics {
        synced: subtitles.map(parse_synced).unwrap_or_default(),
        plain: text.map(str::trim).filter(|s| !s.is_empty()).map(String::from),
        instrumental: false,
        source: Some(match provider.filter(|p| !p.is_empty()) {
            Some(p) => format!("Tidal ({p})"),
            None => "Tidal".to_string(),
        }),
    }
}

/// Whether `candidate` is worth showing instead of `current`: anything beats
/// nothing, and line-synced lyrics beat plain text.
fn is_better(candidate: &ParsedLyrics, current: Option<&ParsedLyrics>) -> bool {
    match current {
        None => candidate.has_text(),
        Some(cur) => candidate.has_synced() && !cur.has_synced(),
    }
}

impl ParsedLyrics {
    pub fn has_synced(&self) -> bool { !self.synced.is_empty() }
    pub fn has_text(&self) -> bool {
        self.has_synced() || self.plain.as_deref().is_some_and(|s| !s.is_empty())
    }
    /// Index of the line whose timestamp is the latest <= `elapsed_ms`.
    pub fn current_line(&self, elapsed_ms: i64) -> Option<usize> {
        if self.synced.is_empty() { return None; }
        let mut hit = None;
        for (i, (ts, _)) in self.synced.iter().enumerate() {
            if *ts <= elapsed_ms { hit = Some(i); } else { break; }
        }
        hit
    }
}

#[derive(Deserialize)]
struct LrclibResp {
    #[serde(default, rename = "plainLyrics")]
    plain_lyrics: Option<String>,
    #[serde(default, rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
    #[serde(default)]
    instrumental: bool,
}

impl LrclibResp {
    fn parse(self) -> ParsedLyrics {
        ParsedLyrics {
            synced: self.synced_lyrics.as_deref().map(parse_synced).unwrap_or_default(),
            plain: self.plain_lyrics.filter(|s| !s.is_empty()),
            instrumental: self.instrumental,
            source: Some("lrclib".to_string()),
        }
    }
}

/// Parse `[mm:ss.xx] text` lines (lrclib synced format). Multiple timestamps
/// per line are supported (the same text appears at each timestamp).
fn parse_synced(s: &str) -> Vec<(i64, String)> {
    let mut out: Vec<(i64, String)> = Vec::new();
    for raw in s.lines() {
        let line = raw.trim_end();
        let mut rest = line;
        let mut stamps: Vec<i64> = Vec::new();
        while let Some(stripped) = rest.strip_prefix('[') {
            // Find matching ']'
            let Some(end) = stripped.find(']') else { break };
            let inner = &stripped[..end];
            if let Some(ms) = parse_stamp(inner) {
                stamps.push(ms);
            } else {
                // Could be metadata like `[ar: ...]`; skip silently.
            }
            rest = &stripped[end + 1..];
        }
        let text = rest.trim().to_string();
        for ms in stamps {
            out.push((ms, text.clone()));
        }
    }
    out.sort_by_key(|(t, _)| *t);
    out
}

fn parse_stamp(s: &str) -> Option<i64> {
    let mut it = s.splitn(2, ':');
    let m: i64 = it.next()?.parse().ok()?;
    let rest = it.next()?;
    let (sec, frac) = match rest.split_once('.') {
        Some((s, f)) => (s, f),
        None => (rest, "0"),
    };
    let sec: i64 = sec.parse().ok()?;
    // Frac is hundredths in most lrclib payloads; treat as fractional second.
    let frac_ms: i64 = match frac.len() {
        1 => frac.parse::<i64>().ok()? * 100,
        2 => frac.parse::<i64>().ok()? * 10,
        3 => frac.parse::<i64>().ok()?,
        _ => frac.parse::<i64>().ok()? * 10,
    };
    Some(m * 60_000 + sec * 1000 + frac_ms)
}

#[derive(Clone)]
pub struct LyricsCache {
    inner: Arc<Mutex<HashMap<String, Option<Arc<ParsedLyrics>>>>>,
    client: reqwest::Client,
}

impl Default for LyricsCache {
    fn default() -> Self { Self::new() }
}

impl LyricsCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            client: reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .timeout(Duration::from_secs(20))
                .build()
                .expect("reqwest build"),
        }
    }

    pub fn get(&self, key: &str) -> Option<Option<Arc<ParsedLyrics>>> {
        self.inner.lock().unwrap().get(key).cloned()
    }
    fn put(&self, key: String, value: Option<Arc<ParsedLyrics>>) {
        self.inner.lock().unwrap().insert(key, value);
    }
}

pub fn cache_key(artist: &str, title: &str, album: &str, duration_ms: i64) -> String {
    let duration_s = (duration_ms / 1000).max(0);
    format!(
        "{}|{}|{}|{}",
        artist.to_lowercase(),
        title.to_lowercase(),
        album.to_lowercase(),
        duration_s,
    )
}

pub async fn fetch_for(
    cache: &LyricsCache,
    artist: &str,
    title: &str,
    album: &str,
    duration_ms: i64,
    tidal: Option<TidalSource>,
) -> Result<Option<Arc<ParsedLyrics>>> {
    if artist.trim().is_empty() || title.trim().is_empty() {
        return Ok(None);
    }
    let key = cache_key(artist, title, album, duration_ms);
    if let Some(c) = cache.get(&key) {
        return Ok(c);
    }
    let duration_s = (duration_ms / 1000).max(0);
    tracing::debug!(target: "mopytui::lyrics", "lrclib lookup: {artist} — {title} ({album}, {duration_s}s)");

    // lrclib is intermittently slow/unreachable from some networks. Try up to
    // 3 times with backoff; only commit a `None` result to the cache when
    // both `/get` and `/search` came back with a confirmed empty response
    // (so a transient network failure doesn't poison the UI as "not found"
    // for the rest of the session).
    let mut result: Option<Arc<ParsedLyrics>> = None;
    for attempt in 1..=3u32 {
        let exact = fetch_exact(&cache.client, artist, title, album, duration_s).await;
        if let Err(e) = &exact {
            tracing::warn!(target: "mopytui::lyrics", "attempt {attempt}: /get failed: {e:#}");
        }
        if let Ok(Some(p)) = &exact {
            result = Some(Arc::new(p.clone()));
            break;
        }

        let search = fetch_search(&cache.client, artist, title).await;
        if let Err(e) = &search {
            tracing::warn!(target: "mopytui::lyrics", "attempt {attempt}: /search failed: {e:#}");
        }
        if let Ok(Some(p)) = &search {
            result = Some(Arc::new(p.clone()));
            break;
        }

        // Both endpoints answered cleanly with "no result" → it really isn't
        // on lrclib. Stop retrying.
        if exact.is_ok() && search.is_ok() {
            break;
        }

        // Transient: back off and retry.
        if attempt < 3 {
            tokio::time::sleep(Duration::from_secs((attempt as u64) * 2)).await;
        }
    }

    // Tidal's own lyrics as a second source, when lrclib has nothing or only
    // unsynced text.
    if result.as_deref().is_none_or(|p| !p.has_synced())
        && let Some(src) = tidal
    {
        match src.client.goodies_lyrics(&src.uri).await {
            Ok(Some(t)) if is_better(&t, result.as_deref()) => {
                tracing::debug!(target: "mopytui::lyrics", "using Tidal lyrics (synced: {})", t.has_synced());
                result = Some(Arc::new(t));
            }
            Ok(_) => tracing::debug!(target: "mopytui::lyrics", "no (better) lyrics on Tidal"),
            Err(e) => tracing::debug!(target: "mopytui::lyrics", "Tidal lyrics: {}", e.0),
        }
    }

    match &result {
        Some(p) if p.has_synced() => tracing::debug!(target: "mopytui::lyrics", "synced lyrics found"),
        Some(p) if p.instrumental => tracing::debug!(target: "mopytui::lyrics", "instrumental"),
        Some(_) => tracing::debug!(target: "mopytui::lyrics", "plain lyrics found"),
        None => tracing::debug!(target: "mopytui::lyrics", "no lyrics found (after retries)"),
    }
    cache.put(key, result.clone());
    Ok(result)
}

async fn fetch_exact(
    client: &reqwest::Client,
    artist: &str,
    title: &str,
    album: &str,
    duration_s: i64,
) -> Result<Option<ParsedLyrics>> {
    let url = format!("{BASE_URL}/get");
    let resp = client
        .get(&url)
        .query(&[
            ("artist_name", artist),
            ("track_name", title),
            ("album_name", album),
            ("duration", &duration_s.to_string()),
        ])
        .send()
        .await?;
    let status = resp.status();
    tracing::debug!(target: "mopytui::lyrics", "lrclib /get → HTTP {status}");
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !status.is_success() {
        return Ok(None);
    }
    let body: LrclibResp = resp.json().await?;
    Ok(Some(body.parse()))
}

async fn fetch_search(
    client: &reqwest::Client,
    artist: &str,
    title: &str,
) -> Result<Option<ParsedLyrics>> {
    let url = format!("{BASE_URL}/search");
    let resp = client
        .get(&url)
        .query(&[("track_name", title), ("artist_name", artist)])
        .send()
        .await?;
    let status = resp.status();
    tracing::debug!(target: "mopytui::lyrics", "lrclib /search → HTTP {status}");
    if !status.is_success() { return Ok(None); }
    let arr: Vec<LrclibResp> = resp.json().await?;
    Ok(arr.into_iter().next().map(|r| r.parse()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidal_subtitles_become_synced_lines_and_carry_the_provider() {
        let l = from_tidal(
            Some("Is this the real life?\nIs this just fantasy?"),
            Some("[00:00.50] Is this the real life?\n[00:04.10] Is this just fantasy?"),
            Some("MUSIXMATCH"),
        );
        assert_eq!(l.synced.len(), 2);
        assert_eq!(l.synced[1].0, 4100);
        assert_eq!(l.synced[1].1, "Is this just fantasy?");
        assert_eq!(l.source.as_deref(), Some("Tidal (MUSIXMATCH)"));
        assert_eq!(l.current_line(4200), Some(1));
        // Plain-only payload.
        let plain = from_tidal(Some(" just text "), None, None);
        assert!(!plain.has_synced() && plain.has_text());
        assert_eq!(plain.source.as_deref(), Some("Tidal"));
        assert!(!from_tidal(None, None, None).has_text());
    }

    #[test]
    fn tidal_replaces_lrclib_only_when_it_adds_something() {
        let synced = from_tidal(Some("a"), Some("[00:01.00] a"), None);
        let plain = from_tidal(Some("a"), None, None);
        let empty = from_tidal(None, None, None);
        // Nothing from lrclib: any text wins, an empty answer doesn't.
        assert!(is_better(&plain, None) && is_better(&synced, None));
        assert!(!is_better(&empty, None));
        // lrclib had plain text only: Tidal's synced lines win, plain doesn't.
        assert!(is_better(&synced, Some(&plain)));
        assert!(!is_better(&plain, Some(&plain)));
        // lrclib already synced: keep it.
        assert!(!is_better(&synced, Some(&synced)));
    }
}
