use std::collections::HashMap;

use axum::{Json, Router, extract::Query, response::Redirect, routing::get};
use reqwest::Url;
use riptv::{AppState, router};
use serde_json::{Value, json};
use tower_http::services::ServeFile;

async fn serve(app: Router) -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    port
}

/// What the app's sign-in does: tell the proxy the provider's host is fine.
async fn sign_in(proxy: u16, host: &str) -> reqwest::StatusCode {
    reqwest::Client::new()
        .post(format!("http://127.0.0.1:{proxy}/allow?host={host}"))
        .send()
        .await
        .unwrap()
        .status()
}

fn proxied(proxy: u16, target: &str) -> Url {
    Url::parse_with_params(
        &format!("http://127.0.0.1:{proxy}/proxy"),
        [("url", target)],
    )
    .unwrap()
}

#[tokio::test]
async fn local_diagnostic_session_keeps_credentials_out_of_reports() {
    let upstream = serve(
        Router::new()
            .route(
                "/index.m3u8",
                get(|| async { "#EXTM3U\n#EXTINF:4,\nsegment.ts\n" }),
            )
            .route(
                "/segment.ts",
                get(|| async {
                    let mut bytes = vec![0_u8; 188 * 4];
                    for packet in bytes.as_chunks_mut::<188>().0 {
                        packet[0] = 0x47;
                    }
                    bytes
                }),
            ),
    )
    .await;
    let proxy = serve(router(AppState::new())).await;
    assert_eq!(sign_in(proxy, "127.0.0.1").await, 204);
    let http = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{proxy}/diagnostics");
    let create = http.post(&base).header("content-type", "application/json").body(json!({
        "url": format!("http://user:very-secret@127.0.0.1:{upstream}/index.m3u8?token=hidden")
    }).to_string()).send().await.unwrap();
    assert_eq!(create.status(), 200);
    let created: Value = serde_json::from_str(&create.text().await.unwrap()).unwrap();
    let id = created["id"].as_str().unwrap();
    assert_eq!(id.len(), 32);
    let listed = http.get(&base).send().await.unwrap().text().await.unwrap();
    assert!(listed.contains(id));
    assert!(!listed.contains("very-secret"));
    assert!(!listed.contains("hidden"));
    let checked = http
        .post(format!("{base}/{id}/probe"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(checked.contains("mpeg_ts"));
    assert!(checked.contains("\"playlist_hops\":1"));
    assert!(!checked.contains("very-secret"));
    assert!(!checked.contains("hidden"));
}

#[tokio::test]
async fn range_passthrough_and_private_addresses() {
    let file = std::env::temp_dir().join(format!("riptv-test-{}.bin", std::process::id()));
    std::fs::write(&file, (0..=255u8).collect::<Vec<_>>()).unwrap();

    let upstream = serve(
        Router::new()
            .route_service("/f.bin", ServeFile::new(&file))
            .route("/hop", get(|| async { Redirect::temporary("/f.bin") }))
            // A provider trying to aim the proxy at the router, by number and by name.
            .route(
                "/evil",
                get(|| async { Redirect::temporary("http://10.255.255.1/x") }),
            )
            .route(
                "/evil-name",
                get(|| async { Redirect::temporary("http://localhost:1/x") }),
            ),
    )
    .await;
    let proxy = serve(router(AppState::new())).await;

    let fetch = |target: String, range: Option<&'static str>| async move {
        let mut req = reqwest::Client::new().get(proxied(proxy, &target));
        if let Some(r) = range {
            req = req.header("Range", r);
        }
        req.send().await.unwrap()
    };
    let up = |path: &str| format!("http://127.0.0.1:{upstream}{path}");

    // The provider here is on loopback, a private address: refused until you sign in with it.
    assert_eq!(fetch(up("/f.bin"), None).await.status(), 403);
    assert_eq!(sign_in(proxy, "127.0.0.1").await, 204);

    // Range request comes back as 206 with exactly the requested bytes.
    let r = fetch(up("/f.bin"), Some("bytes=10-19")).await;
    assert_eq!(r.status(), 206);
    assert_eq!(r.headers()["content-range"], "bytes 10-19/256");
    assert_eq!(r.headers()["content-length"], "10");
    assert_eq!(r.headers()["content-security-policy"], "sandbox");
    assert_eq!(
        r.bytes().await.unwrap().as_ref(),
        &(10..=19u8).collect::<Vec<_>>()[..]
    );

    // No Range: full body. Redirects to the same (approved) host are followed.
    assert_eq!(
        fetch(up("/f.bin"), None).await.bytes().await.unwrap().len(),
        256
    );
    let hopped = fetch(up("/hop"), None).await;
    // The client learns where the redirect ended up (needed to resolve relative playlist entries).
    assert_eq!(hopped.headers()["x-upstream-url"], up("/f.bin").as_str());
    assert_eq!(hopped.bytes().await.unwrap().len(), 256);

    // A redirect to a private address is refused, and the refusal names only the host (the URL
    // carries credentials).
    let evil = fetch(up("/evil"), None).await;
    assert_eq!(evil.status(), 502);
    // The same reason travels in a header, which is how the app finds and shows it.
    let header_why = evil.headers()["x-riptv-error"].to_str().unwrap().to_owned();
    let why = evil.text().await.unwrap();
    assert_eq!(header_why, why);
    assert!(
        why.contains("10.255.255.1") && why.contains("private") && !why.contains("http"),
        "{why}"
    );
    let evil = fetch(up("/evil-name"), None).await;
    assert_eq!(evil.status(), 502);
    assert_eq!(evil.text().await.unwrap(), "localhost is a private address");

    // A server that isn't listening: the page is told why, in words, without the URL.
    let down = fetch("http://127.0.0.1:1/x".into(), None).await;
    assert_eq!(down.status(), 502);
    let why = down.text().await.unwrap();
    assert!(
        why.starts_with("could not connect") && !why.contains("http"),
        "{why}"
    );

    // Straight requests for private addresses, junk, and other schemes.
    for private in [
        "http://192.168.1.1/",
        "http://169.254.169.254/latest/meta-data/",
        "http://[::1]/",
    ] {
        assert_eq!(fetch(private.into(), None).await.status(), 403, "{private}");
    }
    assert_eq!(fetch("not a url".into(), None).await.status(), 400);
    assert_eq!(fetch("file:///etc/passwd".into(), None).await.status(), 403);

    std::fs::remove_file(file).ok();
}

/// Providers commonly turn away requests with no user agent, so the browser's is passed on.
#[tokio::test]
async fn the_browsers_user_agent_reaches_the_provider() {
    let upstream = serve(Router::new().route(
        "/ua",
        get(|headers: axum::http::HeaderMap| async move {
            headers["user-agent"].to_str().unwrap().to_owned()
        }),
    ))
    .await;
    let proxy = serve(router(AppState::new())).await;
    assert_eq!(sign_in(proxy, "127.0.0.1").await, 204);
    let target = format!("http://127.0.0.1:{upstream}/ua");
    let ua = |agent: &'static str| {
        let mut req = reqwest::Client::new().get(proxied(proxy, &target));
        if !agent.is_empty() {
            req = req.header("user-agent", agent);
        }
        async move { req.send().await.unwrap().text().await.unwrap() }
    };
    assert_eq!(ua("Mozilla/5.0 Chrome/140").await, "Mozilla/5.0 Chrome/140");
    // No user agent, or an empty one: a fallback goes out instead.
    assert!(ua("").await.contains("RIPTV"));
}

/// A provider that compresses is passed through as it is: the browser, which asked for it, undoes
/// it as the body arrives. The proxy must neither decode it nor ask for what the browser didn't.
#[tokio::test]
async fn compression_is_left_to_the_browser() {
    const GZIP: &[u8] = &[
        31, 139, 8, 0, 0, 0, 0, 0, 2, 255, 203, 72, 205, 201, 201, 87, 200, 64, 39, 1, 227, 81, 61,
        141, 23, 0, 0, 0,
    ];
    let upstream = serve(Router::new().route(
        "/list",
        get(|headers: axum::http::HeaderMap| async move {
            let asked = headers
                .get("accept-encoding")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_owned();
            let mut answer = axum::http::HeaderMap::new();
            // What the provider was asked for, told in the body (the proxy passes no custom headers).
            if headers.contains_key("range") {
                return (answer, asked.into_bytes());
            }
            if asked.contains("gzip") {
                answer.insert("content-encoding", "gzip".parse().unwrap());
                (answer, GZIP.to_vec())
            } else {
                (answer, b"hello hello hello hello".to_vec())
            }
        }),
    ))
    .await;
    let proxy = serve(router(AppState::new())).await;
    assert_eq!(sign_in(proxy, "127.0.0.1").await, 204);
    let target = format!("http://127.0.0.1:{upstream}/list");
    // (This client is built without decompression too, so it sees what the proxy sent.)
    let get = |encoding: &'static str| {
        let req = reqwest::Client::new()
            .get(proxied(proxy, &target))
            .header("accept-encoding", encoding);
        async move { req.send().await.unwrap() }
    };

    let r = get("gzip, br").await;
    assert_eq!(r.headers()["content-encoding"], "gzip");
    assert_eq!(r.headers()["content-length"], GZIP.len().to_string());
    assert_eq!(
        r.bytes().await.unwrap().as_ref(),
        GZIP,
        "not decoded on the way"
    );

    // A `<video>` says it wants no encoding at all, and is sent none.
    let r = get("identity").await;
    assert!(r.headers().get("content-encoding").is_none());
    assert_eq!(r.text().await.unwrap(), "hello hello hello hello");

    // A byte range is of the file itself, whatever the browser says it can decompress.
    let ranged = reqwest::Client::new()
        .get(proxied(proxy, &target))
        .header("accept-encoding", "gzip")
        .header("range", "bytes=0-4")
        .send()
        .await
        .unwrap();
    assert!(ranged.headers().get("content-encoding").is_none());
    assert_eq!(ranged.text().await.unwrap(), "identity");
}

#[tokio::test]
async fn only_the_app_can_use_or_change_the_proxy() {
    let upstream = serve(Router::new().route("/ok", get(|| async { "hello" }))).await;
    let proxy = serve(router(AppState::new())).await;
    let target = format!("http://127.0.0.1:{upstream}/ok");
    let client = reqwest::Client::new();
    let status = |req: reqwest::RequestBuilder| async move { req.send().await.unwrap().status() };
    let allow = format!("http://127.0.0.1:{proxy}/allow?host=127.0.0.1");

    // Another website can't sign anything in, whether the browser labels the request
    // (`Sec-Fetch-Site`) or is old enough only to send `Origin`; and it doesn't stick.
    for (name, value) in [
        ("sec-fetch-site", "cross-site"),
        ("origin", "http://evil.example"),
    ] {
        assert_eq!(status(client.post(&allow).header(name, value)).await, 403);
    }
    // A page that pointed its own hostname at 127.0.0.1 (DNS rebinding) is same-origin with itself,
    // but its Host header gives it away.
    let rebinding = client
        .post(&allow)
        .header("host", "evil.example")
        .header("sec-fetch-site", "same-origin");
    assert_eq!(status(rebinding).await, 403);
    assert_eq!(status(client.get(proxied(proxy, &target))).await, 403);

    // The app itself is let in.
    let app = |req: reqwest::RequestBuilder| req.header("sec-fetch-site", "same-origin");
    assert_eq!(status(app(client.post(&allow))).await, 204);
    assert_eq!(status(app(client.get(proxied(proxy, &target)))).await, 200);

    // Once signed in, other sites and rebinding pages still can't use the proxy.
    let cross = client
        .get(proxied(proxy, &target))
        .header("sec-fetch-site", "cross-site");
    assert_eq!(status(cross).await, 403);
    let rebinding = client
        .get(proxied(proxy, &target))
        .header("host", "evil.example")
        .header("sec-fetch-site", "same-origin");
    assert_eq!(status(rebinding).await, 403);
}

async fn fake_api(Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    let get = |k: &str| q.get(k).map(String::as_str);
    if get("username") != Some("u") || get("password") != Some("p") {
        return Json(json!({"user_info": {"auth": 0}}));
    }
    Json(match get("action") {
        None => json!({"user_info": {"username": "u", "auth": 1}}),
        Some("get_live_categories") => json!([{"category_id": "1", "category_name": "News"}]),
        Some("get_series_info") => {
            json!({"info": {"name": "S"}, "episodes": {"1": [{"id": 9, "title": "Bloom"}]}})
        }
        _ => json!(false),
    })
}

/// The exact path the browser app takes: `xtream::Client` in proxy mode, through the real proxy,
/// signing in first.
#[tokio::test]
async fn client_via_proxy_end_to_end() {
    let file = std::env::temp_dir().join(format!("riptv-e2e-{}.bin", std::process::id()));
    std::fs::write(&file, (0..=255u8).collect::<Vec<_>>()).unwrap();
    let upstream = serve(
        Router::new()
            .route("/player_api.php", get(fake_api))
            .route_service("/movie/u/p/1.mp4", ServeFile::new(&file)),
    )
    .await;
    let proxy = serve(router(AppState::new())).await;

    let client = |pass: &str| {
        xtream::Client::new(&format!("http://127.0.0.1:{upstream}"), "u", pass)
            .unwrap()
            .via_proxy(&format!("http://127.0.0.1:{proxy}/proxy"))
            .unwrap()
    };
    let c = client("p");

    // Before signing in the proxy won't touch a private address, and says so.
    assert!(
        matches!(c.auth().await, Err(xtream::Error::Proxy(why)) if why == "address not allowed")
    );
    c.approve().await.unwrap();

    assert_eq!(c.auth().await.unwrap().user_info.username, "u");
    assert_eq!(c.live_categories().await.unwrap()[0].category_name, "News");
    assert_eq!(
        c.series_info(1).await.unwrap().seasons[0].episodes[0].title,
        "Bloom"
    );

    // Wrong password: the provider's rejection comes through the proxy intact.
    assert!(matches!(
        client("hunter2").auth().await,
        Err(xtream::Error::AuthFailed)
    ));

    // A `<video src>` URL built by the client seeks through the proxy.
    let r = reqwest::Client::new()
        .get(c.movie_url(1, "mp4"))
        .header("Range", "bytes=0-3")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 206);
    assert_eq!(r.bytes().await.unwrap().as_ref(), [0, 1, 2, 3]);

    std::fs::remove_file(file).ok();
}

#[tokio::test]
async fn serves_web_app_with_csp_and_spa_fallback() {
    let dir = std::env::temp_dir().join(format!("riptv-web-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "<!doctype html><title>app</title>").unwrap();
    let port = serve(router(AppState::new().with_web(&dir))).await;

    for path in ["/", "/some/client/route"] {
        let r = reqwest::get(format!("http://127.0.0.1:{port}{path}"))
            .await
            .unwrap();
        assert_eq!(r.status(), 200, "{path}");
        let csp = r.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(
            csp.contains("script-src 'self' 'wasm-unsafe-eval';"),
            "{csp}"
        );
        assert!(
            !csp.contains("'unsafe-eval'"),
            "scripts must not get plain eval: {csp}"
        );
        assert!(r.text().await.unwrap().contains("<title>app</title>"));
    }

    std::fs::remove_dir_all(dir).ok();
}

const HEVC_AC3: &[u8] = include_bytes!("../fixtures/hevc_ac3.ts");
const H264_AC3: &[u8] = include_bytes!("../fixtures/h264_ac3.ts");
const INTERLACED: &[u8] = include_bytes!("../fixtures/interlaced_576i.ts");
const MOVIE: &[u8] = include_bytes!("../fixtures/h264_ac3.mp4");

fn have(tool: &str) -> bool {
    std::process::Command::new(tool)
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// `ffprobe` on some bytes: the codecs it finds, and how many video frames it decodes.
fn probe_bytes(bytes: &[u8], name: &str) -> (Vec<String>, u32) {
    let file = std::env::temp_dir().join(format!("riptv-{name}-{}.mp4", std::process::id()));
    std::fs::write(&file, bytes).unwrap();
    let ffprobe = |args: &[&str]| {
        let out = std::process::Command::new("ffprobe")
            .args(["-v", "error"])
            .args(args)
            .arg(&file)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let codecs = ffprobe(&["-show_entries", "stream=codec_name", "-of", "csv=p=0"])
        .lines()
        .map(str::to_owned)
        .collect();
    let frames = ffprobe(&[
        "-count_frames",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=nb_read_frames",
        "-of",
        "csv=p=0",
    ])
    .trim()
    .parse()
    .unwrap_or(0);
    std::fs::remove_file(file).ok();
    (codecs, frames)
}

/// Compatibility mode with the real ffmpeg: what Chrome can't play (HEVC, AC-3) comes out as H.264
/// and AAC, and sound-only problems leave the video untouched. Skipped without ffmpeg installed.
#[tokio::test]
async fn compat_mode_turns_hevc_and_ac3_into_h264_and_aac() {
    if !have("ffmpeg") || !have("ffprobe") {
        eprintln!("ffmpeg isn't installed: skipping");
        return;
    }
    let audio_only = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=1",
            "-c:a",
            "aac",
            "-f",
            "mpegts",
            "pipe:1",
        ])
        .output()
        .unwrap();
    assert!(audio_only.status.success());
    let radio = audio_only.stdout;
    let upstream = serve(
        Router::new()
            .route(
                "/hevc.ts",
                get(|| async { ([("content-type", "video/mp2t")], HEVC_AC3) }),
            )
            .route(
                "/h264.ts",
                get(|| async { ([("content-type", "video/mp2t")], H264_AC3) }),
            )
            .route(
                "/movie.mp4",
                get(|| async { ([("content-type", "video/mp4")], MOVIE) }),
            )
            .route(
                "/i.ts",
                get(|| async { ([("content-type", "video/mp2t")], INTERLACED) }),
            )
            .route(
                "/radio.ts",
                get(move || {
                    let audio = radio.clone();
                    async move { ([("content-type", "video/mp2t")], audio) }
                }),
            ),
    )
    .await;
    let proxy = serve(router(AppState::new())).await;
    assert_eq!(sign_in(proxy, "127.0.0.1").await, 204);
    let client = reqwest::Client::new();
    let target = |name: &str| format!("http://127.0.0.1:{upstream}/{name}");
    let ask = |path: &str, name: &str, extra: &str| {
        let url = format!("http://127.0.0.1:{proxy}{path}?url={}{extra}", target(name));
        client.get(url).send()
    };

    // What needs re-encoding, and what only needs its sound fixed.
    let hevc = ask("/compat/check", "hevc.ts", "").await.unwrap();
    assert_eq!(
        (
            hevc.status(),
            hevc.headers()["x-riptv-video"].to_str().unwrap()
        ),
        (reqwest::StatusCode::NO_CONTENT, "transcode")
    );
    let h264 = ask("/compat/check", "h264.ts", "").await.unwrap();
    assert_eq!(
        (
            h264.status(),
            h264.headers()["x-riptv-video"].to_str().unwrap()
        ),
        (reqwest::StatusCode::NO_CONTENT, "copy")
    );
    assert_eq!(h264.headers()["x-riptv-has-video"], "1");
    let radio_check = ask("/compat/check", "radio.ts", "").await.unwrap();
    assert_eq!(radio_check.status(), 204);
    assert_eq!(radio_check.headers()["x-riptv-has-video"], "0");

    for (name, mode) in [("hevc.ts", "transcode"), ("h264.ts", "copy")] {
        let res = ask("/compat", name, &format!("&video={mode}"))
            .await
            .unwrap();
        assert_eq!(res.status(), 200, "{name}");
        assert_eq!(res.headers()["content-type"], "video/mp4");
        let (codecs, frames) = probe_bytes(&res.bytes().await.unwrap(), name);
        assert!(codecs.contains(&"h264".to_string()), "{name}: {codecs:?}");
        assert!(codecs.contains(&"aac".to_string()), "{name}: {codecs:?}");
        assert!(
            frames >= 90,
            "{name}: only {frames} of ~100 frames survived"
        );
    }

    // A movie's length comes with the check, and a conversion can start part-way in: that is
    // what the page's seek bar does, since a converted stream has nothing a `<video>` can seek in.
    // (Six seconds of AC-3 movie; a live stream has no length to report.)
    let movie = ask("/compat/check", "movie.mp4", "").await.unwrap();
    assert_eq!(movie.headers()["x-riptv-duration"], "6");
    assert!(
        h264.headers().get("x-riptv-duration").is_none(),
        "no length for a live stream"
    );
    let whole = ask("/compat", "movie.mp4", "&video=copy").await.unwrap();
    let (_, all_frames) = probe_bytes(&whole.bytes().await.unwrap(), "whole");
    let from_2s = ask("/compat", "movie.mp4", "&video=copy&start=2")
        .await
        .unwrap();
    let (_, later_frames) = probe_bytes(&from_2s.bytes().await.unwrap(), "later");
    assert!(
        later_frames * 100 >= all_frames * 55 && later_frames * 100 <= all_frames * 80,
        "starting 2s into 6s: {later_frames} of {all_frames} frames, expected about two thirds"
    );

    // Interlaced video (25 frames of two fields each, one second) is deinterlaced to full motion
    // rate: 50 progressive frames, not 25 combed ones.
    let i = ask("/compat/check", "i.ts", "").await.unwrap();
    assert_eq!(i.headers()["x-riptv-video"], "transcode");
    let res = ask("/compat", "i.ts", "&video=transcode").await.unwrap();
    let (codecs, frames) = probe_bytes(&res.bytes().await.unwrap(), "interlaced");
    assert!(codecs.contains(&"h264".to_string()), "{codecs:?}");
    assert!(
        (45..=55).contains(&frames),
        "{frames} frames; 50p from 25 interlaced is ~50"
    );

    // The client's one call, as the app makes it: from the address it plays, to a converted stream.
    let c = xtream::Client::new(&format!("http://127.0.0.1:{upstream}"), "u", "p")
        .unwrap()
        .via_proxy(&format!("http://127.0.0.1:{proxy}/proxy"))
        .unwrap();
    let forced = c
        .convert_video(&Url::parse(&target("h264.ts")).unwrap())
        .await
        .unwrap();
    assert!(
        forced
            .at(0)
            .query_pairs()
            .any(|(key, value)| key == "video" && value == "transcode")
    );
    let (codecs, frames) = probe_bytes(
        &client
            .get(forced.at(0))
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap(),
        "forced",
    );
    assert!(codecs.contains(&"h264".to_string()) && frames >= 90);
    assert!(matches!(
        c.convert_video(&Url::parse(&target("radio.ts")).unwrap()).await,
        Err(xtream::Error::Proxy(why)) if why.contains("no video track")
    ));
    let converted = c.convert(&c.live_url(0, "m3u8")).await;
    // (This server has no /live/u/p/0.ts, so ffprobe fails: the reason comes back.)
    assert!(
        matches!(&converted, Err(xtream::Error::Proxy(why)) if why.starts_with("could not read the stream")),
        "{converted:?}"
    );
    let playable = xtream::Url::parse(&format!("http://127.0.0.1:{upstream}/hevc.ts")).unwrap();
    let converted = c.convert(&playable).await.unwrap();
    assert_eq!(converted.duration, None, "a live stream has no length");
    assert!(converted.at(90).as_str().ends_with("&start=90"));
    assert!(!converted.at(0).as_str().contains("start="));
    let url = converted.at(0);
    assert!(
        url.as_str().contains("/compat?url=") && url.as_str().contains("video=transcode"),
        "{url}"
    );
    let (codecs, _) = probe_bytes(
        &reqwest::get(url).await.unwrap().bytes().await.unwrap(),
        "client",
    );
    assert!(
        codecs.contains(&"h264".to_string()) && codecs.contains(&"aac".to_string()),
        "{codecs:?}"
    );

    // A stream that can't be read says why; other sites can't ask at all.
    let down = client
        .get(format!(
            "http://127.0.0.1:{proxy}/compat/check?url=http://127.0.0.1:1/x.ts"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(down.status(), 502);
    assert!(
        down.headers()["x-riptv-error"]
            .to_str()
            .unwrap()
            .starts_with("could not read the stream")
    );
    let cross = client
        .get(format!(
            "http://127.0.0.1:{proxy}/compat/check?url={}",
            target("h264.ts")
        ))
        .header("sec-fetch-site", "cross-site")
        .send()
        .await
        .unwrap();
    assert_eq!(cross.status(), 403);
}
