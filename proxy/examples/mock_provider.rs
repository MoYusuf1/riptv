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
    http::header,
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use serde_json::{Value, json};

static STARTED: OnceLock<Instant> = OnceLock::new();

/// Anamorphic PAL (720x576, 64:45 pixels): must display as 16:9, not stretched to 5:4.
const PAL: &[u8] = include_bytes!("../fixtures/pal_anamorphic.ts");

/// Streams like the ones providers really send, which Chrome on Linux can't play as they are: HEVC
/// video with AC-3 sound, and H.264 with AC-3 sound. The proxy's ffmpeg mode has to fix both.
const HEVC_AC3: &[u8] = include_bytes!("../fixtures/hevc_ac3.ts");
const H264_AC3: &[u8] = include_bytes!("../fixtures/h264_ac3.ts");
/// A movie whose sound is AC-3: it plays, silently, until the page notices and converts it.
const MOVIE_AC3: &[u8] = include_bytes!("../fixtures/h264_ac3.mp4");

const PLAYLIST: [(header::HeaderName, &str); 1] =
    [(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")];

/// Channel 1 redirects to a real VOD-style HLS master playlist. Channel 2 is a simulated live
/// stream: a 3-segment sliding window over the same 64 Big Buck Bunny segments that advances every
/// 10 s and wraps around (which shows up as a timestamp jump, like a real stream restart).
/// Channel 3 is one anamorphic PAL segment.
async fn live(Path((_user, _pass, file)): Path<(String, String, String)>) -> Response {
    for (id, segment) in [("4", "hevc.ts"), ("5", "h264.ts")] {
        if file.starts_with(&format!("{id}.")) {
            if file.ends_with(".ts") {
                return ([(header::CONTENT_TYPE, "video/mp2t")], transport(segment))
                    .into_response();
            }
            let body = format!(
                "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:5\n#EXT-X-MEDIA-SEQUENCE:0\n\
                 #EXTINF:4.000,\nhttp://127.0.0.1:8081/{segment}\n#EXT-X-ENDLIST\n"
            );
            return (PLAYLIST, body).into_response();
        }
    }
    if file.starts_with("3.") {
        let body = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:2\n#EXT-X-MEDIA-SEQUENCE:0\n\
                    #EXTINF:1.000,\nhttp://127.0.0.1:8081/pal.ts\n#EXT-X-ENDLIST\n";
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
            json!([{"category_id": "1", "category_name": "Test Channels"}])
        }
        Some("get_live_streams") => json!([
            {"stream_id": 1, "name": "Big Buck Bunny (HLS via redirect)", "category_id": "1"},
            {"stream_id": 2, "name": "Simulated live (sliding window)", "category_id": "1"},
            {"stream_id": 3, "name": "Anamorphic PAL (720x576, 64:45)", "category_id": "1"},
            {"stream_id": 4, "name": "HEVC video + AC-3 sound (needs conversion)", "category_id": "1"},
            {"stream_id": 5, "name": "H.264 video + AC-3 sound", "category_id": "1"}
        ]),
        Some("get_vod_categories") => {
            json!([{"category_id": "10", "category_name": "Test Movies"}])
        }
        Some("get_vod_streams") => json!([
            {"stream_id": 1, "name": "Flower (CC0 sample)", "container_extension": "mp4",
             "rating": "5", "category_id": "10", "added": "1790000000", "stream_icon": POSTER},
            {"stream_id": 2, "name": "AC-3 sound test", "container_extension": "mp4",
             "rating": "6.5", "category_id": "10", "added": "1780000000"},
            {"stream_id": 3, "name": "An older title", "container_extension": "mp4",
             "rating": "8.2", "category_id": "10", "added": "1700000000"}
        ]),
        Some("get_vod_info") => json!({
            "info": {
                "name": "Flower (CC0 sample)", "genre": "Documentary", "releasedate": "2017-05-12",
                "movie_image": POSTER, "backdrop_path": [POSTER],
                "duration": "00:00:05", "country": "United States of America",
                "cast": "A flower, some bees", "director": "MDN Web Docs", "rating": "5",
                "plot": "A five second time-lapse of a flower opening, published by MDN as a CC0 sample. \
                         It is here so the movie page has enough text to show how a long description is \
                         clipped and expanded: nothing else happens in the film, and it does not get \
                         any longer than this, however many times it is played."
            },
            "movie_data": {"stream_id": 1, "name": "Flower (CC0 sample)", "container_extension": "mp4"}
        }),
        Some("get_series_categories") => {
            json!([{"category_id": "20", "category_name": "Test Series"}])
        }
        Some("get_series") => {
            json!([{"series_id": 1, "name": "Flower: The Series", "category_id": "20",
                    "plot": "Two seasons of the same five seconds.", "rating": "7.5",
                    "last_modified": "1790000000", "cover": POSTER}])
        }
        Some("get_series_info") => json!({
            "info": {"name": "Flower: The Series", "genre": "Drama", "releaseDate": "2021-03-01",
                     "cover": POSTER, "backdrop_path": [POSTER],
                     "cast": "Petal, Stem", "director": "The Bees", "rating": "7.5",
                     "plot": "Two seasons of the same five seconds."},
            "episodes": {
                "1": [{"id": "101", "episode_num": 1, "title": "Bloom", "container_extension": "mp4"},
                      {"id": "102", "episode_num": 2, "title": "Petals", "container_extension": "mp4"}],
                "2": [{"id": "201", "episode_num": 1, "title": "Return", "container_extension": "mp4"}]
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
            "/pal.ts",
            get(|| async { ([(header::CONTENT_TYPE, "video/mp2t")], PAL) }),
        )
        .route(
            "/movie/{user}/{pass}/{file}",
            get(
                |Path((_u, _p, file)): Path<(String, String, String)>| async move {
                    if file.starts_with("2.") {
                        ([(header::CONTENT_TYPE, "video/mp4")], MOVIE_AC3).into_response()
                    } else {
                        Redirect::temporary(FLOWER).into_response()
                    }
                },
            ),
        )
        .route(
            "/series/{user}/{pass}/{file}",
            get(|| async { Redirect::temporary(FLOWER) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8081")
        .await
        .expect("bind 8081");
    println!("mock Xtream provider on http://127.0.0.1:8081  (demo / demo)");
    axum::serve(listener, app).await.expect("serve");
}
