//! Xtream Codes API client. Native and wasm32 (reqwest uses `fetch` there).
//!
//! ponytail: no vod_info or EPG yet (add when the UI needs them). No request timeout:
//! reqwest can't set one on wasm.

pub use reqwest::Url;
use serde::{
    Deserialize, Deserializer,
    de::{DeserializeOwned, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use serde_json::Value;
use std::sync::Arc;

mod art;
#[cfg(test)]
mod bench;
pub mod guide;
mod playlist;
pub mod sniff;
mod stream;
mod text;

pub use art::{Art, sized as sized_art};
use playlist::Playlist;
pub use sniff::Sniff;
pub use text::contains_lowercase;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("bad server url: {0}")]
    BadUrl(String),
    // No `#[from]`: reqwest errors embed the request URL, which carries the password.
    // Every conversion goes through `without_url()` in `get_json`.
    #[error("request failed: {0}")]
    Http(reqwest::Error),
    /// The proxy itself said no, and why (only ever a host name and a reason, never credentials).
    #[error("the proxy couldn't do that: {0}")]
    Proxy(String),
    #[error("unexpected response shape: {0}")]
    Json(#[from] serde_json::Error),
    #[error("login rejected by server")]
    AuthFailed,
    #[error("playlist: {0}")]
    Playlist(&'static str),
}

pub type Result<T> = std::result::Result<T, Error>;

// Providers mix strings, numbers, bools and nulls for the same field. Accept all of them.
//
// Read straight off the parser with a visitor: going through `serde_json::Value` first built a
// throwaway tree (and a `String` for every numeric field written as text) for every field of every
// record, which on a 30,000-title list was half of all the allocations there were.
struct Text;

impl<'de> Visitor<'de> for Text {
    type Value = Option<String>;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a string, number, bool or null")
    }

    fn visit_str<E>(self, v: &str) -> std::result::Result<Self::Value, E> {
        Ok(Some(v.to_owned()))
    }

    fn visit_string<E>(self, v: String) -> std::result::Result<Self::Value, E> {
        Ok(Some(v))
    }

    fn visit_u64<E>(self, v: u64) -> std::result::Result<Self::Value, E> {
        Ok(Some(v.to_string()))
    }

    fn visit_i64<E>(self, v: i64) -> std::result::Result<Self::Value, E> {
        Ok(Some(v.to_string()))
    }

    fn visit_f64<E>(self, v: f64) -> std::result::Result<Self::Value, E> {
        // The same spelling `serde_json` gives a number ("8.0", not "8").
        Ok(Some(
            serde_json::Number::from_f64(v).map_or_else(|| v.to_string(), |n| n.to_string()),
        ))
    }

    fn visit_bool<E>(self, v: bool) -> std::result::Result<Self::Value, E> {
        Ok(Some(v.to_string()))
    }

    fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_some<D: Deserializer<'de>>(self, d: D) -> std::result::Result<Self::Value, D::Error> {
        d.deserialize_any(Text)
    }

    fn visit_seq<A: SeqAccess<'de>>(
        self,
        mut seq: A,
    ) -> std::result::Result<Self::Value, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(None)
    }

    fn visit_map<A: MapAccess<'de>>(
        self,
        mut map: A,
    ) -> std::result::Result<Self::Value, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(None)
    }
}

fn flex_string<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<String>, D::Error> {
    d.deserialize_any(Text)
}

fn flex_str<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    Ok(flex_string(d)?.unwrap_or_default())
}

/// A whole number, from a number, a string of digits, or a bool; anything else is `None`.
struct Count;

impl<'de> Visitor<'de> for Count {
    type Value = Option<u64>;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a number, a string of digits, a bool or null")
    }

    fn visit_u64<E>(self, v: u64) -> std::result::Result<Self::Value, E> {
        Ok(Some(v))
    }

    fn visit_i64<E>(self, v: i64) -> std::result::Result<Self::Value, E> {
        Ok(u64::try_from(v).ok())
    }

    fn visit_f64<E>(self, _: f64) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_str<E>(self, v: &str) -> std::result::Result<Self::Value, E> {
        Ok(v.trim().parse().ok())
    }

    fn visit_bool<E>(self, v: bool) -> std::result::Result<Self::Value, E> {
        Ok(Some(u64::from(v)))
    }

    fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_some<D: Deserializer<'de>>(self, d: D) -> std::result::Result<Self::Value, D::Error> {
        d.deserialize_any(Count)
    }

    fn visit_seq<A: SeqAccess<'de>>(
        self,
        mut seq: A,
    ) -> std::result::Result<Self::Value, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(None)
    }

    fn visit_map<A: MapAccess<'de>>(
        self,
        mut map: A,
    ) -> std::result::Result<Self::Value, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(None)
    }
}

fn flex_u64<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<u64>, D::Error> {
    d.deserialize_any(Count)
}

fn flex_id<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<u64, D::Error> {
    flex_u64(d)?.ok_or_else(|| serde::de::Error::custom("expected a numeric id"))
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
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
    /// Unix time the title was added to the catalogue.
    #[serde(default, deserialize_with = "flex_u64")]
    pub added: Option<u64>,
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
    #[serde(default, deserialize_with = "flex_u64")]
    pub last_modified: Option<u64>,
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
    #[serde(default, deserialize_with = "episode_info")]
    pub info: EpisodeInfo,
}

/// What a panel knows about one episode (its `info` object, which is `[]` when it knows nothing).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EpisodeInfo {
    pub plot: Option<String>,
    pub image: Option<String>,
    pub runtime_secs: Option<u64>,
    pub release_date: Option<String>,
    pub rating: Option<String>,
}

fn episode_info<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<EpisodeInfo, D::Error> {
    let v = Value::deserialize(d)?;
    let v = Some(&v);
    Ok(EpisodeInfo {
        plot: pick(v, &["plot", "description"]),
        image: pick(v, &["movie_image", "cover_big", "cover"]),
        runtime_secs: runtime_secs(v),
        release_date: pick(v, &["releasedate", "releaseDate", "air_date"]),
        rating: pick(v, &["rating"]),
    })
}

#[derive(Debug, Clone)]
pub struct Season {
    pub number: u64,
    pub episodes: Vec<Episode>,
}

#[derive(Debug, Clone)]
pub struct SeriesInfo {
    pub details: Details,
    pub seasons: Vec<Season>,
}

/// The "enriched profile" of a movie or series. Every field is optional in practice: panels fill
/// what their metadata source knows and leave the rest empty.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Details {
    pub name: String,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    pub plot: Option<String>,
    pub genre: Option<String>,
    pub year: Option<String>,
    pub duration: Option<String>,
    pub country: Option<String>,
    pub cast: Option<String>,
    pub director: Option<String>,
    pub rating: Option<String>,
    /// The whole release date as the panel wrote it (`year` is its first four digits).
    pub release_date: Option<String>,
    /// Content rating, such as "PG-13" or "16".
    pub age: Option<String>,
    /// A YouTube video id or address.
    pub trailer: Option<String>,
    /// Length in seconds (for a series, the usual episode).
    pub runtime_secs: Option<u64>,
    /// "1080p · H264", if the panel has probed the file.
    pub video: Option<String>,
    /// "AC3 · 5.1", likewise.
    pub audio: Option<String>,
}

/// A trimmed, non-empty text under the first of `names` that has one.
fn pick(info: Option<&Value>, names: &[&str]) -> Option<String> {
    let s = value_string(info.unwrap_or(&Value::Null), names);
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_owned())
}

/// Length in seconds: `duration_secs`, else `duration` as `HH:MM:SS` or `MM:SS`, else the usual
/// episode length in minutes.
fn runtime_secs(info: Option<&Value>) -> Option<u64> {
    let parsed = |names: &[&str]| pick(info, names)?.parse::<u64>().ok().filter(|n| *n > 0);
    parsed(&["duration_secs"])
        .or_else(|| {
            let text = pick(info, &["duration"])?;
            let parts: Vec<u64> = text
                .split(':')
                .map(|p| p.trim().parse().ok())
                .collect::<Option<_>>()?;
            let secs = match parts[..] {
                [h, m, s] => h * 3600 + m * 60 + s,
                [m, s] => m * 60 + s,
                _ => return None,
            };
            (secs > 0).then_some(secs)
        })
        .or_else(|| parsed(&["episode_run_time"]).map(|minutes| minutes * 60))
}

/// "4K", "1080p" and so on for a picture that many pixels tall.
fn resolution(height: u64) -> String {
    match height {
        2000.. => "4K".into(),
        h => format!("{h}p"),
    }
}

/// `info.video` / `info.audio` are objects when the panel has probed the file, `[]` when not.
fn stream_label(info: Option<&Value>, key: &str) -> Option<String> {
    let s = info?.get(key).filter(|s| s.is_object());
    let number = |name: &str| {
        s?.get(name)
            .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
    };
    let codec = pick(s, &["codec_name"]).map(|c| c.to_uppercase());
    let detail = if key == "video" {
        number("height").filter(|h| *h > 0).map(resolution)
    } else {
        number("channels").map(|c| match c {
            1 => "Mono".to_owned(),
            2 => "Stereo".to_owned(),
            6 => "5.1".to_owned(),
            8 => "7.1".to_owned(),
            n => format!("{n} ch"),
        })
    };
    let parts: Vec<String> = [detail, codec].into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// Reads `info` (an object; some panels send `[]` when they know nothing) under the names the
/// movie and series responses use.
fn parse_details(info: Option<&Value>) -> Details {
    let text = |names: &[&str]| pick(info, names);
    // `backdrop_path` is an array of URLs, a bare URL, or an empty array.
    let backdrop = match info.and_then(|i| i.get("backdrop_path")) {
        Some(Value::Array(a)) => a.iter().find_map(Value::as_str).map(str::to_owned),
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
    .filter(|s| !s.is_empty());
    let release_date = text(&["releasedate", "releaseDate", "release_date", "year"]);
    let year = release_date
        .as_ref()
        .map(|d| d.chars().take(4).collect::<String>())
        .filter(|y| y.len() == 4 && y.chars().all(|c| c.is_ascii_digit()));
    let runtime_secs = runtime_secs(info);
    let duration = text(&["duration"]).or_else(|| {
        let secs =
            runtime_secs.filter(|_| info.is_some_and(|i| i.get("duration_secs").is_some()))?;
        Some(format!(
            "{:02}:{:02}:{:02}",
            secs / 3600,
            secs / 60 % 60,
            secs % 60
        ))
    });
    Details {
        name: text(&["name", "o_name"]).unwrap_or_default(),
        poster: text(&["movie_image", "cover_big", "cover"]),
        backdrop,
        plot: text(&["plot", "description"]),
        genre: text(&["genre"]),
        year,
        duration,
        country: text(&["country"]),
        cast: text(&["cast", "actors"]),
        director: text(&["director"]),
        rating: text(&["rating"]),
        release_date,
        age: text(&["mpaa_rating", "age"]).filter(|a| a != "0"),
        trailer: text(&["youtube_trailer", "trailer"]),
        runtime_secs,
        video: stream_label(info, "video"),
        audio: stream_label(info, "audio"),
    }
}

/// One entry from Xtream's short EPG response. Most panels base64-encode the text fields.
#[derive(Debug, Clone, PartialEq)]
pub struct EpgListing {
    pub title: String,
    pub description: String,
    pub start: String,
    pub end: String,
    /// Unix seconds, when the panel sends them (nearly all do). The text times above are in the
    /// panel's own timezone, so anything laid out on a clock should use these.
    pub start_ts: Option<u64>,
    pub end_ts: Option<u64>,
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
            start_ts: value_string(&v, &["start_timestamp"]).parse().ok(),
            end_ts: value_string(&v, &["stop_timestamp", "end_timestamp"])
                .parse()
                .ok(),
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
        details: parse_details(v.get("info")),
        seasons,
    })
}

/// Malformed records are skipped so one bad channel can't sink 20k good ones; if *every*
/// record is malformed the first error is returned instead of an empty list.
fn collect_lenient<T>(items: impl Iterator<Item = serde_json::Result<T>>) -> Result<Vec<T>> {
    let mut ok = vec![];
    let mut first_err = None;
    for item in items {
        match item {
            Ok(t) => ok.push(t),
            Err(e) => first_err = first_err.or(Some(e)),
        }
    }
    match (ok.is_empty(), first_err) {
        (true, Some(e)) => Err(e.into()),
        _ => Ok(ok),
    }
}

/// For the small lists nested inside another response (a season's episodes).
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
    collect_lenient(items.into_iter().map(serde_json::from_value))
}

/// The big lists (every channel or title a provider has). They come back as an array, an
/// object keyed by index, or `false`/`null` when empty. Parsed straight from the response
/// bytes: the array is only split into borrowed per-record slices and each record is decoded
/// on its own, so a 20,000-entry list never exists as a `Value` tree (one heap allocation per
/// field), which is several times the size of the response itself.
fn vec_from_slice<T: DeserializeOwned>(bytes: &[u8]) -> Result<Vec<T>> {
    collect_lenient(
        split_records(bytes)?
            .into_iter()
            .map(|r| serde_json::from_slice(r.get().as_bytes())),
    )
}

/// One borrowed JSON slice per record, none of them decoded yet. Deliberately not generic, so
/// this code is compiled once rather than once per record type. (Every shape goes through here
/// and then through `from_str`: mixing in the `Value` path would instantiate each record type's
/// deserializer twice, which measured larger than the code it saved.)
fn split_records(bytes: &[u8]) -> Result<Vec<&serde_json::value::RawValue>> {
    match bytes.iter().find(|b| !b.is_ascii_whitespace()) {
        Some(b'[') => Ok(serde_json::from_slice(bytes)?),
        Some(b'{') => {
            let map: std::collections::HashMap<String, &serde_json::value::RawValue> =
                serde_json::from_slice(bytes)?;
            let mut records: Vec<_> = map.into_iter().collect();
            records.sort_by_key(|(k, _)| k.parse::<u64>().unwrap_or(u64::MAX));
            Ok(records.into_iter().map(|(_, r)| r).collect())
        }
        // `false`, `null`, `""`: an empty list. Anything that isn't JSON at all is still an error.
        _ => serde_json::from_slice::<Value>(bytes)
            .map(|_| vec![])
            .map_err(Into::into),
    }
}

/// A stream the proxy converts on the fly. It is one continuous stream with no index, so a
/// `<video>` can't seek in it; to jump, start a new one part-way in with [`Converted::at`].
#[derive(Debug, Clone, PartialEq)]
pub struct Converted {
    url: Url,
    /// Whole seconds, for a movie or episode; `None` for a live stream.
    pub duration: Option<u64>,
}

impl Converted {
    /// The converted stream beginning `start` seconds in (from the beginning if zero).
    pub fn at(&self, start: u64) -> Url {
        let mut url = self.url.clone();
        if start > 0 {
            url.query_pairs_mut()
                .append_pair("start", &start.to_string());
        }
        url
    }
}

/// Header the proxy puts on its own refusals and failures, with the reason. Provider errors that
/// pass through it don't have it.
const PROXY_ERROR: &str = "x-riptv-error";

fn http_error(e: reqwest::Error) -> Error {
    Error::Http(e.without_url())
}

/// Turns an unsuccessful response into an error, preferring the proxy's own explanation.
fn checked(resp: reqwest::Response) -> Result<reqwest::Response> {
    if let Some(why) = resp
        .headers()
        .get(PROXY_ERROR)
        .and_then(|v| v.to_str().ok())
    {
        return Err(Error::Proxy(why.to_owned()));
    }
    resp.error_for_status().map_err(http_error)
}

#[derive(Debug, Clone)]
pub struct Client {
    base: Url,
    user: String,
    pass: String,
    http: reqwest::Client,
    proxy: Option<Url>,
    playlist: Option<Arc<Playlist>>,
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
            playlist: None,
        })
    }

    /// Fetch an M3U channel list, or treat an HLS `.m3u8` manifest as one live channel.
    /// The parsed list is shared across clones of this client; stream bytes still go through the
    /// existing same-origin proxy. (Nothing here stores anything: saving a playlist is the app's choice.)
    pub async fn load_playlist(mut self) -> Result<Self> {
        if !matches!(self.base.scheme(), "http" | "https") {
            return Err(Error::Playlist("use an http(s) playlist URL"));
        }
        let response = checked(
            self.http
                .get(self.proxied(self.base.clone()))
                .send()
                .await
                .map_err(http_error)?,
        )?;
        if response
            .content_length()
            .is_some_and(|n| n > playlist::MAX_BYTES)
        {
            return Err(Error::Playlist("file is too large"));
        }
        let body = response.bytes().await.map_err(http_error)?;
        if body.len() as u64 > playlist::MAX_BYTES {
            return Err(Error::Playlist("file is too large"));
        }
        self.playlist = Some(Arc::new(playlist::parse(&body, &self.base)?));
        Ok(self)
    }

    pub fn is_playlist(&self) -> bool {
        self.playlist.is_some()
    }

    /// Send every API request, and build every stream URL, through a pass-through proxy
    /// (`<proxy>?url=<upstream>`). Browsers need this: IPTV servers send no CORS headers.
    pub fn via_proxy(mut self, proxy: &str) -> Result<Self> {
        self.proxy = Some(Url::parse(proxy).map_err(|e| Error::BadUrl(e.to_string()))?);
        Ok(self)
    }

    /// Sign in with the proxy: ask it to accept this server's address even if that is on a private
    /// network (a box on your LAN, or this machine). The proxy refuses private addresses
    /// otherwise, so a provider can't aim it at your network. Public servers don't need this, and
    /// without a proxy it does nothing.
    pub async fn approve(&self) -> Result<()> {
        let Some(p) = &self.proxy else {
            return Ok(());
        };
        let mut u = p.clone();
        u.set_path("/allow");
        u.set_query(None);
        u.query_pairs_mut()
            .append_pair("host", self.base.host_str().unwrap_or_default());
        checked(self.http.post(u).send().await.map_err(http_error)?)?;
        Ok(())
    }

    /// The server-side address behind a proxied one; anything else comes back unchanged.
    pub fn upstream(&self, media: &Url) -> Url {
        let ours = |p: &Url| {
            (media.host_str(), media.port(), media.path()) == (p.host_str(), p.port(), p.path())
        };
        match &self.proxy {
            Some(p) if ours(p) => media
                .query_pairs()
                .find(|(k, _)| k == "url")
                .and_then(|(_, v)| Url::parse(&v).ok())
                .unwrap_or_else(|| media.clone()),
            _ => media.clone(),
        }
    }

    /// For a stream the browser can't play as it is (HEVC video, AC-3 or MP2 sound, a raw
    /// transport stream): ask the proxy to convert it with ffmpeg. Returns the address of the
    /// converted stream, which any `<video>` can play. Fails, with the reason, if the proxy has no
    /// ffmpeg or can't read the stream. A `.m3u8` address is tried as the plain `.ts` stream first,
    /// which is what ffmpeg reads best.
    pub async fn convert(&self, media: &Url) -> Result<Converted> {
        self.convert_inner(media, false).await
    }

    /// Fallback when playback produced audio but no video frames. Unlike [`Self::convert`],
    /// require a real video track and re-encode it even if the codec looks browser-compatible.
    pub async fn convert_video(&self, media: &Url) -> Result<Converted> {
        self.convert_inner(media, true).await
    }

    /// The proxy's live stream of a channel (`/live`): one fragmented-MP4 response, the codecs
    /// read and converted on the proxy as needed. `can` lists codecs the browser decodes beyond
    /// H.264 and AAC (`hevc,ac3,eac3`); `transcode` re-encodes the video whatever it is.
    pub fn live(&self, media: &Url, can: &str, transcode: bool) -> Option<Url> {
        let mut u = self.proxy.clone()?;
        u.set_path("/live");
        u.set_query(None);
        {
            let mut q = u.query_pairs_mut();
            q.append_pair("url", self.upstream(media).as_str())
                .append_pair("can", can);
            if transcode {
                q.append_pair("video", "transcode");
            }
        }
        Some(u)
    }

    /// Why the proxy refuses `url` (its own explanation, or the HTTP status), or `None` if it
    /// doesn't. For a player whose `<video>` failed and can't see the response.
    pub async fn refusal(&self, url: &Url) -> Option<String> {
        let resp = self.http.get(url.clone()).send().await.ok()?;
        if resp.status().is_success() {
            return None;
        }
        Some(
            resp.headers()
                .get("x-riptv-error")
                .and_then(|v| v.to_str().ok())
                .map_or_else(|| resp.status().to_string(), str::to_owned),
        )
    }

    async fn convert_inner(&self, media: &Url, force_video: bool) -> Result<Converted> {
        let Some(proxy) = &self.proxy else {
            return Err(Error::Proxy("there is no proxy to convert it".into()));
        };
        let raw = self.upstream(media);
        let mut candidates = vec![];
        if let Some(stem) = raw.path().strip_suffix(".m3u8") {
            let mut ts = raw.clone();
            ts.set_path(&format!("{stem}.ts"));
            candidates.push(ts);
        }
        candidates.push(raw);

        let endpoint = |path: &str, pairs: &[(&str, &str)]| {
            let mut u = proxy.clone();
            u.set_path(path);
            u.set_query(None);
            u.query_pairs_mut().extend_pairs(pairs);
            u
        };
        let mut last = Error::Proxy("nothing to convert".into());
        for candidate in candidates {
            let check = endpoint("/compat/check", &[("url", candidate.as_str())]);
            let answer = match self.http.get(check).send().await {
                Ok(resp) => checked(resp),
                Err(e) => Err(http_error(e)),
            };
            match answer {
                Ok(resp) => {
                    let header =
                        |name: &str| resp.headers().get(name).and_then(|v| v.to_str().ok());
                    if force_video && header("x-riptv-has-video") == Some("0") {
                        last = Error::Proxy("this source has audio but no video track".into());
                        continue;
                    }
                    let video = if force_video {
                        "transcode"
                    } else {
                        header("x-riptv-video").unwrap_or("transcode")
                    };
                    return Ok(Converted {
                        url: endpoint("/compat", &[("url", candidate.as_str()), ("video", video)]),
                        duration: header("x-riptv-duration").and_then(|d| d.parse().ok()),
                    });
                }
                Err(e) => last = e,
            }
        }
        Err(last)
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

    /// GET, then hand the response bytes to `parse`, so each caller decides what to decode into.
    async fn get_with<R>(&self, url: Url, parse: impl FnOnce(&[u8]) -> Result<R>) -> Result<R> {
        let resp = self.http.get(self.proxied(url)).send().await;
        let resp = checked(resp.map_err(http_error)?)?;
        let body = resp.bytes().await.map_err(http_error)?;
        parse(&body)
    }

    async fn get_json(&self, url: Url) -> Result<Value> {
        self.get_with(url, |b| Ok(serde_json::from_slice(b)?)).await
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
        self.get_list(self.api(Some(action), &extra)).await
    }

    /// GET a list and read its records as they arrive (see `stream`).
    async fn get_list<T: DeserializeOwned>(&self, url: Url) -> Result<Vec<T>> {
        use futures_core::Stream;
        let resp = self.http.get(self.proxied(url)).send().await;
        let resp = checked(resp.map_err(http_error)?)?;
        let mut chunks = std::pin::pin!(resp.bytes_stream());
        let mut reader = stream::ListReader::<T>::new();
        while let Some(chunk) = std::future::poll_fn(|cx| chunks.as_mut().poll_next(cx)).await {
            reader.feed(&chunk.map_err(http_error)?)?;
        }
        reader.finish()
    }

    /// Fails with [`Error::AuthFailed`] on bad credentials.
    pub async fn auth(&self) -> Result<Auth> {
        parse_auth(self.get_json(self.api(None, &[])).await?)
    }

    pub async fn live_categories(&self) -> Result<Vec<Category>> {
        if let Some(p) = &self.playlist {
            return Ok(p.categories.clone());
        }
        self.list("get_live_categories", None).await
    }

    /// `None` returns every channel, which can be tens of thousands.
    pub async fn live_streams(&self, category: Option<u64>) -> Result<Vec<LiveStream>> {
        if let Some(p) = &self.playlist {
            return Ok(p
                .channels
                .iter()
                .filter(|s| category.is_none_or(|id| s.category_id == Some(id)))
                .cloned()
                .collect());
        }
        self.list("get_live_streams", category).await
    }

    pub async fn vod_categories(&self) -> Result<Vec<Category>> {
        if self.is_playlist() {
            return Ok(vec![]);
        }
        self.list("get_vod_categories", None).await
    }

    pub async fn vod_streams(&self, category: Option<u64>) -> Result<Vec<VodStream>> {
        if self.is_playlist() {
            return Ok(vec![]);
        }
        self.list("get_vod_streams", category).await
    }

    pub async fn series_categories(&self) -> Result<Vec<Category>> {
        if self.is_playlist() {
            return Ok(vec![]);
        }
        self.list("get_series_categories", None).await
    }

    pub async fn series(&self, category: Option<u64>) -> Result<Vec<Series>> {
        if self.is_playlist() {
            return Ok(vec![]);
        }
        self.list("get_series", category).await
    }

    pub async fn series_info(&self, series_id: u64) -> Result<SeriesInfo> {
        let id = series_id.to_string();
        parse_series_info(
            self.get_json(self.api(Some("get_series_info"), &[("series_id", &id)]))
                .await?,
        )
    }

    /// The profile of one movie: plot, cast, backdrop and so on.
    pub async fn vod_info(&self, vod_id: u64) -> Result<Details> {
        let id = vod_id.to_string();
        let v = self
            .get_json(self.api(Some("get_vod_info"), &[("vod_id", &id)]))
            .await?;
        Ok(parse_details(v.get("info")))
    }

    /// A channel's whole schedule (a day or more, past and future), for a timeline.
    pub async fn epg_table(&self, stream_id: u64) -> Result<Vec<EpgListing>> {
        if self.is_playlist() {
            return Ok(vec![]);
        }
        let id = stream_id.to_string();
        parse_short_epg(
            self.get_json(self.api(Some("get_simple_data_table"), &[("stream_id", &id)]))
                .await?,
        )
    }

    /// Fetch a small, on-demand guide for one live channel.
    pub async fn short_epg(&self, stream_id: u64, limit: usize) -> Result<Vec<EpgListing>> {
        if self.is_playlist() {
            return Ok(vec![]);
        }
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
        if let Some(p) = &self.playlist {
            return self.proxied(
                p.urls
                    .get(stream_id.saturating_sub(1) as usize)
                    .unwrap_or(&self.base)
                    .clone(),
            );
        }
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

    /// Runs a value through the same byte-level parser the network path uses.
    fn list<T: DeserializeOwned>(v: Value) -> Result<Vec<T>> {
        vec_from_slice(v.to_string().as_bytes())
    }

    #[test]
    fn messy_provider_json_parses() {
        // Ids as strings, ratings as numbers, nulls: all seen in the wild.
        let v = json!([
            {"stream_id": "12", "name": null, "category_id": "3", "rating": 5.5, "container_extension": "mkv", "added": "1700000000"},
            {"stream_id": 13, "name": "B", "category_id": null},
        ]);
        let s: Vec<VodStream> = list(v).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(
            (s[0].stream_id, s[0].category_id, s[0].name.as_str()),
            (12, Some(3), "")
        );
        assert_eq!(s[0].rating.as_deref(), Some("5.5"));
        assert_eq!((s[0].added, s[1].added), (Some(1_700_000_000), None));
        assert_eq!(s[1].category_id, None);
    }

    #[test]
    fn list_shapes() {
        assert!(list::<Category>(json!(false)).unwrap().is_empty());
        assert!(list::<Category>(json!([])).unwrap().is_empty());

        // Keyed by index, and index 10 must sort after 2.
        let by_index: Value = (0..12)
            .map(|i| (i.to_string(), json!({"category_id": i})))
            .collect();
        let cats: Vec<Category> = list(by_index).unwrap();
        assert_eq!(
            cats.iter().map(|c| c.category_id).collect::<Vec<_>>(),
            (0..12).collect::<Vec<_>>()
        );

        // One bad record is skipped; all-bad is an error.
        assert_eq!(
            list::<Category>(json!([{"category_id": 1}, {"nope": 1}]))
                .unwrap()
                .len(),
            1
        );
        assert!(list::<Category>(json!([{"nope": 1}])).is_err());
    }

    #[test]
    fn a_non_json_body_is_an_error_not_an_empty_list() {
        // An HTML error page from a dead panel must not look like "no channels".
        assert!(vec_from_slice::<Category>(b"<html>502 Bad Gateway</html>").is_err());
        assert!(vec_from_slice::<Category>(b"").is_err());
        assert!(vec_from_slice::<Category>(b"  false ").unwrap().is_empty());
    }

    #[test]
    fn a_big_list_parses_and_skips_only_the_bad_record() {
        let mut body = String::from("[");
        for i in 0..20_000 {
            body += &format!(r#"{{"stream_id":"{i}","name":"Channel {i}","category_id":"7"}},"#);
        }
        body += r#"{"name":"no id"}]"#;
        let all: Vec<LiveStream> = vec_from_slice(body.as_bytes()).unwrap();
        assert_eq!(all.len(), 20_000);
        assert_eq!(all[19_999].stream_id, 19_999);
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
                "1": [{"id": "11", "episode_num": "1", "title": "a", "info": {"plot": "p", "movie_image": "i.jpg", "duration_secs": 2700}},
                      {"id": 12, "title": "a2", "info": []}]
            }
        });
        let i = parse_series_info(keyed).unwrap();
        assert_eq!(
            (
                i.details.name.as_str(),
                i.details.plot.as_deref(),
                i.seasons.len()
            ),
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
        // What each episode knows of itself, and `[]` or nothing when it knows nothing.
        let first = &i.seasons[0].episodes[0].info;
        assert_eq!(
            (
                first.plot.as_deref(),
                first.image.as_deref(),
                first.runtime_secs
            ),
            (Some("p"), Some("i.jpg"), Some(2700))
        );
        assert_eq!(i.seasons[0].episodes[1].info, EpisodeInfo::default());
        assert_eq!(i.seasons[1].episodes[0].info, EpisodeInfo::default());

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
        assert_eq!(
            (epg[0].start_ts, epg[1].start_ts, epg[1].end_ts),
            (None, Some(123), Some(456))
        );
        assert_eq!(base64_text("not base64!"), "not base64!");
    }

    #[test]
    fn details_read_the_shapes_movies_and_series_use() {
        // A movie: array backdrop, "duration" text, a full release date.
        let movie = json!({"info": {
            "movie_image": "http://x/p.jpg", "backdrop_path": ["http://x/b.jpg", "http://x/c.jpg"],
            "plot": " A plot. ", "genre": "Documentary", "releasedate": "2017-05-12",
            "duration": "01:42:38", "country": "United States of America",
            "cast": "Dina Bruno, Scott Levin", "director": "Antonio Santini", "rating": 7.2
        }});
        let d = parse_details(movie.get("info"));
        assert_eq!(d.poster.as_deref(), Some("http://x/p.jpg"));
        assert_eq!(d.backdrop.as_deref(), Some("http://x/b.jpg"));
        assert_eq!(d.plot.as_deref(), Some("A plot."));
        assert_eq!(
            (d.year.as_deref(), d.duration.as_deref()),
            (Some("2017"), Some("01:42:38"))
        );
        assert_eq!(d.rating.as_deref(), Some("7.2"));

        // A series: `releaseDate`, only seconds for the length, no backdrop.
        let d = parse_details(Some(&json!({
            "name": "S", "cover": "c.jpg", "releaseDate": "1999", "duration_secs": 3725,
            "backdrop_path": []
        })));
        assert_eq!(
            (d.year.as_deref(), d.duration.as_deref()),
            (Some("1999"), Some("01:02:05"))
        );
        assert_eq!((d.poster.as_deref(), d.backdrop), (Some("c.jpg"), None));

        // The rest of what the new pages show.
        let d = parse_details(Some(&json!({
            "releasedate": "2006-12-22", "mpaa_rating": "PG-13", "youtube_trailer": "abc123DEF45",
            "duration_secs": "7584",
            "video": {"codec_name": "h264", "width": 1920, "height": "1080"},
            "audio": {"codec_name": "ac3", "channels": 6}
        })));
        assert_eq!(
            (
                d.release_date.as_deref(),
                d.age.as_deref(),
                d.trailer.as_deref()
            ),
            (Some("2006-12-22"), Some("PG-13"), Some("abc123DEF45"))
        );
        assert_eq!(d.runtime_secs, Some(7584));
        assert_eq!(
            (d.video.as_deref(), d.audio.as_deref()),
            (Some("1080p · H264"), Some("5.1 · AC3"))
        );
        // Lengths also come as clock text, or as minutes for a series; probes as `[]`.
        let clock = |v: Value| parse_details(Some(&v)).runtime_secs;
        assert_eq!(clock(json!({"duration": "01:42:38"})), Some(6158));
        assert_eq!(clock(json!({"duration": "42:10"})), Some(2530));
        assert_eq!(clock(json!({"episode_run_time": "45"})), Some(2700));
        assert_eq!(clock(json!({"duration": "garbage"})), None);
        let d = parse_details(Some(&json!({"video": [], "audio": [], "age": "0"})));
        assert_eq!((d.video, d.audio, d.age), (None, None, None));

        // Panels answer `[]` or nothing when they know nothing.
        assert_eq!(parse_details(Some(&json!([]))), Details::default());
        assert_eq!(parse_details(None), Details::default());
    }

    #[test]
    fn the_upstream_behind_a_proxied_url_can_be_recovered() {
        let c = Client::new("http://h.example:8080", "u", "p")
            .unwrap()
            .via_proxy("http://localhost:3000/proxy")
            .unwrap();
        let movie = c.movie_url(7, "mkv");
        assert_ne!(movie.host_str(), Some("h.example"));
        assert_eq!(
            c.upstream(&movie).as_str(),
            "http://h.example:8080/movie/u/p/7.mkv"
        );
        // Not ours: unchanged.
        let other = Url::parse("http://elsewhere.example/a.mp4").unwrap();
        assert_eq!(c.upstream(&other), other);
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
