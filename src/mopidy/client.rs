use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::models::{
    Image, LibRef, Modes, PlayState, PlaybackSnapshot, Playlist, Ref, SearchResult, TlTrack, Track,
};

#[derive(Clone)]
pub struct Client {
    inner: reqwest::Client,
    base: Arc<String>,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct ClientError(pub String);

impl From<reqwest::Error> for ClientError {
    fn from(e: reqwest::Error) -> Self { Self(format!("http: {e}")) }
}
impl From<serde_json::Error> for ClientError {
    fn from(e: serde_json::Error) -> Self { Self(format!("json: {e}")) }
}

pub type Result<T> = std::result::Result<T, ClientError>;

#[derive(Serialize)]
struct RpcReq<'a> {
    jsonrpc: &'a str,
    id: u32,
    method: &'a str,
    params: Value,
}

#[derive(Deserialize)]
struct RpcResp {
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<Value>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct GoodiesFavorite {
    id: String,
}

#[allow(dead_code)]
impl Client {
    pub fn new(host: &str, port: u16) -> Self {
        let inner = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client build");
        Self { inner, base: Arc::new(format!("http://{host}:{port}")) }
    }

    pub fn base_url(&self) -> &str { &self.base }
    pub fn rpc_url(&self) -> String { format!("{}/mopidy/rpc", self.base) }

    fn goodies_url(&self, suffix: &str) -> String {
        format!("{}/tidal_goodies{suffix}", self.base)
    }

    pub fn image_url(&self, uri: &str) -> String {
        if uri.starts_with("http://") || uri.starts_with("https://") {
            uri.to_string()
        } else if uri.starts_with('/') {
            format!("{}{uri}", self.base)
        } else {
            format!("{}/{uri}", self.base)
        }
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let body = RpcReq { jsonrpc: "2.0", id: 1, method, params };
        let resp = self
            .inner
            .post(self.rpc_url())
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<RpcResp>()
            .await?;
        if let Some(err) = resp.error {
            return Err(ClientError(format!("rpc {method}: {err}")));
        }
        Ok(resp.result.unwrap_or(Value::Null))
    }

    // ─── library ────────────────────────────────────────────────────────────

    pub async fn search(&self, query: &str, uris: Option<Vec<String>>) -> Result<Vec<SearchResult>> {
        let v = self
            .call(
                "core.library.search",
                json!({ "query": { "any": [query] }, "uris": uris }),
            )
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn search_query(
        &self,
        query: HashMap<String, Vec<String>>,
        uris: Option<Vec<String>>,
        exact: bool,
    ) -> Result<Vec<SearchResult>> {
        let v = self
            .call(
                "core.library.search",
                json!({ "query": query, "uris": uris, "exact": exact }),
            )
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn browse(&self, uri: Option<String>) -> Result<Vec<LibRef>> {
        let v = self.call("core.library.browse", json!({ "uri": uri })).await?;
        let raw: Vec<Ref> = serde_json::from_value(v)?;
        Ok(raw.into_iter().map(Ref::into_lib).collect())
    }

    pub async fn lookup(&self, uris: Vec<String>) -> Result<HashMap<String, Vec<Track>>> {
        let v = self.call("core.library.lookup", json!({ "uris": uris })).await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn get_images(&self, uris: Vec<String>) -> Result<HashMap<String, Vec<Image>>> {
        let v = self
            .call("core.library.get_images", json!({ "uris": uris }))
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn get_distinct(&self, field: &str) -> Result<Vec<String>> {
        let v = self
            .call(
                "core.library.get_distinct",
                json!({ "field": field, "query": {} }),
            )
            .await?;
        Ok(serde_json::from_value(v).unwrap_or_default())
    }

    pub async fn library_refresh(&self, uri: Option<&str>) -> Result<()> {
        let params = match uri {
            Some(u) => json!({ "uri": u }),
            None => json!({}),
        };
        self.call("core.library.refresh", params).await?;
        Ok(())
    }

    // ─── tracklist ──────────────────────────────────────────────────────────

    pub async fn get_tl_tracks(&self) -> Result<Vec<TlTrack>> {
        let v = self.call("core.tracklist.get_tl_tracks", json!({})).await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn tracklist_add(
        &self,
        uris: Vec<String>,
        at_position: Option<u32>,
    ) -> Result<Vec<TlTrack>> {
        let v = self
            .call(
                "core.tracklist.add",
                json!({ "uris": uris, "at_position": at_position }),
            )
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn tracklist_clear(&self) -> Result<()> {
        self.call("core.tracklist.clear", json!({})).await?;
        Ok(())
    }

    pub async fn tracklist_index(&self, tlid: u32) -> Result<Option<u32>> {
        let v = self.call("core.tracklist.index", json!({ "tlid": tlid })).await?;
        Ok(serde_json::from_value(v).unwrap_or(None))
    }

    pub async fn tracklist_remove(&self, tlids: Vec<u32>) -> Result<Vec<TlTrack>> {
        let v = self
            .call(
                "core.tracklist.remove",
                json!({ "criteria": { "tlid": tlids } }),
            )
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn tracklist_move(&self, start: u32, end: u32, to_position: u32) -> Result<()> {
        self.call(
            "core.tracklist.move",
            json!({ "start": start, "end": end, "to_position": to_position }),
        )
        .await?;
        Ok(())
    }

    pub async fn tracklist_shuffle(&self) -> Result<()> {
        self.call("core.tracklist.shuffle", json!({})).await?;
        Ok(())
    }

    // ─── playback ───────────────────────────────────────────────────────────

    pub async fn playback_play(&self, tlid: Option<u32>) -> Result<()> {
        let params = if let Some(id) = tlid { json!({ "tlid": id }) } else { json!({}) };
        self.call("core.playback.play", params).await?;
        Ok(())
    }
    pub async fn playback_pause(&self) -> Result<()> {
        self.call("core.playback.pause", json!({})).await?;
        Ok(())
    }
    pub async fn playback_resume(&self) -> Result<()> {
        self.call("core.playback.resume", json!({})).await?;
        Ok(())
    }
    pub async fn playback_stop(&self) -> Result<()> {
        self.call("core.playback.stop", json!({})).await?;
        Ok(())
    }
    pub async fn playback_next(&self) -> Result<()> {
        self.call("core.playback.next", json!({})).await?;
        Ok(())
    }
    pub async fn playback_previous(&self) -> Result<()> {
        self.call("core.playback.previous", json!({})).await?;
        Ok(())
    }
    pub async fn playback_seek(&self, time_position_ms: i64) -> Result<()> {
        self.call("core.playback.seek", json!({ "time_position": time_position_ms }))
            .await?;
        Ok(())
    }

    // ─── mixer ──────────────────────────────────────────────────────────────

    pub async fn set_volume(&self, volume: u32) -> Result<()> {
        self.call("core.mixer.set_volume", json!({ "volume": volume.min(100) }))
            .await?;
        Ok(())
    }
    pub async fn toggle_mute(&self) -> Result<bool> {
        let curr = self.call("core.mixer.get_mute", json!({})).await?;
        let curr: Option<bool> = serde_json::from_value(curr).unwrap_or(None);
        let next = !curr.unwrap_or(false);
        self.call("core.mixer.set_mute", json!({ "mute": next })).await?;
        Ok(next)
    }

    // ─── modes (random / repeat / single / consume) ─────────────────────────

    pub async fn get_modes(&self) -> Result<Modes> {
        let (r, rep, s, c) = tokio::join!(
            self.call("core.tracklist.get_random", json!({})),
            self.call("core.tracklist.get_repeat", json!({})),
            self.call("core.tracklist.get_single", json!({})),
            self.call("core.tracklist.get_consume", json!({})),
        );
        Ok(Modes {
            random: r.ok().and_then(|v| serde_json::from_value(v).ok()).unwrap_or(false),
            repeat: rep.ok().and_then(|v| serde_json::from_value(v).ok()).unwrap_or(false),
            single: s.ok().and_then(|v| serde_json::from_value(v).ok()).unwrap_or(false),
            consume: c.ok().and_then(|v| serde_json::from_value(v).ok()).unwrap_or(false),
        })
    }
    pub async fn set_random(&self, on: bool) -> Result<()> {
        self.call("core.tracklist.set_random", json!({ "value": on })).await?;
        Ok(())
    }
    pub async fn set_repeat(&self, on: bool) -> Result<()> {
        self.call("core.tracklist.set_repeat", json!({ "value": on })).await?;
        Ok(())
    }
    pub async fn set_single(&self, on: bool) -> Result<()> {
        self.call("core.tracklist.set_single", json!({ "value": on })).await?;
        Ok(())
    }
    pub async fn set_consume(&self, on: bool) -> Result<()> {
        self.call("core.tracklist.set_consume", json!({ "value": on })).await?;
        Ok(())
    }

    // ─── playlists ──────────────────────────────────────────────────────────

    pub async fn playlists_as_list(&self) -> Result<Vec<LibRef>> {
        let v = self.call("core.playlists.as_list", json!({})).await?;
        let raw: Vec<Ref> = serde_json::from_value(v)?;
        Ok(raw.into_iter().map(Ref::into_lib).collect())
    }

    pub async fn playlist_lookup(&self, uri: &str) -> Result<Option<Playlist>> {
        let v = self.call("core.playlists.lookup", json!({ "uri": uri })).await?;
        if v.is_null() { return Ok(None); }
        Ok(serde_json::from_value(v)?)
    }

    pub async fn playlist_save(&self, playlist: &Playlist) -> Result<Option<Playlist>> {
        let v = self
            .call("core.playlists.save", json!({ "playlist": playlist }))
            .await?;
        if v.is_null() { return Ok(None); }
        Ok(serde_json::from_value(v)?)
    }

    pub async fn playlist_create(&self, name: &str, scheme: Option<&str>) -> Result<Option<Playlist>> {
        let params = match scheme {
            Some(s) => json!({ "name": name, "uri_scheme": s }),
            None => json!({ "name": name }),
        };
        let v = self.call("core.playlists.create", params).await?;
        if v.is_null() { return Ok(None); }
        Ok(serde_json::from_value(v)?)
    }

    pub async fn playlist_delete(&self, uri: &str) -> Result<bool> {
        let v = self.call("core.playlists.delete", json!({ "uri": uri })).await?;
        Ok(serde_json::from_value(v).unwrap_or(false))
    }

    pub async fn playlist_uri_schemes(&self) -> Result<Vec<String>> {
        let v = self.call("core.playlists.get_uri_schemes", json!({})).await?;
        Ok(serde_json::from_value(v).unwrap_or_default())
    }

    // ─── aggregated state ───────────────────────────────────────────────────

    pub async fn fetch_playback(&self) -> Result<PlaybackSnapshot> {
        let (state, tl_track, pos, vol) = tokio::join!(
            self.call("core.playback.get_state", json!({})),
            self.call("core.playback.get_current_tl_track", json!({})),
            self.call("core.playback.get_time_position", json!({})),
            self.call("core.mixer.get_volume", json!({})),
        );
        let state_str: String = state
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_else(|| "stopped".to_string());
        let (current, current_tlid) = match tl_track {
            Ok(v) if !v.is_null() => {
                let tlid = v.get("tlid").and_then(|x| x.as_u64()).map(|x| x as u32);
                let track: Option<Track> =
                    v.get("track").cloned().and_then(|t| serde_json::from_value(t).ok());
                (track, tlid)
            }
            _ => (None, None),
        };
        let elapsed_ms = pos.ok().and_then(|v| serde_json::from_value::<i64>(v).ok()).unwrap_or(0);
        let volume = vol
            .ok()
            .and_then(|v| serde_json::from_value::<Option<i32>>(v).ok())
            .flatten()
            .unwrap_or(-1);
        Ok(PlaybackSnapshot {
            state: PlayState::from_str(&state_str),
            current,
            current_tlid,
            elapsed_ms,
            volume,
        })
    }

    pub async fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>> {
        Ok(self
            .inner
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .to_vec())
    }

    // ─── tidal-goodies (optional plugin) ────────────────────────────────────

    pub async fn goodies_health(&self) -> Result<Option<Value>> {
        let resp = self.inner.get(self.goodies_url("/_health")).send().await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        Ok(Some(resp.error_for_status()?.json().await?))
    }

    pub async fn goodies_favorite_album_ids(&self) -> Result<Option<Vec<String>>> {
        let resp = self
            .inner
            .get(self.goodies_url("/favorites/albums"))
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let items: Vec<GoodiesFavorite> = resp.error_for_status()?.json().await?;
        Ok(Some(items.into_iter().map(|i| i.id).collect()))
    }

    pub async fn goodies_set_album_favorite(&self, id: &str, favorited: bool) -> Result<bool> {
        let resp = if favorited {
            self.inner
                .post(self.goodies_url("/favorites/albums"))
                .json(&json!({ "id": id }))
                .send()
                .await?
        } else {
            self.inner
                .delete(self.goodies_url(&format!("/favorites/albums/{id}")))
                .send()
                .await?
        };
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(false);
        }
        resp.error_for_status()?;
        Ok(true)
    }

    pub async fn goodies_stats_recent(&self, limit: u32) -> Result<Value> {
        let url = format!("{}?limit={limit}", self.goodies_url("/stats/recent"));
        Ok(self.inner.get(url).send().await?.error_for_status()?.json().await?)
    }

    pub async fn goodies_stats_most_played(&self, limit: u32, since: Option<i64>) -> Result<Value> {
        let mut url = format!("{}?limit={limit}", self.goodies_url("/stats/most-played"));
        if let Some(s) = since { url.push_str(&format!("&since={s}")); }
        Ok(self.inner.get(url).send().await?.error_for_status()?.json().await?)
    }

    pub async fn goodies_stats_top_artists(&self, limit: u32, since: Option<i64>) -> Result<Value> {
        let mut url = format!("{}?limit={limit}", self.goodies_url("/stats/top-artists"));
        if let Some(s) = since { url.push_str(&format!("&since={s}")); }
        Ok(self.inner.get(url).send().await?.error_for_status()?.json().await?)
    }

    pub async fn goodies_stats_top_albums(&self, limit: u32, since: Option<i64>) -> Result<Value> {
        let mut url = format!("{}?limit={limit}", self.goodies_url("/stats/top-albums"));
        if let Some(s) = since { url.push_str(&format!("&since={s}")); }
        Ok(self.inner.get(url).send().await?.error_for_status()?.json().await?)
    }

    pub async fn goodies_stats_by_genre(&self, limit: u32, since: Option<i64>) -> Result<Value> {
        let mut url = format!("{}?limit={limit}", self.goodies_url("/stats/by-genre"));
        if let Some(s) = since { url.push_str(&format!("&since={s}")); }
        Ok(self.inner.get(url).send().await?.error_for_status()?.json().await?)
    }

    pub async fn goodies_stats_by_day_of_week(&self) -> Result<Value> {
        Ok(self
            .inner
            .get(self.goodies_url("/stats/by-day-of-week"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn goodies_stats_by_hour(&self) -> Result<Value> {
        Ok(self
            .inner
            .get(self.goodies_url("/stats/by-hour"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn goodies_stats_totals(&self) -> Result<Value> {
        Ok(self
            .inner
            .get(self.goodies_url("/stats/totals"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
}
