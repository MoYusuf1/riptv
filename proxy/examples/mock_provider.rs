//! Fake Xtream provider for local development. Login: demo / demo. Streams redirect to public
//! test media (MDN's CC0 flower.mp4, Mux's Big Buck Bunny HLS). It listens on loopback, which the
//! proxy only reaches because the demo sign-in approves 127.0.0.1.
//!   cargo run -p riptv --example mock_provider     (listens on 127.0.0.1:8081)

use std::{
    collections::HashMap,
    sync::OnceLock,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    extract::{Path, Query},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use serde_json::{Value, json};

static STARTED: OnceLock<Instant> = OnceLock::new();

/// `MOCK_TITLES=30000` adds that many synthetic channels, movies and (a fifth as many) series to
/// every list, spread over dozens of categories: the size of a big provider's catalogue, for trying
/// how the app copes with one.
fn titles() -> usize {
    static N: OnceLock<usize> = OnceLock::new();
    *N.get_or_init(|| {
        std::env::var("MOCK_TITLES")
            .ok()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0)
    })
}

/// A made-up title (mixed case, accents, digits, like real catalogues) for the n-th item.
fn port() -> u16 {
    std::env::var("MOCK_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8081)
}

/// Synthetic titles all share one tiny local picture, so a big catalogue makes no outside requests.
fn poster_url() -> String {
    format!("http://127.0.0.1:{}/poster.svg", port())
}

fn synthetic_name(n: usize) -> String {
    const A: [&str; 16] = [
        "The",
        "Última",
        "Crimson",
        "Éternel",
        "Night",
        "La",
        "Silent",
        "Wild",
        "Mohamed's",
        "Golden",
        "Broken",
        "Hidden",
        "Electric",
        "Último",
        "Paper",
        "Midnight",
    ];
    const B: [&str; 16] = [
        "River", "Empire", "Voyage", "Garden", "Secret", "Horizon", "Legacy", "Storm", "Ciudad",
        "Promise", "Machine", "Kingdom", "Letters", "Harvest", "Signal", "Orchard",
    ];
    let h = (n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let (a, b) = (A[(h >> 8) as usize % 16], B[(h >> 20) as usize % 16]);
    format!("{a} {b} {}", 1900 + (h >> 33) % 125)
}

fn synthetic_list(kind: &str, poster: &str) -> Vec<Value> {
    let n = titles();
    let (count, categories) = match kind {
        "live" => (n, 60),
        "vod" => (n, 40),
        _ => (n / 5, 20),
    };
    (0..count)
        .map(|i| {
            let id = 1000 + i;
            let name = synthetic_name(i);
            let category = (i * 7 % categories + 1).to_string();
            let added = (1_600_000_000 + i as u64 * 3_601 % 190_000_000).to_string();
            let rating = format!("{}.{}", i % 10, i * 3 % 10);
            match kind {
                "live" => json!({"stream_id": id, "name": name, "category_id": category,
                                 "stream_icon": poster, "epg_channel_id": format!("ch{i}")}),
                "vod" => json!({"stream_id": id, "name": name, "container_extension": "mkv",
                                "rating": rating, "category_id": category, "added": added,
                                "stream_icon": poster}),
                _ => json!({"series_id": id, "name": name, "category_id": category, "cover": poster,
                            "plot": "A synthetic series.", "rating": rating, "last_modified": added}),
            }
        })
        .collect()
}

fn synthetic_categories(kind: &str) -> Vec<Value> {
    let categories = match kind {
        "live" => 60,
        "vod" => 40,
        _ => 20,
    };
    (1..=categories)
        .map(|c| json!({"category_id": c.to_string(), "category_name": format!("{} category {c}", kind)}))
        .collect()
}

/// A file of your own to try (`MOCK_MOVIE=/path/film.mkv`), listed as movie 6: a real film, with
/// whatever sound and index it really has, is the best test of the player's seeking.
static OWN: OnceLock<Option<(&'static [u8], &'static str)>> = OnceLock::new();

fn own() -> Option<(&'static [u8], &'static str)> {
    *OWN.get_or_init(|| {
        let path = std::env::var("MOCK_MOVIE").ok()?;
        let bytes = std::fs::read(&path).ok()?;
        let kind = if path.ends_with(".mkv") {
            "video/x-matroska"
        } else {
            "video/mp4"
        };
        Some((Box::leak(bytes.into_boxed_slice()) as &'static [u8], kind))
    })
}

/// Anamorphic PAL (720x576, 64:45 pixels): must display as 16:9, not stretched to 5:4.
const PAL: &[u8] = include_bytes!("../fixtures/pal_anamorphic.ts");

/// Streams like the ones providers really send, which Chrome on Linux can't play as they are: HEVC
/// video with AC-3 sound, and H.264 with AC-3 sound. The proxy's ffmpeg mode has to fix both.
const HEVC_AC3: &[u8] = include_bytes!("../fixtures/hevc_ac3.ts");
const H264_AC3: &[u8] = include_bytes!("../fixtures/h264_ac3.ts");
/// Movies whose sound a browser on Linux can't decode, in the containers providers really use: the
/// same 16 s clip (H.264 with B-frames, 5.1 AC-3) as MP4 and as MKV, and a 3 s clip with DTS sound,
/// which nothing here decodes, so the proxy's ffmpeg has to.
const MOVIE_AC3_MP4: &[u8] = include_bytes!("../fixtures/movie_ac3.mp4");
const MOVIE_AC3_MKV: &[u8] = include_bytes!("../fixtures/movie_ac3.mkv");
const MOVIE_DTS_MKV: &[u8] = include_bytes!("../fixtures/movie_dts.mkv");

const PLAYLIST: [(header::HeaderName, &str); 1] =
    [(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")];

/// Channel 1 redirects to a real VOD-style HLS master playlist. Channel 2 is a simulated live
/// stream: a 3-segment sliding window over the same 64 Big Buck Bunny segments that advances every
/// 10 s and wraps around (which shows up as a timestamp jump, like a real stream restart).
/// Channel 3 is one anamorphic PAL segment. Channel 6 is offline (404), and channel 7 is a provider
/// too slow to keep up (each 4 s segment takes 9 s), for trying `riptv --logs` diagnostics.
async fn live(Path((_user, _pass, file)): Path<(String, String, String)>) -> Response {
    for (id, segment) in [("4", "hevc.ts"), ("5", "h264.ts")] {
        if file.starts_with(&format!("{id}.")) {
            if file.ends_with(".ts") {
                return ([(header::CONTENT_TYPE, "video/mp2t")], transport(segment))
                    .into_response();
            }
            let body = format!(
                "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:5\n#EXT-X-MEDIA-SEQUENCE:0\n\
                 #EXTINF:4.000,\nhttp://127.0.0.1:{}/{segment}\n#EXT-X-ENDLIST\n",
                port()
            );
            return (PLAYLIST, body).into_response();
        }
    }
    if file.starts_with("3.") {
        let body = format!(
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:2\n#EXT-X-MEDIA-SEQUENCE:0\n\
             #EXTINF:1.000,\nhttp://127.0.0.1:{}/pal.ts\n#EXT-X-ENDLIST\n",
            port()
        );
        return (PLAYLIST, body).into_response();
    }
    if file.starts_with("6.") {
        return (StatusCode::NOT_FOUND, "stream not found").into_response();
    }
    if file.starts_with("7.") {
        // A live window of 4 s segments that each take 9 s to arrive: it can't keep up.
        let n = STARTED.get_or_init(Instant::now).elapsed().as_secs() / 4;
        let mut body = format!(
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:{n}\n"
        );
        for i in n..n + 3 {
            body += &format!(
                "#EXT-X-DISCONTINUITY\n#EXTINF:4.000,\nhttp://127.0.0.1:{}/slow/{i}.ts\n",
                port()
            );
        }
        return (PLAYLIST, body).into_response();
    }
    if !file.starts_with("2.") {
        return Redirect::temporary(HLS).into_response();
    }
    let n = STARTED.get_or_init(Instant::now).elapsed().as_secs() / 10;
    let mut body =
        format!("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:11\n#EXT-X-MEDIA-SEQUENCE:{n}\n");
    for i in n..n + 3 {
        body += &format!(
            "#EXTINF:10.000,\nhttps://test-streams.mux.dev/x36xhzz/url_6/url_{}/193039199_mp4_h264_aac_hq_7.ts\n",
            846 + i % 64
        );
    }
    (PLAYLIST, body).into_response()
}

const FLOWER: &str = "https://interactive-examples.mdn.mozilla.net/media/cc0-videos/flower.mp4";
/// Artwork (Wikimedia Commons, CC-BY) so the poster grid and detail pages have something to show.
const POSTER: &str = "https://upload.wikimedia.org/wikipedia/commons/thumb/c/c5/Big_buck_bunny_poster_big.jpg/500px-Big_buck_bunny_poster_big.jpg";
const HLS: &str = "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8";

/// A file, with byte ranges the way real providers serve them (players seek by asking for one).
fn ranged(headers: &HeaderMap, body: &'static [u8], kind: &'static str) -> Response {
    let len = body.len();
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|r| r.split_once('-'))
        .and_then(|(a, b)| match (a.parse::<usize>(), b.parse::<usize>()) {
            (Ok(a), Ok(b)) => Some((a, b.min(len - 1))),
            (Ok(a), Err(_)) => Some((a, len - 1)),
            (Err(_), Ok(n)) => Some((len.saturating_sub(n), len - 1)),
            _ => None,
        })
        .filter(|(a, b)| a <= b && *a < len);
    match range {
        Some((a, b)) => (
            StatusCode::PARTIAL_CONTENT,
            [
                (header::CONTENT_TYPE, kind.to_string()),
                (header::ACCEPT_RANGES, "bytes".into()),
                (header::CONTENT_RANGE, format!("bytes {a}-{b}/{len}")),
            ],
            body[a..=b].to_vec(),
        )
            .into_response(),
        None => (
            [
                (header::CONTENT_TYPE, kind.to_string()),
                (header::ACCEPT_RANGES, "bytes".into()),
            ],
            body,
        )
            .into_response(),
    }
}

fn transport(name: &str) -> &'static [u8] {
    if name == "hevc.ts" {
        HEVC_AC3
    } else {
        H264_AC3
    }
}

fn b64(s: &str) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in s.as_bytes().chunks(3) {
        let at = |i: usize| u32::from(*c.get(i).unwrap_or(&0));
        let n = at(0) << 16 | at(1) << 8 | at(2);
        let sextet = |shift: u32| T[(n >> shift) as usize & 63] as char;
        out.push(sextet(18));
        out.push(sextet(12));
        out.push(if c.len() > 1 { sextet(6) } else { '=' });
        out.push(if c.len() > 2 { sextet(0) } else { '=' });
    }
    out
}

/// A day of programmes around now: six hours back, eighteen ahead, one of them airing right now.
fn schedule(stream_id: u64) -> Vec<Value> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (kinds, lengths) = (
        ["News", "Match", "Documentary", "Film", "Magazine"],
        [30, 60, 45, 90, 60],
    );
    let mut t = now / 3600 * 3600 - 6 * 3600;
    let mut listings = vec![];
    for k in stream_id as usize.. {
        let end = t + lengths[k % 5] * 60;
        listings.push(json!({
            "title": b64(&format!("{} {}", kinds[k % 5], k % 97)),
            "description": b64("A made-up programme for the mock provider, long enough to show how descriptions wrap in the guide."),
            "start_timestamp": t.to_string(),
            "stop_timestamp": end.to_string(),
            "start": "", "end": "",
        }));
        t = end;
        if t > now + 18 * 3600 {
            return listings;
        }
    }
    listings
}

fn vod_streams() -> Value {
    let mut movies = vec![
        json!({"stream_id": 1, "name": "Flower (CC0 sample)", "container_extension": "mp4",
               "rating": "5", "category_id": "10", "added": "1790000000", "stream_icon": POSTER}),
        json!({"stream_id": 2, "name": "AC-3 sound in an MP4", "container_extension": "mp4",
               "rating": "6.5", "category_id": "10", "added": "1780000000", "stream_icon": POSTER}),
        json!({"stream_id": 3, "name": "An older title", "container_extension": "mp4",
               "rating": "8.2", "category_id": "10", "added": "1700000000"}),
        json!({"stream_id": 4, "name": "AC-3 sound in an MKV", "container_extension": "mkv",
               "rating": "7.1", "category_id": "10", "added": "1785000000", "stream_icon": POSTER}),
        json!({"stream_id": 5, "name": "DTS sound (needs ffmpeg)", "container_extension": "mkv",
               "rating": "4.0", "category_id": "10", "added": "1775000000"}),
    ];
    movies.extend(synthetic_list("vod", &poster_url()));
    if own().is_some() {
        movies.push(json!({"stream_id": 6, "name": "Your own file (MOCK_MOVIE)",
                           "container_extension": "mkv", "rating": "9.0", "category_id": "10",
                           "added": "1795000000"}));
    }
    Value::Array(movies)
}

async fn api(Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    let get = |k: &str| q.get(k).map(String::as_str);
    if get("username") != Some("demo") || get("password") != Some("demo") {
        return Json(json!({"user_info": {"auth": 0}}));
    }
    let stream_id: u64 = get("stream_id").and_then(|s| s.parse().ok()).unwrap_or(1);
    Json(match get("action") {
        None => json!({
            "user_info": {"username": "demo", "auth": 1, "status": "Active", "exp_date": null,
                          "max_connections": "1", "active_cons": "0"},
            "server_info": {"url": "127.0.0.1"}
        }),
        Some("get_live_categories") => {
            let mut all = vec![json!({"category_id": "1", "category_name": "Test Channels"})];
            if titles() > 0 {
                all.extend(synthetic_categories("live"));
            }
            Value::Array(all)
        }
        Some("get_live_streams") => {
            let mut all = vec![
                json!({"stream_id": 1, "name": "Big Buck Bunny (HLS via redirect)", "category_id": "1"}),
                json!({"stream_id": 2, "name": "Simulated live (sliding window)", "category_id": "1"}),
                json!({"stream_id": 3, "name": "Anamorphic PAL (720x576, 64:45)", "category_id": "1"}),
                json!({"stream_id": 4, "name": "HEVC video + AC-3 sound (needs conversion)", "category_id": "1"}),
                json!({"stream_id": 5, "name": "H.264 video + AC-3 sound", "category_id": "1"}),
                json!({"stream_id": 6, "name": "Offline channel (gives 404)", "category_id": "1"}),
                json!({"stream_id": 7, "name": "Slow provider (stalls)", "category_id": "1"}),
            ];
            all.extend(synthetic_list("live", &poster_url()));
            Value::Array(all)
        }
        Some("get_vod_categories") => {
            let mut all = vec![json!({"category_id": "10", "category_name": "Test Movies"})];
            if titles() > 0 {
                all.extend(synthetic_categories("vod"));
            }
            Value::Array(all)
        }
        Some("get_vod_streams") => vod_streams(),
        Some("get_vod_info") => {
            let id = get("vod_id").unwrap_or("1");
            let name = match id {
                "2" => "AC-3 sound in an MP4",
                "4" => "AC-3 sound in an MKV",
                "5" => "DTS sound (needs ffmpeg)",
                _ => "Flower (CC0 sample)",
            };
            json!({
                "info": {
                    "name": name, "genre": "Documentary, Animation, Short", "releasedate": "2017-05-12",
                    "movie_image": POSTER, "backdrop_path": [POSTER],
                    "duration": "02:53:12", "duration_secs": 10392, "country": "United States of America",
                    "cast": "Jan Morgenstern, Sacha Goedegebure, Ton Roosendaal, Pablo Vazquez, Aleks Kourbatov",
                    "director": "Sacha Goedegebure", "rating": "7.6", "mpaa_rating": "PG",
                    "youtube_trailer": "aqz-KE-bpKQ",
                    "video": {"codec_name": "h264", "width": 1920, "height": 1080},
                    "audio": {"codec_name": "ac3", "channels": 6},
                    "plot": "A giant rabbit with a heart bigger than himself takes revenge on the three bullies \
                             who ruined his morning. The page has enough text to show how a long description \
                             is clipped and expanded: nothing else happens in the film, and it does not get \
                             any longer than this, however many times it is played."
                },
                "movie_data": {"stream_id": 1, "name": name, "container_extension": "mp4"}
            })
        }
        Some("get_series_categories") => {
            let mut all = vec![json!({"category_id": "20", "category_name": "Test Series"})];
            if titles() > 0 {
                all.extend(synthetic_categories("series"));
            }
            Value::Array(all)
        }
        Some("get_series") => {
            let mut all = vec![
                json!({"series_id": 1, "name": "Flower: The Series", "category_id": "20",
                    "plot": "Two seasons of the same five seconds.", "rating": "7.5",
                    "last_modified": "1790000000", "cover": POSTER}),
            ];
            all.extend(synthetic_list("series", &poster_url()));
            Value::Array(all)
        }
        Some("get_series_info") => json!({
            "info": {"name": "Flower: The Series", "genre": "Drama, Nature", "releaseDate": "2021-03-01",
                     "cover": POSTER, "backdrop_path": [POSTER], "episode_run_time": "45",
                     "cast": "Petal, Stem, A very patient bee", "director": "The Bees", "rating": "7.5",
                     "youtube_trailer": "https://www.youtube.com/watch?v=aqz-KE-bpKQ",
                     "plot": "Two seasons of the same short film."},
            "episodes": {
                "1": [{"id": "101", "episode_num": 1, "title": "Bloom", "container_extension": "mp4",
                       "info": {"plot": "It starts, as these things do, with a bud.", "movie_image": POSTER,
                                "duration_secs": 16, "releasedate": "2021-03-01"}},
                      {"id": "102", "episode_num": 2, "title": "Petals", "container_extension": "mkv",
                       "info": {"plot": "The same clip again, now in a different container.",
                                "duration_secs": 16, "releasedate": "2021-03-08"}}],
                "2": [{"id": "201", "episode_num": 1, "title": "Return", "container_extension": "mp4",
                       "info": []}]
            }
        }),
        Some("get_simple_data_table") => json!({"epg_listings": schedule(stream_id)}),
        Some("get_short_epg") => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let upcoming: Vec<Value> = schedule(stream_id)
                .into_iter()
                .filter(|l| l["stop_timestamp"].as_str().and_then(|s| s.parse().ok()) > Some(now))
                .take(4)
                .collect();
            json!({"epg_listings": upcoming})
        }
        _ => json!(false), // real panels answer `false` for empty lists
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let app = Router::new()
        .route("/player_api.php", get(api))
        .route("/live/{user}/{pass}/{file}", get(live))
        .route(
            "/hevc.ts",
            get(|| async { ([(header::CONTENT_TYPE, "video/mp2t")], HEVC_AC3) }),
        )
        .route(
            "/h264.ts",
            get(|| async { ([(header::CONTENT_TYPE, "video/mp2t")], H264_AC3) }),
        )
        .route(
            "/slow/{n}",
            get(|| async {
                tokio::time::sleep(std::time::Duration::from_secs(9)).await;
                ([(header::CONTENT_TYPE, "video/mp2t")], H264_AC3)
            }),
        )
        .route(
            "/pal.ts",
            get(|| async { ([(header::CONTENT_TYPE, "video/mp2t")], PAL) }),
        )
        .route(
            "/movie/{user}/{pass}/{file}",
            get(
                |Path((_u, _p, file)): Path<(String, String, String)>, headers: HeaderMap| async move {
                    let (n, _) = file.split_once('.').unwrap_or((&file, ""));
                    match n {
                        "2" => ranged(&headers, MOVIE_AC3_MP4, "video/mp4"),
                        "4" => ranged(&headers, MOVIE_AC3_MKV, "video/x-matroska"),
                        "5" => ranged(&headers, MOVIE_DTS_MKV, "video/x-matroska"),
                        "6" if own().is_some() => {
                            let (bytes, kind) = own().expect("checked");
                            ranged(&headers, bytes, kind)
                        }
                        _ => Redirect::temporary(FLOWER).into_response(),
                    }
                },
            ),
        )
        .route(
            "/series/{user}/{pass}/{file}",
            get(
                |Path((_u, _p, file)): Path<(String, String, String)>, headers: HeaderMap| async move {
                    match file.split_once('.').map_or(file.as_str(), |f| f.0) {
                        "101" => ranged(&headers, MOVIE_AC3_MP4, "video/mp4"),
                        "102" => ranged(&headers, MOVIE_AC3_MKV, "video/x-matroska"),
                        _ => Redirect::temporary(FLOWER).into_response(),
                    }
                },
            ),
        );
    let app = app.route("/poster.svg", get(|| async {
        ([(header::CONTENT_TYPE, "image/svg+xml")],
         r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 3"><rect width="2" height="3" fill="#3a2a33"/></svg>"##)
    }));
    let port = port();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .unwrap_or_else(|_| panic!("bind {port}"));
    println!("mock Xtream provider on http://127.0.0.1:{port}  (demo / demo)");
    axum::serve(listener, app).await.expect("serve");
}
