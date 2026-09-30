//! Env: IPTV_PORT (default 3000), IPTV_WEB (built web app, default: the `dx build` output).

use riptv::{AppState, router};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let env = |k: &str| std::env::var(k).ok();

    let web = env("IPTV_WEB").unwrap_or_else(|| "target/dx/app/release/web/public".into());
    if !std::path::Path::new(&web).join("index.html").exists() {
        eprintln!(
            "warning: no web app in {web}; run `dx build --web --release -p app` (or set IPTV_WEB)"
        );
    }

    let port: u16 = env("IPTV_PORT")
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    // ponytail: localhost only. Anyone who can reach the port can use it (see the checks in lib.rs
    // for what that means); add auth before binding wider.
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("bind");
    println!("RIPTV is running: http://127.0.0.1:{port}");
    axum::serve(listener, router(AppState::new().with_web(web)))
        .await
        .expect("serve");
}
