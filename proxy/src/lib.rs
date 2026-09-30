//! Pass-through proxy so a browser page can reach IPTV servers, which usually send no CORS
//! headers and serve plain http. Also serves the compiled web app from the same origin.
//!
//! ponytail: a dumb byte pipe with Range support. HLS playlists are not rewritten because the
//! client resolves segment URLs itself. Hosts are allowlisted (this is otherwise an open
//! proxy), so bind to localhost only.

use std::{collections::HashSet, path::PathBuf, sync::Arc, time::Duration};

use axum::{
    Router,
    body::Body,
    extract::{Query, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use reqwest::{Url, redirect::Policy};
use serde::Deserialize;
use tower_http::{
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
};

/// Policy for the web app. It's same-origin with no inline scripts; `'wasm-unsafe-eval'` is
/// what lets the browser compile WebAssembly (it does not allow `eval`). Images may come from
/// anywhere because providers host channel logos and posters themselves.
const APP_CSP: &str = "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'unsafe-inline'; \
    img-src http: https: data:; media-src 'self' blob:; connect-src 'self'; base-uri 'none'; form-action 'none'; \
    frame-ancestors 'none'";

/// Response headers worth forwarding. Everything else (cookies, CORS, hop-by-hop) is dropped.
const PASS: [HeaderName; 6] = [
    header::CONTENT_TYPE,
    header::CONTENT_LENGTH, // valid because reqwest is built without decompression
    header::CONTENT_RANGE,
    header::ACCEPT_RANGES,
    header::ETAG,
    header::LAST_MODIFIED,
];

#[derive(Clone)]
pub struct AppState {
    http: reqwest::Client,
    allow: Arc<HashSet<String>>,
    web: Option<PathBuf>,
}

impl AppState {
    /// `allow` is a list of hostnames (no port) the proxy may fetch from, including redirect targets.
    pub fn new(allow: impl IntoIterator<Item = String>) -> Self {
        let allow: Arc<HashSet<String>> = Arc::new(allow.into_iter().collect());
        let a = allow.clone();
        let http = reqwest::Client::builder()
            .redirect(Policy::custom(move |att| {
                if att.previous().len() >= 5 {
                    att.error("too many redirects")
                } else if host_ok(&a, att.url()) {
                    att.follow()
                } else {
                    let msg = format!(
                        "redirect to {} is not allowed",
                        att.url().host_str().unwrap_or("another host")
                    );
                    att.error(msg)
                }
            }))
            // Connect timeout only: a total timeout would cut off live streams.
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("http client");
        Self {
            http,
            allow,
            web: None,
        }
    }

    /// Also serve the compiled web app in `dir` (a single-page app) at `/`.
    pub fn with_web(mut self, dir: impl Into<PathBuf>) -> Self {
        self.web = Some(dir.into());
        self
    }
}

fn host_ok(allow: &HashSet<String>, url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url
            .host_str()
            .is_some_and(|h| allow.contains(&h.to_ascii_lowercase()))
}

pub fn router(state: AppState) -> Router {
    let r = Router::new().route("/proxy", get(proxy));
    let r = match state.web.clone() {
        // Unknown paths get index.html with a 200 (`not_found_service` would keep the 404),
        // so client-side routes survive a reload.
        Some(dir) => {
            r.fallback_service(ServeDir::new(&dir).fallback(ServeFile::new(dir.join("index.html"))))
        }
        None => r,
    };
    // `if_not_present` keeps the stricter `sandbox` policy that `/proxy` responses set themselves.
    r.layer(SetResponseHeaderLayer::if_not_present(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(APP_CSP),
    ))
    .with_state(state)
}

#[derive(Deserialize)]
struct ProxyQuery {
    url: String,
}

async fn proxy(
    State(s): State<AppState>,
    Query(q): Query<ProxyQuery>,
    headers: HeaderMap,
) -> Response {
    let Ok(url) = Url::parse(&q.url) else {
        return (StatusCode::BAD_REQUEST, "bad url").into_response();
    };
    if !host_ok(&s.allow, &url) {
        return (StatusCode::FORBIDDEN, "host not allowed").into_response();
    }

    let mut req = s.http.get(url);
    for h in [header::RANGE, header::IF_RANGE] {
        if let Some(v) = headers.get(&h) {
            req = req.header(h, v);
        }
    }
    // Error text would include the upstream URL and its credentials, so it is never passed on.
    // The one exception is our own redirect refusal, which names only a hostname.
    let up = match req.send().await {
        Ok(up) => up,
        Err(e) => {
            let why = std::error::Error::source(&e)
                .filter(|_| e.is_redirect())
                .map(|s| format!("{s} (add its host to the allow list)"))
                .unwrap_or_else(|| "upstream unreachable".into());
            return (StatusCode::BAD_GATEWAY, why).into_response();
        }
    };

    let mut res = Response::builder().status(up.status());
    for h in PASS {
        if let Some(v) = up.headers().get(&h) {
            res = res.header(h, v);
        }
    }
    // Where redirects ended up. HLS playlists list segments relative to that URL, which the
    // client never sees otherwise. The caller already knows the URL it asked for, so this leaks nothing.
    if let Ok(v) = HeaderValue::from_str(up.url().as_str()) {
        res = res.header(HeaderName::from_static("x-upstream-url"), v);
    }
    res
        // Upstream content shares our origin: keep it from running scripts if someone opens it directly.
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::CONTENT_SECURITY_POLICY, "sandbox")
        // Streamed, never buffered; dropping the client connection drops the upstream request.
        .body(Body::from_stream(up.bytes_stream()))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}
