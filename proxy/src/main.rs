//! Env: IPTV_PORT (default 3000), IPTV_WEB (built web app, default: the `dx build` output).

use riptv::{AppState, router};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

fn web_root(override_path: Option<String>, executable: Option<PathBuf>) -> PathBuf {
    if let Some(path) = override_path {
        return path.into();
    }
    if let Some(dir) = executable.and_then(|p| p.parent().map(|p| p.to_owned())) {
        let packaged = dir.join("web");
        if packaged.join("index.html").is_file() {
            return packaged;
        }
    }
    "target/dx/app/release/web/public".into()
}

fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    let result = Command::new("cmd")
        .args(["/C", "start", "", url])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    #[cfg(target_os = "macos")]
    let result = Command::new("open")
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let result = Command::new("xdg-open")
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if result.is_err() {
        println!("Open {url} in your browser.");
    }
}

async fn shutdown() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut terminate) = signal(SignalKind::terminate()) {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
            return;
        }
    }
    #[cfg(windows)]
    {
        if let Ok(mut close) = tokio::signal::windows::ctrl_close() {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = close.recv() => {} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let env = |k: &str| std::env::var(k).ok();

    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("RIPTV {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "RIPTV {}\nUsage: riptv [--logs] [--open | --no-open]\n       riptv --check-updates\n\nDownloads open your browser automatically. Keep this window open while watching.\nPress Ctrl+C to stop. Set IPTV_PORT to change port 3000.",
            env!("CARGO_PKG_VERSION")
        );
        return;
    }
    if args.as_slice() == ["--check-updates"] {
        let info = riptv::updates::Checker::default().check(true).await;
        if !info.checked {
            eprintln!("Couldn’t check for updates. Try again later.");
            std::process::exit(1);
        }
        if let Some(url) = info.download {
            println!(
                "RIPTV {} is ready. Download: {url}",
                info.latest.unwrap_or_default()
            );
        } else {
            println!("RIPTV {} is up to date.", info.current);
        }
        return;
    }
    if let Some(arg) = args
        .iter()
        .find(|a| !matches!(a.as_str(), "--logs" | "--open" | "--no-open"))
    {
        eprintln!("Unknown option: {arg}. Use --help for options.");
        std::process::exit(2);
    }
    let _bundle = match riptv::bundle::prepare() {
        Ok(bundle) => bundle,
        Err(error) => {
            eprintln!("Could not prepare RIPTV: {error}");
            std::process::exit(1);
        }
    };
    let web = env("IPTV_WEB")
        .map(PathBuf::from)
        .or_else(|| riptv::bundle::root().map(|p| p.join("web")))
        .unwrap_or_else(|| web_root(None, std::env::current_exe().ok()));
    let packaged = _bundle.is_some()
        || std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("web")))
            .is_some_and(|p| p.join("index.html").is_file());
    if !web.join("index.html").exists() {
        eprintln!(
            "warning: no web app in {}; download the RIPTV app, or run the source setup script (or set IPTV_WEB)",
            web.display()
        );
    }

    let port: u16 = env("IPTV_PORT")
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let logs = args.iter().any(|arg| arg == "--logs");
    let mut state = AppState::new().with_web(web.to_string_lossy().into_owned());
    if logs {
        let path = std::env::temp_dir().join("riptv-diagnostics.log");
        println!("Sanitized stream diagnostics: {}", path.display());
        state = state.with_logs(path);
    }
    // ponytail: localhost only. Anyone who can reach the port can use it (see the checks in lib.rs
    // for what that means); add auth before binding wider.
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!(
                "Could not start RIPTV on port {port}: {error}\nAnother copy may already be running. Close it or choose another IPTV_PORT."
            );
            drop(_bundle);
            std::process::exit(1);
        }
    };
    println!("RIPTV is running: http://127.0.0.1:{port}");
    println!("Keep this window open while watching. Press Ctrl+C to stop.");
    let checker = state.update_checker();
    tokio::spawn(async move {
        let mut announced = None;
        loop {
            let info = checker.check(false).await;
            if info.available && info.latest != announced {
                println!(
                    "RIPTV {} is ready. Download: {}",
                    info.latest.as_deref().unwrap_or_default(),
                    info.download.as_deref().unwrap_or_default()
                );
                announced = info.latest;
            }
            tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
        }
    });
    if !args.iter().any(|a| a == "--no-open") && (packaged || args.iter().any(|a| a == "--open")) {
        open_browser(&format!("http://127.0.0.1:{port}"));
    }
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown())
        .await
        .expect("serve");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_web_path_wins() {
        assert_eq!(
            web_root(Some("custom web".into()), Some("/app/riptv".into())),
            PathBuf::from("custom web")
        );
    }

    #[test]
    fn source_build_keeps_its_existing_default() {
        assert_eq!(
            web_root(None, None),
            PathBuf::from("target/dx/app/release/web/public")
        );
    }
}
