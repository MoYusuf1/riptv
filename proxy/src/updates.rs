//! Bounded, cached checks against our public release page; never send IPTV credentials.
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use xtream::updates::UpdateInfo;

const RELEASES: &str = "https://api.github.com/repos/MoYusuf1/riptv/releases/latest";
const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
pub struct Checker {
    http: reqwest::Client,
    cache: Arc<Mutex<Option<(Instant, UpdateInfo)>>>,
}

impl Default for Checker {
    fn default() -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent(concat!("RIPTV/", env!("CARGO_PKG_VERSION")))
                .timeout(Duration::from_secs(8))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("update client"),
            cache: Arc::new(Mutex::new(None)),
        }
    }
}

impl Checker {
    pub async fn check(&self, force: bool) -> UpdateInfo {
        let mut cache = self.cache.lock().await;
        if let Some((at, info)) = &*cache {
            let ttl = if force {
                60
            } else if info.checked {
                6 * 3600
            } else {
                15 * 60
            };
            if at.elapsed() < Duration::from_secs(ttl) {
                return info.clone();
            }
        }
        let info = self.fetch().await.unwrap_or_else(|| UpdateInfo {
            current: CURRENT.into(),
            latest: None,
            available: false,
            checked: false,
            download: None,
        });
        *cache = Some((Instant::now(), info.clone()));
        info
    }

    async fn fetch(&self) -> Option<UpdateInfo> {
        let mut response = self
            .http
            .get(RELEASES)
            .header("accept", "application/vnd.github+json")
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            if bytes.len() + chunk.len() > 1024 * 1024 {
                return None;
            }
            bytes.extend_from_slice(&chunk);
        }
        let release: Release = serde_json::from_slice(&bytes).ok()?;
        status(CURRENT, release, platform_asset())
    }
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    state: String,
}

fn version(s: &str) -> Option<[u64; 3]> {
    let pieces: Vec<_> = s.strip_prefix('v').unwrap_or(s).split('.').collect();
    if pieces.len() != 3 {
        return None;
    }
    let mut out = [0; 3];
    for (i, piece) in pieces.iter().enumerate() {
        if piece.is_empty()
            || !piece.bytes().all(|b| b.is_ascii_digit())
            || (piece.len() > 1 && piece.starts_with('0'))
        {
            return None;
        }
        out[i] = piece.parse().ok()?;
    }
    Some(out)
}

fn platform_asset() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("RIPTV-Windows.exe"),
        ("linux", "x86_64") => Some("RIPTV-Linux"),
        ("macos", "aarch64") => Some("RIPTV-Mac-AppleSilicon.zip"),
        ("macos", "x86_64") => Some("RIPTV-Mac-Intel.zip"),
        _ => None,
    }
}

fn status(current: &str, release: Release, asset: Option<&str>) -> Option<UpdateInfo> {
    if release.draft || release.prerelease {
        return None;
    }
    let newer = version(&release.tag_name)?;
    let available = newer > version(current)?;
    let tag = format!("v{}.{}.{}", newer[0], newer[1], newer[2]);
    let download = asset
        .filter(|name| {
            release
                .assets
                .iter()
                .any(|a| a.name == *name && a.state == "uploaded")
        })
        .map_or_else(
            || format!("https://github.com/MoYusuf1/riptv/releases/tag/{tag}"),
            |name| format!("https://github.com/MoYusuf1/riptv/releases/download/{tag}/{name}"),
        );
    Some(UpdateInfo {
        current: current.into(),
        latest: Some(tag[1..].into()),
        available,
        checked: true,
        download: available.then_some(download),
    })
}

pub(crate) async fn automatic(State(s): State<crate::AppState>, headers: HeaderMap) -> Response {
    respond(s, headers, false).await
}
pub(crate) async fn manual(State(s): State<crate::AppState>, headers: HeaderMap) -> Response {
    respond(s, headers, true).await
}
async fn respond(s: crate::AppState, headers: HeaderMap, force: bool) -> Response {
    if !crate::from_app(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut response = Json(s.updates.check(force).await).into_response();
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release(tag: &str) -> Release {
        Release {
            tag_name: tag.into(),
            draft: false,
            prerelease: false,
            assets: vec![Asset {
                name: "RIPTV-Linux".into(),
                state: "uploaded".into(),
            }],
        }
    }
    #[test]
    fn compares_versions_numerically() {
        assert!(
            status("0.2.9", release("v0.2.10"), Some("RIPTV-Linux"))
                .unwrap()
                .available
        );
        assert!(!status("0.2.10", release("v0.2.9"), None).unwrap().available);
        assert!(
            !status("0.2.10", release("v0.2.10"), None)
                .unwrap()
                .available
        );
    }
    #[test]
    fn rejects_untrusted_tags_and_preview_releases() {
        for tag in ["v0.3.0-beta", "../bad", "0.3", "v00.3.0", "0.3.0?x"] {
            assert!(status("0.2.1", release(tag), None).is_none());
        }
        let mut preview = release("v0.3.0");
        preview.prerelease = true;
        assert!(status("0.2.1", preview, None).is_none());
    }
    #[test]
    fn constructs_only_our_download_urls() {
        let found = status("0.2.1", release("v0.3.0"), Some("RIPTV-Linux")).unwrap();
        assert_eq!(
            found.download.as_deref(),
            Some("https://github.com/MoYusuf1/riptv/releases/download/v0.3.0/RIPTV-Linux")
        );
        let fallback = status("0.2.1", release("v0.3.0"), Some("missing")).unwrap();
        assert_eq!(
            fallback.download.as_deref(),
            Some("https://github.com/MoYusuf1/riptv/releases/tag/v0.3.0")
        );
    }
    #[tokio::test]
    async fn shares_cached_status_between_clones() {
        let checker = Checker::default();
        let info = status(CURRENT, release("v9.0.0"), None).unwrap();
        *checker.cache.lock().await = Some((Instant::now(), info.clone()));
        assert_eq!(checker.clone().check(false).await, info);
        assert_eq!(checker.check(true).await, info);
    }
    #[tokio::test]
    async fn app_checks_share_the_cli_cache_and_reject_other_sites() {
        let state = crate::AppState::new();
        let info = status(CURRENT, release("v9.0.0"), None).unwrap();
        *state.update_checker().cache.lock().await = Some((Instant::now(), info.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/updates", listener.local_addr().unwrap());
        let task =
            tokio::spawn(async move { axum::serve(listener, crate::router(state)).await.unwrap() });
        let client = reqwest::Client::new();
        for method in [reqwest::Method::GET, reqwest::Method::POST] {
            let response = client.request(method.clone(), &url).send().await.unwrap();
            assert_eq!(response.status(), 200);
            assert_eq!(response.headers()["cache-control"], "no-store");
            assert_eq!(
                serde_json::from_slice::<UpdateInfo>(&response.bytes().await.unwrap()).unwrap(),
                info
            );
            let cross_site = client
                .request(method, &url)
                .header("origin", "https://other.example")
                .send()
                .await
                .unwrap();
            assert_eq!(cross_site.status(), 403);
        }
        task.abort();
    }
}
