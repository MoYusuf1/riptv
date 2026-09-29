//! Env: IPTV_ALLOW (hostnames the proxy may reach, comma-separated, including redirect targets),
//! IPTV_PORT (default 3000), IPTV_WEB (built web app, default: the `dx build` output).

use iptv_proxy::{AppState, router};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let env = |k: &str| std::env::var(k).ok();

    let allow: Vec<String> = env("IPTV_ALLOW")
        .unwrap_or_default()
        .split(',')
        .map(|h| h.trim().to_ascii_lowercase())
        .filter(|h| !h.is_empty())
        .collect();

    let web = env("IPTV_WEB").unwrap_or_else(|| "target/dx/app/release/web/public".into());
    if !std::path::Path::new(&web).join("index.html").exists() {
        eprintln!(
            "warning: no web app in {web}; run `dx build --web --release -p app` (or set IPTV_WEB)"
        );
    }

    let port: u16 = env("IPTV_PORT")
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    // ponytail: localhost only. This proxies to allowlisted hosts without auth; add auth before binding wider.
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("bind");
    println!("http://127.0.0.1:{port}   allowed upstream hosts: {allow:?}");
    axum::serve(listener, router(AppState::new(allow).with_web(web)))
        .await
        .expect("serve");
}
