//! Pass-through proxy so a browser page can reach IPTV servers, which usually send no CORS
//! headers and serve plain http. Also serves the compiled web app from the same origin.
//!
//! ponytail: a dumb byte pipe with Range support. HLS playlists are not rewritten because the
//! client resolves segment URLs itself. There is no host list to maintain; instead:
//!  - only the app itself may use the proxy (not another web page, not a DNS-rebinding page);
//!  - it only connects to public addresses, so a provider (or a redirect it sends) can't point it
//!    at the router or another machine on your network;
//!  - signing in approves that provider's own address, which is how a server on your LAN, or this
//!    machine, works. Bind to localhost only.

mod compat;

use std::{
    collections::HashSet,
    error::Error,
    fmt,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    extract::{Query, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use reqwest::{
    Url,
    dns::{Addrs, Name, Resolve, Resolving},
    redirect::Policy,
};
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

/// Says why the proxy itself refused or failed (a host name and a reason, never credentials). The
/// client shows it; errors from the provider pass through without it.
const ERROR_HEADER: HeaderName = HeaderName::from_static("x-riptv-error");

/// Used when the request carries none. Providers commonly turn away clients with no user agent.
pub(crate) const FALLBACK_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) RIPTV";

/// Most private-network hosts the user can approve in one run (each is a sign-in).
const MAX_APPROVED: usize = 100;

/// Hosts (lowercase, no port, no brackets) the user signed in with, allowed to be private.
type Approved = Arc<RwLock<HashSet<String>>>;

#[derive(Clone)]
pub struct AppState {
    http: reqwest::Client,
    pub(crate) approved: Approved,
    web: Option<PathBuf>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        let approved = Approved::default();
        let a = approved.clone();
        let http = reqwest::Client::builder()
            .dns_resolver(Arc::new(PublicOnly(approved.clone())))
            .redirect(Policy::custom(move |att| {
                if att.previous().len() >= 5 {
                    att.error("too many redirects")
                } else if url_ok(&a, att.url()) {
                    att.follow()
                } else {
                    let msg = format!(
                        "redirect to {} is not allowed (private address)",
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
            approved,
            web: None,
        }
    }

    /// Also serve the compiled web app in `dir` (a single-page app) at `/`.
    pub fn with_web(mut self, dir: impl Into<PathBuf>) -> Self {
        self.web = Some(dir.into());
        self
    }
}

/// Not reachable from the internet: loopback, LAN, link-local (including cloud metadata
/// addresses), carrier-grade NAT, and the unspecified, multicast and reserved ranges.
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !(v.is_loopback()
                || v.is_private()
                || v.is_link_local()
                || v.is_broadcast()
                || v.is_multicast()
                || o[0] == 0
                || o[0] >= 240
                || (o[0] == 100 && (64..128).contains(&o[1])))
        }
        IpAddr::V6(v) => match v.to_ipv4_mapped() {
            Some(v4) => is_public(IpAddr::V4(v4)),
            None => {
                let first = v.segments()[0];
                !(v.is_loopback()
                    || v.is_unspecified()
                    || v.is_multicast()
                    || first & 0xfe00 == 0xfc00 // unique local
                    || first & 0xffc0 == 0xfe80) // link local
            }
        },
    }
}

/// An address the proxy may connect to: public, or approved by a sign-in.
fn literal_ok(approved: &Approved, ip: IpAddr) -> bool {
    is_public(ip) || approved.read().is_ok_and(|a| a.contains(&ip.to_string()))
}

/// Checked on the first URL and on every redirect. Addresses written out as numbers are judged
/// here; names are judged by [`PublicOnly`] when the connection is made.
pub(crate) fn url_ok(approved: &Approved, url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    matches!(url.scheme(), "http" | "https")
        && host
            .trim_matches(['[', ']'])
            .parse::<IpAddr>()
            .map_or(true, |ip| literal_ok(approved, ip))
}

#[derive(Debug)]
struct Private(String);

impl fmt::Display for Private {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} is a private address", self.0)
    }
}

impl Error for Private {}

/// Name lookups that only return public addresses (unless the user signed in with that name).
/// Doing it here, on the addresses actually connected to, leaves no gap for DNS rebinding.
struct PublicOnly(Approved);

impl Resolve for PublicOnly {
    fn resolve(&self, name: Name) -> Resolving {
        let approved = self.0.clone();
        Box::pin(async move {
            let host = name.as_str().to_ascii_lowercase();
            let found: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            let signed_in = approved.read().is_ok_and(|a| a.contains(&host));
            let usable: Vec<SocketAddr> = found
                .into_iter()
                .filter(|a| signed_in || is_public(a.ip()))
                .collect();
            if usable.is_empty() {
                Err(Box::new(Private(host)) as Box<dyn Error + Send + Sync>)
            } else {
                Ok(Box::new(usable.into_iter()) as Addrs)
            }
        })
    }
}

/// Only the app itself may use the proxy. A page on another site must not (it could reach your
/// network through it), and neither may a page that made its own hostname point at 127.0.0.1
/// (DNS rebinding), which is why the `Host` header is checked too. Browsers label every request
/// with `Sec-Fetch-Site`; clients that don't (curl) send no `Origin` either.
pub(crate) fn from_app(h: &HeaderMap) -> bool {
    let host = h
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let name = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => host.split(':').next().unwrap_or(""),
    };
    let same_site = match h.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        Some(site) => site == "same-origin",
        None => !h.contains_key(header::ORIGIN),
    };
    matches!(name, "127.0.0.1" | "localhost" | "::1") && same_site
}

pub(crate) fn refusal(status: StatusCode, why: &str) -> Response {
    let header_safe: String = why
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() || c == ' ' {
                c
            } else {
                '?'
            }
        })
        .take(300)
        .collect();
    let mut res = (status, header_safe.clone()).into_response();
    if let Ok(v) = HeaderValue::from_str(&header_safe) {
        res.headers_mut().insert(ERROR_HEADER, v);
    }
    res
}

pub fn router(state: AppState) -> Router {
    let r = Router::new()
        .route("/proxy", get(proxy))
        .route("/allow", post(allow))
        .route("/compat/check", get(compat::check))
        .route("/compat", get(compat::stream));
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
struct AllowQuery {
    host: String,
}

/// Sign-in calls this with the provider's host, so a provider on a private network works.
async fn allow(
    State(s): State<AppState>,
    Query(q): Query<AllowQuery>,
    headers: HeaderMap,
) -> Response {
    if !from_app(&headers) {
        return refusal(StatusCode::FORBIDDEN, "only the app may use the proxy");
    }
    let host = q.host.trim().trim_matches(['[', ']']).to_ascii_lowercase();
    if host.is_empty() || host.len() > 253 {
        return refusal(StatusCode::BAD_REQUEST, "bad host");
    }
    if let Ok(mut approved) = s.approved.write()
        && approved.len() < MAX_APPROVED
    {
        approved.insert(host);
    }
    StatusCode::NO_CONTENT.into_response()
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
    if !from_app(&headers) {
        return refusal(StatusCode::FORBIDDEN, "only the app may use the proxy");
    }
    let Ok(url) = Url::parse(&q.url) else {
        return refusal(StatusCode::BAD_REQUEST, "bad url");
    };
    if !url_ok(&s.approved, &url) {
        return refusal(StatusCode::FORBIDDEN, "address not allowed");
    }

    // Pass the browser's own user agent on: to the provider this looks like a player, not a script.
    let ua = headers
        .get(header::USER_AGENT)
        .filter(|v| !v.is_empty())
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static(FALLBACK_UA));
    let mut req = s.http.get(url).header(header::USER_AGENT, ua);
    for h in [header::RANGE, header::IF_RANGE] {
        if let Some(v) = headers.get(&h) {
            req = req.header(h, v);
        }
    }
    // Error text would include the upstream URL and its credentials, so it is never passed on.
    // The exceptions are our own refusals, which name only a hostname.
    let up = match req.send().await {
        Ok(up) => up,
        Err(e) => {
            let mut private = None;
            let mut cause: Option<&(dyn Error + 'static)> = Some(&e);
            while let Some(c) = cause {
                if let Some(p) = c.downcast_ref::<Private>() {
                    private = Some(p.to_string());
                    break;
                }
                cause = c.source();
            }
            let redirect = e
                .is_redirect()
                .then(|| e.source().map(ToString::to_string))
                .flatten();
            // Otherwise the causes underneath ("dns error: ...", "invalid peer certificate: ...");
            // the top-level message is skipped because reqwest puts the URL in it.
            let mut causes = vec![];
            let mut next = e.source();
            while let Some(c) = next {
                let text = c.to_string();
                if !text.starts_with("client error") {
                    causes.push(text);
                }
                next = c.source();
            }
            let why = private
                .or(redirect)
                .unwrap_or_else(|| format!("could not connect: {}", causes.join(": ")));
            return refusal(StatusCode::BAD_GATEWAY, &why);
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
        .unwrap_or_else(|_| refusal(StatusCode::BAD_GATEWAY, "bad upstream response"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn only_internet_addresses_count_as_public() {
        for public in [
            "8.8.8.8",
            "1.1.1.1",
            "45.33.32.156",
            "2606:4700:4700::1111",
            "100.63.0.1",
            "172.32.0.1",
        ] {
            assert!(is_public(ip(public)), "{public}");
        }
        for private in [
            "127.0.0.1",
            "127.8.8.8",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "100.127.255.255",
            "0.0.0.0",
            "224.0.0.1",
            "255.255.255.255",
            "::1",
            "::",
            "fe80::1",
            "fd00::1",
            "fc00::1",
            "ff02::1",
            "::ffff:192.168.0.1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!is_public(ip(private)), "{private}");
        }
    }

    #[test]
    fn urls_are_judged_by_their_address_and_sign_in_lifts_a_private_one() {
        let approved = Approved::default();
        let ok = |u: &str| url_ok(&approved, &Url::parse(u).unwrap());
        assert!(
            ok("http://8.8.8.8/x") && ok("https://example.com/x") && ok("http://[2606:4700::1]/")
        );
        // Numbers in any spelling the URL parser understands are still numbers.
        for private in [
            "http://127.0.0.1/",
            "http://2130706433/",
            "http://0x7f.1/",
            "http://[::1]/",
            "http://192.168.1.1:8080/",
        ] {
            assert!(!ok(private), "{private}");
        }
        assert!(!ok("file:///etc/passwd") && !ok("ftp://8.8.8.8/"));

        approved.write().unwrap().insert("192.168.1.1".into());
        assert!(ok("http://192.168.1.1:8080/x") && !ok("http://192.168.1.2/"));
    }

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        pairs
            .iter()
            .map(|(k, v)| (HeaderName::from_static(k), HeaderValue::from_static(v)))
            .collect()
    }

    #[test]
    fn only_the_app_is_let_in() {
        let app = [
            ("host", "127.0.0.1:3000"),
            ("sec-fetch-site", "same-origin"),
        ];
        assert!(from_app(&headers(&app)));
        assert!(
            from_app(&headers(&[("host", "localhost:3000")])),
            "curl-like: no browser headers"
        );
        assert!(from_app(&headers(&[("host", "[::1]:3000")])));
        // Another site, even with a loopback Host.
        assert!(!from_app(&headers(&[
            ("host", "127.0.0.1:3000"),
            ("sec-fetch-site", "cross-site")
        ])));
        assert!(!from_app(&headers(&[
            ("host", "127.0.0.1:3000"),
            ("sec-fetch-site", "same-site")
        ])));
        // An old browser posting from elsewhere still sends Origin.
        assert!(!from_app(&headers(&[
            ("host", "127.0.0.1:3000"),
            ("origin", "http://evil.example")
        ])));
        // DNS rebinding: the page is same-origin with *its own* name.
        assert!(!from_app(&headers(&[
            ("host", "evil.example:3000"),
            ("sec-fetch-site", "same-origin")
        ])));
        assert!(
            !from_app(&headers(&[("sec-fetch-site", "same-origin")])),
            "no Host at all"
        );
    }

    /// The real resolver, on a name every machine resolves without a network: `localhost`.
    #[tokio::test]
    async fn names_that_resolve_to_private_addresses_are_refused_unless_signed_in() {
        let approved = Approved::default();
        let resolver = PublicOnly(approved.clone());
        let name = || "localhost".parse::<Name>().unwrap();
        let refused = resolver.resolve(name()).await.err().expect("refused");
        assert_eq!(refused.to_string(), "localhost is a private address");

        approved.write().unwrap().insert("localhost".into());
        assert!(resolver.resolve(name()).await.unwrap().next().is_some());
    }
}
