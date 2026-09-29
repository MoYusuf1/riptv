//! Xtream Codes API client. Native and wasm32 (reqwest uses `fetch` there).
//!
//! ponytail: no vod_info or EPG yet (add when the UI needs them). No request timeout:
//! reqwest can't set one on wasm.

pub use reqwest::Url;
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("bad server url: {0}")]
    BadUrl(String),
    // No `#[from]`: reqwest errors embed the request URL, which carries the password.
    // Every conversion goes through `without_url()` in `get_json`.
    #[error("request failed: {0}")]
    Http(reqwest::Error),
    #[error("unexpected response shape: {0}")]
    Json(#[from] serde_json::Error),
    #[error("login rejected by server")]
    AuthFailed,
}

pub type Result<T> = std::result::Result<T, Error>;

// Providers mix strings, numbers, bools and nulls for the same field. Accept all of them.
fn flex_string<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<String>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::String(s) => Some(s),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    })
}

fn flex_str<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    Ok(flex_string(d)?.unwrap_or_default())
}

fn flex_u64<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<u64>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(b as u64),
        _ => None,
    })
}

fn flex_id<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<u64, D::Error> {
    flex_u64(d)?.ok_or_else(|| serde::de::Error::custom("expected a numeric id"))
}

#[derive(Debug, Clone, Deserialize)]
pub struct Category {
    #[serde(deserialize_with = "flex_id")]
    pub category_id: u64,
    #[serde(default, deserialize_with = "flex_str")]
    pub category_name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LiveStream {
    #[serde(deserialize_with = "flex_id")]
    pub stream_id: u64,
    #[serde(default, deserialize_with = "flex_str")]
    pub name: String,
    #[serde(default, deserialize_with = "flex_string")]
    pub stream_icon: Option<String>,
    #[serde(default, deserialize_with = "flex_string")]
    pub epg_channel_id: Option<String>,
    #[serde(default, deserialize_with = "flex_u64")]
    pub category_id: Option<u64>,
    #[serde(default, deserialize_with = "flex_u64")]
    pub tv_archive: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VodStream {
    #[serde(deserialize_with = "flex_id")]
    pub stream_id: u64,
    #[serde(default, deserialize_with = "flex_str")]
    pub name: String,
    #[serde(default, deserialize_with = "flex_string")]
    pub stream_icon: Option<String>,
    #[serde(default, deserialize_with = "flex_string")]
    pub rating: Option<String>,
    #[serde(default, deserialize_with = "flex_string")]
    pub container_extension: Option<String>,
    #[serde(default, deserialize_with = "flex_u64")]
    pub category_id: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Series {
    #[serde(deserialize_with = "flex_id")]
    pub series_id: u64,
    #[serde(default, deserialize_with = "flex_str")]
    pub name: String,
    #[serde(default, deserialize_with = "flex_string")]
    pub cover: Option<String>,
    #[serde(default, deserialize_with = "flex_string")]
    pub plot: Option<String>,
    #[serde(default, deserialize_with = "flex_string")]
    pub rating: Option<String>,
    #[serde(default, deserialize_with = "flex_u64")]
    pub category_id: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Episode {
    #[serde(deserialize_with = "flex_id")]
    pub id: u64,
    #[serde(default, deserialize_with = "flex_u64")]
    pub episode_num: Option<u64>,
    #[serde(default, deserialize_with = "flex_str")]
    pub title: String,
    #[serde(default, deserialize_with = "flex_string")]
    pub container_extension: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Season {
    pub number: u64,
    pub episodes: Vec<Episode>,
}

#[derive(Debug, Clone)]
pub struct SeriesInfo {
    pub name: String,
    pub plot: Option<String>,
    pub cover: Option<String>,
    pub seasons: Vec<Season>,
}

/// One entry from Xtream's short EPG response. Most panels base64-encode the text fields.
#[derive(Debug, Clone, PartialEq)]
pub struct EpgListing {
    pub title: String,
    pub description: String,
    pub start: String,
    pub end: String,
}

fn base64_text(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut bits = 0_u32;
    let mut count = 0_u8;
    for b in s.bytes().take_while(|&b| b != b'=') {
        let value = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'\r' | b'\n' | b' ' => continue,
            _ => return s.to_owned(),
        };
        bits = (bits << 6) | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_owned())
}

fn value_string(v: &Value, names: &[&str]) -> String {
    names
        .iter()
        .find_map(|name| v.get(name))
        .and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

fn parse_short_epg(v: Value) -> Result<Vec<EpgListing>> {
    let entries = v.get("epg_listings").cloned().unwrap_or(v);
    let raw: Vec<Value> = match entries {
        Value::Array(a) => a,
        Value::Object(m) => m.into_values().collect(),
        _ => return Ok(vec![]),
    };
    Ok(raw
        .into_iter()
        .map(|v| EpgListing {
            title: base64_text(&value_string(&v, &["title"])),
            description: base64_text(&value_string(&v, &["description", "desc"])),
            start: value_string(&v, &["start", "start_timestamp"]),
            end: value_string(&v, &["end", "stop", "stop_timestamp"]),
        })
        .collect())
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserInfo {
    #[serde(default, deserialize_with = "flex_str")]
    pub username: String,
    #[serde(default, deserialize_with = "flex_u64")]
    pub auth: Option<u64>,
    #[serde(default, deserialize_with = "flex_string")]
    pub status: Option<String>,
    /// Unix seconds; `None` means no expiry.
    #[serde(default, deserialize_with = "flex_string")]
    pub exp_date: Option<String>,
    #[serde(default, deserialize_with = "flex_u64")]
    pub max_connections: Option<u64>,
    #[serde(default, deserialize_with = "flex_u64")]
    pub active_cons: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ServerInfo {
    #[serde(default, deserialize_with = "flex_string")]
    pub url: Option<String>,
    #[serde(default, deserialize_with = "flex_string")]
    pub port: Option<String>,
    #[serde(default, deserialize_with = "flex_string")]
    pub server_protocol: Option<String>,
    #[serde(default, deserialize_with = "flex_string")]
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Auth {
    pub user_info: UserInfo,
    #[serde(default)]
    pub server_info: ServerInfo,
}

fn parse_auth(v: Value) -> Result<Auth> {
    let a: Auth = serde_json::from_value(v)?;
    if a.user_info.auth == Some(1) {
        Ok(a)
    } else {
        Err(Error::AuthFailed)
    }
}

/// `info` is sometimes `[]`, and `episodes` is an object keyed by season number, a plain
/// array of seasons (numbered from 1 here), or `false`/`[]` when empty.
fn parse_series_info(v: Value) -> Result<SeriesInfo> {
    let meta = |k: &str| {
        v.get("info")
            .and_then(|i| i.get(k))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let groups: Vec<(u64, Value)> = match v.get("episodes") {
        Some(Value::Object(m)) => m
            .iter()
            .map(|(k, v)| (k.parse().unwrap_or(0), v.clone()))
            .collect(),
        Some(Value::Array(a)) => a
            .iter()
            .enumerate()
            .map(|(i, v)| (i as u64 + 1, v.clone()))
            .collect(),
        _ => vec![],
    };
    let mut seasons = groups
        .into_iter()
        .map(|(number, v)| {
            Ok(Season {
                number,
                episodes: vec_from_value(v)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    seasons.retain(|s| !s.episodes.is_empty());
    seasons.sort_by_key(|s| s.number);
    Ok(SeriesInfo {
        name: meta("name").unwrap_or_default(),
        plot: meta("plot"),
        cover: meta("cover"),
        seasons,
    })
}

/// Lists come back as an array, an object keyed by index, or `false`/`null` when empty.
/// Malformed records are skipped so one bad channel can't sink 20k good ones; if
/// *every* record is malformed the first error is returned instead of an empty list.
fn vec_from_value<T: DeserializeOwned>(v: Value) -> Result<Vec<T>> {
    let items: Vec<Value> = match v {
        Value::Array(a) => a,
        Value::Object(m) => {
            let mut e: Vec<_> = m.into_iter().collect();
            e.sort_by_key(|(k, _)| k.parse::<u64>().unwrap_or(u64::MAX));
            e.into_iter().map(|(_, v)| v).collect()
        }
        _ => return Ok(vec![]),
    };
    let mut ok = Vec::with_capacity(items.len());
    let mut first_err = None;
    for i in items {
        match serde_json::from_value(i) {
            Ok(t) => ok.push(t),
            Err(e) => first_err = first_err.or(Some(e)),
        }
    }
    match (ok.is_empty(), first_err) {
        (true, Some(e)) => Err(e.into()),
        _ => Ok(ok),
    }
}

#[derive(Debug, Clone)]
pub struct Client {
    base: Url,
    user: String,
    pass: String,
    http: reqwest::Client,
    proxy: Option<Url>,
}

impl Client {
    /// `base` is the server root, e.g. `http://host:8080`.
    pub fn new(base: &str, user: impl Into<String>, pass: impl Into<String>) -> Result<Self> {
        let base = Url::parse(base.trim()).map_err(|e| Error::BadUrl(e.to_string()))?;
        if base.cannot_be_a_base() {
            return Err(Error::BadUrl(format!("{base} is not an http(s) url")));
        }
        Ok(Self {
            base,
            user: user.into(),
            pass: pass.into(),
            http: reqwest::Client::new(),
            proxy: None,
        })
    }

    /// Send every API request, and build every stream URL, through a pass-through proxy
    /// (`<proxy>?url=<upstream>`). Browsers need this: IPTV servers send no CORS headers.
    pub fn via_proxy(mut self, proxy: &str) -> Result<Self> {
        self.proxy = Some(Url::parse(proxy).map_err(|e| Error::BadUrl(e.to_string()))?);
        Ok(self)
    }

    /// Wrap an upstream URL for the proxy (no-op if none is configured). Needed for URLs found
    /// inside playlists, which the client did not build itself.
    pub fn proxied(&self, upstream: Url) -> Url {
        let Some(p) = &self.proxy else {
            return upstream;
        };
        let mut u = p.clone();
        u.query_pairs_mut().append_pair("url", upstream.as_str());
        u
    }

    fn root(&self) -> Url {
        let mut u = self.base.clone();
        u.set_query(None);
        u.set_fragment(None);
        u
    }

    fn api(&self, action: Option<&str>, extra: &[(&str, &str)]) -> Url {
        let mut u = self.root();
        u.path_segments_mut()
            .expect("checked in new")
            .pop_if_empty()
            .push("player_api.php");
        {
            let mut q = u.query_pairs_mut();
            q.append_pair("username", &self.user)
                .append_pair("password", &self.pass);
            if let Some(a) = action {
                q.append_pair("action", a);
            }
            for (k, v) in extra {
                q.append_pair(k, v);
            }
        }
        u
    }

    async fn get_json(&self, url: Url) -> Result<Value> {
        let go = async {
            self.http
                .get(self.proxied(url))
                .send()
                .await?
                .error_for_status()?
                .json::<Value>()
                .await
        };
        go.await.map_err(|e| Error::Http(e.without_url()))
    }

    async fn list<T: DeserializeOwned>(
        &self,
        action: &str,
        category: Option<u64>,
    ) -> Result<Vec<T>> {
        let id = category.map(|c| c.to_string());
        let extra: Vec<(&str, &str)> = id
            .as_deref()
            .map(|s| ("category_id", s))
            .into_iter()
            .collect();
        vec_from_value(self.get_json(self.api(Some(action), &extra)).await?)
    }

    /// Fails with [`Error::AuthFailed`] on bad credentials.
    pub async fn auth(&self) -> Result<Auth> {
        parse_auth(self.get_json(self.api(None, &[])).await?)
    }

    pub async fn live_categories(&self) -> Result<Vec<Category>> {
        self.list("get_live_categories", None).await
    }

    /// `None` returns every channel, which can be tens of thousands.
    pub async fn live_streams(&self, category: Option<u64>) -> Result<Vec<LiveStream>> {
        self.list("get_live_streams", category).await
    }

    pub async fn vod_categories(&self) -> Result<Vec<Category>> {
        self.list("get_vod_categories", None).await
    }

    pub async fn vod_streams(&self, category: Option<u64>) -> Result<Vec<VodStream>> {
        self.list("get_vod_streams", category).await
    }

    pub async fn series_categories(&self) -> Result<Vec<Category>> {
        self.list("get_series_categories", None).await
    }

    pub async fn series(&self, category: Option<u64>) -> Result<Vec<Series>> {
        self.list("get_series", category).await
    }

    pub async fn series_info(&self, series_id: u64) -> Result<SeriesInfo> {
        let id = series_id.to_string();
        parse_series_info(
            self.get_json(self.api(Some("get_series_info"), &[("series_id", &id)]))
                .await?,
        )
    }

    /// Fetch a small, on-demand guide for one live channel.
    pub async fn short_epg(&self, stream_id: u64, limit: usize) -> Result<Vec<EpgListing>> {
        let id = stream_id.to_string();
        let limit = limit.to_string();
        parse_short_epg(
            self.get_json(self.api(
                Some("get_short_epg"),
                &[("stream_id", &id), ("limit", &limit)],
            ))
            .await?,
        )
    }

    fn media_url(&self, kind: &str, id: u64, ext: &str) -> Url {
        let mut u = self.root();
        let file = format!("{id}.{ext}");
        u.path_segments_mut()
            .expect("checked in new")
            .pop_if_empty()
            .extend([kind, self.user.as_str(), self.pass.as_str(), file.as_str()]);
        self.proxied(u)
    }

    /// `ext` is `m3u8` (HLS) or `ts`. Browsers can only play `m3u8`.
    pub fn live_url(&self, stream_id: u64, ext: &str) -> Url {
        self.media_url("live", stream_id, ext)
    }

    /// `ext` is the stream's `container_extension`.
    pub fn movie_url(&self, stream_id: u64, ext: &str) -> Url {
        self.media_url("movie", stream_id, ext)
    }

    pub fn episode_url(&self, episode_id: u64, ext: &str) -> Url {
        self.media_url("series", episode_id, ext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn messy_provider_json_parses() {
        // Ids as strings, ratings as numbers, nulls: all seen in the wild.
        let v = json!([
            {"stream_id": "12", "name": null, "category_id": "3", "rating": 5.5, "container_extension": "mkv"},
            {"stream_id": 13, "name": "B", "category_id": null},
        ]);
        let s: Vec<VodStream> = vec_from_value(v).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(
            (s[0].stream_id, s[0].category_id, s[0].name.as_str()),
            (12, Some(3), "")
        );
        assert_eq!(s[0].rating.as_deref(), Some("5.5"));
        assert_eq!(s[1].category_id, None);
    }

    #[test]
    fn list_shapes() {
        assert!(vec_from_value::<Category>(json!(false)).unwrap().is_empty());
        assert!(vec_from_value::<Category>(json!([])).unwrap().is_empty());

        // Keyed by index, and index 10 must sort after 2.
        let by_index: Value = (0..12)
            .map(|i| (i.to_string(), json!({"category_id": i})))
            .collect();
        let cats: Vec<Category> = vec_from_value(by_index).unwrap();
        assert_eq!(
            cats.iter().map(|c| c.category_id).collect::<Vec<_>>(),
            (0..12).collect::<Vec<_>>()
        );

        // One bad record is skipped; all-bad is an error.
        assert_eq!(
            vec_from_value::<Category>(json!([{"category_id": 1}, {"nope": 1}]))
                .unwrap()
                .len(),
            1
        );
        assert!(vec_from_value::<Category>(json!([{"nope": 1}])).is_err());
    }

    #[test]
    fn auth_accepted_and_rejected() {
        assert!(matches!(
            parse_auth(json!({"user_info": {"auth": 0}})),
            Err(Error::AuthFailed)
        ));
        let ok = parse_auth(json!({
            "user_info": {"username": "u", "auth": "1", "exp_date": null, "max_connections": "2"},
            "server_info": {"port": 8080}
        }))
        .unwrap();
        assert_eq!(ok.user_info.max_connections, Some(2));
        assert_eq!(ok.server_info.port.as_deref(), Some("8080"));
    }

    #[test]
    fn urls_survive_awkward_credentials_and_base_paths() {
        let c = Client::new(
            "http://h.example:8080/panel/?junk=1",
            "us er",
            "p@ss/w?d&x=1",
        )
        .unwrap();

        let u = c.live_url(7, "m3u8");
        let segs: Vec<_> = u.path_segments().unwrap().collect();
        assert_eq!(
            (segs.len(), segs[0], segs[1], segs[4]),
            (5, "panel", "live", "7.m3u8")
        );
        assert_eq!(u.query(), None);

        let u = c.api(Some("get_live_streams"), &[("category_id", "3")]);
        assert_eq!(u.path(), "/panel/player_api.php");
        let q: std::collections::HashMap<_, _> = u.query_pairs().collect();
        assert_eq!(q["password"], "p@ss/w?d&x=1");
        assert_eq!(
            (&*q["action"], &*q["category_id"]),
            ("get_live_streams", "3")
        );
        assert!(!q.contains_key("junk"));
    }

    #[test]
    fn series_info_handles_both_episode_shapes() {
        let keyed = json!({
            "info": {"name": "S", "plot": null},
            "episodes": {
                "2": [{"id": "22", "title": "b", "container_extension": "mkv"}],
                "1": [{"id": "11", "episode_num": "1", "title": "a"}, {"id": 12, "title": "a2"}]
            }
        });
        let i = parse_series_info(keyed).unwrap();
        assert_eq!(
            (i.name.as_str(), i.plot.as_deref(), i.seasons.len()),
            ("S", None, 2)
        );
        assert_eq!(
            (
                i.seasons[0].number,
                i.seasons[0].episodes.len(),
                i.seasons[1].episodes[0].id
            ),
            (1, 2, 22)
        );

        // Array of seasons, and `info` as `[]`.
        let listy =
            parse_series_info(json!({"info": [], "episodes": [[{"id": 5}], [{"id": 6}]]})).unwrap();
        assert_eq!(
            listy
                .seasons
                .iter()
                .map(|s| s.episodes[0].id)
                .collect::<Vec<_>>(),
            [5, 6]
        );

        assert!(
            parse_series_info(json!({"episodes": false}))
                .unwrap()
                .seasons
                .is_empty()
        );
    }

    #[test]
    fn short_epg_decodes_text_and_messy_shapes() {
        let epg = parse_short_epg(json!({"epg_listings": [
            {"title": "TmV3cw==", "description": "TGF0ZXN0IGhlYWRsaW5lcw==",
             "start": "2026-09-29 18:00:00", "end": "2026-09-29 19:00:00"},
            {"title": "Live sport", "start_timestamp": 123, "stop_timestamp": "456"}
        ]}))
        .unwrap();
        assert_eq!(epg[0].title, "News");
        assert_eq!(epg[0].description, "Latest headlines");
        assert_eq!((epg[1].start.as_str(), epg[1].end.as_str()), ("123", "456"));
        assert_eq!(base64_text("not base64!"), "not base64!");
    }

    #[test]
    fn proxy_mode_wraps_api_and_stream_urls() {
        let c = Client::new("http://h.example:8080", "u", "p&q")
            .unwrap()
            .via_proxy("http://localhost:3000/proxy")
            .unwrap();
        let unwrap = |u: Url| {
            assert_eq!((u.host_str(), u.path()), (Some("localhost"), "/proxy"));
            u.query_pairs()
                .find(|(k, _)| k == "url")
                .unwrap()
                .1
                .into_owned()
        };
        assert_eq!(
            unwrap(c.movie_url(1, "mp4")),
            "http://h.example:8080/movie/u/p&q/1.mp4"
        );
        assert!(
            unwrap(c.proxied(c.api(None, &[])))
                .starts_with("http://h.example:8080/player_api.php?username=u&password=p%26q")
        );
        assert!(
            Client::new("http://h", "u", "p")
                .unwrap()
                .via_proxy("nope")
                .is_err()
        );
    }

    #[test]
    fn bad_base_urls_are_rejected() {
        assert!(matches!(
            Client::new("not a url", "u", "p"),
            Err(Error::BadUrl(_))
        ));
        assert!(matches!(
            Client::new("mailto:x@y.z", "u", "p"),
            Err(Error::BadUrl(_))
        ));
    }
}
