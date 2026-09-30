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

#[tokio::test]
async fn range_passthrough_and_allowlist() {
    let file = std::env::temp_dir().join(format!("riptv-test-{}.bin", std::process::id()));
    std::fs::write(&file, (0..=255u8).collect::<Vec<_>>()).unwrap();

    let upstream = serve(
        Router::new()
            .route_service("/f.bin", ServeFile::new(&file))
            .route("/hop", get(|| async { Redirect::temporary("/f.bin") }))
            .route(
                "/evil",
                get(|| async { Redirect::temporary("http://blocked.invalid/x") }),
            ),
    )
    .await;
    let proxy = serve(router(AppState::new(["127.0.0.1".into()]))).await;

    let fetch = |target: String, range: Option<&'static str>| async move {
        let url = Url::parse_with_params(
            &format!("http://127.0.0.1:{proxy}/proxy"),
            [("url", target)],
        )
        .unwrap();
        let mut req = reqwest::Client::new().get(url);
        if let Some(r) = range {
            req = req.header("Range", r);
        }
        req.send().await.unwrap()
    };
    let up = |path: &str| format!("http://127.0.0.1:{upstream}{path}");

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

    // No Range: full body. Allowed redirects are followed.
    assert_eq!(
        fetch(up("/f.bin"), None).await.bytes().await.unwrap().len(),
        256
    );
    let hopped = fetch(up("/hop"), None).await;
    // The client learns where the redirect ended up (needed to resolve relative playlist entries).
    assert_eq!(hopped.headers()["x-upstream-url"], up("/f.bin").as_str());
    assert_eq!(hopped.bytes().await.unwrap().len(), 256);

    // Not allowed: unlisted host, redirect to an unlisted host, junk url, non-http scheme.
    assert_eq!(
        fetch("http://example.com/x".into(), None).await.status(),
        403
    );
    let evil = fetch(up("/evil"), None).await;
    assert_eq!(evil.status(), 502);
    // The refusal names the offending host, and only the host (the URL carries credentials).
    let why = evil.text().await.unwrap();
    assert!(
        why.contains("blocked.invalid") && why.contains("allow list") && !why.contains("http"),
        "{why}"
    );
    assert_eq!(fetch("not a url".into(), None).await.status(), 400);
    assert_eq!(fetch("file:///etc/passwd".into(), None).await.status(), 403);

    std::fs::remove_file(file).ok();
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

/// The exact path the browser app takes: `xtream::Client` in proxy mode, through the real proxy.
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
    let proxy = serve(router(AppState::new(["127.0.0.1".into()]))).await;

    let c = xtream::Client::new(&format!("http://127.0.0.1:{upstream}"), "u", "p")
        .unwrap()
        .via_proxy(&format!("http://127.0.0.1:{proxy}/proxy"))
        .unwrap();

    assert_eq!(c.auth().await.unwrap().user_info.username, "u");
    assert_eq!(c.live_categories().await.unwrap()[0].category_name, "News");
    assert_eq!(
        c.series_info(1).await.unwrap().seasons[0].episodes[0].title,
        "Bloom"
    );

    // Wrong password: the provider's rejection comes through the proxy intact.
    let bad = xtream::Client::new(&format!("http://127.0.0.1:{upstream}"), "u", "hunter2")
        .unwrap()
        .via_proxy(&format!("http://127.0.0.1:{proxy}/proxy"))
        .unwrap();
    assert!(matches!(bad.auth().await, Err(xtream::Error::AuthFailed)));

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
    let port = serve(router(AppState::new(Vec::<String>::new()).with_web(&dir))).await;

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
