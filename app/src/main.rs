//! Dioxus web UI. It talks to IPTV servers only through the local proxy
//! (`xtream::Client::via_proxy`), because servers send no CORS headers.
//!
//! ponytail: credentials live in memory only (reload = log in again), no favourites,
//! no adaptive bitrate, and live streams must be HLS with MPEG-TS segments (H.264 + AAC).

use dioxus::prelude::*;
use std::{cell::RefCell, rc::Rc};

use web_sys::wasm_bindgen::JsCast;
use xtream::{Client, LiveStream, VodStream};

const SHOW: usize = 300;

const CSS: &str = "
:root { color-scheme: dark; --bg: #080808; --panel: #181818; --soft: #2a2a2a; --fg: #f5f5f5; --dim: #a8a8a8; --red: #e50914; --line: #363636; }
* { box-sizing: border-box; }
html { background: var(--bg); }
body { margin: 0; min-width: 320px; background: var(--bg); color: var(--fg); font: 14px/1.45 system-ui, -apple-system, BlinkMacSystemFont, \"Segoe UI\", sans-serif; }
button, input { font: inherit; color: inherit; }
button { cursor: pointer; }
.dim { color: var(--dim); } .err { color: #ff7c82; } .note { background: #231719; border-left: 3px solid var(--red); padding: .65rem .9rem; margin: 0 0 1rem; }
.brand { color: var(--red); font-size: 1.45rem; font-weight: 900; letter-spacing: -.07em; line-height: 1; text-transform: uppercase; }
.login-page { min-height: 100vh; padding: 1.4rem 3vw; background: radial-gradient(circle at 50% 0, #1b2c45 0, transparent 37%), #0b0d12; }
.login-page > .brand { display: inline-flex; align-items: center; gap: .55rem; color: #e6ebf4; font-size: .9rem; letter-spacing: .08em; }
.login-page > .brand .rust-mark { width: 1.9rem; height: 1.9rem; color: #91b9e6; }
.login { width: min(100%, 28rem); margin: 10vh auto 0; display: flex; flex-direction: column; gap: .85rem; padding: 2.25rem; border: 1px solid #272d38; border-radius: 12px; background: rgba(18,21,28,.96); box-shadow: 0 28px 80px rgba(0,0,0,.45); }
.login h1 { margin: 0; color: #eef2f8; font-size: 1.65rem; letter-spacing: -.025em; } .login .intro { margin: 0 0 .65rem; color: #8992a2; }
.login label.field { display: grid; gap: .35rem; color: #aeb6c4; font-size: .72rem; font-weight: 650; }
.login input { width: 100%; padding: .8rem .9rem; background: #171b23; border: 1px solid #303743; border-radius: 7px; outline: 0; }
.login input:focus { border-color: #5689c5; box-shadow: 0 0 0 3px rgba(86,137,197,.13); }
.login button { padding: .78rem 1rem; border-radius: 7px; border: 0; background: #5791d2; color: #08111d; font-weight: 750; }
.login button:hover { background: #6da6e7; } .login button:disabled { opacity: .65; cursor: wait; }
.login button.ghost { background: #222833; color: #c9d0dc; } .login button.ghost:hover { background: #2a313e; color: #fff; }
.login-divider { display: flex; align-items: center; gap: .7rem; color: #666f7e; font-size: .65rem; text-transform: uppercase; } .login-divider::before, .login-divider::after { content: ''; flex: 1; height: 1px; background: #2b313c; }
.topbar { position: sticky; z-index: 20; top: 0; display: flex; gap: 1.5rem; align-items: center; min-height: 4rem; padding: .65rem 3.5vw; background: #05080f; border-bottom: 1px solid #151a24; }
.topbar .brand { flex: 0 0 auto; } .nav { display: flex; gap: .25rem; align-items: center; }
.topbar .grow { flex: 1; }
.tab, .out, .cat { border: 0; background: transparent; }
.tab { padding: .5rem .65rem; color: #c7c7c7; transition: color .15s; } .tab:hover { color: #fff; } .tab.on { color: #fff; font-weight: 700; }
.search { position: relative; display: flex; align-items: center; }
.search::before { content: '⌕'; position: absolute; left: .75rem; top: .28rem; font-size: 1.4rem; color: #ddd; pointer-events: none; transform: rotate(-18deg); }
.search input { width: 13rem; padding: .55rem .75rem .55rem 2.3rem; background: #111; border: 1px solid #777; border-radius: 3px; outline: none; transition: width .2s, border-color .2s; }
.search input:focus { width: 17rem; border-color: #fff; }
.out { padding: .52rem .8rem; border-radius: 4px; color: #ccc; } .out:hover { color: #fff; background: #2a2a2a; }
.browse { padding: 0 3.5vw 4rem; }
.page-head { display: flex; justify-content: space-between; align-items: end; min-height: 8.5rem; padding: 2rem 0 1.35rem; }
.page-head h1 { margin: 0; font-size: clamp(2rem, 3vw, 2.75rem); letter-spacing: -.04em; }
.page-head .count { margin: 0 0 .35rem; color: #818898; }
.layout { display: grid; grid-template-columns: 14rem minmax(0, 1fr); gap: 2rem; align-items: start; }
.categories { position: sticky; top: 5.1rem; max-height: calc(100vh - 6.5rem); padding-right: .45rem; overflow: auto; scrollbar-width: thin; }
.categories h2 { margin: 0 0 .6rem; color: #8a91a0; font-size: .72rem; letter-spacing: .13em; text-transform: uppercase; }
.category-search { width: 100%; margin-bottom: .65rem; padding: .62rem .7rem; border: 1px solid #2b303a; border-radius: 5px; outline: 0; background: #11151d; color: #fff; }
.category-search:focus { border-color: #697386; }
.cat { display: block; width: 100%; padding: .5rem .7rem; border-radius: 5px; color: #9fa6b5; text-align: left; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cat:hover { color: #fff; background: #151a23; } .cat.on { color: #fff; background: #282e3a; font-weight: 650; box-shadow: inset 3px 0 var(--red); }
.shelf-head { display: flex; justify-content: space-between; align-items: baseline; gap: 1rem; margin-bottom: .8rem; }
.shelf-head h2 { margin: 0; font-size: 1.45rem; letter-spacing: -.02em; } .shelf-head p { margin: 0; font-size: .82rem; }
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(230px, 1fr)); gap: 1.4rem .8rem; }
.row { position: relative; min-width: 0; padding: 0; border: 0; border-radius: 5px; overflow: hidden; background: #171717; color: #fff; text-align: left; aspect-ratio: 16/9; box-shadow: 0 6px 18px rgba(0,0,0,.28); isolation: isolate; transition: transform .18s ease, box-shadow .18s ease; }
.row:hover, .row:focus-visible { z-index: 2; transform: scale(1.045); box-shadow: 0 13px 32px #000; outline: 2px solid rgba(255,255,255,.55); }
.ico { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; background: #222; }
.fallback { position: absolute; inset: 0; display: grid; place-items: center; background: linear-gradient(145deg, #391014, #141414 62%); color: rgba(255,255,255,.65); font-size: 2.2rem; font-weight: 900; }
.card-shade { position: absolute; inset: 32% 0 0; background: linear-gradient(transparent, rgba(0,0,0,.94)); }
.card-title { position: absolute; z-index: 1; left: .75rem; right: .75rem; bottom: .58rem; overflow: hidden; font-weight: 700; text-overflow: ellipsis; white-space: nowrap; text-shadow: 0 2px 5px #000; }
.empty { min-height: 14rem; display: grid; place-items: center; border: 1px dashed #333; border-radius: 6px; color: #888; }
.overlay { position: fixed; z-index: 50; inset: 0; display: grid; place-items: center; padding: 4vh 4vw; background: rgba(0,0,0,.9); }
.player, .series { width: min(100%, 70rem); max-height: 92vh; overflow: auto; background: #090909; border-radius: 8px; box-shadow: 0 24px 90px #000; }
.bar { display: flex; gap: 1rem; justify-content: space-between; align-items: center; min-height: 3.4rem; padding: .7rem 1rem; background: var(--panel); }
.bar strong { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.player video { width: 100%; max-height: 58vh; display: block; background: #000; }
.guide { padding: 1rem; background: #0d0f13; } .guide-head { display: flex; justify-content: space-between; align-items: baseline; gap: 1rem; margin-bottom: .8rem; } .guide h2 { margin: 0; font-size: 1.15rem; }
.guide-list { display: grid; grid-template-columns: repeat(auto-fit, minmax(13rem, 1fr)); gap: .55rem; }
.programme { min-width: 0; padding: .8rem; border-left: 3px solid #3f4654; border-radius: 3px; background: #181c24; } .programme:first-child { border-left-color: var(--red); background: #211719; }
.programme time { display: block; margin-bottom: .25rem; color: #929aaa; font-size: .75rem; } .programme strong { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; } .programme p { display: -webkit-box; margin: .35rem 0 0; overflow: hidden; color: #aeb4c0; font-size: .8rem; -webkit-box-orient: vertical; -webkit-line-clamp: 2; }
.series { padding: 0 1rem 1.2rem; } .series .bar { position: sticky; top: 0; margin: 0 -1rem .75rem; }
.series h3 { margin: 1.2rem .4rem .35rem; } .ep { display: block; width: 100%; padding: .72rem .8rem; border: 0; border-bottom: 1px solid #292929; border-radius: 3px; background: transparent; color: #ddd; text-align: left; } .ep:hover { background: #292929; color: #fff; }
@media (max-width: 800px) { .topbar { gap: .7rem; flex-wrap: wrap; padding: .8rem 1rem; } .topbar .brand { margin-right: auto; } .nav { order: 3; width: 100%; } .tab { flex: 1; } .search input, .search input:focus { width: 8.5rem; } .browse { padding: 0 1rem 3rem; } .page-head { min-height: 6.5rem; } .layout { grid-template-columns: 1fr; gap: 1rem; } .categories { position: static; display: flex; gap: .35rem; max-height: none; overflow-x: auto; padding-bottom: .45rem; } .categories h2, .category-search { display: none; } .cat { width: auto; flex: 0 0 auto; padding: .45rem .8rem; background: #181818; } .cat.on { box-shadow: inset 0 -2px var(--red); } .grid { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
@media (max-width: 480px) { .login-page { padding: 1.2rem; } .login { margin-top: 7vh; padding: 1.5rem; } .topbar .out { display: none; } .search input, .search input:focus { width: 7.6rem; } .grid { grid-template-columns: 1fr 1fr; } }
@media (prefers-reduced-motion: reduce) { * { scroll-behavior: auto !important; transition: none !important; } }

/* Sanctum desktop shell */
:root { --shell: #101218; --pane: #12151c; --pane-2: #171b24; --pane-3: #20263a; --blue: #69a7ff; --blue-soft: #263a59; }
body { background: #0b0d12; color: #dce1eb; }
.topbar { position: fixed; inset: 0 0 auto 0; height: 3.4rem; min-height: 3.4rem; padding: .45rem .8rem .45rem 3.6rem; gap: .55rem; background: #101218; border-color: #222630; }
.app-logo { position: fixed; z-index: 30; top: .52rem; left: .48rem; display: grid; width: 2.35rem; height: 2.35rem; place-items: center; color: #d6dce6; }
.rust-mark { width: 1.8rem; height: 1.8rem; }
.account { position: relative; }
.brandbox { position: relative; display: flex; flex-direction: column; justify-content: center; width: 13rem; height: 2.45rem; padding: .25rem 2rem .25rem .7rem; border: 1px solid #282e39; border-radius: 7px; background: #191d25; text-align: left; cursor: pointer; }
.brandbox strong { overflow: hidden; color: #e8ebf2; font-size: .78rem; letter-spacing: .01em; text-overflow: ellipsis; white-space: nowrap; } .brandbox small { color: #7f8796; font-size: .6rem; }
.brandbox::after { content: '⌄'; position: absolute; top: 50%; right: .7rem; color: #87909f; line-height: 1; transform: translateY(-55%); }
.account-menu { position: absolute; z-index: 40; top: calc(100% + .45rem); left: 0; width: 13rem; padding: .35rem; border: 1px solid #303642; border-radius: 8px; background: #191d25; box-shadow: 0 14px 35px rgba(0,0,0,.45); }
.account-menu button { width: 100%; padding: .62rem .7rem; border: 0; border-radius: 6px; background: transparent; color: #c7ced9; text-align: left; } .account-menu button:hover { background: #252b36; color: #fff; }
.topbar > .brand { display: none; }
.nav { position: fixed; z-index: 25; inset: 3.4rem auto 0 0; display: flex; flex-direction: column; width: 3.25rem; padding: .55rem .35rem; gap: .3rem; background: #11141b; border-right: 1px solid #242936; }
.nav .tab { position: relative; width: 2.5rem; height: 2.5rem; padding: 0; overflow: hidden; border-radius: 8px; color: transparent; text-indent: -999px; }
.nav .tab svg { position: absolute; inset: .68rem; width: 1.14rem; height: 1.14rem; color: #aeb6c4; text-indent: 0; }
.nav .tab:hover { background: #1e2330; } .nav .tab.on { background: #25344d; outline: 1px solid #4675ad; }
.nav .tab.on svg { color: #77b1ff; }
.topbar .search { flex: 0 1 29rem; height: 2.45rem; margin-left: .25rem; }
.topbar .search::before { display: none; }
.topbar .search svg { position: absolute; z-index: 1; left: .8rem; width: 1rem; height: 1rem; color: #768092; pointer-events: none; }
.topbar .search input, .topbar .search input:focus { width: 100%; height: 100%; padding: 0 1rem 0 2.35rem; background: #171b23; border-color: #2c323e; border-radius: 8px; }
.topbar .search input:focus { border-color: #4e79ad; box-shadow: 0 0 0 3px rgba(82,137,201,.12); }
.topbar .search input::placeholder { color: #6f7786; }
.top-actions { display: flex; gap: .2rem; }
.icon-action { display: grid; place-items: center; width: 2.35rem; height: 2.35rem; border: 0; border-radius: 7px; background: transparent; color: #9ea7b6; } .icon-action:hover { background: #222733; color: #fff; } .icon-action svg { width: 1.05rem; height: 1.05rem; }
.topbar .out { color: #8d95a3; }
.workspace { position: fixed; inset: 3.4rem 0 0 3.25rem; min-height: 0; }
.live-shell { display: grid; grid-template-columns: 16rem 20rem minmax(0, 1fr); height: 100%; min-height: 0; background: #0b0d12; }
.category-panel, .channel-panel { min-width: 0; min-height: 0; background: var(--pane); border-right: 1px solid #282d38; }
.channel-panel { background: var(--pane-2); }
.panel-head { display: flex; align-items: center; justify-content: space-between; min-height: 3.1rem; padding: .65rem .8rem; background: #171a22; border-bottom: 1px solid #282d38; }
.channel-panel .panel-head { background: #22283e; }
.panel-head h2 { margin: 0; font-size: .8rem; } .panel-head span { color: #8e96a6; font-size: .68rem; }
.category-scroll, .channel-scroll { height: calc(100% - 3.1rem); overflow: auto; scrollbar-width: thin; }
.category-panel .categories { position: static; max-height: none; padding: .55rem .45rem 1rem; overflow: visible; }
.category-panel .categories > h2 { display: none; }
.category-panel .category-search { margin: 0 0 .45rem; background: #181c24; }
.category-panel .cat { padding: .52rem .7rem; color: #bcc2ce; }
.category-panel .cat.on { background: #263a59; outline: 1px solid #4778b1; box-shadow: none; color: #78b1ff; }
.channel-list { padding: .3rem .35rem 1rem; }
.channel { display: grid; grid-template-columns: 2.6rem minmax(0, 1fr) 1rem; gap: .65rem; align-items: center; width: 100%; min-height: 4.05rem; padding: .5rem .55rem; border: 1px solid transparent; border-radius: 7px; background: transparent; color: #d4d9e3; text-align: left; }
.channel:hover { background: #202631; } .channel.on { background: #263a59; border-color: #5489c8; }
.channel-logo { display: grid; place-items: center; width: 2.6rem; height: 2.6rem; overflow: hidden; border-radius: 7px; background: #292e38; color: #8f97a5; font-size: .56rem; font-weight: 800; }
.channel-logo img { width: 100%; height: 100%; object-fit: contain; }
.channel-copy { min-width: 0; } .channel-copy strong, .channel-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.channel-copy strong { font-size: .78rem; } .channel-copy small { margin-top: .18rem; color: #858d9c; font-size: .67rem; }
.channel-star { color: #8991a0; font-size: 1rem; }
.stage-panel { min-width: 0; min-height: 0; overflow: hidden; background: #000; }
.stage-empty { height: 100%; display: grid; place-items: center; background: radial-gradient(circle, #11151d, #000 60%); color: #7d8594; text-align: center; }
.stage-empty strong { display: block; margin-bottom: .35rem; color: #cbd1dc; font-size: 1rem; }
.live-stage { display: grid; grid-template-rows: minmax(0, 1fr) auto; width: 100%; height: 100%; background: #000; }
.video-wrap { position: relative; min-height: 0; display: grid; place-items: center; overflow: hidden; }
.live-stage video { width: 100%; height: 100%; max-height: none; object-fit: contain; background: #000; }
.live-stage .bar { position: absolute; z-index: 2; top: 0; right: 0; left: 0; min-height: 2.8rem; background: linear-gradient(rgba(0,0,0,.82), transparent); }
.live-stage .bar .out { color: #fff; }
.live-stage .guide { min-height: 12.5rem; padding: .7rem .8rem 1rem; border-top: 1px solid #283049; background: #1c2232; }
.live-stage .guide-head { margin-bottom: .65rem; } .live-stage .guide-head h2 { overflow: hidden; font-size: .88rem; text-overflow: ellipsis; white-space: nowrap; }
.live-stage .guide-list { display: flex; gap: .45rem; overflow-x: auto; padding-bottom: .35rem; scroll-snap-type: x proximity; }
.live-stage .programme { position: relative; flex: 0 0 15rem; min-height: 7.1rem; padding: .82rem; overflow: hidden; background: #171c27; border: 1px solid #303849; border-left: 1px solid #303849; border-radius: 7px; scroll-snap-align: start; }
.live-stage .programme:first-child { background: linear-gradient(135deg, #2b4364, #21344f); border-color: #5792d5; }
.programme .on-now { position: absolute; top: .65rem; right: .65rem; padding: .15rem .35rem; border-radius: 3px; background: #67a8ef; color: #0c1724; font-size: .55rem; font-weight: 850; letter-spacing: .06em; }
.programme-progress { position: absolute; right: 0; bottom: 0; left: 0; height: 3px; background: rgba(255,255,255,.1); } .programme-progress::after { content: ''; display: block; width: 42%; height: 100%; background: #73b3fa; }
.media-stage { display: grid; grid-template-rows: auto minmax(0,1fr); width: 100%; height: 100%; background: #000; }
.media-stage .bar { min-height: 3rem; background: #171b23; border-bottom: 1px solid #282e38; }
.media-stage video { width: 100%; height: 100%; min-height: 0; object-fit: contain; background: #000; }
.stage-panel > .series { width: 100%; height: 100%; max-height: none; margin: 0; border-radius: 0; background: #12151c; box-shadow: none; }
.media-browser { display: grid; grid-template-columns: 16rem minmax(0,1fr); height: 100%; min-height: 0; background: #11141b; }
.media-browser > .category-panel { height: 100%; }
.poster-content { min-width: 0; height: 100%; overflow: auto; background: #141821; }
.media-head { position: sticky; z-index: 3; top: 0; display: flex; align-items: center; min-height: 3.1rem; padding: .6rem 1rem; background: #22283e; border-bottom: 1px solid #30374a; }
.media-head h1 { display: inline; margin: 0 .6rem 0 0; font-size: .85rem; } .media-head span { color: #8790a1; font-size: .68rem; }
.poster-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(8.2rem, 1fr)); gap: 1.25rem .7rem; padding: 1rem; }
.poster { min-width: 0; padding: 0; border: 0; background: transparent; color: #d9dee8; text-align: left; }
.poster-art { position: relative; display: block; overflow: hidden; aspect-ratio: 2/3; border: 1px solid #2c323d; border-radius: 7px; background: linear-gradient(145deg, #26344a, #181c24); box-shadow: 0 5px 15px rgba(0,0,0,.22); transition: border-color .15s, transform .15s; }
.poster:hover .poster-art, .poster:focus-visible .poster-art { transform: translateY(-2px); border-color: #6597d0; }
.poster-art img { width: 100%; height: 100%; object-fit: cover; }
.poster-art > span { display: grid; height: 100%; place-items: center; color: #7598c1; font-size: 2rem; font-weight: 800; }
.poster-art em { position: absolute; top: .35rem; right: .35rem; padding: .1rem .3rem; border-radius: 4px; background: #173c2a; color: #6bd596; font-size: .52rem; font-style: normal; font-weight: 800; }
.poster > strong { display: block; margin: .45rem .15rem 0; overflow: hidden; font-size: .7rem; font-weight: 550; text-overflow: ellipsis; white-space: nowrap; }
.library-shell { height: 100%; overflow: auto; padding: 1.4rem 2rem 4rem; background: #0d1016; }
.library-shell .page-head { min-height: 5rem; padding: .5rem 0 1rem; }
.library-shell .layout { grid-template-columns: 14rem minmax(0, 1fr); }
.library-shell .categories { top: 0; max-height: calc(100vh - 7rem); }
@media (max-width: 900px) { .topbar { padding-left: .7rem; } .brandbox { width: 9rem; } .nav { position: static; width: auto; flex-direction: row; padding: 0; border: 0; background: transparent; } .nav .tab { width: 2.25rem; height: 2.25rem; } .workspace { left: 0; } .live-shell { grid-template-columns: 12rem 16rem minmax(0,1fr); } }
@media (max-width: 700px) { .brandbox { display: none; } .topbar .search { flex: 1; } .topbar .search input, .topbar .search input:focus { padding-left: 2.2rem; } .crumb, .key, .topbar .search::after { display: none; } .live-shell { display: block; overflow: auto; } .category-panel, .channel-panel { height: auto; max-height: 18rem; } .stage-panel { height: 70vh; } .library-shell { padding: 1rem; } }
";

fn main() {
    dioxus::launch(App);
}

/// Same-origin proxy endpoint, e.g. `http://127.0.0.1:3000/proxy`.
fn proxy_url() -> String {
    let origin = web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_default();
    format!("{origin}/proxy")
}

async fn login(url: &str, user: &str, pass: &str) -> xtream::Result<Client> {
    let c = Client::new(url, user, pass)?.via_proxy(&proxy_url())?;
    c.auth().await?;
    Ok(c)
}

fn explain(e: &xtream::Error) -> String {
    let s = e.to_string();
    if s.contains("403") {
        format!(
            "{s}. The proxy only reaches hosts it was started with: restart it with IPTV_ALLOW=<that host>."
        )
    } else {
        s
    }
}

fn epg_clock(value: &str) -> String {
    let clock = value.rsplit([' ', 'T']).next().unwrap_or(value);
    if clock.len() >= 5 && clock.as_bytes().get(2) == Some(&b':') {
        clock[..5].to_string()
    } else {
        value.to_string()
    }
}

#[component]
fn App() -> Element {
    let session = use_context_provider(|| Signal::new(None::<Client>));
    let _playlist = use_context_provider(|| Signal::new("IPTV".to_string()));
    rsx! {
        // No `document::Title`: Dioxus web sets it via eval(), which the app's CSP forbids.
        // The title comes from Dioxus.toml instead.
        style { "{CSS}" }
        if session.read().is_some() { Browse {} } else { Login {} }
    }
}

#[component]
fn RustMark() -> Element {
    rsx! {
        svg {
            class: "rust-mark",
            view_box: "0 0 64 64",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "4",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            circle { cx: "32", cy: "32", r: "21" }
            path { d: "M32 2v8M32 54v8M2 32h8M54 32h8M11 11l6 6M47 47l6 6M53 11l-6 6M17 47l-6 6M20 5l3 8M44 51l3 8M5 20l8 3M51 44l8 3M44 5l-3 8M23 51l-3 8M59 20l-8 3M13 41l-8 3" }
            path { d: "M23 44V20h11c6 0 10 4 10 9s-4 9-10 9H23M34 38l11 7" }
        }
    }
}

#[component]
fn Login() -> Element {
    let mut session = use_context::<Signal<Option<Client>>>();
    let mut playlist = use_context::<Signal<String>>();
    let mut url = use_signal(String::new);
    let mut user = use_signal(String::new);
    let mut pass = use_signal(String::new);
    let mut status = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);

    let connect = move |(u, n, p, label): (String, String, String, String)| {
        spawn(async move {
            busy.set(true);
            status.set(None);
            match login(&u, &n, &p).await {
                Ok(c) => {
                    playlist.set(label);
                    session.set(Some(c));
                }
                Err(e) => status.set(Some(explain(&e))),
            }
            busy.set(false);
        });
    };

    rsx! {
        div { class: "login-page",
            div { class: "brand", RustMark {} "RIPTV" }
            form { class: "login",
                onsubmit: move |e| {
                    e.prevent_default();
                    let label = xtream::Url::parse(&url())
                        .ok()
                        .and_then(|u| u.host_str().map(str::to_owned))
                        .unwrap_or_else(|| "IPTV".to_string());
                    connect((url(), user(), pass(), label));
                },
                h1 { "Sign in" }
                input { aria_label: "Server URL", placeholder: "Server URL", value: "{url}", oninput: move |e| url.set(e.value()) }
                input { aria_label: "Username", autocomplete: "username", placeholder: "Username", value: "{user}", oninput: move |e| user.set(e.value()) }
                input { r#type: "password", aria_label: "Password", autocomplete: "current-password", placeholder: "Password", value: "{pass}", oninput: move |e| pass.set(e.value()) }
                button { r#type: "submit", disabled: busy(), if busy() { "Connecting…" } else { "Connect" } }
                div { class: "login-divider", "or" }
                button { r#type: "button", class: "ghost",
                    onclick: move |_| connect(("http://127.0.0.1:8081".into(), "demo".into(), "demo".into(), "Test".into())),
                    "Demo"
                }
                if let Some(msg) = status() { p { class: "err", "{msg}" } }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Live,
    Movies,
    Series,
}

#[derive(Clone, PartialEq)]
enum Target {
    Live { id: u64, url: String },
    Play(String),
    Series(u64),
}

#[derive(Clone, PartialEq)]
struct Row {
    key: u64,
    title: String,
    icon: Option<String>,
    target: Target,
}

enum Items {
    Live(Vec<LiveStream>),
    Movies(Vec<VodStream>),
    Series(Vec<xtream::Series>),
}

/// The rows to show for a category: items whose title contains `q` (lowercase), at most `SHOW`,
/// plus how many matched. Only the rows actually shown are built (that includes making a stream
/// URL), so a 20,000-entry category stays cheap on every keystroke in the filter box.
fn rows(items: &Items, c: &Client, q: &str) -> (Vec<Row>, usize) {
    fn take<T>(
        list: &[T],
        q: &str,
        title: impl Fn(&T) -> &str,
        row: impl Fn(&T) -> Row,
    ) -> (Vec<Row>, usize) {
        let mut hits = list
            .iter()
            .filter(|t| q.is_empty() || title(t).to_lowercase().contains(q));
        let shown: Vec<Row> = hits.by_ref().take(SHOW).map(&row).collect();
        let total = shown.len() + hits.count();
        (shown, total)
    }
    match items {
        Items::Live(v) => take(
            v,
            q,
            |s| &s.name,
            |s| Row {
                key: s.stream_id,
                title: s.name.clone(),
                icon: s.stream_icon.clone(),
                target: Target::Live {
                    id: s.stream_id,
                    url: c.live_url(s.stream_id, "m3u8").to_string(),
                },
            },
        ),
        Items::Movies(v) => take(
            v,
            q,
            |s| &s.name,
            |s| {
                let ext = s
                    .container_extension
                    .as_deref()
                    .filter(|e| !e.is_empty())
                    .unwrap_or("mp4");
                Row {
                    key: s.stream_id,
                    title: s.name.clone(),
                    icon: s.stream_icon.clone(),
                    target: Target::Play(c.movie_url(s.stream_id, ext).to_string()),
                }
            },
        ),
        Items::Series(v) => take(
            v,
            q,
            |s| &s.name,
            |s| Row {
                key: s.series_id,
                title: s.name.clone(),
                icon: s.cover.clone(),
                target: Target::Series(s.series_id),
            },
        ),
    }
}

#[component]
fn Browse() -> Element {
    let mut session = use_context::<Signal<Option<Client>>>();
    let playlist = use_context::<Signal<String>>();
    let client = use_hook(|| {
        session
            .read()
            .clone()
            .expect("Browse only renders when logged in")
    });

    let mut kind = use_signal(|| Kind::Live);
    let mut category = use_signal(|| None::<u64>);
    let mut search = use_signal(String::new);
    let mut category_search = use_signal(String::new);
    let mut playing = use_signal(|| None::<(String, String)>); // (title, url)
    let mut live = use_signal(|| None::<(u64, String, String)>); // (id, title, playlist url)
    let mut open_series = use_signal(|| None::<u64>);
    let mut notice = use_signal(|| None::<String>);
    let mut account_open = use_signal(|| false);
    let mut player_revision = use_signal(|| 0_u64);

    let (c_cats, c_items) = (client.clone(), client.clone());
    let cats = use_resource(move || {
        let (c, k) = (c_cats.clone(), kind());
        async move {
            match k {
                Kind::Live => c.live_categories().await,
                Kind::Movies => c.vod_categories().await,
                Kind::Series => c.series_categories().await,
            }
        }
    });
    let items = use_resource(move || {
        let (c, k, cat) = (c_items.clone(), kind(), category());
        async move {
            Ok::<_, xtream::Error>(match k {
                Kind::Live => Items::Live(c.live_streams(cat).await?),
                Kind::Movies => Items::Movies(c.vod_streams(cat).await?),
                Kind::Series => Items::Series(c.series(cat).await?),
            })
        }
    });

    let mut pick_kind = move |k: Kind| {
        kind.set(k);
        category.set(None);
        search.set(String::new());
        category_search.set(String::new());
        open_series.set(None);
        notice.set(None);
        playing.set(None);
        live.set(None);
    };
    let pick = move |row: Row| match row.target {
        Target::Live { id, url } => {
            notice.set(None);
            playing.set(None);
            live.set(Some((id, row.title, url)));
        }
        Target::Play(url) => {
            notice.set(None);
            live.set(None);
            open_series.set(None);
            playing.set(Some((row.title, url)));
        }
        Target::Series(id) => {
            playing.set(None);
            open_series.set(Some(id));
        }
    };

    let cat_list = match &*cats.read() {
        None => rsx! { p { class: "dim", "Loading..." } },
        Some(Err(e)) => rsx! { p { class: "err", "{e}" } },
        Some(Ok(list)) => rsx! {
            h2 { "Categories" }
            input {
                class: "category-search",
                aria_label: "Filter categories",
                placeholder: "Filter categories…",
                value: "{category_search}",
                oninput: move |e| category_search.set(e.value())
            }
            button {
                class: if category().is_none() { "cat on" } else { "cat" },
                onclick: move |_| {
                    category.set(None);
                    search.set(String::new());
                    open_series.set(None);
                },
                "All"
            }
            for c in list.iter().filter(|c| {
                let q = category_search().to_lowercase();
                q.is_empty() || c.category_name.to_lowercase().contains(&q)
            }) {
                button {
                    key: "{c.category_id}",
                    class: if category() == Some(c.category_id) { "cat on" } else { "cat" },
                    onclick: {
                        let id = c.category_id;
                        move |_| {
                            category.set(Some(id));
                            search.set(String::new());
                            open_series.set(None);
                        }
                    },
                    "{c.category_name}"
                }
            }
        },
    };

    let item_list = match &*items.read() {
        None => rsx! { div { class: "empty", "Loading your library…" } },
        Some(Err(e)) => rsx! { p { class: "err", "{e}" } },
        Some(Ok(it)) => {
            let (shown, _total) = rows(it, &client, &search().to_lowercase());
            rsx! {
                if shown.is_empty() {
                    div { class: "empty", "No titles match your search." }
                } else if kind() == Kind::Live {
                    div { class: "channel-scroll",
                        div { class: "channel-list",
                            for r in shown {
                                ChannelItem {
                                    key: "{r.key}",
                                    row: r.clone(),
                                    active: live().as_ref().map(|(id, _, _)| *id) == Some(r.key),
                                    onpick: pick
                                }
                            }
                        }
                    }
                } else {
                    div { class: "poster-grid",
                        for r in shown {
                            PosterItem { key: "{r.key}", row: r.clone(), onpick: pick }
                        }
                    }
                }
            }
        }
    };

    let player = playing().map(|(title, url)| {
        rsx! {
            div { class: "overlay",
                div { class: "player",
                    div { class: "bar", strong { "{title}" } button { class: "out", onclick: move |_| playing.set(None), "×" } }
                    video { key: "{url}", controls: true, autoplay: true, src: "{url}" }
                }
            }
        }
    });
    let live_panel = live().map(|(id, title, url)| {
        let player_key = format!("{url}-{}", player_revision());
        rsx! { LivePlayer { key: "{player_key}", id, title, url, onclose: move |_| live.set(None) } }
    });
    let note = notice().map(|n| rsx! { p { class: "note", "{n}" } });
    let series_panel = open_series().map(|id| {
        rsx! {
            div { class: "overlay",
                SeriesView {
                    key: "{id}",
                    id,
                    onplay: move |t: (String, String)| { live.set(None); playing.set(Some(t)); },
                    onclose: move |_| open_series.set(None),
                }
            }
        }
    });

    let tab = |k: Kind| if kind() == k { "tab on" } else { "tab" };
    let page_name = match kind() {
        Kind::Live => "Live TV",
        Kind::Movies => "Movies",
        Kind::Series => "Series",
    };
    let category_name = match &*cats.read() {
        Some(Ok(list)) => category()
            .and_then(|id| list.iter().find(|c| c.category_id == id))
            .map(|c| c.category_name.clone())
            .unwrap_or_else(|| "All items".to_string()),
        _ => "All items".to_string(),
    };
    let playlist_name = playlist();
    rsx! {
        header { class: "topbar",
            div { class: "app-logo", RustMark {} }
            div { class: "account",
                button {
                    class: "brandbox",
                    title: "Account and connection",
                    aria_expanded: account_open(),
                    onclick: move |_| account_open.set(!account_open()),
                    strong { "{playlist_name}" }
                    small { "Xtream Code" }
                }
                if account_open() {
                    div { class: "account-menu",
                        if live().is_some() {
                            button {
                                onclick: move |_| {
                                    player_revision += 1;
                                    account_open.set(false);
                                },
                                "Refresh stream"
                            }
                        }
                        button { onclick: move |_| session.set(None), "Disconnect" }
                    }
                }
            }
            nav { class: "nav", aria_label: "Library",
                button { class: tab(Kind::Live), title: "Live TV", aria_label: "Live TV", onclick: move |_| pick_kind(Kind::Live),
                    svg { view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "1.8",
                        rect { x: "3", y: "5", width: "18", height: "13", rx: "2" }
                        path { d: "M8 21h8M12 18v3M9 9l6 3-6 3V9z" }
                    }
                }
                button { class: tab(Kind::Movies), title: "Movies", aria_label: "Movies", onclick: move |_| pick_kind(Kind::Movies),
                    svg { view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "1.8",
                        rect { x: "3", y: "4", width: "18", height: "16", rx: "2" }
                        path { d: "M7 4v16M17 4v16M3 9h4M3 15h4M17 9h4M17 15h4" }
                    }
                }
                button { class: tab(Kind::Series), title: "Series", aria_label: "Series", onclick: move |_| pick_kind(Kind::Series),
                    svg { view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "1.8",
                        rect { x: "4", y: "4", width: "16", height: "5", rx: "1.5" }
                        rect { x: "4", y: "11", width: "16", height: "9", rx: "1.5" }
                        path { d: "M8 15h8" }
                    }
                }
            }
            label { class: "search",
                svg { view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: "2", path { d: "m21 21-4.35-4.35M19 11a8 8 0 1 1-16 0 8 8 0 0 1 16 0Z" } }
                input { aria_label: "Search this section", placeholder: "Search {page_name}…", value: "{search}", oninput: move |e| search.set(e.value()) }
            }
            span { class: "grow" }
        }
        main { class: "workspace",
            if kind() == Kind::Live {
                div { class: "live-shell",
                    aside { class: "category-panel",
                        div { class: "panel-head", h2 { "Categories" } }
                        div { class: "category-scroll",
                            div { class: "categories", {cat_list} }
                        }
                    }
                    section { class: "channel-panel",
                        div { class: "panel-head", h2 { "{category_name}" } span { "Channels" } }
                        {item_list}
                    }
                    section { class: "stage-panel",
                        if live().is_some() {
                            {live_panel}
                        } else {
                            div { class: "stage-empty",
                                div { strong { "Select a channel" } "Choose a live channel to start watching." }
                            }
                        }
                        {note}
                    }
                }
            } else {
                div { class: "media-browser",
                    aside { class: "category-panel",
                        div { class: "panel-head", h2 { "Categories" } }
                        div { class: "category-scroll",
                            div { class: "categories", {cat_list} }
                        }
                    }
                    section { class: "poster-content",
                        div { class: "media-head",
                            div { h1 { "{category_name}" } span { "{page_name}" } }
                        }
                        {item_list}
                    }
                    {player}
                    {series_panel}
                }
            }
        }
    }
}

#[component]
fn ChannelItem(row: Row, active: bool, onpick: EventHandler<Row>) -> Element {
    let r = row.clone();
    rsx! {
        button {
            class: if active { "channel on" } else { "channel" },
            title: "{row.title}",
            onclick: move |_| onpick.call(r.clone()),
            span { class: "channel-logo",
                if let Some(src) = row.icon.as_deref().filter(|s| s.starts_with("http")) {
                    img { src: "{src}", loading: "lazy" }
                } else { "TV" }
            }
            span { class: "channel-copy",
                strong { "{row.title}" }
                small { if active { "Now playing" } else { "Programme information on open" } }
            }
            span { class: "channel-star", "☆" }
        }
    }
}

#[component]
fn PosterItem(row: Row, onpick: EventHandler<Row>) -> Element {
    let r = row.clone();
    let fallback = if matches!(&row.target, Target::Series(_)) {
        "S"
    } else {
        "M"
    };
    rsx! {
        button {
            class: "poster",
            title: "{row.title}",
            onclick: move |_| onpick.call(r.clone()),
            span { class: "poster-art",
                if let Some(src) = row.icon.as_deref().filter(|s| s.starts_with("http")) {
                    img { src: "{src}", loading: "lazy" }
                } else { span { "{fallback}" } }
                em { "HD" }
            }
            strong { "{row.title}" }
        }
    }
}

#[component]
fn SeriesView(
    id: u64,
    onplay: EventHandler<(String, String)>,
    onclose: EventHandler<()>,
) -> Element {
    let session = use_context::<Signal<Option<Client>>>();
    let client = use_hook(|| session.read().clone().expect("logged in"));
    let c = client.clone();
    let info = use_resource(move || {
        let c = c.clone();
        async move { c.series_info(id).await }
    });

    let body = match &*info.read() {
        None => rsx! { p { class: "dim", "Loading..." } },
        Some(Err(e)) => rsx! { p { class: "err", "{e}" } },
        Some(Ok(i)) if i.seasons.is_empty() => rsx! { p { class: "dim", "No episodes listed." } },
        Some(Ok(i)) => rsx! {
            if let Some(plot) = &i.plot { p { class: "dim", "{plot}" } }
            for season in i.seasons.iter() {
                h3 { key: "s{season.number}", "Season {season.number}" }
                for (n, ep) in season.episodes.iter().enumerate() {
                    button {
                        key: "{ep.id}",
                        class: "ep",
                        onclick: {
                            let ext = ep.container_extension.as_deref().filter(|e| !e.is_empty()).unwrap_or("mp4");
                            let url = client.episode_url(ep.id, ext).to_string();
                            let title = format!("{} S{}E{}: {}", i.name, season.number, ep.episode_num.unwrap_or(n as u64 + 1), ep.title);
                            move |_| onplay.call((title.clone(), url.clone()))
                        },
                        "{ep.episode_num.unwrap_or(n as u64 + 1)}. {ep.title}"
                    }
                }
            }
        },
    };

    let name = match &*info.read() {
        Some(Ok(i)) if !i.name.is_empty() => i.name.clone(),
        _ => "Series".to_string(),
    };
    rsx! {
        div { class: "series",
            div { class: "bar", strong { "{name}" } button { class: "out", onclick: move |_| onclose.call(()), "Close" } }
            {body}
        }
    }
}

/// A live stream through the Rust HLS player. The player stops when this component goes away
/// (its handle is dropped), so closing the panel or picking another channel ends the download loop.
#[component]
fn LivePlayer(id: u64, title: String, url: String, onclose: EventHandler<()>) -> Element {
    let session = use_context::<Signal<Option<Client>>>();
    let client = use_hook(|| session.read().clone().expect("logged in"));
    let mut status = use_signal(|| "Connecting...".to_string());
    let handle = use_hook(|| Rc::new(RefCell::new(None::<player::mse::Player>)));
    let guide_client = client.clone();
    let guide = use_resource(move || {
        let c = guide_client.clone();
        async move { c.short_epg(id, 6).await }
    });

    use_effect(move || {
        let video = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("live-video"))
            .and_then(|e| e.dyn_into::<web_sys::HtmlVideoElement>().ok());
        let (Some(video), Ok(playlist)) = (video, xtream::Url::parse(&url)) else {
            status.set("Could not start the player".into());
            return;
        };
        let c = client.clone();
        *handle.borrow_mut() = Some(player::mse::start(
            video,
            playlist,
            move |u| c.proxied(u),
            move |s| {
                status.set(match s {
                    player::mse::Status::Playing => "Live".into(),
                    player::mse::Status::Note(n) => n,
                    player::mse::Status::Ended => "Stream ended".into(),
                    player::mse::Status::Failed(e) => format!("Playback failed: {e}"),
                })
            },
        ));
    });

    let guide_body = match &*guide.read() {
        None => rsx! { p { class: "dim", "Loading programme guide…" } },
        Some(Err(e)) => rsx! { p { class: "dim", "Guide unavailable: {e}" } },
        Some(Ok(entries)) if entries.is_empty() => {
            rsx! { p { class: "dim", "This provider has no guide data for the channel." } }
        }
        Some(Ok(entries)) => rsx! {
            div { class: "guide-list",
                for (index, entry) in entries.iter().enumerate() {
                    article { class: "programme", key: "{index}-{entry.start}",
                        time { "{epg_clock(&entry.start)} – {epg_clock(&entry.end)}" }
                        if index == 0 { span { class: "on-now", "ON NOW" } }
                        strong { if entry.title.is_empty() { "Untitled programme" } else { "{entry.title}" } }
                        if !entry.description.is_empty() { p { "{entry.description}" } }
                        if index == 0 { span { class: "programme-progress" } }
                    }
                }
            }
        },
    };

    rsx! {
        div { class: "live-stage",
            div { class: "video-wrap",
                div { class: "bar",
                    strong { "{title}" }
                    span { class: "dim", "{status}" }
                    button { class: "out", title: "Close player", onclick: move |_| onclose.call(()), "×" }
                }
                video { id: "live-video", controls: true, autoplay: true }
            }
            section { class: "guide",
                div { class: "guide-head",
                    h2 { "{title}" }
                    span { class: "dim", "Today · Now & next" }
                }
                {guide_body}
            }
        }
    }
}
