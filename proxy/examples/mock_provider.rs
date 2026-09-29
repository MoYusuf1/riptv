//! Fake Xtream provider for local development. Login: demo / demo. Streams redirect to public
//! test media (MDN's CC0 flower.mp4, Mux's Big Buck Bunny HLS), so the proxy needs
//! IPTV_ALLOW=interactive-examples.mdn.mozilla.net,test-streams.mux.dev to follow them.
//!   cargo run -p iptv-proxy --example mock_provider     (listens on 127.0.0.1:8081)

use std::{collections::HashMap, sync::OnceLock, time::Instant};

use axum::{
    Json, Router,
    extract::{Path, Query},
    http::header,
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use serde_json::{Value, json};

static STARTED: OnceLock<Instant> = OnceLock::new();

/// Channel 1 redirects to a real VOD-style HLS master playlist. Channel 2 is a simulated live
/// stream: a 3-segment sliding window over the same 64 Big Buck Bunny segments that advances every
/// 10 s and wraps around (which shows up as a timestamp jump, like a real stream restart).
async fn live(Path((_user, _pass, file)): Path<(String, String, String)>) -> Response {
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
    (
        [(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")],
        body,
    )
        .into_response()
}

const FLOWER: &str = "https://interactive-examples.mdn.mozilla.net/media/cc0-videos/flower.mp4";
const HLS: &str = "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8";

async fn api(Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    let get = |k: &str| q.get(k).map(String::as_str);
    if get("username") != Some("demo") || get("password") != Some("demo") {
        return Json(json!({"user_info": {"auth": 0}}));
    }
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
            {"stream_id": 2, "name": "Simulated live (sliding window)", "category_id": "1"}
        ]),
        Some("get_vod_categories") => {
            json!([{"category_id": "10", "category_name": "Test Movies"}])
        }
        Some("get_vod_streams") => json!([
            {"stream_id": 1, "name": "Flower (CC0 sample)", "container_extension": "mp4", "rating": "5", "category_id": "10"}
        ]),
        Some("get_series_categories") => {
            json!([{"category_id": "20", "category_name": "Test Series"}])
        }
        Some("get_series") => {
            json!([{"series_id": 1, "name": "Flower: The Series", "category_id": "20",
                                      "plot": "Two seasons of the same five seconds."}])
        }
        Some("get_series_info") => json!({
            "info": {"name": "Flower: The Series", "plot": "Two seasons of the same five seconds."},
            "episodes": {
                "1": [{"id": "101", "episode_num": 1, "title": "Bloom", "container_extension": "mp4"},
                      {"id": "102", "episode_num": 2, "title": "Petals", "container_extension": "mp4"}],
                "2": [{"id": "201", "episode_num": 1, "title": "Return", "container_extension": "mp4"}]
            }
        }),
        Some("get_short_epg") => json!({"epg_listings": [
            {"title": "Tm93OiBCaWcgQnVjayBCdW5ueQ==",
             "description": "QSBiaWcgYnVubnksIGEgYmlnIGFkdmVudHVyZS4=",
             "start": "18:00", "end": "18:30"},
            {"title": "VXAgbmV4dDogQmxvb21pbmcgZ2FyZGVucw==",
             "description": "QSBzbG93LCByZWxheGluZyBsb29rIGF0IG5hdHVyZS4=",
             "start": "18:30", "end": "19:00"},
            {"title": "RXZlbmluZyBjaW5lbWE=", "description": "RmVhdHVyZSBwcmVzZW50YXRpb24u",
             "start": "19:00", "end": "21:00"}
        ]}),
        _ => json!(false), // real panels answer `false` for empty lists
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let app = Router::new()
        .route("/player_api.php", get(api))
        .route("/live/{user}/{pass}/{file}", get(live))
        .route(
            "/movie/{user}/{pass}/{file}",
            get(|| async { Redirect::temporary(FLOWER) }),
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
