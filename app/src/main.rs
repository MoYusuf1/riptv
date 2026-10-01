//! Dioxus web UI. It talks to IPTV servers only through the local proxy
//! (`xtream::Client::via_proxy`), because servers send no CORS headers.
//!
//! ponytail: profiles (and their passwords) are kept as plain text in this browser's storage, no
//! favourites, downloads are
//! plain browser downloads (no queue), no adaptive bitrate, and live streams must be HLS with
//! MPEG-TS segments (H.264 + AAC).

use dioxus::prelude::*;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::Duration,
};

mod controls;
mod diag;
mod fetch;
mod frame_stats;
mod media_session;
mod preferences;
mod profiles;
mod shelves;
mod standard;

use controls::{IdleHide, Skip};
use fetch::Proxied;
use profiles::Login;
use rstreamkit::{Unsupported, vod::Verdict};
use web_sys::{
    js_sys,
    wasm_bindgen::{JsCast, JsValue, closure::Closure},
};
use xtream::{Client, Details, Episode, LiveStream, Season, VodStream};

/// Items per page: a screen or so of posters, and the narrower channel list beside a player.
const PAGE_GRID: usize = 60;
const PAGE_LIST: usize = 40;

/// One stylesheet, grouped by component. The comments live here, not in the shipped string.
const CSS: &str = concat!(
    // Palette and page basics
    r#":root{color-scheme:dark;--bg:#0b0709;--panel:#130d10;--row:#1c1418;--hair:rgba(255,255,255,.08);--text:#f5eef1;--dim:#a0919a;--faint:#6f6068;--accent:#ff7d92;--fill:#e11d48;--soft:rgba(255,125,146,.14);--glass:rgba(24,16,20,.72)}
*{box-sizing:border-box}
body{margin:0;min-width:320px;background:var(--bg);color:var(--text);font:14px/1.45 -apple-system,BlinkMacSystemFont,"SF Pro Text","Segoe UI",system-ui,sans-serif;-webkit-font-smoothing:antialiased}
button,input{font:inherit;color:inherit}
button{padding:0;border:0;background:none;text-align:inherit;cursor:pointer}
svg{width:1.1rem;height:1.1rem}
.dim{color:var(--dim)}
.err{color:#ffb454}
.grow{flex:1}
.empty{display:grid;place-items:center;min-height:12rem;color:var(--dim)}
.icon-btn{display:grid;place-items:center;flex:none;width:2rem;height:2rem;border-radius:50%;background:rgba(255,255,255,.12);color:#fff;font-size:1.1rem;line-height:1}
.icon-btn:hover{background:rgba(255,255,255,.22)}
.icon-btn svg{width:1rem;height:1rem}
.icon-btn.on{color:var(--accent)}
.icon-btn.on svg{fill:currentColor}
"#,
    // Sign-in
    r#".login-page{display:grid;place-items:center;min-height:100vh;min-height:100dvh;padding:1.4rem;background:radial-gradient(circle at 15% 80%,rgba(110,58,151,.24),transparent 40%),radial-gradient(circle at 85% 12%,rgba(225,29,72,.18),transparent 42%),var(--bg)}
.login-stage{display:grid;grid-template-columns:minmax(19rem,24rem) minmax(18rem,26rem);width:min(100%,50rem);min-height:34rem;overflow:hidden;border:1px solid var(--hair);border-radius:28px;background:var(--panel);box-shadow:0 30px 90px rgba(0,0,0,.46)}
.login{display:flex;flex-direction:column;justify-content:center;gap:.72rem;width:100%;padding:2.2rem 2.35rem;background:rgba(12,8,11,.94)}
.login .brand{align-self:flex-start;color:var(--text)}
.login h1{margin:1.05rem 0 0;font-size:2rem;line-height:1.1;letter-spacing:-.04em}
.login-intro{margin:0 0 .55rem;color:var(--dim);font-size:.87rem}
.login input{width:100%;min-height:2.9rem;padding:.76rem .95rem;border:1px solid var(--hair);border-radius:11px;outline:0;background:rgba(255,255,255,.045)}
.login input:focus{border-color:var(--accent);box-shadow:0 0 0 3px var(--soft)}
.login input::placeholder{color:var(--faint)}
.login button{padding:.78rem;border-radius:11px;background:var(--fill);color:#fff;font-weight:650;text-align:center;transition:background .15s ease,transform .15s ease}
.login button:not(:disabled):hover{background:#f02b58;transform:translateY(-1px)}
.login button:disabled{opacity:.6;cursor:wait}
.login button.ghost{background:none;color:var(--dim)}
.login button.ghost:hover{background:rgba(255,255,255,.05);color:var(--text)}
.login-modes{display:flex;gap:.2rem;padding:.23rem;border:1px solid var(--hair);border-radius:12px;background:rgba(255,255,255,.04)}
.login .login-modes button{flex:1;padding:.56rem;border-radius:9px;background:none;color:var(--dim);font-size:.85rem}
.login .login-modes button.on{background:rgba(255,255,255,.1);color:var(--text);box-shadow:0 2px 8px rgba(0,0,0,.2)}
.login-art{position:relative;min-height:100%;overflow:hidden;background:radial-gradient(circle at 77% 24%,#f7a271 0,transparent 30%),linear-gradient(145deg,#ee7a70 0%,#953e93 36%,#4d388d 68%,#252956 100%)}
.login-art:before,.login-art:after{content:"";position:absolute;width:135%;height:56%;left:-18%;border-radius:50%;transform:rotate(-22deg)}
.login-art:before{top:32%;background:#503486;box-shadow:0 -12px 0 rgba(255,255,255,.11)}
.login-art:after{top:63%;background:#252950;box-shadow:0 -12px 0 rgba(187,166,238,.24)}
.art-orbit{position:absolute;z-index:1;width:115%;height:22%;left:-9%;border:16px solid rgba(205,194,255,.8);border-radius:50%;transform:rotate(-24deg)}
.orbit-one{top:49%}.orbit-two{top:66%;left:8%}.orbit-three{top:82%;left:-22%}
.art-glow{position:absolute;z-index:1;right:5%;top:9%;width:7rem;height:7rem;border-radius:50%;background:#ffd2a3;filter:blur(1px);opacity:.85}
.art-caption{position:absolute;z-index:2;left:2rem;bottom:2rem;display:grid;gap:.4rem;color:#fff;text-shadow:0 2px 12px rgba(0,0,0,.35)}
.art-caption span{font-size:.68rem;font-weight:700;letter-spacing:.22em;opacity:.78}
.art-caption strong{font-size:1.55rem;line-height:1.15;white-space:pre-line}
@media(max-width:680px){.profile-page{padding:1rem}.login-stage{display:block;width:min(100%,25rem);min-height:0;border-radius:22px}.login{padding:1.55rem}.login-art{display:none}}
"#,
    // Floating chrome: top bar, section rail, account menu
    r#".topbar,.rail{position:fixed;z-index:20;display:flex;border:1px solid var(--hair);background:var(--glass);box-shadow:0 10px 30px rgba(0,0,0,.35);backdrop-filter:blur(24px) saturate(180%)}
.topbar{inset:.6rem .6rem auto;align-items:center;gap:.9rem;height:3rem;padding:0 .6rem 0 .9rem;border-radius:16px}
.brand{display:inline-flex;align-items:center;gap:.5rem;font-size:.85rem;font-weight:700;letter-spacing:.06em}
.rust-mark{width:1.6rem;height:1.6rem;color:var(--accent)}
.search{position:relative;display:flex;align-items:center;flex:1;max-width:24rem}
.search svg{position:absolute;left:.75rem;width:.95rem;height:.95rem;color:var(--faint);pointer-events:none}
.search input{width:100%;height:2.1rem;padding:0 .9rem 0 2.2rem;border:0;border-radius:10px;outline:0;background:var(--row)}
.search input:focus{box-shadow:0 0 0 2px var(--soft)}
.search input::placeholder{color:var(--faint)}
.rail{z-index:25;top:50%;left:.6rem;flex-direction:column;gap:.25rem;padding:.35rem;border-radius:18px;transform:translateY(-50%)}
.rail button{display:grid;place-items:center;align-content:center;width:2.7rem;height:2.7rem;border-radius:13px;color:var(--dim);text-align:center}
.rail button.on{background:var(--soft);color:var(--accent)}
.rail svg{width:1.3rem;height:1.3rem}
.rail span{display:none;font-size:.62rem;font-weight:500}
.topbar.detail-mode{inset:.65rem .7rem auto auto;width:auto;height:2.8rem;padding:.25rem;border-radius:999px;background:rgba(22,16,20,.82)}
.topbar.detail-mode .grow{display:none}
.topbar.detail-mode .who{background:transparent}
.topbar.detail-mode .who b{display:none}
.detail-return{position:fixed;z-index:18;top:.7rem;left:1rem;display:flex;align-items:center;gap:.35rem}
.detail-return .icon-btn{background:rgba(0,0,0,.4)}
.account{position:relative}
.who{display:flex;align-items:center;gap:.5rem;height:2.1rem;padding:0 .7rem 0 .25rem;border-radius:999px;background:var(--row);font-size:.8rem;font-weight:500}
.who.solo{padding:.25rem}
.who i svg{width:.9rem;height:.9rem}
.who i{display:grid;place-items:center;width:1.6rem;height:1.6rem;border-radius:50%;background:var(--fill);color:#fff;font-size:.72rem;font-style:normal;font-weight:700;text-transform:uppercase}
.scrim{position:fixed;z-index:35;inset:0;cursor:default}
.menu{position:absolute;z-index:40;top:calc(100% + .5rem);right:0;min-width:12rem;padding:.3rem;border-radius:14px;background:rgba(32,23,28,.94);box-shadow:0 18px 50px rgba(0,0,0,.55);backdrop-filter:blur(20px)}
.menu button{display:block;width:100%;padding:.7rem .8rem;border-radius:10px}
.menu button:hover{background:var(--soft)}
"#,
    // Workspace: category sidebar, headers, scrolling lists
    r#".workspace{position:fixed;inset:4.2rem 0 0 4.4rem;display:grid;grid-template-columns:15rem minmax(0,1fr);gap:.7rem;padding:0 .7rem .7rem 0}
.sidebar{min-height:0;padding:.75rem .6rem 1rem;overflow:auto;border-radius:16px;background:var(--panel);scrollbar-width:thin}
.filter{width:100%;margin:0 0 .5rem;padding:.55rem .8rem;border:0;border-radius:10px;outline:0;background:var(--row);font-size:.85rem}
.filter:focus{box-shadow:0 0 0 2px var(--soft)}
.cat{display:flex;align-items:center;justify-content:space-between;gap:.5rem;width:100%;padding:.5rem .7rem;border-radius:10px}
.cat span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.cat small{color:var(--faint);font-size:.72rem;font-variant-numeric:tabular-nums}
.cat:hover{background:var(--row)}
.cat.on{background:var(--soft);color:var(--accent);font-weight:600}
.cat.on small{color:var(--accent)}
.content{position:relative;display:flex;flex-direction:column;min-width:0;min-height:0}
.main,.channels{display:flex;flex-direction:column;flex:1;min-width:0;min-height:0}
.main.covered{visibility:hidden}
.workspace.detail-open{display:none}
.head{display:flex;align-items:flex-end;justify-content:space-between;gap:1rem;padding:.4rem 1rem .7rem .4rem}
.head h1{margin:0;font-size:1.6rem;letter-spacing:-.03em;line-height:1.1}
.head span{color:var(--dim);font-size:.8rem}
.tools{display:flex;align-items:center;gap:.5rem}
.sortwrap{position:relative}
.sort-btn{display:inline-flex;align-items:center;gap:.4rem;padding:.4rem .8rem;border-radius:999px;background:var(--row);font-size:.8rem;color:var(--dim)}
.sort-btn:hover{color:var(--text)}
.sort-menu{min-width:11rem}
.menu button.on{color:var(--accent);font-weight:600}
.menu:has(.setting){min-width:15rem}
.menu .setting{display:flex;align-items:center;justify-content:space-between;gap:1rem}
.switch{flex:none;position:relative;width:2.3rem;height:1.4rem;border-radius:99px;background:var(--hair);transition:background .15s}
.switch::after{content:"";position:absolute;top:.15rem;left:.15rem;width:1.1rem;height:1.1rem;border-radius:50%;background:#fff;transition:transform .15s}
.setting[aria-checked=true] .switch{background:var(--fill)}
.setting[aria-checked=true] .switch::after{transform:translateX(.9rem)}
.menu button:has(svg){display:flex;align-items:center;justify-content:space-between}
.channels .sort-btn span{display:none}
.channels .sort-btn{padding:.45rem}
.cats-btn{display:none;align-items:center;gap:.4rem;flex:none;padding:.4rem .8rem;border-radius:999px;background:var(--row);font-size:.8rem}
.scroll{flex:1;min-height:0;padding:0 .4rem 2rem;overflow:auto;scrollbar-width:thin}
"#,
    // Posters and channel tiles
    r#".grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(8.6rem,1fr));gap:1.5rem 1rem}
.card{min-width:0;text-align:center;transition:transform .15s;content-visibility:auto;contain-intrinsic-size:auto 17rem}
.card:hover{transform:scale(1.03)}
.art{position:relative;display:grid;place-items:center;aspect-ratio:2/3;overflow:hidden;border-radius:12px;background:var(--row);box-shadow:0 6px 18px rgba(0,0,0,.35);color:var(--faint);font-size:1.6rem;font-weight:700}
.tiles .art{aspect-ratio:16/10}
.art img{position:absolute;inset:0;width:100%;height:100%;object-fit:cover}
.tiles .art img{padding:.7rem;object-fit:contain}
.score{position:absolute;top:.4rem;right:.4rem;padding:.1rem .45rem;border-radius:999px;background:rgba(0,0,0,.55);color:#fff;font-size:.68rem;font-weight:600;backdrop-filter:blur(8px)}
.score::before{content:'★ ';color:#ffcc4d}
.card strong{display:-webkit-box;margin-top:.5rem;overflow:hidden;color:var(--dim);font-size:.74rem;font-weight:500;-webkit-box-orient:vertical;-webkit-line-clamp:2}
"#,
    // Movie and series pages: a full-bleed destination under the floating chrome
    r#".detail{position:fixed;z-index:15;inset:0;overflow:hidden auto;background:var(--bg);scrollbar-width:thin}
.d-hero{position:relative;display:flex;flex-direction:column;justify-content:flex-end;min-height:72vh;padding:7rem 3rem 2.4rem 6.2rem;background:radial-gradient(ellipse at 78% 28%,#49313d 0%,transparent 42%),radial-gradient(ellipse at 28% 65%,#302c4c 0%,transparent 53%),linear-gradient(125deg,#201820,#11151f 65%,#130f17)}
.d-hero.no-art::before{content:"";position:absolute;right:12%;top:18%;width:min(32vw,27rem);aspect-ratio:1;border:1px solid rgba(255,255,255,.07);border-radius:50%;box-shadow:0 0 0 5rem rgba(255,255,255,.015),0 0 0 10rem rgba(255,255,255,.01);pointer-events:none}
.d-art{position:absolute;inset:0;width:100%;height:100%;object-fit:cover;object-position:center 20%}
.d-poster{position:absolute;top:8%;right:10%;width:min(24vw,19rem);max-height:72%;object-fit:contain;border-radius:14px;box-shadow:0 24px 70px rgba(0,0,0,.55)}
.d-shade{position:absolute;inset:0;background:linear-gradient(90deg,rgba(11,7,9,.85),rgba(11,7,9,.28) 62%,rgba(11,7,9,.3)),linear-gradient(transparent 38%,rgba(11,7,9,.4) 76%,var(--bg))}
.d-main{position:relative;max-width:41rem}
.d-title{margin:0 0 .7rem;font-size:clamp(2.4rem,5.4vw,4.4rem);font-weight:800;line-height:.98;letter-spacing:-.035em;text-wrap:balance;text-shadow:0 4px 34px rgba(0,0,0,.55)}
.d-genres{display:flex;flex-wrap:wrap;align-items:center;gap:.5rem;font-weight:500}
.d-genres span+span::before{content:'•';margin-right:.5rem;color:var(--faint)}
.d-actions{display:flex;align-items:center;gap:.6rem;margin:1.3rem 0 1.4rem}
.play{display:inline-flex;align-items:center;gap:.5rem;height:2.9rem;padding:0 1.5rem 0 1.2rem;border-radius:999px;background:#fff;color:#111;font-weight:600;white-space:nowrap}
.play svg{fill:currentColor}
.play:hover{background:#ece4e8}
.d-actions .icon-btn{width:2.9rem;height:2.9rem}
.d-meta{display:flex;flex-wrap:wrap;align-items:center;gap:.7rem;font-size:.92rem}
.badge{padding:0 .4rem;border:1px solid rgba(255,255,255,.55);border-radius:4px;font-size:.72rem;font-weight:600;line-height:1.55}
.star{color:#ffcc4d}
.d-by{margin:.6rem 0 .8rem;color:var(--dim)}
.d-by b{color:var(--text);font-weight:500}
.plot{margin:0;color:#e6dbe0;line-height:1.55}
.plot.clamp{display:-webkit-box;overflow:hidden;-webkit-box-orient:vertical;-webkit-line-clamp:3}
.more{margin-top:.3rem;font-size:.72rem;font-weight:600;color:var(--dim)}
.more:hover{color:var(--text)}
.d-facts{position:absolute;right:2.6rem;bottom:3.4rem;width:19rem;margin:0;overflow:hidden;border:1px solid var(--hair);border-radius:12px;background:var(--glass);backdrop-filter:blur(24px)}
.d-facts div{display:flex;justify-content:space-between;gap:1rem;padding:.6rem .9rem;font-size:.75rem}
.d-facts div+div{border-top:1px solid var(--hair)}
.d-facts dt{color:var(--faint)}
.d-facts dd{margin:0;text-align:right}
.d-sec{position:relative;padding:1rem 3rem 1.2rem 6.2rem;content-visibility:auto;contain-intrinsic-size:auto 14rem}
.d-sec h2{margin:0 0 1rem;font-size:1.25rem;letter-spacing:-.01em}
.cast{display:flex;flex-wrap:wrap;gap:.55rem}
.person{padding:.55rem .85rem;border:1px solid var(--hair);border-radius:999px;background:var(--row)}
.person strong{font-size:.78rem;font-weight:500;line-height:1.3}
.trailer{position:relative;display:block;width:16rem;color:#fff;aspect-ratio:16/9;overflow:hidden;border-radius:12px;background:var(--row)}
.trailer img{width:100%;height:100%;object-fit:cover;transition:transform .2s}
.trailer:hover img{transform:scale(1.04)}
.trailer span{position:absolute;inset:auto 0 0;padding:1.6rem .8rem .6rem;background:linear-gradient(transparent,rgba(0,0,0,.8));font-size:.78rem;font-weight:600}
.seasons{display:flex;flex-wrap:wrap;gap:.5rem;margin-bottom:1.1rem}
.seasons button{padding:.4rem .95rem;border-radius:999px;background:var(--row);color:var(--dim);font-size:.8rem}
.seasons button:hover{color:var(--text)}
.seasons button.on{background:#fff;color:#111;font-weight:600}
.ep-heading{display:flex;align-items:center;justify-content:space-between;gap:1rem;margin-bottom:1rem}
.ep-heading h2{margin:0}
.ep-tools{display:flex;align-items:center;gap:.6rem;color:var(--dim);font-size:.78rem}
.season-select{max-width:10rem;padding:.5rem 1.8rem .5rem .8rem;border:1px solid var(--hair);border-radius:999px;background:var(--row);color:var(--text);outline:0;cursor:pointer}
.season-select:focus-visible{outline:2px solid var(--accent)}
.eps{display:flex;gap:1.15rem;overflow-x:auto;padding:.15rem .1rem 1rem;scrollbar-width:thin;scroll-snap-type:x proximity}
.epc{flex:0 0 clamp(14rem,13vw,18rem);min-width:0;scroll-snap-align:start}
.thumb{position:relative;display:grid;place-items:center;width:100%;aspect-ratio:16/9;overflow:hidden;border-radius:12px;background:var(--row);color:var(--faint);font-size:1.4rem;font-weight:700}
.thumb img{position:absolute;inset:0;width:100%;height:100%;object-fit:cover}
.thumb .ep-num{position:absolute;top:.45rem;left:.45rem;padding:.18rem .45rem;border:1px solid rgba(255,255,255,.35);border-radius:999px;background:rgba(0,0,0,.65);color:#fff;font-size:.67rem;font-weight:700;line-height:1}
.thumb .go{position:absolute;inset:0;display:grid;place-items:center;background:rgba(0,0,0,.4);color:#fff;opacity:0;transition:opacity .15s}
.thumb .go svg{width:2rem;height:2rem;fill:currentColor}
.thumb:hover .go,.thumb:focus-visible .go{opacity:1}
.thumb .len{position:absolute;right:.45rem;bottom:.45rem;padding:.05rem .4rem;border-radius:6px;background:rgba(0,0,0,.7);color:#fff;font-size:.68rem;font-variant-numeric:tabular-nums}
.epc-head{display:flex;align-items:flex-start;justify-content:space-between;gap:.5rem;margin-top:.65rem}
.epc h4{margin:0;font-size:.85rem;font-weight:650;line-height:1.3}
.epc small{display:block;margin-top:.1rem;color:var(--faint);font-size:.72rem}
.epc p{display:-webkit-box;margin:.35rem 0 0;overflow:hidden;color:var(--dim);font-size:.78rem;line-height:1.45;-webkit-box-orient:vertical;-webkit-line-clamp:2}
"#,
    // Live TV: channel list, player, timeline guide
    r#".live{display:grid;flex:1;grid-template-columns:18rem minmax(0,1fr);min-width:0;min-height:0;overflow:hidden;border:1px solid var(--hair);border-radius:16px}
.channels{border-right:1px solid var(--hair);background:var(--panel)}
.channels .head{padding:1rem 1rem .6rem}
.channels .head h1{font-size:1.15rem}
.channels .scroll{padding:0 .6rem 1rem}
.row{display:flex;align-items:center;gap:.7rem;width:100%;padding:.5rem .6rem;border-radius:12px;content-visibility:auto;contain-intrinsic-size:auto 3.6rem}
.row:hover{background:var(--row)}
.row.on{background:var(--soft)}
.logo{position:relative;display:grid;place-items:center;flex:none;width:2.5rem;height:2.5rem;overflow:hidden;border-radius:9px;background:var(--row);color:var(--faint);font-size:.6rem;font-weight:700}
.logo img{position:absolute;inset:0;width:100%;height:100%;padding:.2rem;object-fit:contain}
.row div{min-width:0}
.row strong,.row small{display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.row strong{font-weight:500}
.row small{color:var(--accent);font-size:.7rem}
.stage{display:flex;flex-direction:column;min-width:0;min-height:0;background:var(--bg)}
.stage-empty{display:grid;flex:1;place-content:center;justify-items:center;gap:.8rem;color:var(--dim)}
.stage-empty svg{width:3.4rem;height:3.4rem;opacity:.55}
.live-stage{display:grid;grid-template:minmax(0,1fr) auto/minmax(0,1fr);height:100%}
.player{position:relative;min-height:0;margin:.7rem .7rem 0;overflow:hidden;border-radius:16px;background:#000;outline:0}
.player:fullscreen,.player.fill{margin:0;border-radius:0}
.player.fill{position:fixed;z-index:60;inset:0}
.player:not(.active):not(.paused){cursor:none}
.player video{width:100%;height:100%;object-fit:contain}
.audio-only{position:absolute;inset:0;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:.45rem;background:radial-gradient(circle at 50% 42%,rgba(225,29,72,.2),transparent 33%),#100b11;color:var(--text);pointer-events:none}
.audio-only .audio-orb{display:grid;place-items:center;width:5rem;height:5rem;margin-bottom:.45rem;border:1px solid rgba(255,255,255,.15);border-radius:50%;background:var(--soft);color:var(--accent);font-size:2rem}
.audio-only strong{font-size:1.1rem}.audio-only small{color:var(--dim)}
.sound-note{position:absolute;left:1rem;bottom:4.6rem;max-width:calc(100% - 2rem);padding:.35rem .8rem;border-radius:10px;background:rgba(0,0,0,.68);color:#ffd48a;font-size:.78rem;backdrop-filter:blur(10px)}
.stats{position:absolute;top:.8rem;left:.8rem;padding:.3rem .7rem;border-radius:8px;background:rgba(0,0,0,.66);color:#fff;font:600 .72rem ui-monospace,monospace;backdrop-filter:blur(8px)}
.channel-dial{position:absolute;top:1rem;right:1rem;min-width:3rem;padding:.45rem .75rem;border-radius:10px;background:rgba(0,0,0,.72);color:#fff;text-align:center;font-weight:700}
.hud{position:absolute;inset:0;display:grid;place-items:center;color:#fff;pointer-events:none}
.hud span{padding:.4rem .9rem;border-radius:999px;background:rgba(0,0,0,.6);backdrop-filter:blur(10px)}
.loading-group{display:grid;justify-items:center;gap:.7rem}
.spinner{width:2.6rem;height:2.6rem;border:3px solid rgba(255,255,255,.25);border-top-color:#fff;border-radius:50%;animation:spin .8s linear infinite}
@keyframes spin{to{transform:rotate(1turn)}}
.bigplay{position:absolute;top:50%;left:50%;display:grid;place-items:center;width:4.6rem;height:4.6rem;border-radius:50%;background:rgba(20,13,16,.55);color:#fff;transform:translate(-50%,-50%);backdrop-filter:blur(16px)}
.bigplay svg{width:1.8rem;height:1.8rem;fill:currentColor}
.controls{position:absolute;inset:auto 0 0;display:flex;align-items:center;gap:.4rem;padding:3rem 1rem .8rem;background:linear-gradient(transparent,rgba(0,0,0,.78));opacity:0;transition:opacity .25s}
.player.active .controls,.player.paused .controls,.controls:focus-within{opacity:1}
.ctl{display:grid;place-items:center;width:2.5rem;height:2.5rem;border-radius:50%;color:#fff}
.ctl:hover{background:rgba(255,255,255,.18)}
.ctl svg{width:1.3rem;height:1.3rem}
.volume{display:flex;align-items:center}
.vol{width:0;margin:0;opacity:0;transition:width .2s,opacity .2s;accent-color:var(--accent)}
.volume:hover .vol,.vol:focus-visible{width:5.5rem;margin:0 .5rem 0 .2rem;opacity:1}
.live-pill{display:inline-flex;align-items:center;gap:.4rem;margin-left:.3rem;padding:.25rem .7rem;border-radius:999px;background:var(--fill);color:#fff;font-size:.7rem;font-weight:700;letter-spacing:.07em}
.live-pill::before{content:'';width:.45rem;height:.45rem;border-radius:50%;background:#fff}
.player:not(.paused) .live-pill::before{animation:pulse 1.6s ease-in-out infinite}
@keyframes pulse{50%{opacity:.25}}
.player.paused .live-pill{background:rgba(255,255,255,.18)}
.guide{min-width:0;min-height:12.5rem;padding:.8rem 1rem 1rem;border-top:1px solid var(--hair);background:var(--panel)}
.guide-head{display:flex;flex-direction:column;margin-bottom:.6rem}
.guide-head strong,.guide-head small{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.guide-head small{color:var(--dim)}
.timeline{overflow-x:auto;padding-bottom:.4rem;scrollbar-width:thin}
.tl{position:relative;height:6.4rem}
.tick{position:absolute;top:0;height:100%;padding-left:.4rem;border-left:1px solid var(--hair);color:var(--faint);font-size:.7rem}
.block{position:absolute;top:1.5rem;bottom:0;padding:.5rem .7rem;border-right:3px solid var(--panel);border-radius:10px;background:var(--row)}
.block.now{background:var(--soft);outline:1px solid rgba(255,125,146,.45);outline-offset:-1px}
.block .txt{position:sticky;left:.7rem;max-width:min(100%,18rem);overflow:hidden}
.block.compact{padding:.5rem .35rem}.block.compact time,.block.compact p{display:none}.block.compact strong{font-size:.72rem}
.block time{display:block;color:var(--dim);font-size:.7rem}
.block strong{display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.block p{display:-webkit-box;margin:.25rem 0 0;overflow:hidden;color:var(--dim);font-size:.75rem;-webkit-box-orient:vertical;-webkit-line-clamp:2}
.prog{position:absolute;bottom:0;left:0;height:3px;background:var(--accent)}
.now-line{position:absolute;z-index:2;top:0;bottom:0;width:2px;background:var(--fill)}
.now-line span{position:absolute;top:0;left:-1.3rem;padding:.02rem .35rem;border-radius:5px;background:var(--fill);color:#fff;font-size:.65rem;font-weight:600}
"#,
    // The category picker sheet
    r#".overlay{position:fixed;z-index:50;inset:0;display:grid;place-items:center;padding:4vh 4vw;background:rgba(0,0,0,.6);backdrop-filter:blur(10px)}
.overlay.bottom{place-items:end center;padding:0}
.sheet{width:min(100%,64rem);max-height:92vh;overflow:auto;border-radius:20px;background:var(--panel);box-shadow:0 30px 90px rgba(0,0,0,.6)}
.sheet.cats{width:100%;max-height:78vh;border-radius:22px 22px 0 0}
.sheet-body{padding:0 .8rem 1rem}
.bar{display:flex;align-items:center;justify-content:space-between;gap:1rem;padding:.8rem 1rem}
.bar strong{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
"#,
    // The full-page player for movies and episodes
    r#".watch{position:fixed;z-index:60;inset:0;overflow:hidden;background:#000;color:#fff;outline:0}
.watch video{position:absolute;inset:0;width:100%;height:100%;background:#000;object-fit:contain}
.watch:not(.active):not(.paused){cursor:none}
.watch:has(.w-load),.watch:has(.w-fail){cursor:default}
.w-load,.w-fail{position:absolute;inset:0;display:grid;place-content:center;justify-items:center;gap:.7rem;padding:2rem;text-align:center;color:var(--dim);background:radial-gradient(circle at 50% 48%,rgba(255,125,146,.1),transparent 32%),#070607}
.w-load .spinner{width:2rem;height:2rem;margin-bottom:.6rem;border-width:2px}
.w-load strong{max-width:min(80vw,32rem);overflow:hidden;color:#fff;font-size:1.15rem;font-weight:600;text-overflow:ellipsis;white-space:nowrap}
.w-load small{max-width:min(80vw,32rem);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.w-load span{font-size:.78rem}
.w-top,.w-bottom{position:absolute;inset-inline:0;opacity:0;pointer-events:none;transition:opacity .25s}
.w-top{top:0;display:grid;grid-template-columns:1fr auto 1fr;align-items:center;gap:1rem;padding:1rem 1.4rem 3rem;background:linear-gradient(rgba(0,0,0,.8),transparent)}
.w-top .icon-btn{background:rgba(255,255,255,.14)}
.w-top .end{display:flex;justify-content:flex-end}
.w-title{min-width:0;text-align:center}
.w-title strong,.w-title small{display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.w-title small{color:var(--dim);font-size:.75rem}
.w-bottom{bottom:0;padding:3.6rem 1.4rem .9rem;background:linear-gradient(transparent,rgba(0,0,0,.88))}
.watch.active .w-top,.watch.active .w-bottom,.watch.paused .w-top,.watch.paused .w-bottom,.w-bottom:focus-within{opacity:1;pointer-events:auto}
.watch:has(.w-load) .w-top,.watch:has(.w-fail) .w-top{opacity:1;pointer-events:auto}
.w-seek{display:block;width:100%;height:1.2rem;margin:0;background:none;cursor:pointer;-webkit-appearance:none;appearance:none}
.w-seek::-webkit-slider-runnable-track{height:4px;border-radius:99px;background:linear-gradient(90deg,var(--fill) var(--p),rgba(255,255,255,.5) var(--p) var(--b),rgba(255,255,255,.2) var(--b))}
.w-seek::-moz-range-track{height:4px;border-radius:99px;background:linear-gradient(90deg,var(--fill) var(--p),rgba(255,255,255,.5) var(--p) var(--b),rgba(255,255,255,.2) var(--b))}
.w-seek::-webkit-slider-thumb{-webkit-appearance:none;width:0;height:0}
.w-seek::-moz-range-thumb{width:0;height:0;border:0}
.w-seek:hover::-webkit-slider-runnable-track,.w-seek:focus-visible::-webkit-slider-runnable-track{height:6px}
.w-seek:hover::-webkit-slider-thumb,.w-seek:focus-visible::-webkit-slider-thumb{-webkit-appearance:none;width:14px;height:14px;margin-top:-4px;border-radius:50%;background:#fff}
.w-seek:hover::-moz-range-thumb{width:14px;height:14px;border-radius:50%;background:#fff}
.w-seek:disabled{cursor:default}
.w-row{display:flex;align-items:center;gap:.3rem}
.w-row .ctl{display:grid;place-items:center;flex:none;width:2.6rem;height:2.6rem}
.w-row .ctl svg{display:block}
.w-row .skip svg{width:1.55rem;height:1.55rem}
.w-time{margin-left:.5rem;font-size:.82rem;font-variant-numeric:tabular-nums;white-space:nowrap}
.w-time span{color:var(--dim)}
.next-btn{display:inline-flex;align-items:center;gap:.4rem;height:2.4rem;margin-right:.3rem;padding:0 .9rem;border-radius:999px;background:rgba(255,255,255,.16);font-size:.8rem;font-weight:600}
.next-btn:hover{background:rgba(255,255,255,.26)}
.w-menu{position:absolute;z-index:40;right:1.4rem;bottom:5.6rem;width:15rem;padding:.4rem;border-radius:14px;background:rgba(28,20,24,.94);box-shadow:0 18px 50px rgba(0,0,0,.6);backdrop-filter:blur(20px)}
.w-menu h5{margin:.5rem .7rem .3rem;color:var(--faint);font-size:.68rem;font-weight:600;letter-spacing:.07em;text-transform:uppercase}
.w-menu button{display:flex;align-items:center;justify-content:space-between;width:100%;padding:.5rem .7rem;border-radius:9px;font-size:.85rem}
.w-menu button:hover{background:var(--soft)}
.w-menu button.on{color:var(--accent);font-weight:600}
.w-menu p{margin:.2rem .7rem .5rem;color:var(--dim);font-size:.75rem;line-height:1.4}
.toast{position:absolute;left:50%;bottom:7rem;display:flex;align-items:center;gap:.9rem;padding:.55rem .6rem .55rem 1rem;border-radius:999px;background:rgba(20,13,16,.86);font-size:.82rem;transform:translateX(-50%);backdrop-filter:blur(14px)}
.toast button{padding:.25rem .8rem;border-radius:999px;background:rgba(255,255,255,.16);font-weight:600}
.upnext{position:absolute;right:1.6rem;bottom:6.4rem;display:grid;gap:.4rem;width:16rem;padding:1rem;border-radius:16px;background:rgba(20,13,16,.9);backdrop-filter:blur(16px)}
.upnext small{color:var(--dim)}
.upnext div{display:flex;gap:.5rem;margin-top:.4rem}
.upnext button{flex:1;padding:.5rem;border-radius:10px;background:rgba(255,255,255,.14);text-align:center;font-weight:600}
.upnext button.go{background:#fff;color:#111}
"#,
    // Who's watching: saved profiles as tiles. A phone gets a picture on top and a curved panel of
    // three columns under it; a desktop gets a centred row with the picture as a faint backdrop.
    r#".login-page.who{position:relative;display:flex;flex-direction:column;align-items:center;justify-content:center;min-height:100vh;min-height:100dvh;padding:0;background:radial-gradient(ellipse at 80% 40%,rgba(225,29,72,.16),transparent 55%),radial-gradient(ellipse at 10% 100%,rgba(95,60,140,.14),transparent 50%),var(--bg);gap:0;overflow-x:clip}
.who-hero{position:absolute;inset:0;z-index:0;pointer-events:none}
.who-mark{position:absolute;right:-7rem;top:50%;width:min(46vw,44rem);aspect-ratio:1;transform:translateY(-50%);opacity:.07}
.who-mark .rust-mark{width:100%;height:100%}
.who-hero .lockup{position:absolute;top:1.6rem;left:2.2rem;display:flex;align-items:baseline;gap:.7rem}
.who-hero .lockup strong{font-size:1.25rem;font-weight:800;letter-spacing:.14em;color:var(--accent)}
.who-hero .lockup span,.who-hero .chip{display:none}
.who-panel{position:relative;z-index:1;display:flex;flex-direction:column;align-items:center;gap:2.2rem;width:100%;padding:2rem 1.4rem}
.who-panel h1{margin:0;font-size:clamp(1.9rem,3.6vw,2.8rem);font-weight:600;letter-spacing:-.03em}
.who-panel h1 .m{display:none}
.profiles{display:flex;flex-wrap:wrap;justify-content:center;gap:2.2rem 2rem;max-width:min(64rem,92vw)}
.profile{display:grid;justify-items:center;gap:.6rem;width:9rem;color:var(--dim);text-align:center;animation:rise .45s calc(var(--i,0) * 45ms) cubic-bezier(.2,.8,.2,1) backwards}
.profile:hover,.profile:focus-visible{color:var(--text)}
.profile:disabled{cursor:wait;opacity:.6}
.profile strong{max-width:100%;overflow:hidden;font-size:1rem;font-weight:500;text-overflow:ellipsis;white-space:nowrap}
.profile small{margin-top:-.35rem;color:var(--faint);font-size:.64rem;letter-spacing:.08em;text-transform:uppercase}
.avatar{position:relative;display:grid;place-items:center;width:9rem;height:9rem;border-radius:1.1rem;background:radial-gradient(circle at 27% 18%,rgba(255,255,255,.3),transparent 55%),var(--c,#e11d48);color:#fff;font-size:3.4rem;font-weight:700;box-shadow:0 12px 34px rgba(0,0,0,.42);transition:transform .18s ease,box-shadow .18s ease}
.avatar .avatar-art{width:76%;height:76%;filter:drop-shadow(0 5px 8px rgba(0,0,0,.18))}
.profile:not(:disabled):hover .avatar,.profile:focus-visible .avatar{transform:scale(1.06);box-shadow:0 0 0 3px var(--text),0 16px 38px rgba(0,0,0,.5)}
.avatar .spinner{width:2.4rem;height:2.4rem}
.avatar .edit{position:absolute;inset:0;display:grid;place-items:center;border-radius:inherit;background:rgba(0,0,0,.55)}
.avatar .edit svg{width:2rem;height:2rem}
.avatar.tool{border:1px solid var(--hair);background:rgba(255,255,255,.06);box-shadow:none;color:var(--dim)}
.avatar.tool svg{width:2.2rem;height:2.2rem}
.profile:not(:disabled):hover .avatar.tool{background:rgba(255,255,255,.12);box-shadow:0 0 0 3px var(--text);color:var(--text)}
.edit-tile{display:none}
.who .manage{padding:.65rem 1.6rem;border:1px solid var(--hair);border-radius:10px;color:var(--dim);font-size:.76rem;letter-spacing:.1em;text-transform:uppercase}
.who .manage:hover{border-color:var(--text);color:var(--text)}
.note{color:var(--faint);font-size:.72rem}
.login .note{text-align:center}
.login .avatar.preview{align-self:center;width:4.7rem;height:4.7rem;margin:.1rem 0 .25rem;border-radius:1rem;font-size:2.6rem;box-shadow:0 8px 22px rgba(0,0,0,.3)}
.login button.danger{color:#ff8a8a}
@media(max-width:820px){.login-page.who{justify-content:flex-start;background:radial-gradient(ellipse at 50% 26%,rgba(225,29,72,.3),transparent 58%),var(--bg)}
.who-hero{position:relative;inset:auto;flex:1 0 auto;display:grid;place-content:center;justify-items:center;gap:.8rem;width:100%;padding:max(2.6rem,env(safe-area-inset-top)) 1rem 2.2rem;background:radial-gradient(rgba(255,255,255,.055) 1px,transparent 1.6px) 0 0/14px 14px}
.who-mark{position:relative;inset:auto;right:auto;top:auto;width:min(44vw,11rem);transform:none;opacity:1;filter:drop-shadow(0 0 42px rgba(255,125,146,.4))}
.who-hero .lockup{position:static;flex-direction:column;align-items:center;gap:.25rem;text-align:center}
.who-hero .lockup strong{font-size:2rem;letter-spacing:.2em;color:var(--text)}
.who-hero .lockup span{display:block;color:var(--dim);font-size:.85rem}
.who-hero .chip{display:inline-flex;align-items:center;gap:.4rem;margin-top:.5rem;padding:.3rem .8rem;border-radius:999px;background:rgba(255,255,255,.08);color:var(--text);font-size:.78rem;font-weight:600}
.who-panel{flex:none;gap:1.4rem;padding:2.2rem 1.4rem max(2rem,env(safe-area-inset-bottom));background:#161b23;background:color-mix(in srgb,var(--panel) 70%,#1b2430);border-radius:50% 50% 0 0/2.6rem 2.6rem 0 0;box-shadow:0 -20px 50px rgba(0,0,0,.35)}
.who-panel h1{color:var(--dim);font-size:1.05rem;font-weight:500;letter-spacing:0}
.who-panel h1 .m{display:inline}
.who-panel h1 .d{display:none}
.profiles{display:grid;grid-template-columns:repeat(3,1fr);gap:1.3rem 1rem;width:100%;max-width:27rem}
.profile{width:auto;min-width:0}
.avatar{width:100%;height:auto;aspect-ratio:1;border-radius:.9rem;font-size:2.4rem}
.profile strong{font-size:1.05rem;color:var(--text)}
.edit-tile{display:grid}
.who .manage{display:none}}
"#,
    // Pictures fade in over a quiet placeholder (main.rs marks each one as it loads), pages and
    // lists arrive instead of appearing, and what is loading is drawn as what it will become.
    r#".art img,.logo img,.thumb img,.trailer img,.d-art,.d-poster{opacity:0;transition:opacity .4s ease}
.art img[data-ready],.logo img[data-ready],.thumb img[data-ready],.trailer img[data-ready],.d-art[data-ready],.d-poster[data-ready]{opacity:1}
img[data-failed]{display:none}
.ph{position:absolute;inset:0;display:grid;place-items:center;color:var(--faint);font-weight:700;transition:opacity .4s ease}
.art:has(img[data-ready]) .ph,.logo:has(img[data-ready]) .ph,.thumb:has(img[data-ready]) .ph{opacity:0}
.prog{position:absolute;inset:auto 0 0;height:3px;background:rgba(255,255,255,.22)}
.prog i{display:block;height:100%;border-radius:0 3px 3px 0;background:var(--fill)}
@keyframes rise{from{opacity:0;transform:translateY(10px)}}
@keyframes fade{from{opacity:0}}
@keyframes pop{from{opacity:0;transform:scale(.97) translateY(-4px)}}
@keyframes pulse{50%{opacity:.5}}
.scroll{animation:fade .22s ease-out}
.detail{animation:rise .3s cubic-bezier(.2,.8,.2,1)}
.d-main{animation:rise .45s .06s cubic-bezier(.2,.8,.2,1) backwards}
.watch{animation:fade .2s ease-out}
.menu,.w-menu{animation:pop .14s ease-out}
.toast,.upnext{animation:rise .25s ease-out}
.sr{position:absolute;width:1px;height:1px;overflow:hidden;clip-path:inset(50%);white-space:nowrap}
.sk{pointer-events:none}
.sk .art,.sk .logo,.sk strong{background:var(--row);animation:pulse 1.4s ease-in-out infinite}
.sk strong{display:block;width:70%;height:.8rem;margin:.5rem auto 0;border-radius:6px}
.row.sk strong{width:60%;margin:0}
.pager{display:flex;align-items:center;justify-content:space-between;flex:none;gap:1rem;padding:.5rem .8rem;border-top:1px solid var(--hair);color:var(--dim);font-size:.78rem}
.pager .range{white-space:nowrap;font-variant-numeric:tabular-nums}
.pages{display:flex;align-items:center;gap:.2rem}
.pages button{display:grid;place-items:center;min-width:2rem;height:2rem;padding:0 .45rem;border-radius:9px;color:var(--dim);font-variant-numeric:tabular-nums}
.pages button:hover:not(:disabled){background:var(--row);color:var(--text)}
.pages button.on{background:var(--soft);color:var(--accent);font-weight:700}
.pages button:disabled{opacity:.3;cursor:default}
.pages svg{width:1rem;height:1rem}
.pages .gap{padding:0 .25rem;color:var(--faint)}
.pages .of{display:none;margin:0 .3rem;color:var(--faint);white-space:nowrap}
.channels .pager{justify-content:center}
.channels .pager .range,.channels .pages .gap,.channels .pages button:not(.on):not(.step){display:none}
.channels .pages .of{display:inline}
.shelf h2,.all-head{margin:.2rem 0 .7rem;font-size:1rem;font-weight:600}
.strip{display:flex;gap:1rem;padding:.4rem 0 1rem;overflow-x:auto;scroll-snap-type:x proximity;scrollbar-width:thin}
.strip .card{flex:none;width:8.6rem;scroll-snap-align:start}
.card small{display:block;margin-top:.1rem;color:var(--faint);font-size:.68rem}
"#,
    // Narrow screens: the rail becomes a floating tab bar, categories a button
    r#"@media(max-width:820px){.brand span,.who b{display:none}.topbar{gap:.5rem}.rail{top:auto;bottom:.8rem;left:50%;flex-direction:row;transform:translateX(-50%)}.rail button{width:auto;min-width:4.4rem;height:3rem;padding:0 .8rem}.rail span{display:block}.workspace{inset:4.2rem 0 0;grid-template-columns:1fr;padding:0 .6rem .6rem}.sidebar{display:none}.cats-btn{display:inline-flex}.scroll,.episodes{padding-bottom:5.5rem}.d-hero{min-height:auto;padding:7rem 1rem 1.6rem}.d-facts{position:relative;inset:auto;width:auto;margin-top:1.4rem}.d-sec{padding:1rem 1rem .8rem}.w-top{padding:.8rem .8rem 2.5rem}.w-bottom{padding:3rem .8rem .6rem}.w-row .vol,.w-row .volume{display:none}.w-menu{right:.8rem;bottom:5.2rem}}
/* Live TV on a phone: keep the picture above the channels, without squeezing in the guide. */
@media(max-width:820px){.live{display:flex;flex-direction:column;margin:0 -.6rem;border:0;border-radius:0}.stage{order:-1;flex:none}.stage.idle{display:none}.live-stage{display:block;height:auto}.player{margin:0;border-radius:0;aspect-ratio:16/9}.player.fill{aspect-ratio:auto}.controls{padding:2.2rem .5rem .3rem}.ctl{width:2.8rem;height:2.8rem}.vol{display:none}.bigplay{width:4.2rem;height:4.2rem}.sound-note{bottom:3.9rem}.guide{min-height:0;padding:.7rem .9rem .8rem;border-top:0;background:var(--bg)}.tl{height:5.6rem}.channels{flex:1;border:0;background:none}.channels .head{padding:.8rem 1rem .4rem}.channels .scroll{padding-bottom:5.5rem}}
@media(max-width:820px){.live.watching .stage{display:block;width:100%;background:#000}.live.watching .live-stage{display:block;width:100%}.live.watching .player{width:100%;min-height:0;aspect-ratio:16/9}.live.watching .player video{display:block}.live.watching .guide{display:none}.live.watching .channels{min-height:0;overflow:hidden}.live.watching .channels .scroll{min-height:0;overflow-y:auto}.live.watching .controls{gap:.1rem}.live.watching .controls .ctl{width:2.5rem;height:2.5rem}.live.watching .sound-note{font-size:.7rem}}
@media(max-width:480px){.live.watching .controls{overflow-x:auto;scrollbar-width:none}.live.watching .controls::-webkit-scrollbar{display:none}.live.watching .controls .ctl,.live.watching .controls .live-pill{flex:none}}
/* A title is a page, not a sheet: portrait art, a legible fade, and actions within reach. */
@media(max-width:820px){.detail .d-hero{min-height:100svh;padding:5.5rem 1.1rem 5.5rem}.detail .d-art{object-position:center top}.detail .d-shade{background:linear-gradient(180deg,rgba(11,7,9,.16) 0%,rgba(11,7,9,.1) 28%,rgba(11,7,9,.75) 62%,var(--bg) 100%)}.detail .d-main{max-width:none}.detail .d-title{font-size:clamp(2.1rem,9vw,3.5rem);text-align:center}.detail .d-genres,.detail .d-actions,.detail .d-meta{justify-content:center}.detail .d-by,.detail .plot,.detail .more{text-align:left}.detail .d-facts{margin-top:1.2rem}.detail .d-sec{padding:1rem 1.1rem}.detail .ep-heading{align-items:flex-start}.detail .epc{flex-basis:min(74vw,18rem)}}
@media(max-width:820px){.detail .d-poster{top:5.5rem;right:50%;width:min(54vw,15rem);max-height:45svh;transform:translateX(50%)}.watch .w-row{gap:0}.watch .w-row .ctl{width:2.35rem;height:2.35rem}.watch .w-row .skip svg{width:1.45rem;height:1.45rem}}
@media(max-width:820px){.detail-return{left:.75rem}}
/* A phone turned sideways while watching: just the picture. */
@media(max-height:500px) and (orientation:landscape){body:has(.live.watching) .topbar,body:has(.live.watching) .rail,.workspace:has(.live.watching) .sidebar,.live.watching .channels,.live.watching .guide{display:none}.workspace:has(.live.watching){inset:0;grid-template-columns:1fr;padding:0}.live.watching{grid-template-columns:1fr;margin:0}.live.watching .stage{flex:1}.live.watching .live-stage{height:100%}.live.watching .player{height:100%;aspect-ratio:auto}}
@media(max-width:820px){.pager{justify-content:center;padding-bottom:4.8rem}.pager .range,.pages .gap,.pages button:not(.on):not(.step){display:none}.pages .of{display:inline}}
@media(prefers-reduced-motion:reduce){*{transition:none!important;animation:none!important}}
"#,
);

// Icons: 24x24 outlines, drawn as one path each.
const LIVE_TV: &str = "M5 5h14a2 2 0 012 2v9a2 2 0 01-2 2H5a2 2 0 01-2-2V7a2 2 0 012-2zM8 21h8M12 18v3M10 9l5 3-5 3V9z";
const MOVIE: &str = "M5 4h14a2 2 0 012 2v12a2 2 0 01-2 2H5a2 2 0 01-2-2V6a2 2 0 012-2zM7 4v16M17 4v16M3 9h4M3 15h4M17 9h4M17 15h4";
const SERIES: &str = "M5.5 4h13A1.5 1.5 0 0120 5.5v3a1.5 1.5 0 01-1.5 1.5h-13A1.5 1.5 0 014 8.5v-3A1.5 1.5 0 015.5 4zM5.5 11h13a1.5 1.5 0 011.5 1.5v6a1.5 1.5 0 01-1.5 1.5h-13A1.5 1.5 0 014 18.5v-6A1.5 1.5 0 015.5 11zM8 15h8";
const SEARCH: &str = "m21 21-4.35-4.35M19 11a8 8 0 1 1-16 0 8 8 0 0 1 16 0Z";
const PLAY: &str = "M7 4l13 8-13 8V4z";
const PAUSE: &str = "M8 5v14M16 5v14";
const VOLUME: &str = "M11 5L6 9H3v6h3l5 4V5zM15.5 8.5a5 5 0 010 7M18.5 5.5a9 9 0 010 13";
const MUTED: &str = "M11 5L6 9H3v6h3l5 4V5zM22 9l-6 6M16 9l6 6";
const FULLSCREEN: &str = "M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5";
const DOWNLOAD: &str = "M12 4v11m0 0-4-4m4 4 4-4M5 20h14";
const BACK: &str = "M15 5l-7 7 7 7";
const NEXT_PAGE: &str = "M9 5l7 7-7 7";
const USER: &str = "M20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2M12 11a4 4 0 100-8 4 4 0 000 8z";
const PIP: &str = "M4 5h16a1 1 0 011 1v7M3 6v11a1 1 0 001 1h6M13 14h7a1 1 0 011 1v3a1 1 0 01-1 1h-7a1 1 0 01-1-1v-3a1 1 0 011-1z";
const SORT: &str = "M3 6h11M3 12h7M3 18h4M17 6v12m0 0l-3-3m3 3l3-3";
const CHECK: &str = "M5 12l5 5 9-10";
const INFO: &str = "M12 3a9 9 0 100 18 9 9 0 000-18zM12 8h.01M11 12h1v5h1";
const HEART: &str =
    "M12 20s-7.5-4.6-7.5-10.2A4.3 4.3 0 0112 7.3a4.3 4.3 0 017.5 2.5C19.5 15.4 12 20 12 20z";
const CLOSE: &str = "M6 6l12 12M18 6L6 18";
const LIST: &str = "M4 6h16M4 12h16M4 18h10";
const NEXT: &str = "M6 5l10 7-10 7V5zM19 5v14";
const EXTERNAL: &str = "M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 01-1 1H5a1 1 0 01-1-1V7a1 1 0 011-1h5";
const GEAR: &str = "M12 9a3 3 0 100 6 3 3 0 000-6zM19.4 15a1.7 1.7 0 00.3 1.8l.1.1a2 2 0 11-2.8 2.8l-.1-.1a1.7 1.7 0 00-1.8-.3 1.7 1.7 0 00-1 1.5V21a2 2 0 11-4 0v-.1a1.7 1.7 0 00-1.1-1.5 1.7 1.7 0 00-1.8.3l-.1.1a2 2 0 11-2.8-2.8l.1-.1a1.7 1.7 0 00.3-1.8 1.7 1.7 0 00-1.5-1H3a2 2 0 110-4h.1a1.7 1.7 0 001.5-1.1 1.7 1.7 0 00-.3-1.8l-.1-.1a2 2 0 112.8-2.8l.1.1a1.7 1.7 0 001.8.3H9a1.7 1.7 0 001-1.5V3a2 2 0 114 0v.1a1.7 1.7 0 001 1.5 1.7 1.7 0 001.8-.3l.1-.1a2 2 0 112.8 2.8l-.1.1a1.7 1.7 0 00-.3 1.8V9a1.7 1.7 0 001.5 1H21a2 2 0 110 4h-.1a1.7 1.7 0 00-1.5 1z";

/// The Rust logo, as the paths of the ring's teeth and of the R inside it (the ring is a circle).
const MARK_TEETH: &str = "M32 2v8M32 54v8M2 32h8M54 32h8M11 11l6 6M47 47l6 6M53 11l-6 6M17 47l-6 6M20 5l3 8M44 51l3 8M5 20l8 3M51 44l8 3M44 5l-3 8M23 51l-3 8M59 20l-8 3M13 41l-8 3";
const MARK_R: &str = "M23 44V20h11c6 0 10 4 10 9s-4 9-10 9H23M34 38l11 7";

/// The tab icon: the same logo, in the accent colour on the page's own background. It is a data
/// address so there is no file to serve (and the page's policy allows `data:` images).
fn set_favicon() {
    let svg = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'>\
         <rect width='64' height='64' rx='14' fill='#0b0709'/>\
         <g fill='none' stroke='#ff7d92' stroke-width='4' stroke-linecap='round' stroke-linejoin='round'>\
         <circle cx='32' cy='32' r='21'/><path d='{MARK_TEETH}'/><path d='{MARK_R}'/></g></svg>"
    );
    let href = format!(
        "data:image/svg+xml,{}",
        svg.replace('#', "%23")
            .replace('<', "%3C")
            .replace('>', "%3E")
            .replace(' ', "%20")
    );
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    if let (Ok(link), Ok(Some(head))) = (doc.create_element("link"), doc.query_selector("head")) {
        let _ = link.set_attribute("rel", "icon");
        let _ = link.set_attribute("type", "image/svg+xml");
        let _ = link.set_attribute("href", &href);
        let _ = head.append_child(&link);
    }
}

fn main() {
    // The page is built to abort on a panic, which the browser reports as a bare "unreachable".
    // Say what happened first.
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&JsValue::from_str(&format!("RIPTV crashed: {info}")));
    }));
    set_favicon();
    fade_pictures_in();
    dioxus::launch(App);
}

/// Marks every picture on the page when it has loaded (`data-ready`) or failed (`data-failed`), so
/// the stylesheet can fade it in over its placeholder. One listener does it for all of them: a
/// `load` event doesn't bubble, but it can be caught on the way down, so no picture needs a handler
/// of its own (there can be hundreds).
fn fade_pictures_in() {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    for (event, mark) in [("load", "data-ready"), ("error", "data-failed")] {
        let on = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            if let Some(img) = e
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                .filter(|el| el.tag_name() == "IMG")
            {
                let _ = img.set_attribute(mark, "");
            }
        });
        let _ =
            doc.add_event_listener_with_callback_and_bool(event, on.as_ref().unchecked_ref(), true);
        // For as long as the page lives.
        on.forget();
    }
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
    c.approve().await?;
    c.auth().await?;
    Ok(c)
}

async fn add_playlist(url: &str) -> xtream::Result<Client> {
    let c = Client::new(url, "", "")?.via_proxy(&proxy_url())?;
    c.approve().await?;
    c.load_playlist().await
}

/// The error, plus what usually fixes it when the provider itself said no.
fn explain(e: &xtream::Error) -> String {
    let s = e.to_string();
    if s.contains("401") || s.contains("403") || s.contains("404") {
        format!(
            "{s}. The server turned the request away: check the address (it may need a port, like :8080)."
        )
    } else {
        s
    }
}

fn now_secs() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

/// Local `HH:MM` for a unix time.
fn clock(ts: u64) -> String {
    let d = js_sys::Date::new(&JsValue::from_f64(ts as f64 * 1000.0));
    format!("{:02}:{:02}", d.get_hours(), d.get_minutes())
}

fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A rating worth showing: providers send "0" or "0.0" for unrated titles.
fn score(rating: &Option<String>) -> Option<String> {
    rating
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.chars().all(|c| c == '0' || c == '.'))
        .map(|r| r.chars().take(3).collect())
}

/// `Title.ext`, safe as a file name. `url` is a stream URL, which ends in `<id>.<ext>`.
fn file_name(title: &str, url: &str) -> String {
    let ext = url
        .rsplit('.')
        .next()
        .filter(|e| (1..=4).contains(&e.len()) && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("mp4");
    let title: String = title
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    format!("{title}.{ext}")
}

fn video_el() -> Option<web_sys::HtmlVideoElement> {
    web_sys::window()?
        .document()?
        .get_element_by_id("live-video")?
        .dyn_into()
        .ok()
}

/// Prefer the device's media pipeline for HTTPS HLS where the browser has one (Safari, recent
/// Chrome). A media element can fetch cross-origin video without CORS, and Rust/proxy playback
/// remains the fallback if it cannot.
fn native_hls_source(client: &Client, url: &str) -> Option<String> {
    let media = xtream::Url::parse(url).ok()?;
    let upstream = client.upstream(&media);
    let hls = upstream.path().to_ascii_lowercase().ends_with(".m3u8");
    (upstream.scheme() == "https" && hls && rstreamkit::mse::plays_hls_natively())
        .then(|| upstream.to_string())
}

fn toggle(v: &web_sys::HtmlVideoElement) {
    if v.paused() {
        let _ = v.play();
    } else {
        let _ = v.pause();
    }
}

fn toggle_play() {
    if let Some(v) = video_el() {
        toggle(&v);
    }
}

/// `1:05:09` or `4:07`.
fn hms(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Jump to the newest video the player has buffered.
fn go_live() {
    if let Some(v) = video_el() {
        let buffered = v.buffered();
        if let Some(last) = buffered.length().checked_sub(1)
            && let Ok(end) = buffered.end(last)
        {
            v.set_current_time(end);
        }
    }
}

/// Picture-in-picture. web-sys has no bindings for it, so it is called through JS reflection.
fn toggle_pip(video: Option<web_sys::HtmlVideoElement>) {
    let (Some(video), Some(doc)) = (video, web_sys::window().and_then(|w| w.document())) else {
        return;
    };
    let call = |target: &JsValue, method: &str| {
        js_sys::Reflect::get(target, &JsValue::from_str(method))
            .ok()
            .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
            .and_then(|f| f.call0(target).ok())
    };
    let doc: &JsValue = doc.as_ref();
    let in_pip = js_sys::Reflect::get(doc, &JsValue::from_str("pictureInPictureElement"))
        .is_ok_and(|e| !e.is_null() && !e.is_undefined());
    if in_pip {
        call(doc, "exitPictureInPicture");
    } else {
        call(video.as_ref(), "requestPictureInPicture");
    }
}

fn toggle_mute() -> Option<bool> {
    let v = video_el()?;
    v.set_muted(!v.muted());
    Some(v.muted())
}

/// Real fullscreen where the browser really does it. Some embedded browsers accept the request and
/// never act on it (and iOS Safari only fullscreens a bare `<video>`), so if nothing happened a
/// moment later the player fills the page instead (`expanded`).
fn toggle_fullscreen(id: &'static str, mut expanded: Signal<bool>) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    if doc.fullscreen_element().is_some() {
        doc.exit_fullscreen();
    } else if expanded() {
        expanded.set(false);
    } else {
        if let Some(player) = doc.get_element_by_id(id) {
            let _ = player.request_fullscreen();
        }
        // Not Dioxus's `spawn`: the key listener calls this from outside its runtime.
        wasm_bindgen_futures::spawn_local(async move {
            rstreamkit::mse::sleep(Duration::from_millis(600)).await;
            if doc.fullscreen_element().is_none() {
                expanded.set(true);
            }
        });
    }
}

/// Chromium counts the audio bytes it has decoded. Zero after a few seconds of playing, with the
/// element not muted, means the stream's sound isn't in a format this browser can decode (AC-3,
/// DTS and friends) or it has none. Other browsers don't report it, and then this stays quiet.
/// Stops a `<video>`'s download for good. A player that goes away must do this: the browser
/// otherwise keeps a removed element's stream open for a while, and a provider that allows one
/// connection then refuses the next channel.
fn release(video: &web_sys::HtmlVideoElement) {
    let _ = video.remove_attribute("src");
    video.load();
}

/// What a failed stream request means for the viewer, from the provider's answer.
fn channel_trouble(e: &xtream::Error) -> String {
    let text = e.to_string();
    let has = |s: &str| text.contains(s);
    if has("404") || has("Not Found") {
        "This channel is offline: your provider doesn't have it right now (404).".into()
    } else if has("401") || has("403") || has("Forbidden") || has("Unauthorized") {
        "Your provider refused this channel (403). Your subscription may not include it, or \
         another device is using your connection."
            .into()
    } else if has("429") || has("458") || has("509") {
        "Your provider says too many streams are open on your account. Stop it on your other \
         devices and try again."
            .into()
    } else if has("5XX") || has("500") || has("502") || has("503") || has("Service Unavailable") {
        "Your provider's server is failing for this channel right now (5xx). Try again later, or \
         check that your account isn't streaming on another device."
            .into()
    } else if has("timed out") || has("took too long") {
        "Your provider isn't answering for this channel. Try again in a moment.".into()
    } else if has("ffmpeg isn't installed") {
        text
    } else {
        format!("Can't play this channel: {text}")
    }
}

/// A conversion the standard player already knows it needs (no check first).
fn known_conversion(client: &Client, url: &str, plan: standard::Plan) -> Feed {
    standard::conversion(client, url, plan).map_or(Feed::Pending, Feed::Converted)
}

/// Shown, with the spinner, when a channel takes unusually long to start.
const STILL_STARTING: &str = "Still starting: your provider is slow to answer…";

/// Playing for `after` seconds, unmuted, and not one byte of sound decoded (Chrome counts them):
/// the browser can't decode this sound.
fn no_audio_decoded(v: &web_sys::HtmlVideoElement, after: f64) -> bool {
    let bytes = js_sys::Reflect::get(v, &JsValue::from_str("webkitAudioDecodedByteCount"))
        .ok()
        .and_then(|n| n.as_f64());
    v.current_time() > after && !v.muted() && bytes == Some(0.0)
}

/// Browser storage key for the opt-in experimental Rust playback engine.
const RUST_SOUND: &str = "riptv.experimental-rust-player";

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

#[component]
fn App() -> Element {
    let session = use_context_provider(|| Signal::new(None::<Client>));
    use_context_provider(|| Signal::new(String::new())); // the account name, if any
    // The browser/ffmpeg path is the stable default; Rust playback is opt-in.
    use_context_provider(|| {
        Signal::new(
            storage()
                .and_then(|s| s.get_item(RUST_SOUND).ok().flatten())
                .as_deref()
                == Some("1"),
        )
    });
    rsx! {
        // No `document::Title`: Dioxus web sets it via eval(), which the app's CSP forbids.
        // The title comes from Dioxus.toml instead.
        style { "{CSS}" }
        if session.read().is_some() { Browse {} } else { Login {} }
    }
}

#[component]
fn Icon(d: &'static str) -> Element {
    rsx! {
        svg {
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            path { d: "{d}" }
        }
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
            path { d: "{MARK_TEETH}" }
            path { d: "{MARK_R}" }
        }
    }
}

/// How a section's list is ordered. The first entry of `Sort::options` is each section's default.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Sort {
    Provider,
    Newest,
    Oldest,
    AZ,
    ZA,
    Rating,
}

impl Sort {
    fn saved(self) -> u8 {
        self as u8
    }

    fn restored(kind: Kind, value: u8) -> Self {
        Self::options(kind)
            .iter()
            .copied()
            .find(|option| option.saved() == value)
            .unwrap_or(Self::options(kind)[0])
    }

    fn label(self) -> &'static str {
        match self {
            Sort::Provider => "Provider order",
            Sort::Newest => "Newest added",
            Sort::Oldest => "Oldest added",
            Sort::AZ => "A to Z",
            Sort::ZA => "Z to A",
            Sort::Rating => "Top rated",
        }
    }

    fn options(kind: Kind) -> &'static [Sort] {
        match kind {
            Kind::Live => &[Sort::Provider, Sort::AZ, Sort::ZA],
            _ => &[Sort::Newest, Sort::Oldest, Sort::AZ, Sort::ZA, Sort::Rating],
        }
    }
}

/// "7.3" as 73, "8" as 80, junk as 0: enough to rank by without parsing floats.
fn tenths(rating: &Option<String>) -> u32 {
    let r = rating.as_deref().unwrap_or("").trim();
    let (whole, frac) = r.split_once('.').unwrap_or((r, ""));
    let digit = frac
        .bytes()
        .next()
        .filter(u8::is_ascii_digit)
        .map_or(0, |b| u32::from(b - b'0'));
    whole.parse::<u32>().unwrap_or(0) * 10 + digit
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Live,
    Movies,
    Series,
}

#[derive(Clone, Copy, PartialEq)]
enum ChannelAction {
    Up,
    Down,
    Number(usize),
}

#[derive(Clone, PartialEq)]
enum Target {
    Live { id: u64, url: String },
    Movie { id: u64, url: String },
    Series(u64),
}

#[derive(Clone, PartialEq)]
struct Row {
    key: u64,
    title: String,
    icon: Option<String>,
    category: Option<u64>,
    score: Option<String>,
    target: Target,
}

enum Items {
    Live(Vec<LiveStream>),
    Movies(Vec<VodStream>),
    Series(Vec<xtream::Series>),
}

/// Everything the provider lists for one section, loaded once. Categories are filtered
/// client-side, so switching category is instant and the sidebar can show counts.
struct Library {
    kind: Kind,
    items: Items,
    counts: HashMap<u64, usize>,
}

impl Library {
    /// Positions in the list, in the order `sort` asks for. The list itself stays as the provider
    /// sent it; sorting 30,000 titles by name takes a few milliseconds, once per change of sort.
    fn order(&self, sort: Sort) -> Vec<u32> {
        // Build only the index plus the cached keys for the requested sort. The previous code
        // allocated three full-length arrays even for provider order and alphabetical sorts.
        let mut order: Vec<u32> = (0..self.len() as u32).collect();
        let name = |i: u32| match &self.items {
            Items::Live(v) => v[i as usize].name.as_str(),
            Items::Movies(v) => v[i as usize].name.as_str(),
            Items::Series(v) => v[i as usize].name.as_str(),
        };
        // Every numeric sort is "smallest key first"; titles with no date go last either way.
        let number = |i: u32| -> u64 {
            let i = i as usize;
            let (date, rating) = match &self.items {
                Items::Live(_) => (None, 0),
                Items::Movies(v) => (v[i].added, tenths(&v[i].rating)),
                Items::Series(v) => (v[i].last_modified, tenths(&v[i].rating)),
            };
            match sort {
                Sort::Newest => date.map_or(u64::MAX, |d| u64::MAX - d),
                Sort::Oldest => date.unwrap_or(u64::MAX),
                _ => u64::from(u32::MAX - rating),
            }
        };
        match sort {
            Sort::Provider => {}
            Sort::AZ | Sort::ZA => {
                order.sort_by_cached_key(|&i| name(i).to_lowercase());
                if sort == Sort::ZA {
                    order.reverse();
                }
            }
            _ => order.sort_by_cached_key(|&i| number(i)),
        }
        order
    }

    fn len(&self) -> usize {
        match &self.items {
            Items::Live(v) => v.len(),
            Items::Movies(v) => v.len(),
            Items::Series(v) => v.len(),
        }
    }
}

fn count<T>(list: &[T], category: impl Fn(&T) -> Option<u64>) -> HashMap<u64, usize> {
    let mut counts = HashMap::new();
    for id in list.iter().filter_map(category) {
        *counts.entry(id).or_default() += 1;
    }
    counts
}

async fn load(c: &Client, kind: Kind) -> xtream::Result<Library> {
    let items = match kind {
        Kind::Live => Items::Live(c.live_streams(None).await?),
        Kind::Movies => Items::Movies(c.vod_streams(None).await?),
        Kind::Series => Items::Series(c.series(None).await?),
    };
    let counts = match &items {
        Items::Live(v) => count(v, |s| s.category_id),
        Items::Movies(v) => count(v, |s| s.category_id),
        Items::Series(v) => count(v, |s| s.category_id),
    };
    Ok(Library {
        kind,
        items,
        counts,
    })
}

/// One page of rows: of the items in `cat` (all if `None`) whose title contains `q` (lowercase),
/// `limit` of them after the first `skip`, plus how many matched in all. Only the rows on the page
/// are built (that includes making a stream URL), so a 30,000-title library stays cheap on every
/// keystroke in the search box and every turn of the page.
fn rows(
    lib: &Library,
    order: &[u32],
    c: &Client,
    cat: Option<u64>,
    q: &str,
    (skip, limit): (usize, usize),
) -> (Vec<Row>, usize) {
    fn take<T>(
        (list, order): (&[T], &[u32]),
        (cat, q, (skip, limit), total_hint): (Option<u64>, &str, (usize, usize), Option<usize>),
        info: impl Fn(&T) -> (&str, Option<u64>),
        row: impl Fn(&T) -> Row,
    ) -> (Vec<Row>, usize) {
        let mut hits = order.iter().map(|&i| &list[i as usize]).filter(|t| {
            let (title, category) = info(t);
            (cat.is_none() || category == cat) && xtream::contains_lowercase(title, q)
        });
        let skipped = hits.by_ref().take(skip).count();
        let shown: Vec<Row> = hits.by_ref().take(limit).map(&row).collect();
        let total = total_hint.unwrap_or_else(|| skipped + shown.len() + hits.count());
        (shown, total)
    }
    // Category totals are already counted on load. Avoid scanning thousands of remaining items
    // every render when the viewer isn't searching.
    let total_hint = q
        .is_empty()
        .then(|| cat.map_or(lib.len(), |id| lib.counts.get(&id).copied().unwrap_or(0)));
    let filter = (cat, q, (skip, limit), total_hint);
    match &lib.items {
        Items::Live(v) => take(
            (v, order),
            filter,
            |s| (&s.name, s.category_id),
            |s| Row {
                key: s.stream_id,
                title: s.name.clone(),
                icon: s.stream_icon.clone(),
                category: s.category_id,
                score: None,
                target: Target::Live {
                    id: s.stream_id,
                    url: c.live_url(s.stream_id, "m3u8").to_string(),
                },
            },
        ),
        Items::Movies(v) => take(
            (v, order),
            filter,
            |s| (&s.name, s.category_id),
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
                    category: s.category_id,
                    score: score(&s.rating),
                    target: Target::Movie {
                        id: s.stream_id,
                        url: c.movie_url(s.stream_id, ext).to_string(),
                    },
                }
            },
        ),
        Items::Series(v) => take(
            (v, order),
            filter,
            |s| (&s.name, s.category_id),
            |s| Row {
                key: s.series_id,
                title: s.name.clone(),
                icon: s.cover.clone(),
                category: s.category_id,
                score: score(&s.rating),
                target: Target::Series(s.series_id),
            },
        ),
    }
}

/// Resolve a channel button or number against the same filtered order shown in the list.
/// This scans the existing index, without building another playlist-sized allocation.
fn channel_target<'a>(
    lib: &'a Library,
    order: &[u32],
    category: Option<u64>,
    query: &str,
    current: u64,
    action: ChannelAction,
) -> Option<(usize, &'a LiveStream)> {
    let Items::Live(streams) = &lib.items else {
        return None;
    };
    let mut first = None;
    let mut last = None;
    let mut previous = None;
    let mut next = None;
    let mut found_current = false;
    let mut position = 0;
    for &index in order {
        let stream = streams.get(index as usize)?;
        if category.is_some_and(|id| stream.category_id != Some(id))
            || !xtream::contains_lowercase(&stream.name, query)
        {
            continue;
        }
        let item = (position, stream);
        if first.is_none() {
            first = Some(item);
        }
        if let ChannelAction::Number(number) = action
            && number == position + 1
        {
            return Some(item);
        }
        if found_current && next.is_none() {
            next = Some(item);
        }
        if stream.stream_id == current {
            found_current = true;
            previous = last;
        }
        last = Some(item);
        position += 1;
    }
    match action {
        ChannelAction::Up => previous.or(last),
        ChannelAction::Down => next.or(first),
        ChannelAction::Number(_) => None,
    }
}

#[component]
fn Browse() -> Element {
    let mut session = use_context::<Signal<Option<Client>>>();
    let playlist = use_context::<Signal<String>>();
    let mut rust_sound = use_context::<Signal<bool>>();
    let client = use_hook(|| {
        session
            .read()
            .clone()
            .expect("Browse only renders when logged in")
    });

    let remembered = use_hook(preferences::load);
    let mut kind = use_signal(|| Kind::Live);
    let mut category = use_signal(|| remembered.categories[0]);
    let mut search = use_signal(String::new);
    let mut category_search = use_signal(String::new);
    let mut playing = use_signal(|| None::<Play>);
    let mut queue = use_signal(Vec::<Play>::new); // the episodes after the one playing
    let mut live = use_signal(|| None::<(u64, String, String)>); // (id, title, playlist url)
    let mut open = use_signal(|| None::<Row>); // the movie or series page
    let mut sort = use_signal(|| Sort::restored(Kind::Live, remembered.sorts[0]));
    let mut sort_open = use_signal(|| false);
    let mut account_open = use_signal(|| false);
    let mut cats_open = use_signal(|| false);
    let mut player_revision = use_signal(|| 0_u64);
    let mut refresh = use_signal(|| 0_u64);
    let mut page = use_signal(|| 0_usize);

    let (c_cats, c_lib) = (client.clone(), client.clone());
    let cats = use_resource(move || {
        let (c, k, _) = (c_cats.clone(), kind(), refresh());
        async move {
            let list = match k {
                Kind::Live => c.live_categories().await,
                Kind::Movies => c.vod_categories().await,
                Kind::Series => c.series_categories().await,
            };
            list.map(|l| (k, l))
        }
    });
    let library = use_resource(move || {
        let (c, k, _) = (c_lib.clone(), kind(), refresh());
        async move { load(&c, k).await }
    });

    // A remembered category may have been removed since the last visit.
    use_effect(move || {
        if let Some(Ok((loaded_kind, list))) = &*cats.read()
            && *loaded_kind == kind()
            && let Some(id) = category()
            && !list.iter().any(|c| c.category_id == id)
        {
            category.set(None);
            preferences::update(|p| p.categories[kind() as usize] = None);
        }
    });

    // The list's order for the chosen sort; recomputed only when the library or the sort changes.
    let order = use_memo(move || {
        library
            .read()
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .filter(|l| l.kind == kind())
            .map(|l| l.order(sort()))
    });

    // Live TV shows a tile grid until a category (or a channel) is picked, then list + player.
    let three_pane = kind() == Kind::Live && (category().is_some() || live().is_some());

    let mut pick_kind = move |k: Kind| {
        let prefs = preferences::load();
        kind.set(k);
        sort.set(Sort::restored(k, prefs.sorts[k as usize]));
        category.set(prefs.categories[k as usize]);
        search.set(String::new());
        category_search.set(String::new());
        open.set(None);
        playing.set(None);
        live.set(None);
        page.set(0);
    };
    let mut select = move |cat: Option<u64>| {
        category.set(cat);
        preferences::update(|p| p.categories[kind() as usize] = cat);
        search.set(String::new());
        open.set(None);
        cats_open.set(false);
        if cat.is_none() {
            live.set(None);
        }
        page.set(0);
    };
    let mut pick = move |row: Row| match row.target {
        Target::Live { id, url } => {
            playing.set(None);
            if !three_pane {
                category.set(row.category);
                page.set(0);
            }
            live.set(Some((id, row.title, url)));
        }
        Target::Movie { .. } | Target::Series(_) => open.set(Some(row)),
    };

    // A shelf remembers a title's id, not its address (which carries the account's credentials).
    let c_shelf = client.clone();
    let reopen = use_callback(move |e: shelves::Entry| {
        let target = match e.kind {
            shelves::Title::Movie => Target::Movie {
                id: e.id,
                url: c_shelf
                    .movie_url(e.id, e.ext.as_deref().unwrap_or("mp4"))
                    .to_string(),
            },
            shelves::Title::Series => Target::Series(e.id),
        };
        pick(Row {
            key: e.id,
            title: e.title,
            icon: e.icon,
            category: None,
            score: None,
            target,
        });
    });

    let cat_list = move || -> Element {
        match &*cats.read() {
            Some(Err(e)) => rsx! { p { class: "err", "{e}" } },
            Some(Ok((k, list))) if *k == kind() => {
                let lib = library.read();
                let lib = lib
                    .as_ref()
                    .and_then(|r| r.as_ref().ok())
                    .filter(|l| l.kind == kind());
                let total = lib.map_or(0, Library::len);
                let q = category_search().to_lowercase();
                rsx! {
                    input {
                        class: "filter",
                        aria_label: "Filter categories",
                        placeholder: "Filter categories",
                        value: "{category_search}",
                        oninput: move |e| category_search.set(e.value())
                    }
                    button {
                        class: if category().is_none() { "cat on" } else { "cat" },
                        onclick: move |_| select(None),
                        span { "All" } small { "{thousands(total)}" }
                    }
                    for c in list.iter().filter(|c| q.is_empty() || xtream::contains_lowercase(&c.category_name, &q)) {
                        button {
                            key: "{c.category_id}",
                            class: if category() == Some(c.category_id) { "cat on" } else { "cat" },
                            onclick: {
                                let id = c.category_id;
                                move |_| select(Some(id))
                            },
                            span { "{c.category_name}" }
                            small { "{thousands(lib.map_or(0, |l| l.counts.get(&c.category_id).copied().unwrap_or(0)))}" }
                        }
                    }
                }
            }
            _ => rsx! { p { class: "dim", "Loading…" } },
        }
    };

    // A new key makes a new list, which is what lets a page arrive instead of just changing.
    let scope = format!(
        "{}-{:?}-{}-{}-{:?}-{}",
        kind() as u8,
        category(),
        search(),
        refresh(),
        sort(),
        page()
    );
    let body = match &*library.read() {
        Some(Err(e)) => rsx! { p { class: "err", "{e}" } },
        Some(Ok(lib)) if lib.kind == kind() => {
            let order = order.read();
            let order = order
                .as_deref()
                .filter(|o| o.len() == lib.len())
                .unwrap_or(&[]);
            let size = if three_pane { PAGE_LIST } else { PAGE_GRID };
            let q = search().to_lowercase();
            let (mut shown, total) =
                rows(lib, order, &client, category(), &q, (page() * size, size));
            let pages = total.div_ceil(size).max(1);
            let now = page().min(pages - 1);
            // A refresh can leave the page asked for past the end: show the last one instead.
            if now != page() {
                shown = rows(lib, order, &client, category(), &q, (now * size, size)).0;
            }
            let list = if shown.is_empty() {
                rsx! { div { class: "empty", "Nothing matches." } }
            } else if three_pane {
                rsx! {
                    for r in shown {
                        ChannelRow {
                            key: "{r.key}",
                            row: r.clone(),
                            active: live().as_ref().map(|(id, _, _)| *id) == Some(r.key),
                            onpick: pick
                        }
                    }
                }
            } else {
                rsx! {
                    div { class: if kind() == Kind::Live { "grid tiles" } else { "grid" },
                        for r in shown {
                            Card { key: "{r.key}", row: r.clone(), onpick: pick }
                        }
                    }
                }
            };
            // The shelves sit above the whole section, not above a category, search or later page.
            let shelves_now: Vec<_> =
                if kind() != Kind::Live && category().is_none() && q.is_empty() && now == 0 {
                    let what = if kind() == Kind::Movies {
                        shelves::Title::Movie
                    } else {
                        shelves::Title::Series
                    };
                    [
                        ("Continue watching", shelves::recent()),
                        ("My List", shelves::mine()),
                    ]
                    .into_iter()
                    .map(|(name, mut entries)| {
                        entries.retain(|e| e.kind == what);
                        (name, entries)
                    })
                    .filter(|(_, entries)| !entries.is_empty())
                    .collect()
                } else {
                    vec![]
                };
            rsx! {
                div { class: "scroll", key: "{scope}",
                    if !shelves_now.is_empty() {
                        for (name, entries) in shelves_now {
                            Shelf { key: "{name}", title: name, entries, onpick: reopen }
                        }
                        h2 { class: "all-head", if kind() == Kind::Movies { "All movies" } else { "All series" } }
                    }
                    {list}
                }
                Pager { page: now, pages, total, size, onpage: move |n| page.set(n) }
            }
        }
        // Still loading, or showing the previous section's data: don't pass that off as this one.
        _ => rsx! {
            div { class: "scroll", role: "status", aria_busy: "true",
                span { class: "sr", "Loading your library…" }
                if three_pane {
                    for n in 0..12 {
                        div { class: "row sk", key: "{n}", span { class: "logo" } div { strong {} } }
                    }
                } else {
                    div { class: if kind() == Kind::Live { "grid tiles" } else { "grid" },
                        for n in 0..18 {
                            div { class: "card sk", key: "{n}", span { class: "art" } strong {} }
                        }
                    }
                }
            }
        },
    };

    let player = playing().map(|play| {
        let next = queue()
            .first()
            .map(|p| p.subtitle.clone().unwrap_or_else(|| p.title.clone()));
        rsx! {
            Watch {
                key: "{play.url}",
                play,
                next,
                onclose: move |_| {
                    playing.set(None);
                    queue.set(vec![]);
                },
                onnext: move |_| {
                    let mut rest = queue();
                    if !rest.is_empty() {
                        playing.set(Some(rest.remove(0)));
                        queue.set(rest);
                    }
                },
            }
        }
    });
    let channel_client = client.clone();
    let live_panel = live().map(|(id, title, url)| {
        let player_key = format!("{url}-{}", player_revision());
        rsx! {
            LivePlayer {
                key: "{player_key}",
                id,
                title,
                url,
                onchannel: move |action| {
                    let selected = {
                        let library = library.read();
                        let order = order.read();
                        library.as_ref()
                            .and_then(|r| r.as_ref().ok())
                            .zip(order.as_deref())
                            .and_then(|(lib, order)| channel_target(
                                lib, order, category(), &search().to_lowercase(), id, action,
                            ))
                            .map(|(position, stream)| (
                                position,
                                stream.stream_id,
                                stream.name.clone(),
                            ))
                    };
                    if let Some((position, next_id, name)) = selected
                        && next_id != id
                    {
                        page.set(position / PAGE_LIST);
                        live.set(Some((next_id, name, channel_client.live_url(next_id, "m3u8").to_string())));
                    }
                },
            }
        }
    });
    let cats_sheet = cats_open().then(|| {
        rsx! {
            div { class: "overlay bottom", onclick: move |_| cats_open.set(false),
                div { class: "sheet cats", onclick: move |e| e.stop_propagation(),
                    div { class: "bar",
                        strong { "Categories" }
                        button { class: "icon-btn", aria_label: "Close", onclick: move |_| cats_open.set(false), Icon { d: CLOSE } }
                    }
                    div { class: "sheet-body", {cat_list()} }
                }
            }
        }
    });

    let tab = |k: Kind| if kind() == k { "on" } else { "" };
    let page_name = match kind() {
        Kind::Live => "Live TV",
        Kind::Movies => "Movies",
        Kind::Series => "Series",
    };
    let category_name = match &*cats.read() {
        Some(Ok((k, list))) if *k == kind() => category()
            .and_then(|id| list.iter().find(|c| c.category_id == id))
            .map(|c| c.category_name.clone())
            .unwrap_or_else(|| "All".to_string()),
        _ => "All".to_string(),
    };
    let sort_menu = move || -> Element {
        rsx! {
            div { class: "sortwrap",
                button {
                    class: "sort-btn",
                    title: "Sort",
                    aria_label: "Sort",
                    aria_expanded: sort_open(),
                    onclick: move |_| sort_open.set(!sort_open()),
                    Icon { d: SORT }
                    span { "{sort().label()}" }
                }
                if sort_open() {
                    button { class: "scrim", aria_label: "Close menu", onclick: move |_| sort_open.set(false) }
                    div { class: "menu sort-menu",
                        for option in Sort::options(kind()).iter().copied() {
                            button {
                                key: "{option.label()}",
                                class: if option == sort() { "on" } else { "" },
                                onclick: move |_| {
                                    sort.set(option);
                                    preferences::update(|p| p.sorts[kind() as usize] = option.saved());
                                    page.set(0);
                                    sort_open.set(false);
                                },
                                span { "{option.label()}" }
                                if option == sort() { Icon { d: CHECK } }
                            }
                        }
                    }
                }
            }
        }
    };
    // Shown only if the user gave the account a name; otherwise just the avatar, so the server
    // isn't on screen.
    let account_name = playlist();
    let initial = account_name.chars().next();
    rsx! {
        header { class: if open().is_some() { "topbar detail-mode" } else { "topbar" },
            if open().is_none() {
                label { class: "search",
                    Icon { d: SEARCH }
                    input {
                        aria_label: "Search this section",
                        placeholder: "Search {page_name}",
                        value: "{search}",
                        oninput: move |e| { search.set(e.value()); page.set(0); }
                    }
                }
            }
            span { class: "grow" }
            div { class: "account",
                button {
                    class: if initial.is_some() { "who" } else { "who solo" },
                    aria_label: "Account",
                    aria_expanded: account_open(),
                    onclick: move |_| account_open.set(!account_open()),
                    i {
                        if let Some(letter) = initial { "{letter}" } else { Icon { d: USER } }
                    }
                    if initial.is_some() { b { "{account_name}" } }
                }
                if account_open() {
                    button {
                        class: "scrim",
                        aria_label: "Close menu",
                        onclick: move |_| account_open.set(false)
                    }
                    div { class: "menu",
                        button {
                            onclick: move |_| {
                                refresh += 1;
                                player_revision += 1;
                                page.set(0);
                                account_open.set(false);
                            },
                            "Refresh"
                        }
                        button {
                            class: "setting",
                            role: "switch",
                            aria_checked: rust_sound(),
                            title: "Experimental Rust playback engine. Applies to the next stream you open.",
                            onclick: move |_| {
                                let on = !rust_sound();
                                rust_sound.set(on);
                                if let Some(s) = storage() {
                                    let _ = s.set_item(RUST_SOUND, if on { "1" } else { "0" });
                                }
                            },
                            "Experimental player"
                            i { class: "switch" }
                        }
                        button { onclick: move |_| session.set(None), "Switch profile" }
                    }
                }
            }
        }
        nav { class: "rail", aria_label: "Library",
            button { class: tab(Kind::Live), title: "Live TV", onclick: move |_| pick_kind(Kind::Live), Icon { d: LIVE_TV } span { "Live TV" } }
            if !client.is_playlist() {
                button { class: tab(Kind::Movies), title: "Movies", onclick: move |_| pick_kind(Kind::Movies), Icon { d: MOVIE } span { "Movies" } }
                button { class: tab(Kind::Series), title: "Series", onclick: move |_| pick_kind(Kind::Series), Icon { d: SERIES } span { "Series" } }
            }
        }
        main { class: if open().is_some() { "workspace detail-open" } else { "workspace" },
            aside { class: "sidebar", {cat_list()} }
            div { class: "content",
                if three_pane {
                    div { class: if live().is_some() { "live watching" } else { "live" },
                        section { class: "channels",
                            div { class: "head",
                                h1 { "{category_name}" }
                                div { class: "tools",
                                    {sort_menu()}
                                    button { class: "cats-btn", onclick: move |_| cats_open.set(true), Icon { d: LIST } "Categories" }
                                }
                            }
                            {body}
                        }
                        section { class: if live().is_some() { "stage" } else { "stage idle" },
                            if live().is_some() {
                                {live_panel}
                            } else {
                                div { class: "stage-empty",
                                    Icon { d: LIVE_TV }
                                    "Please select a channel to start playback"
                                }
                            }
                        }
                    }
                } else {
                    section { class: if open().is_some() { "main covered" } else { "main" },
                        div { class: "head",
                            h1 { "{category_name}" }
                            div { class: "tools",
                                {sort_menu()}
                                button { class: "cats-btn", onclick: move |_| cats_open.set(true), Icon { d: LIST } "Categories" }
                            }
                        }
                        {body}
                    }
                }
            }
            {cats_sheet}
        }
        if let Some(row) = open() {
            DetailPage {
                key: "{row.key}",
                row,
                onback: move |_| open.set(None),
                onplay: move |(play, rest): (Play, Vec<Play>)| {
                    playing.set(Some(play));
                    queue.set(rest);
                },
            }
        }
        {player}
    }
}

/// The page numbers to offer: the first, the last and a few around this one, with `None` where
/// pages are left out (a gap of one page is just shown).
fn page_window(page: usize, pages: usize) -> Vec<Option<usize>> {
    let mut shown: Vec<usize> = [0, pages - 1]
        .into_iter()
        .chain(page.saturating_sub(2)..=(page + 2).min(pages - 1))
        .collect();
    shown.sort_unstable();
    shown.dedup();
    let mut out = vec![];
    for (i, &p) in shown.iter().enumerate() {
        if i > 0 {
            match p - shown[i - 1] {
                1 => {}
                2 => out.push(Some(shown[i - 1] + 1)),
                _ => out.push(None),
            }
        }
        out.push(Some(p));
    }
    out
}

/// Which page of a long list this is, and a way to turn it. Nothing for a list of one page.
#[component]
fn Pager(
    page: usize,
    pages: usize,
    total: usize,
    size: usize,
    onpage: EventHandler<usize>,
) -> Element {
    if pages <= 1 {
        return rsx! {};
    }
    let (first, last) = (page * size + 1, ((page + 1) * size).min(total));
    rsx! {
        nav { class: "pager", aria_label: "Pages",
            span { class: "range", "{thousands(first)}–{thousands(last)} of {thousands(total)}" }
            div { class: "pages",
                button { class: "step", disabled: page == 0, aria_label: "Previous page", title: "Previous page",
                    onclick: move |_| onpage.call(page.saturating_sub(1)), Icon { d: BACK } }
                for slot in page_window(page, pages) {
                    if let Some(n) = slot {
                        button {
                            key: "{n}",
                            class: if n == page { "on" } else { "" },
                            aria_current: if n == page { "page" } else { "false" },
                            onclick: move |_| onpage.call(n),
                            "{thousands(n + 1)}"
                        }
                    } else {
                        span { class: "gap", "…" }
                    }
                }
                span { class: "of", "of {thousands(pages)}" }
                button { class: "step", disabled: page + 1 >= pages, aria_label: "Next page", title: "Next page",
                    onclick: move |_| onpage.call(page + 1), Icon { d: NEXT_PAGE } }
            }
        }
    }
}

#[component]
fn Download(title: String, url: String) -> Element {
    rsx! {
        a {
            class: "icon-btn",
            href: "{url}",
            download: "{file_name(&title, &url)}",
            title: "Download",
            aria_label: "Download",
            Icon { d: DOWNLOAD }
        }
    }
}

#[component]
fn ChannelRow(row: Row, active: bool, onpick: EventHandler<Row>) -> Element {
    let r = row.clone();
    rsx! {
        button {
            class: if active { "row on" } else { "row" },
            title: "{row.title}",
            onclick: move |_| onpick.call(r.clone()),
            span { class: "logo",
                span { class: "ph", "TV" }
                if let Some(src) = row.icon.as_deref().filter(|s| s.starts_with("http")) {
                    img { src: "{src}", loading: "lazy", decoding: "async" }
                }
            }
            div {
                strong { "{row.title}" }
                if active { small { "Now playing" } }
            }
        }
    }
}

/// A poster (movies, series) or a logo tile (channels), depending on the grid it sits in.
#[component]
fn Card(row: Row, onpick: EventHandler<Row>) -> Element {
    let r = row.clone();
    let fallback = match &row.target {
        Target::Series(_) => "S",
        Target::Live { .. } => "TV",
        Target::Movie { .. } => "M",
    };
    // How far the viewer got, if they did (a movie; a series is watched a episode at a time).
    let progress = match &row.target {
        Target::Movie { id, .. } => shelves::percent(&format!("movie:{id}")),
        _ => None,
    };
    rsx! {
        button {
            class: "card",
            title: "{row.title}",
            onclick: move |_| onpick.call(r.clone()),
            span { class: "art",
                span { class: "ph", "{fallback}" }
                if let Some(src) = row.icon.as_deref().filter(|s| s.starts_with("http")) {
                    img { src: "{xtream::sized_art(src, xtream::Art::Thumb)}", loading: "lazy", decoding: "async" }
                }
                if let Some(s) = &row.score { span { class: "score", "{s}" } }
                if let Some(p) = progress { span { class: "prog", i { style: "width:{p}%" } } }
            }
            strong { "{row.title}" }
        }
    }
}

/// A row of titles the viewer has put aside ("Continue watching"), as a strip that scrolls sideways.
#[component]
fn Shelf(
    title: &'static str,
    entries: Vec<shelves::Entry>,
    onpick: EventHandler<shelves::Entry>,
) -> Element {
    rsx! {
        section { class: "shelf",
            h2 { "{title}" }
            div { class: "strip",
                for e in entries {
                    button {
                        key: "{e.id}",
                        class: "card",
                        title: "{e.title}",
                        onclick: {
                            let e = e.clone();
                            move |_| onpick.call(e.clone())
                        },
                        span { class: "art",
                            span { class: "ph", if e.kind == shelves::Title::Series { "S" } else { "M" } }
                            if let Some(src) = e.icon.as_deref().filter(|s| s.starts_with("http")) {
                                img { src: "{xtream::sized_art(src, xtream::Art::Thumb)}", loading: "lazy", decoding: "async" }
                            }
                            if let Some(p) = e.percent() { span { class: "prog", i { style: "width:{p}%" } } }
                        }
                        strong { "{e.title}" }
                        if let Some(sub) = &e.sub { small { "{sub}" } }
                    }
                }
            }
        }
    }
}

/// Something to watch: a movie or an episode.
#[derive(Clone, PartialEq)]
struct Play {
    /// Remembers where the viewer got to (`movie:12`, `episode:340`).
    key: String,
    title: String,
    /// "S1 E3 · Title" for an episode.
    subtitle: Option<String>,
    /// What a download is called.
    file: String,
    url: String,
    /// The title as a shelf remembers it (a series, for an episode).
    entry: Option<shelves::Entry>,
}

/// What a key press asks the player's page to do, from outside Dioxus (see `Watch`).
#[derive(Clone, Copy, PartialEq)]
enum Act {
    Close,
    Next,
}

const WATCH_MEDIA_ACTIONS: &[(&str, media_session::Action)] = &[
    ("play", media_session::Action::Play),
    ("pause", media_session::Action::Pause),
    ("seekbackward", media_session::Action::Back),
    ("seekforward", media_session::Action::Forward),
    ("previoustrack", media_session::Action::Previous),
    ("stop", media_session::Action::Stop),
];
const WATCH_MEDIA_ACTIONS_NEXT: &[(&str, media_session::Action)] = &[
    ("play", media_session::Action::Play),
    ("pause", media_session::Action::Pause),
    ("seekbackward", media_session::Action::Back),
    ("seekforward", media_session::Action::Forward),
    ("previoustrack", media_session::Action::Previous),
    ("nexttrack", media_session::Action::Next),
    ("stop", media_session::Action::Stop),
];

const LIVE_MEDIA_ACTIONS: &[(&str, media_session::Action)] = &[
    ("play", media_session::Action::Play),
    ("pause", media_session::Action::Pause),
    ("stop", media_session::Action::Stop),
];

/// What plays a movie or episode. It is settled before anything plays (see [`choose`]), so sound
/// is never "fixed" while the viewer waits.
#[derive(Clone)]
enum Engine {
    /// The browser plays the file itself.
    Native,
    /// rstreamkit reads the file and decodes its sound in Rust.
    Rust(Rc<rstreamkit::vod::Movie>, xtream::Url),
    /// The proxy's ffmpeg converts it; the text says what needed that.
    Converted(xtream::Converted, String),
    Failed(String),
}

async fn choose(c: &Client, url: &str, experimental: bool) -> Engine {
    let Ok(media) = xtream::Url::parse(url) else {
        return Engine::Failed("that address is not valid".into());
    };
    let media = c.upstream(&media);
    // A file we can't read is left to the browser, which will say if it can't play it either.
    let Ok(movie) = rstreamkit::mse::probe(&Proxied(c.clone()), media.as_str()).await else {
        return Engine::Native;
    };
    match movie.verdict(&rstreamkit::mse::can_play) {
        Verdict::Native => Engine::Native,
        Verdict::Rust if experimental => Engine::Rust(movie, media),
        Verdict::Rust => convert(c, &media, "a format requiring conversion".into()).await,
        Verdict::Unsupported(why) => convert(c, &media, why.to_string()).await,
        _ => Engine::Failed("unsupported movie format".into()),
    }
}

async fn convert(c: &Client, media: &xtream::Url, why: String) -> Engine {
    match c.convert(media).await {
        Ok(converted) => Engine::Converted(converted, why),
        Err(e) => Engine::Failed(format!(
            "This has {why}, which your browser can't play, and it could not be converted: {e}"
        )),
    }
}

/// Hands a title over to the proxy's ffmpeg from `at` seconds in (a last resort after the browser
/// or the Rust player turned out not to cope).
fn switch_to_convert(
    c: Client,
    url: String,
    why: String,
    at: f64,
    mut engine: Signal<Option<Engine>>,
    mut start: Signal<u64>,
) {
    // Not Dioxus's `spawn`: callers run outside its runtime (player callbacks).
    wasm_bindgen_futures::spawn_local(async move {
        let Ok(media) = xtream::Url::parse(&url) else {
            return;
        };
        let converted = convert(&c, &media, why).await;
        // The viewer may have left while the proxy was looking at the file.
        if engine.try_peek().is_ok() {
            start.set(at as u64);
            engine.set(Some(converted));
        }
    });
}

fn watch_video() -> Option<web_sys::HtmlVideoElement> {
    web_sys::window()?
        .document()?
        .get_element_by_id("watch-video")?
        .dyn_into()
        .ok()
}

/// Seconds buffered up to, in the range the playhead is in.
fn buffered_end(v: &web_sys::HtmlVideoElement) -> f64 {
    let (b, t) = (v.buffered(), v.current_time());
    (0..b.length())
        .filter_map(|i| Some((b.start(i).ok()?, b.end(i).ok()?)))
        .find(|(s, e)| *s <= t + 0.1 && t <= *e)
        .map_or(0.0, |r| r.1)
}

fn fullscreen_watch() {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    if doc.fullscreen_element().is_some() {
        doc.exit_fullscreen();
    } else if let Some(el) = doc.get_element_by_id("watch") {
        let _ = el.request_fullscreen();
    }
}

/// The container a stream address ends in (`mkv`), if it looks like one.
fn extension(url: &str) -> Option<String> {
    url.rsplit('.')
        .next()
        .filter(|e| (1..=4).contains(&e.len()) && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(str::to_owned)
}

/// What a shelf calls this title.
fn identity(row: &Row) -> (shelves::Title, u64) {
    match &row.target {
        Target::Series(id) => (shelves::Title::Series, *id),
        Target::Movie { id, .. } | Target::Live { id, .. } => (shelves::Title::Movie, *id),
    }
}

/// A title as the shelves remember it.
fn entry_of(
    row: &Row,
    title: &str,
    icon: Option<String>,
    ext: Option<String>,
    sub: Option<String>,
) -> shelves::Entry {
    let (kind, id) = identity(row);
    shelves::Entry {
        kind,
        id,
        title: title.to_owned(),
        icon,
        ext,
        sub,
        at: 0.0,
        total: 0.0,
    }
}

/// "2h 53m" or "42m".
fn runtime_label(secs: u64) -> String {
    let (h, m) = (secs / 3600, secs / 60 % 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{}m", m.max(1))
    }
}

/// The local time, `secs` from now: "2:23 PM".
fn ends_at(secs: u64) -> String {
    let d = js_sys::Date::new(&JsValue::from_f64(
        js_sys::Date::now() + secs as f64 * 1000.0,
    ));
    let h = d.get_hours();
    format!(
        "{}:{:02} {}",
        if h.is_multiple_of(12) { 12 } else { h % 12 },
        d.get_minutes(),
        if h < 12 { "AM" } else { "PM" }
    )
}

/// "Jul 14, 2026" from "2026-07-14"; anything else as it came.
fn long_date(raw: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut parts = raw.get(..10).unwrap_or(raw).split('-');
    if let (Some(y), Some(m), Some(d)) = (parts.next(), parts.next(), parts.next())
        && y.len() == 4
        && let (Ok(m @ 1..=12), Ok(d)) = (m.parse::<usize>(), d.parse::<u32>())
    {
        return format!("{} {d}, {y}", MONTHS[m - 1]);
    }
    raw.to_owned()
}

/// The names in a cast list ("A, B / C").
fn people(list: &str) -> Vec<&str> {
    list.split([',', '/', ';'])
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .take(24)
        .collect()
}

/// The id in a YouTube address or a bare id.
fn youtube_id(text: &str) -> Option<String> {
    let ok = |s: &str| {
        s.len() == 11
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };
    let text = text.trim();
    [
        text,
        text.split("v=").nth(1).unwrap_or(""),
        text.rsplit('/').next().unwrap_or(""),
    ]
    .iter()
    .map(|s| s.split(['&', '?']).next().unwrap_or(""))
    .find(|s| ok(s))
    .map(str::to_owned)
}

/// "3. Title", or the title alone when it already says which episode it is.
fn episode_name(ep: &Episode, nth: usize) -> String {
    let num = ep.episode_num.unwrap_or(nth as u64 + 1);
    if ep.title.to_uppercase().contains(&format!("E{num:02}")) {
        ep.title.clone()
    } else {
        format!("{num}. {}", ep.title)
    }
}

/// The full-page player for a movie or episode: the picture, a bar along the top with the title,
/// and one along the bottom with the seek bar. Which engine plays it is [`choose`]'s decision.
#[component]
fn Watch(
    play: Play,
    next: Option<String>,
    onclose: EventHandler<()>,
    onnext: EventHandler<()>,
) -> Element {
    let session = use_context::<Signal<Option<Client>>>();
    let client = use_hook(|| session.read().clone().expect("logged in"));
    let experimental = use_context::<Signal<bool>>();
    let key = play.key.clone();
    let resume = use_hook(|| shelves::position(&key));
    let remembered = use_hook(preferences::load);
    let saved_volume = u32::from(remembered.volume);
    let saved_speed = remembered.speed;
    let has_next = next.is_some();

    let mut engine = use_signal(|| None::<Engine>);
    // A converted stream has no index a `<video>` could seek in, so the page restarts it: this is
    // the second of the movie where it begins.
    let mut start = use_signal(move || resume as u64);
    let mut pos = use_signal(move || resume);
    let mut total = use_signal(|| 0.0_f64);
    let mut ahead = use_signal(|| 0.0_f64);
    let mut paused = use_signal(|| false);
    let mut waiting = use_signal(|| true);
    let mut muted = use_signal(move || saved_volume == 0);
    let mut volume = use_signal(move || saved_volume);
    let mut rate = use_signal(move || saved_speed);
    let mut scrubbing = use_signal(|| false);
    let mut menu = use_signal(|| false);
    let mut resumed = use_signal(move || (resume > 0.0).then_some(resume as u64));
    let mut up_next = use_signal(|| None::<u32>);
    let active = use_signal(|| true);
    let idle = use_hook(|| IdleHide::new(active, 2800.0));
    let handle = use_hook(|| Rc::new(RefCell::new(None::<rstreamkit::mse::Player>)));
    let saved = use_hook(|| Rc::new(Cell::new(resume)));

    let watch_element = use_hook(|| Rc::new(RefCell::new(None::<web_sys::HtmlVideoElement>)));
    // With `riptv --logs`: what this title's player does, for the proxy's diagnostics log.
    let trace = use_hook(|| {
        let upstream = xtream::Url::parse(&play.url)
            .map(|u| client.upstream(&u).to_string())
            .unwrap_or_default();
        diag::Trace::start(
            &upstream,
            serde_json::json!({
                "kind": play.key.split(':').next().unwrap_or("title"),
                "title": play.title,
                "episode": play.subtitle,
                "resume_s": resume as u64,
                "experimental": *experimental.peek(),
            }),
        )
    });
    {
        let (trace, watch_element) = (trace.clone(), watch_element.clone());
        use_effect(move || {
            match engine() {
                None => return,
                Some(Engine::Native) => trace.event("engine", serde_json::json!({ "engine": "native" })),
                Some(Engine::Rust(..)) => trace.event("engine", serde_json::json!({ "engine": "rust" })),
                Some(Engine::Converted(c, why)) => trace.event(
                    "engine",
                    serde_json::json!({ "engine": "ffmpeg", "why": why, "from_s": *start.peek(), "duration_s": c.duration }),
                ),
                Some(Engine::Failed(why)) => trace.event("failure", serde_json::json!({ "text": why })),
            }
            if let Some(video) = watch_video() {
                trace.follow(&video);
                *watch_element.borrow_mut() = Some(video);
            }
        });
    }
    {
        let trace = trace.clone();
        let watch_element = watch_element.clone();
        use_drop(move || {
            trace.close();
            if let Some(video) = watch_element.borrow_mut().take() {
                release(&video);
            }
        });
    }

    // Decide what plays it, before any of it plays.
    let (c, url) = (client.clone(), play.url.clone());
    use_future(move || {
        let (c, url) = (c.clone(), url.clone());
        async move { engine.set(Some(choose(&c, &url, *experimental.peek()).await)) }
    });
    // The hint about resuming fades on its own.
    use_future(move || async move {
        rstreamkit::mse::sleep(Duration::from_secs(7)).await;
        resumed.set(None);
    });

    // The Rust engine drives a `<video>` of its own making; the others are plain attributes.
    let (c, url) = (client.clone(), play.url.clone());
    use_effect(move || {
        let Some(Engine::Rust(movie, media)) = engine() else {
            handle.borrow_mut().take();
            return;
        };
        let Some(video) = watch_video() else { return };
        let (c, url) = (c.clone(), url.clone());
        *handle.borrow_mut() = Some(rstreamkit::mse::play_movie(
            video,
            movie,
            media.to_string(),
            Proxied(c.clone()),
            *pos.peek(),
            move |s| match s {
                rstreamkit::mse::Status::Playing => waiting.set(false),
                rstreamkit::mse::Status::Unsupported(why) => switch_to_convert(
                    c.clone(),
                    url.clone(),
                    why.to_string(),
                    *pos.peek(),
                    engine,
                    start,
                ),
                rstreamkit::mse::Status::Failed(e) => engine.set(Some(Engine::Failed(e))),
                rstreamkit::mse::Status::Note(_) | rstreamkit::mse::Status::Ended => {}
                _ => {}
            },
        ));
    });

    // What the `<video>` reports, as seconds of the movie.
    let mut sync = move || {
        let Some(v) = watch_video() else { return };
        let (offset, known) = match engine.peek().as_ref() {
            Some(Engine::Converted(c, _)) => (*start.peek() as f64, c.duration.map(|d| d as f64)),
            Some(Engine::Rust(m, _)) => (0.0, Some(m.duration)),
            _ => (0.0, None),
        };
        if !*scrubbing.peek() {
            pos.set(offset + v.current_time());
        }
        let d = v.duration();
        let length = if d.is_finite() && d > 0.0 && offset == 0.0 {
            d
        } else {
            known.unwrap_or(0.0)
        };
        if length != *total.peek() {
            total.set(length);
        }
        ahead.set(offset + buffered_end(&v));
    };
    let mut seek_to = move |t: f64| {
        let length = *total.peek();
        let t = if length > 0.0 {
            t.clamp(0.0, length)
        } else {
            t.max(0.0)
        };
        pos.set(t);
        if matches!(engine.peek().as_ref(), Some(Engine::Converted(..))) {
            start.set(t as u64);
            waiting.set(true);
        } else if let Some(v) = watch_video() {
            v.set_current_time(t);
        }
    };
    let mut set_level = move |level: u32| {
        let level = level.min(100);
        volume.set(level);
        muted.set(level == 0);
        preferences::update(|p| p.volume = level as u8);
        if let Some(v) = watch_video() {
            v.set_volume(f64::from(level) / 100.0);
            v.set_muted(level == 0);
        }
    };
    let toggle_watch = move || {
        if let Some(v) = watch_video() {
            toggle(&v);
        }
    };

    // Keys, on the whole page (a plain listener: Dioxus's keyboard events add ~26 KB of wasm). The
    // listener runs outside Dioxus, where only signals may be touched: no `spawn`, no
    // `EventHandler`. So it asks, and an effect (inside Dioxus) does.
    let mut act = use_signal(|| None::<Act>);
    use_effect(move || {
        if let Some(what) = act() {
            act.set(None);
            match what {
                Act::Close => onclose.call(()),
                Act::Next => onnext.call(()),
            }
        }
    });
    let mut media_seek = use_signal(|| None::<f64>);
    use_effect(move || {
        if let Some(delta) = media_seek() {
            media_seek.set(None);
            seek_to(if delta.is_infinite() {
                0.0
            } else {
                *pos.peek() + delta
            });
        }
    });
    let media = use_hook(|| Rc::new(RefCell::new(None::<media_session::Session>)));
    let media_for_install = media.clone();
    let media_title = play.title.clone();
    let media_artist = play.subtitle.clone().unwrap_or_else(|| "RIPTV".into());
    use_effect(move || {
        let installed = media_session::Session::install(
            &media_title,
            &media_artist,
            if has_next {
                WATCH_MEDIA_ACTIONS_NEXT
            } else {
                WATCH_MEDIA_ACTIONS
            },
            move |action| match action {
                media_session::Action::Play => {
                    if let Some(v) = watch_video() {
                        let _ = v.play();
                    }
                }
                media_session::Action::Pause => {
                    if let Some(v) = watch_video() {
                        let _ = v.pause();
                    }
                }
                media_session::Action::Back => media_seek.set(Some(-15.0)),
                media_session::Action::Forward => media_seek.set(Some(15.0)),
                media_session::Action::Previous => media_seek.set(Some(f64::INFINITY)),
                media_session::Action::Next if has_next => act.set(Some(Act::Next)),
                media_session::Action::Stop => act.set(Some(Act::Close)),
                _ => {}
            },
        );
        if let (Some(session), Some(video)) = (installed.as_ref(), watch_video()) {
            session.playing(!video.paused());
        }
        *media_for_install.borrow_mut() = installed;
    });
    let media_on_play = media.clone();
    let media_on_pause = media.clone();
    let media_on_time = media.clone();
    use_drop(move || {
        media.borrow_mut().take();
    });
    let last_media_second = use_hook(|| Rc::new(Cell::new(u64::MAX)));
    let key_idle = idle.clone();
    let listener = use_hook(|| {
        Rc::new(RefCell::new(
            None::<Closure<dyn FnMut(web_sys::KeyboardEvent)>>,
        ))
    });
    let installed = listener.clone();
    use_effect(move || {
        let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let key_idle = key_idle.clone();
        let on_key =
            Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
                if e.ctrl_key() || e.meta_key() || e.alt_key() {
                    return;
                }
                let key = e.key().to_ascii_lowercase();
                // Values are read into locals first: a `peek()` guard lives to the end of its
                // statement, and these calls write the same signals.
                let (now, level) = (pos(), volume());
                match key.as_str() {
                    " " | "k" => toggle_watch(),
                    "arrowleft" | "j" => seek_to(now - 15.0),
                    "arrowright" | "l" => seek_to(now + 15.0),
                    "arrowup" => set_level((level + 10).min(100)),
                    "arrowdown" => set_level(level.saturating_sub(10)),
                    "m" => {
                        if let Some(v) = watch_video() {
                            v.set_muted(!v.muted());
                        }
                    }
                    "f" => fullscreen_watch(),
                    "n" if has_next => act.set(Some(Act::Next)),
                    "escape" => act.set(Some(Act::Close)),
                    _ => return,
                }
                e.prevent_default();
                key_idle.wake();
            });
        let _ = doc.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
        *installed.borrow_mut() = Some(on_key);
    });
    use_drop(move || {
        if let (Some(on_key), Some(doc)) = (
            listener.borrow_mut().take(),
            web_sys::window().and_then(|w| w.document()),
        ) {
            let _ =
                doc.remove_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
        }
    });

    let (engine_now, kind, src) = match engine() {
        Some(Engine::Native) => (true, "native", Some(play.url.clone())),
        Some(Engine::Rust(..)) => (true, "rust", None),
        Some(Engine::Converted(c, _)) => (true, "converted", Some(c.at(start()).to_string())),
        _ => (false, "", None),
    };
    let about = match engine() {
        Some(Engine::Native) => "Played by your browser.".to_string(),
        Some(Engine::Rust(m, _)) => format!(
            "Played in Rust{}: nothing is converted, so nothing waits.",
            m.audio
                .as_ref()
                .filter(|a| a.codec.is_empty() || a.name == "AC-3" || a.name == "E-AC-3")
                .map_or(String::new(), |a| format!(
                    ", with {} sound decoded here",
                    a.name
                ))
        ),
        Some(Engine::Converted(_, why)) => {
            format!("Converted by ffmpeg because of {why}. A seek takes a moment.")
        }
        _ => String::new(),
    };
    let (p, b) = {
        let t = total().max(1.0);
        (
            (pos() / t * 100.0).clamp(0.0, 100.0),
            (ahead() / t * 100.0).clamp(0.0, 100.0),
        )
    };
    let shown_volume = if muted() { 0 } else { volume() };
    let class = format!(
        "watch{}{}",
        if paused() { " paused" } else { "" },
        if active() { " active" } else { "" }
    );
    let (c, url, key, saved, entry) = (
        client.clone(),
        play.url.clone(),
        play.key.clone(),
        saved.clone(),
        play.entry.clone(),
    );
    let native_url = play.url.clone();
    rsx! {
        div {
            class: "{class}",
            id: "watch",
            tabindex: "0",
            onmousemove: move |_| idle.wake(),
            ondoubleclick: move |_| fullscreen_watch(),
            if !engine_now {
                if let Some(Engine::Failed(why)) = engine() {
                    div { class: "w-fail",
                        strong { "This can't be played" }
                        p { "{why}" }
                    }
                } else {
                    div { class: "w-load",
                        i { class: "spinner" }
                        strong { "{play.title}" }
                        if let Some(sub) = &play.subtitle { small { "{sub}" } }
                        span { "Starting playback…" }
                    }
                }
            } else {
                video {
                    key: "{kind}:{src.as_deref().unwrap_or_default()}",
                    id: "watch-video",
                    autoplay: true,
                    playsinline: true,
                    src: src,
                    onclick: move |_| { menu.set(false); toggle_watch(); },
                    onplay: move |_| {
                        paused.set(false);
                        if let Some(s) = media_on_play.borrow().as_ref() { s.playing(true); }
                    },
                    onpause: move |_| {
                        paused.set(true);
                        if let Some(s) = media_on_pause.borrow().as_ref() { s.playing(false); }
                    },
                    onwaiting: move |_| waiting.set(true),
                    onplaying: move |_| { waiting.set(false); paused.set(false); },
                    oncanplay: move |_| waiting.set(false),
                    onloadedmetadata: move |_| {
                        if let Some(v) = watch_video() {
                            v.set_volume(f64::from(volume()) / 100.0);
                            v.set_muted(volume() == 0);
                            v.set_playback_rate(rate());
                        }
                        // A plain file starts where the viewer left off; the other engines were told.
                        let native = matches!(engine.peek().as_ref(), Some(Engine::Native));
                        if resume > 0.0
                            && native
                            && let Some(v) = watch_video()
                        {
                            v.set_current_time(resume);
                        }
                        sync();
                    },
                    ondurationchange: move |_| sync(),
                    onprogress: move |_| sync(),
                    onratechange: move |_| {
                        if let Some(v) = watch_video() { rate.set(v.playback_rate()); }
                    },
                    onvolumechange: move |_| {
                        if let Some(v) = watch_video() {
                            muted.set(v.muted());
                            let level = (v.volume() * 100.0).round() as u32;
                            volume.set(level);
                        }
                    },
                    ontimeupdate: {
                        let (c, url, key, saved, entry) = (c.clone(), url.clone(), key.clone(), saved.clone(), entry.clone());
                        move |_| {
                            sync();
                            if let Some(s) = media_on_time.borrow().as_ref() {
                                let second = *pos.peek() as u64;
                                if last_media_second.get() != second {
                                    last_media_second.set(second);
                                    s.position(*pos.peek(), *total.peek(), *rate.peek());
                                }
                            }
                            // Some of the file's sound the browser can't decode after all: go
                            // round through the converter rather than leave the film silent.
                            let native = matches!(engine.peek().as_ref(), Some(Engine::Native));
                            if native
                                && let Some(v) = watch_video()
                                && no_audio_decoded(&v, 4.0)
                            {
                                switch_to_convert(c.clone(), url.clone(), "sound the browser can't decode".into(), *pos.peek(), engine, start);
                            }
                            if (*pos.peek() - saved.get()).abs() >= 5.0 {
                                saved.set(*pos.peek());
                                shelves::save(&key, *pos.peek(), *total.peek(), entry.as_ref());
                            }
                        }
                    },
                    onerror: {
                        let (c, url) = (c.clone(), url.clone());
                        move |_| {
                            let native = matches!(engine.peek().as_ref(), Some(Engine::Native));
                            let converted = matches!(engine.peek().as_ref(), Some(Engine::Converted(..)));
                            if native {
                                switch_to_convert(c.clone(), url.clone(), "a format the browser can't play".into(), *pos.peek(), engine, start);
                            } else if converted {
                                engine.set(Some(Engine::Failed("The converted stream stopped: the source may have ended.".into())));
                            }
                        }
                    },
                    onended: move |_| {
                        paused.set(true);
                        shelves::finish(&key, entry.as_ref());
                        if has_next {
                            up_next.set(Some(8));
                            spawn(async move {
                                while let Some(n) = up_next() {
                                    if n == 0 {
                                        onnext.call(());
                                        break;
                                    }
                                    rstreamkit::mse::sleep(Duration::from_secs(1)).await;
                                    if up_next().is_some() { up_next.set(Some(n - 1)); }
                                }
                            });
                        }
                    },
                }
                if waiting() && !paused() { div { class: "hud", i { class: "spinner" } } }
                if paused() && !waiting() && up_next().is_none() {
                    button { class: "bigplay", aria_label: "Play", onclick: move |_| toggle_watch(), Icon { d: PLAY } }
                }
            }
            div { class: "w-top",
                div { button { class: "icon-btn", aria_label: "Back", title: "Back (Esc)", onclick: move |_| onclose.call(()), Icon { d: BACK } } }
                div { class: "w-title",
                    if engine_now {
                        strong { "{play.title}" }
                        if let Some(sub) = &play.subtitle { small { "{sub}" } }
                    }
                }
                div { class: "end",
                    if engine_now { Download { title: play.file.clone(), url: native_url } }
                }
            }
            if engine_now {
                if menu() {
                    button { class: "scrim", aria_label: "Close menu", onclick: move |_| menu.set(false) }
                    div { class: "w-menu",
                        h5 { "Speed" }
                        for r in [0.5_f64, 0.75, 1.0, 1.25, 1.5, 2.0] {
                            button {
                                key: "{r}",
                                class: if rate() == r { "on" } else { "" },
                                onclick: move |_| {
                                    if let Some(v) = watch_video() { v.set_playback_rate(r); }
                                    rate.set(r);
                                    preferences::update(|p| p.speed = r);
                                },
                                span { if r == 1.0 { "Normal" } else { "{r}×" } }
                                if rate() == r { Icon { d: CHECK } }
                            }
                        }
                        h5 { "Playback" }
                        p { "{about}" }
                    }
                }
                if let Some(t) = resumed() {
                    div { class: "toast",
                        "Resumed at {hms(t)}"
                        button { onclick: move |_| { seek_to(0.0); resumed.set(None); }, "Start over" }
                    }
                }
                if let (Some(n), Some(label)) = (up_next(), next.clone()) {
                    div { class: "upnext",
                        small { "Up next in {n}" }
                        strong { "{label}" }
                        div {
                            button { onclick: move |_| up_next.set(None), "Cancel" }
                            button { class: "go", onclick: move |_| onnext.call(()), "Play now" }
                        }
                    }
                }
                div { class: "w-bottom",
                    input {
                        class: "w-seek",
                        r#type: "range",
                        min: "0",
                        max: "{total() as u64}",
                        step: "1",
                        value: "{pos() as u64}",
                        style: "--p:{p}%;--b:{b}%",
                        disabled: total() <= 0.0,
                        aria_label: "Seek",
                        oninput: move |e| {
                            scrubbing.set(true);
                            pos.set(e.value().parse().unwrap_or(0.0));
                        },
                        onchange: move |e| {
                            scrubbing.set(false);
                            seek_to(e.value().parse().unwrap_or(0.0));
                        },
                    }
                    div { class: "w-row",
                        button { class: "ctl", aria_label: "Play or pause", title: "Play or pause (Space)", onclick: move |_| toggle_watch(),
                            Icon { d: if paused() { PLAY } else { PAUSE } }
                        }
                        button { class: "ctl skip", aria_label: "Back 15 seconds", title: "Back 15 seconds (←)", onclick: move |_| seek_to(pos() - 15.0), Skip { back: true } }
                        button { class: "ctl skip", aria_label: "Forward 15 seconds", title: "Forward 15 seconds (→)", onclick: move |_| seek_to(pos() + 15.0), Skip { back: false } }
                        div { class: "volume",
                            button {
                                class: "ctl",
                                aria_label: "Mute",
                                title: "Mute (M)",
                                onclick: move |_| {
                                    if let Some(v) = watch_video() { v.set_muted(!v.muted()); }
                                },
                                Icon { d: if muted() { MUTED } else { VOLUME } }
                            }
                            input {
                                class: "vol",
                                r#type: "range",
                                min: "0",
                                max: "100",
                                aria_label: "Volume",
                                value: "{shown_volume}",
                                oninput: move |e| set_level(e.value().parse().unwrap_or(100)),
                            }
                        }
                        span { class: "w-time",
                            "{hms(pos() as u64)}"
                            if total() > 0.0 { span { " / {hms(total().round() as u64)}" } }
                        }
                        span { class: "grow" }
                        if has_next {
                            button { class: "next-btn", title: "Next episode (N)", onclick: move |_| onnext.call(()),
                                Icon { d: NEXT } "Next"
                            }
                        }
                        button { class: "ctl", aria_label: "Settings", title: "Settings", onclick: move |_| menu.set(!menu()), Icon { d: GEAR } }
                        button { class: "ctl", aria_label: "Picture in picture", title: "Picture in picture", onclick: move |_| toggle_pip(watch_video()), Icon { d: PIP } }
                        button { class: "ctl", aria_label: "Fullscreen", title: "Fullscreen (F)", onclick: move |_| fullscreen_watch(), Icon { d: FULLSCREEN } }
                    }
                }
            }
        }
    }
}

/// The page for one movie or series: backdrop, title, facts, cast, trailer, and Play or the episodes.
#[component]
fn DetailPage(
    row: Row,
    onback: EventHandler<()>,
    onplay: EventHandler<(Play, Vec<Play>)>,
) -> Element {
    let session = use_context::<Signal<Option<Client>>>();
    let client = use_hook(|| session.read().clone().expect("logged in"));
    let mut expanded = use_signal(|| false);
    let mut chosen = use_signal(|| None::<u64>);
    let mut listed = use_signal(|| {
        let (kind, id) = identity(&row);
        shelves::is_mine(kind, id)
    });
    let mut backdrop_failed = use_signal(|| false);
    let mut poster_failed = use_signal(|| false);
    let (c, target) = (client.clone(), row.target.clone());
    let info = use_resource(move || {
        let (c, target) = (c.clone(), target.clone());
        async move {
            match target {
                Target::Movie { id, .. } => c.vod_info(id).await.map(|d| (d, Vec::<Season>::new())),
                Target::Series(id) => c.series_info(id).await.map(|s| (s.details, s.seasons)),
                Target::Live { .. } => Ok((Details::default(), vec![])),
            }
        }
    });

    let guard = info.read();
    let details = match &*guard {
        Some(Ok((d, _))) => d.clone(),
        _ => Details::default(),
    };
    let title = if details.name.is_empty() {
        row.title.clone()
    } else {
        details.name.clone()
    };
    let poster = details.poster.clone().or_else(|| row.icon.clone());
    let real_backdrop = details.backdrop.clone();
    let backdrop_src = real_backdrop
        .as_deref()
        .filter(|src| src.starts_with("http") && !backdrop_failed());
    let poster_src = poster
        .as_deref()
        .filter(|src| src.starts_with("http") && !poster_failed());
    let genres: Vec<String> = details
        .genre
        .as_deref()
        .map(|g| {
            g.split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .take(3)
                .collect()
        })
        .unwrap_or_default();
    let quality = details
        .video
        .as_deref()
        .and_then(|v| v.split(" · ").next())
        .filter(|q| q.ends_with('p') || *q == "4K")
        .map(str::to_owned);
    let long = details
        .plot
        .as_ref()
        .is_some_and(|p| p.chars().count() > 240);
    let cast = details.cast.clone().unwrap_or_default();
    let trailer = details.trailer.as_deref().and_then(youtube_id);
    let is_movie = matches!(row.target, Target::Movie { .. });
    let for_list = entry_of(
        &row,
        &title,
        poster.clone(),
        match &row.target {
            Target::Movie { url, .. } => extension(url),
            _ => None,
        },
        None,
    );

    let mut facts: Vec<(&str, String)> = vec![];
    if let Some(secs) = details.runtime_secs {
        facts.push((
            "Runtime",
            if is_movie {
                format!("{} · Ends {}", runtime_label(secs), ends_at(secs))
            } else {
                format!("{} per episode", runtime_label(secs))
            },
        ));
    }
    if let Some(date) = &details.release_date {
        facts.push(("Release date", long_date(date)));
    }
    if let Some(country) = &details.country {
        facts.push(("Country", country.clone()));
    }
    if let Some(v) = &details.video {
        facts.push(("Video", v.clone()));
    }
    if let Some(a) = &details.audio {
        facts.push(("Audio", a.clone()));
    }

    // Everything there is to watch, in order: a movie is one, a series is its episodes.
    let mut all: Vec<Play> = vec![];
    let mut spans: Vec<(u64, std::ops::Range<usize>)> = vec![];
    match (&row.target, &*guard) {
        (Target::Movie { id, url }, _) => all.push(Play {
            key: format!("movie:{id}"),
            title: title.clone(),
            subtitle: None,
            file: title.clone(),
            url: url.clone(),
            entry: Some(entry_of(&row, &title, poster.clone(), extension(url), None)),
        }),
        (Target::Series(_), Some(Ok((_, seasons)))) => {
            for season in seasons {
                let from = all.len();
                for (n, ep) in season.episodes.iter().enumerate() {
                    let num = ep.episode_num.unwrap_or(n as u64 + 1);
                    let ext = ep
                        .container_extension
                        .as_deref()
                        .filter(|e| !e.is_empty())
                        .unwrap_or("mp4");
                    all.push(Play {
                        key: format!("episode:{}", ep.id),
                        title: title.clone(),
                        subtitle: Some(format!("S{} E{num} · {}", season.number, ep.title)),
                        file: format!("{title} S{}E{num}: {}", season.number, ep.title),
                        url: client.episode_url(ep.id, ext).to_string(),
                        entry: Some(entry_of(
                            &row,
                            &title,
                            poster.clone(),
                            None,
                            Some(format!("S{} E{num}", season.number)),
                        )),
                    });
                }
                spans.push((season.number, from..all.len()));
            }
        }
        _ => {}
    }
    let all = Rc::new(all);
    let play_at = {
        let all = all.clone();
        move |i: usize| onplay.call((all[i].clone(), all[i + 1..].to_vec()))
    };
    // The main button plays the episode last left half watched (or the movie), else the first.
    let (start_at, resume) = all
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, p)| shelves::progress(&p.key).map(|r| (i, Some(r))))
        .unwrap_or((0, None));
    let episode_now = all
        .get(start_at)
        .and_then(|p| p.subtitle.as_deref())
        .and_then(|s| s.split(" · ").next())
        .map(str::to_owned);
    let season_now = chosen().or_else(|| spans.first().map(|s| s.0));

    rsx! {
        section { class: "detail",
            div { class: "detail-return",
                button { class: "icon-btn", aria_label: "Back", onclick: move |_| onback.call(()), Icon { d: BACK } }
            }
            div { class: if backdrop_src.is_none() && poster_src.is_none() { "d-hero no-art" } else { "d-hero" },
                if let Some(src) = backdrop_src {
                    img { class: "d-art", src: "{xtream::sized_art(src, xtream::Art::Backdrop)}", decoding: "async", fetchpriority: "high", onerror: move |_| backdrop_failed.set(true) }
                } else if let Some(src) = poster_src {
                    img { class: "d-poster", src: "{xtream::sized_art(src, xtream::Art::Poster)}", decoding: "async", fetchpriority: "high", onerror: move |_| poster_failed.set(true) }
                }
                div { class: "d-shade" }
                div { class: "d-main",
                    h1 { class: "d-title", "{title}" }
                    if !genres.is_empty() {
                        div { class: "d-genres", for g in genres.iter() { span { key: "{g}", "{g}" } } }
                    }
                    div { class: "d-actions",
                        if !all.is_empty() {
                            button { class: "play", onclick: { let play_at = play_at.clone(); move |_| play_at(start_at) },
                                Icon { d: PLAY }
                                if let Some((at, total)) = resume {
                                    "Resume"
                                    if let Some(ep) = &episode_now { " {ep}" }
                                    if total > at { " · {runtime_label((total - at) as u64)} left" }
                                } else { "Play" }
                            }
                        }
                        button {
                            class: if listed() { "icon-btn on" } else { "icon-btn" },
                            title: if listed() { "Remove from My List" } else { "Add to My List" },
                            aria_label: "My List",
                            aria_pressed: "{listed()}",
                            onclick: move |_| listed.set(shelves::toggle_mine(for_list.clone())),
                            Icon { d: HEART }
                        }
                        if let Some(id) = &trailer {
                            a { class: "icon-btn", href: "https://www.youtube.com/watch?v={id}", target: "_blank", rel: "noopener noreferrer", title: "Trailer on YouTube", aria_label: "Trailer on YouTube", Icon { d: EXTERNAL } }
                        }
                        if is_movie && let Some(p) = all.first() { Download { title: p.file.clone(), url: p.url.clone() } }
                    }
                    div { class: "d-meta",
                        if let Some(y) = &details.year { span { "{y}" } }
                        if let Some(secs) = details.runtime_secs { span { "{runtime_label(secs)}" } }
                        if let Some(age) = &details.age { span { class: "badge", "{age}" } }
                        if let Some(q) = &quality { span { class: "badge", "{q}" } }
                        if let Some(r) = score(&details.rating) { span { span { class: "star", "★ " } "{r}" } }
                    }
                    if let Some(director) = &details.director { p { class: "d-by", "Director: " b { "{director}" } } }
                    if let Some(plot) = &details.plot {
                        p { class: if expanded() || !long { "plot" } else { "plot clamp" }, "{plot}" }
                        if long {
                            button { class: "more", onclick: move |_| expanded.set(!expanded()), if expanded() { "Show less" } else { "Read more" } }
                        }
                    }
                    match &*guard {
                        None => rsx! { p { class: "dim", "Loading…" } },
                        Some(Err(e)) => rsx! { p { class: "err", "{e}" } },
                        Some(Ok(_)) => rsx! {},
                    }
                }
                if !facts.is_empty() {
                    dl { class: "d-facts",
                        for (name, value) in facts { div { key: "{name}", dt { "{name}" } dd { "{value}" } } }
                    }
                }
            }
            if !spans.is_empty() {
                div { class: "d-sec",
                    div { class: "ep-heading",
                        h2 { "Episodes" }
                        div { class: "ep-tools",
                            span { "{spans.iter().find(|s| Some(s.0) == season_now).map_or(0, |s| s.1.len())} episodes" }
                            if spans.len() > 1 {
                                select {
                                    class: "season-select",
                                    aria_label: "Season",
                                    value: "{season_now.unwrap_or_default()}",
                                    onchange: move |e| {
                                        if let Ok(n) = e.value().parse::<u64>() { chosen.set(Some(n)); }
                                    },
                                    for (number, _) in spans.iter() {
                                        option { key: "{number}", value: "{number}", "Season {number}" }
                                    }
                                }
                            }
                        }
                    }
                    div { class: "eps",
                        if let (Some((_, range)), Some(Ok((_, seasons)))) = (spans.iter().find(|s| Some(s.0) == season_now), &*guard) {
                            for (i, ep) in seasons.iter().flat_map(|s| s.episodes.iter()).enumerate().skip(range.start).take(range.len()) {
                                div { class: "epc", key: "{ep.id}",
                                    button { class: "thumb", aria_label: "Play", onclick: { let play_at = play_at.clone(); move |_| play_at(i) },
                                        span { class: "ph", "E{ep.episode_num.unwrap_or(0)}" }
                                        if let Some(img) = ep.info.image.as_deref().filter(|s| s.starts_with("http")) {
                                            img { src: "{xtream::sized_art(img, xtream::Art::Still)}", loading: "lazy", decoding: "async" }
                                        }
                                        if let Some(p) = shelves::percent(&all[i].key) { span { class: "prog", i { style: "width:{p}%" } } }
                                        span { class: "ep-num", "E{ep.episode_num.unwrap_or((i - range.start + 1) as u64)}" }
                                        span { class: "go", Icon { d: PLAY } }
                                        if let Some(secs) = ep.info.runtime_secs { span { class: "len", "{runtime_label(secs)}" } }
                                    }
                                    div { class: "epc-head",
                                        div {
                                            h4 { "{episode_name(ep, i - range.start)}" }
                                            if let Some(date) = &ep.info.release_date { small { "{long_date(date)}" } }
                                        }
                                        Download { title: all[i].file.clone(), url: all[i].url.clone() }
                                    }
                                    if let Some(plot) = &ep.info.plot { p { "{plot}" } }
                                }
                            }
                        }
                    }
                }
            }
            if !cast.is_empty() {
                div { class: "d-sec",
                    h2 { "Cast" }
                    div { class: "cast",
                        for name in people(&cast) {
                            div { class: "person", key: "{name}",
                                strong { "{name}" }
                            }
                        }
                    }
                }
            }
            if let Some(id) = &trailer {
                div { class: "d-sec",
                    h2 { "Trailer" }
                    a { class: "trailer", href: "https://www.youtube.com/watch?v={id}", target: "_blank", rel: "noopener noreferrer",
                        img { src: "https://img.youtube.com/vi/{id}/hqdefault.jpg", loading: "lazy" }
                        span { "Watch the trailer" }
                    }
                }
            }
            div { class: "d-sec" }
        }
    }
}

/// A live stream through the Rust HLS player, with its own controls and the channel's guide. The
/// player stops when this component goes away (its handle is dropped), so picking another channel
/// ends the download loop.
/// What is driving a live channel's `<video>`: the Rust player, the Rust player with the sound
/// given up on (when the proxy can't convert it), or the proxy's ffmpeg-converted stream.
#[derive(Clone, PartialEq)]
enum Feed {
    Pending,
    Direct(String),
    Rust,
    Partial,
    Converted(String),
}

#[component]
fn LivePlayer(
    id: u64,
    title: String,
    url: String,
    onchannel: EventHandler<ChannelAction>,
) -> Element {
    let session = use_context::<Signal<Option<Client>>>();
    let rust_sound = use_context::<Signal<bool>>();
    let client = use_hook(|| session.read().clone().expect("logged in"));
    let media = use_hook(|| Rc::new(RefCell::new(None::<media_session::Session>)));
    let media_for_install = media.clone();
    let media_title = title.clone();
    use_effect(move || {
        let installed = media_session::Session::install(
            &media_title,
            "Live TV · RIPTV",
            LIVE_MEDIA_ACTIONS,
            move |action| match action {
                media_session::Action::Play => {
                    if let Some(v) = video_el() {
                        let _ = v.play();
                    }
                }
                media_session::Action::Pause | media_session::Action::Stop => {
                    if let Some(v) = video_el() {
                        let _ = v.pause();
                    }
                }
                _ => {}
            },
        );
        if let (Some(session), Some(video)) = (installed.as_ref(), video_el()) {
            session.playing(!video.paused());
        }
        *media_for_install.borrow_mut() = installed;
    });
    let media_on_play = media.clone();
    let media_on_pause = media.clone();
    use_drop(move || {
        media.borrow_mut().take();
    });
    let saved_volume = u32::from(use_hook(preferences::load).volume);
    let mut status = use_signal(|| "Starting playback…".to_string());
    let mut picture_ready = use_signal(|| false);
    let mut audio_only = use_signal(|| false);
    let mut paused = use_signal(|| false);
    let mut muted = use_signal(move || saved_volume == 0);
    let mut volume = use_signal(move || saved_volume);
    let mut buffering = use_signal(|| false);
    let mut expanded = use_signal(|| false);
    // Why there is no sound, when the player knows: its own note, or what the browser reports.
    let mut note = use_signal(|| None::<String>);
    let mut silent = use_signal(|| false);
    // The controls fade out after a moment of no mouse movement while playing.
    let active = use_signal(|| true);
    let idle = use_hook(|| IdleHide::new(active, 2500.0));
    let handle = use_hook(|| Rc::new(RefCell::new(None::<rstreamkit::mse::Player>)));
    // With `riptv --logs`: what this channel's player does, for the proxy's diagnostics log.
    let trace = use_hook(|| {
        let upstream = xtream::Url::parse(&url)
            .map(|u| client.upstream(&u).to_string())
            .unwrap_or_default();
        diag::Trace::start(
            &upstream,
            serde_json::json!({
                "kind": "live",
                "channel": id,
                "title": title,
                "experimental": *rust_sound.peek(),
            }),
        )
    });

    let mut feed = use_signal(|| {
        if *rust_sound.peek() {
            Feed::Rust
        } else {
            // A channel seen before starts the way that worked; a new one is sniffed first.
            match standard::remembered(id) {
                Some(standard::Plan::Browser) => {
                    native_hls_source(&client, &url).map_or(Feed::Pending, Feed::Direct)
                }
                Some(plan) => known_conversion(&client, &url, plan),
                None => Feed::Pending,
            }
        }
    });
    // Standard mode decides here, once, before any connection to the channel: what the stream is
    // (container, codecs), then the browser plays it, or ffmpeg converts its sound or its video.
    // Never alongside playback: providers that allow one connection per channel end the first
    // when a second asks for the same stream. The experimental player skips this entirely.
    let mut sniffed = use_signal(|| false);
    let mut native_failed = use_signal(|| false);
    {
        let (client, url, trace) = (client.clone(), url.clone(), trace.clone());
        let _decide = use_resource(move || {
            let pending = matches!(feed(), Feed::Pending);
            let (client, url, trace) = (client.clone(), url.clone(), trace.clone());
            async move {
                if !pending {
                    return;
                }
                let native = (!*native_failed.peek())
                    .then(|| native_hls_source(&client, &url))
                    .flatten();
                if !*rust_sound.peek() && !*sniffed.peek() {
                    let found = standard::sniff(&client, &url).await;
                    if sniffed.try_peek().is_err() {
                        return; // the viewer left
                    }
                    sniffed.set(true);
                    let found = found.filter(|f| f.container == "mpeg_ts");
                    let plan = found.map(|f| standard::plan(&f, standard::browser_decodes));
                    trace.event(
                        "stream",
                        serde_json::json!({
                            "container": found.map_or("not_mpeg_ts", |f| f.container),
                            "video": found.and_then(|f| f.video),
                            "audio": found.and_then(|f| f.audio),
                            "plan": plan.map(standard::Plan::name),
                        }),
                    );
                    if let Some(plan) = plan {
                        standard::remember(id, plan);
                    }
                    match (plan, native) {
                        (Some(standard::Plan::Browser) | None, Some(src)) => {
                            feed.set(Feed::Direct(src));
                            return;
                        }
                        (Some(plan), _) => {
                            if let Some(src) = standard::conversion(&client, &url, plan) {
                                feed.set(Feed::Converted(src));
                                return;
                            }
                        }
                        (None, None) => {}
                    }
                }
                // Unknown, or the browser already failed with it: the proxy checks and converts.
                let Ok(media) = xtream::Url::parse(&url) else {
                    status.set("This channel's address is invalid".into());
                    return;
                };
                match client.convert(&media).await {
                    Ok(converted) => {
                        let src = converted.at(0).to_string();
                        // What worked, for next time (copy is the sound-only conversion).
                        if !*rust_sound.peek() {
                            standard::remember(
                                id,
                                if src.contains("video=transcode") {
                                    standard::Plan::ConvertVideo
                                } else {
                                    standard::Plan::ConvertSound
                                },
                            );
                        }
                        feed.set(Feed::Converted(src));
                    }
                    Err(e) => status.set(channel_trouble(&e)),
                }
            }
        });
    }
    // A readout for telling a slow stream from a slow decoder: what the picture really is,
    // frames per second actually shown, frames dropped, and how much is buffered.
    let mut show_stats = use_signal(|| false);
    let mut stats = use_signal(String::new);
    {
        let trace = trace.clone();
        use_effect(move || {
            let engine = match feed() {
                Feed::Pending => "starting_ffmpeg",
                Feed::Direct(_) => "native_hls",
                Feed::Rust => "rust",
                Feed::Partial => "rust_no_sound",
                Feed::Converted(_) => "ffmpeg",
            };
            trace.event("engine", serde_json::json!({ "engine": engine }));
        });
    }
    {
        let trace = trace.clone();
        use_effect(move || {
            let text = status();
            let name = if diag::is_failure(&text) {
                "failure"
            } else {
                "status"
            };
            trace.event(name, serde_json::json!({ "text": text }));
        });
    }
    {
        let trace = trace.clone();
        use_effect(move || {
            if let Some(why) = note() {
                trace.event("no_sound", serde_json::json!({ "text": why }));
            } else if silent() {
                trace.event(
                    "no_sound",
                    serde_json::json!({ "text": "browser decoded no audio" }),
                );
            }
        });
    }
    {
        let trace = trace.clone();
        use_drop(move || trace.close());
    }
    let mut channel_action = use_signal(|| None::<ChannelAction>);
    let mut dial = use_signal(String::new);
    let digits = use_hook(|| Rc::new(RefCell::new(String::new())));
    let dial_epoch = use_hook(|| Rc::new(Cell::new(0_u64)));
    use_effect(move || {
        if let Some(action) = channel_action() {
            channel_action.set(None);
            onchannel.call(action);
        }
    });
    // A resource, not a future: it starts again when `show_stats` changes (a future runs once),
    // so nothing polls while the readout is off.
    let _stats_task = use_resource(move || async move {
        if !show_stats() {
            return;
        }
        let mut last = (0_u64, js_sys::Date::now());
        let mut frame_counter = None;
        let mut frame_counter_checked = false;
        loop {
            rstreamkit::mse::sleep(Duration::from_secs(1)).await;
            let Some(v) = video_el() else {
                continue;
            };
            if !frame_counter_checked {
                frame_counter = frame_stats::Counter::start(&v);
                frame_counter_checked = true;
                last = (0, js_sys::Date::now());
            }
            let quality = v.get_video_playback_quality();
            let (frames, now) = (
                frame_counter.as_ref().map_or(
                    u64::from(quality.total_video_frames()),
                    frame_stats::Counter::presented,
                ),
                js_sys::Date::now(),
            );
            let fps = (frames.saturating_sub(last.0) as f64 * 1000.0 / (now - last.1).max(1.0))
                .round() as u32;
            last = (frames, now);
            let buffered = v.buffered();
            let ahead = buffered
                .length()
                .checked_sub(1)
                .and_then(|i| buffered.end(i).ok())
                .map_or(0.0, |end| (end - v.current_time()).max(0.0))
                as u32;
            let source = match feed() {
                Feed::Pending => "starting compatibility mode",
                Feed::Direct(_) => "native HLS",
                Feed::Rust => "Rust player",
                Feed::Partial => "Rust player, no sound",
                Feed::Converted(_) => "converted by ffmpeg",
            };
            stats.set(format!(
                "{}×{} · {fps} fps · {} dropped · {ahead}s buffered · {source}",
                v.video_width(),
                v.video_height(),
                quality.dropped_video_frames()
            ));
        }
    });
    let watchdog_client = client.clone();
    let watchdog_url = url.clone();
    let player_video = use_hook(|| Rc::new(RefCell::new(None::<web_sys::HtmlVideoElement>)));
    {
        let player_video = player_video.clone();
        use_drop(move || {
            if let Some(video) = player_video.borrow_mut().take() {
                release(&video);
            }
        });
    }
    use_future(move || async move {
        rstreamkit::mse::sleep(Duration::from_secs(12)).await;
        if status.try_peek().is_ok_and(|s| *s == "Starting playback…") {
            status.set(STILL_STARTING.into());
        }
    });
    let (sound_client, sound_url) = (client.clone(), url.clone());
    let trace_for_player = trace.clone();
    use_effect(move || {
        let Some(video) = video_el() else {
            status.set("Could not start the player".into());
            return;
        };
        *player_video.borrow_mut() = Some(video.clone());
        trace_for_player.follow(&video);
        video.set_volume(f64::from(*volume.peek()) / 100.0);
        video.set_muted(*muted.peek());
        let partial = match feed() {
            Feed::Pending => return,
            Feed::Direct(src) => {
                handle.borrow_mut().take();
                video.set_src(&src);
                let _ = video.play();
                return;
            }
            Feed::Converted(src) => {
                // Stop the Rust player and let the plain `<video>` play the converted stream.
                handle.borrow_mut().take();
                note.set(None);
                video.set_src(&src);
                let _ = video.play();
                return;
            }
            Feed::Partial => true,
            Feed::Rust => false,
        };
        let Ok(playlist) = xtream::Url::parse(&url) else {
            status.set("Could not start the player".into());
            return;
        };
        let (c, converter, media) = (client.clone(), client.clone(), playlist.clone());
        let upstream = client.upstream(&playlist);
        let unsupported_trace = trace_for_player.clone();
        *handle.borrow_mut() = Some(rstreamkit::mse::start(
            video,
            upstream.to_string(),
            Proxied(c),
            partial,
            // `peek`: changing the setting must not restart a channel that is playing.
            *rust_sound.peek(),
            move |s| match s {
                rstreamkit::mse::Status::Playing => {
                    status.set(
                        if *picture_ready.peek() {
                            "Live"
                        } else {
                            "Waiting for picture…"
                        }
                        .into(),
                    );
                }
                rstreamkit::mse::Status::Note(n) => note.set(Some(n)),
                rstreamkit::mse::Status::Unsupported(reason) => {
                    unsupported_trace.event(
                        "unsupported",
                        serde_json::json!({ "reason": reason.to_string() }),
                    );
                    // Keep one neutral loading state across the Rust → ffmpeg handoff.
                    status.set("Starting playback…".into());
                    let (converter, media) = (converter.clone(), media.clone());
                    let sound_only = matches!(reason, Unsupported::Sound(_));
                    // Not Dioxus's `spawn`: this callback runs outside its runtime.
                    wasm_bindgen_futures::spawn_local(async move {
                        match converter.convert(&media).await {
                            Ok(converted) => feed.set(Feed::Converted(converted.at(0).to_string())),
                            // No ffmpeg (or it can't read the stream): a sound problem still
                            // plays, without sound, and says why; anything else can't play.
                            Err(e) if sound_only => {
                                note.set(Some(format!("{reason} ({e})")));
                                feed.set(Feed::Partial);
                            }
                            Err(e) => status.set(channel_trouble(&e)),
                        }
                    });
                }
                rstreamkit::mse::Status::Ended => status.set("Stream ended".into()),
                rstreamkit::mse::Status::Failed(e) => status.set(format!("Playback failed: {e}")),
                _ => {}
            },
        ));
    });

    // Some providers append data successfully but never deliver a decodable picture. In that
    // case MSE can say "Playing" while the screen stays black. Watch only until the first frame;
    // each fallback gets one chance, so a broken upstream does not loop forever.
    use_future(move || {
        let client = watchdog_client.clone();
        let url = watchdog_url.clone();
        async move {
            let mut mode = None::<Feed>;
            let mut since = js_sys::Date::now();
            let mut forced_video = false;
            loop {
                rstreamkit::mse::sleep(Duration::from_secs(2)).await;
                let current = feed.peek().clone();
                if mode.as_ref() != Some(&current) {
                    mode = Some(current.clone());
                    since = js_sys::Date::now();
                }
                // `ontimeupdate` notices the first decoded frame.
                if *picture_ready.peek() || *audio_only.peek() {
                    break;
                }
                let Some(video) = video_el() else { continue };
                if video.paused()
                    || web_sys::window()
                        .and_then(|w| w.document())
                        .is_some_and(|d| d.hidden())
                {
                    since = js_sys::Date::now();
                    continue;
                }
                // Time moving with no frame decoded means the video can't be decoded here: act
                // soon. Nothing moving at all is a slow provider, which another method from the
                // same provider won't fix quickly: give it longer.
                let wait_ms = if video.current_time() > 1.0 {
                    5_000.0
                } else {
                    20_000.0
                };
                if js_sys::Date::now() - since < wait_ms {
                    continue;
                }
                match current {
                    Feed::Pending => continue,
                    Feed::Direct(_) => {
                        status.set("Trying another playback method…".into());
                        native_failed.set(true);
                        feed.set(if *rust_sound.peek() {
                            Feed::Rust
                        } else {
                            Feed::Pending
                        });
                    }
                    Feed::Rust | Feed::Partial => {
                        status.set("Trying a compatible stream…".into());
                        let Ok(media) = xtream::Url::parse(&url) else {
                            status.set("This channel's address is invalid".into());
                            break;
                        };
                        match client.convert_video(&media).await {
                            Ok(converted) => {
                                forced_video = true;
                                feed.set(Feed::Converted(converted.at(0).to_string()));
                            }
                            Err(xtream::Error::Proxy(why)) if why.contains("no video track") => {
                                audio_only.set(true);
                                picture_ready.set(true);
                                status.set("Audio only".into());
                                video.set_muted(*muted.peek());
                                break;
                            }
                            Err(e) => {
                                status.set(format!("No picture from this channel: {e}"));
                                break;
                            }
                        }
                    }
                    Feed::Converted(_) => {
                        if forced_video {
                            status.set("No picture after video conversion. The source may be offline or sending blank frames.".into());
                            break;
                        }
                        status.set("Re-encoding the video…".into());
                        let Ok(media) = xtream::Url::parse(&url) else {
                            status.set("This channel's address is invalid".into());
                            break;
                        };
                        match client.convert_video(&media).await {
                            Ok(converted) => {
                                forced_video = true;
                                feed.set(Feed::Converted(converted.at(0).to_string()));
                            }
                            Err(xtream::Error::Proxy(why)) if why.contains("no video track") => {
                                audio_only.set(true);
                                picture_ready.set(true);
                                status.set("Audio only".into());
                                video.set_muted(*muted.peek());
                                break;
                            }
                            Err(e) => {
                                status.set(format!("No picture from this channel: {e}"));
                                break;
                            }
                        }
                    }
                }
            }
        }
    });

    // A document listener also works after a channel-row click leaves focus outside the player.
    // Ignore search fields so typing a category or title never changes channel.
    let keys = use_hook(|| {
        Rc::new(RefCell::new(
            None::<Closure<dyn FnMut(web_sys::KeyboardEvent)>>,
        ))
    });
    let keys_for_install = keys.clone();
    let digits_for_keys = digits.clone();
    let epoch_for_keys = dial_epoch.clone();
    use_effect(move || {
        let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let digits = digits_for_keys.clone();
        let epoch = epoch_for_keys.clone();
        let on_key =
            Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
                if e.alt_key() || e.ctrl_key() || e.meta_key() {
                    return;
                }
                if e.target()
                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                    .is_some_and(|el| {
                        matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT")
                    })
                {
                    return;
                }
                match e.key().to_ascii_lowercase().as_str() {
                    " " => {
                        e.prevent_default();
                        toggle_play();
                    }
                    "arrowup" => {
                        e.prevent_default();
                        digits.borrow_mut().clear();
                        dial.set(String::new());
                        epoch.set(epoch.get().wrapping_add(1));
                        channel_action.set(Some(ChannelAction::Up));
                    }
                    "arrowdown" => {
                        e.prevent_default();
                        digits.borrow_mut().clear();
                        dial.set(String::new());
                        epoch.set(epoch.get().wrapping_add(1));
                        channel_action.set(Some(ChannelAction::Down));
                    }
                    "f" => toggle_fullscreen("live-player", expanded),
                    "i" => show_stats.set(!show_stats()),
                    "escape" => {
                        digits.borrow_mut().clear();
                        dial.set(String::new());
                        epoch.set(epoch.get().wrapping_add(1));
                        expanded.set(false);
                    }
                    "m" => muted.set(toggle_mute().unwrap_or(false)),
                    "enter" => {
                        let number = digits.borrow().parse().ok();
                        digits.borrow_mut().clear();
                        dial.set(String::new());
                        epoch.set(epoch.get().wrapping_add(1));
                        if let Some(number) = number {
                            channel_action.set(Some(ChannelAction::Number(number)));
                        }
                    }
                    key if key.len() == 1 && key.as_bytes()[0].is_ascii_digit() => {
                        e.prevent_default();
                        let typed = {
                            let mut value = digits.borrow_mut();
                            if value.len() >= 5 {
                                value.clear();
                            }
                            value.push_str(key);
                            value.clone()
                        };
                        dial.set(typed);
                        let revision = epoch.get().wrapping_add(1);
                        epoch.set(revision);
                        let (digits, epoch) = (digits.clone(), epoch.clone());
                        wasm_bindgen_futures::spawn_local(async move {
                            rstreamkit::mse::sleep(Duration::from_millis(1400)).await;
                            if epoch.get() == revision {
                                let number = digits.borrow().parse().ok();
                                digits.borrow_mut().clear();
                                dial.set(String::new());
                                if let Some(number) = number {
                                    channel_action.set(Some(ChannelAction::Number(number)));
                                }
                            }
                        });
                    }
                    _ => {}
                }
            });
        let _ = doc.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
        *keys_for_install.borrow_mut() = Some(on_key);
    });
    use_drop(move || {
        dial_epoch.set(dial_epoch.get().wrapping_add(1));
        if let (Some(on_key), Some(doc)) = (
            keys.borrow_mut().take(),
            web_sys::window().and_then(|w| w.document()),
        ) {
            let _ =
                doc.remove_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
        }
    });

    let class = format!(
        "player{}{}{}",
        if paused() { " paused" } else { "" },
        if active() { " active" } else { "" },
        if expanded() { " fill" } else { "" }
    );
    rsx! {
        div { class: "live-stage",
            div {
                class: "{class}",
                id: "live-player",
                tabindex: "0",
                onmousemove: move |_| idle.wake(),
                video {
                    id: "live-video",
                    autoplay: true,
                    playsinline: true,
                    onplay: move |_| {
                        paused.set(false);
                        if let Some(s) = media_on_play.borrow().as_ref() { s.playing(true); }
                    },
                    onpause: move |_| {
                        paused.set(true);
                        if let Some(s) = media_on_pause.borrow().as_ref() { s.playing(false); }
                    },
                    onwaiting: move |_| buffering.set(true),
                    onplaying: move |_| {
                        buffering.set(false);
                        paused.set(false);
                        if matches!(feed(), Feed::Direct(_) | Feed::Converted(_)) {
                            status.set(if audio_only() { "Audio only" } else if picture_ready() { "Live" } else { "Waiting for picture…" }.into());
                        }
                    },
                    onerror: {
                        let (error_client, error_url) = (sound_client.clone(), sound_url.clone());
                        move |_| {
                            // MEDIA_ERR_DECODE: the browser's video decoder can't take this
                            // picture (interlaced broadcasts, typically). Copying it through ffmpeg
                            // would hit the same decoder: re-encode it, and remember that.
                            let decode_error = video_el()
                                .and_then(|v| js_sys::Reflect::get(&v, &"error".into()).ok())
                                .and_then(|e| js_sys::Reflect::get(&e, &"code".into()).ok())
                                .and_then(|c| c.as_f64())
                                == Some(3.0);
                            let current = feed.peek().clone();
                            let copying = matches!(&current, Feed::Converted(src) if src.contains("video=copy"));
                            if decode_error && !*rust_sound.peek() && (copying || matches!(current, Feed::Direct(_))) {
                                native_failed.set(true);
                                standard::remember(id, standard::Plan::ConvertVideo);
                                status.set("Re-encoding the video…".into());
                                feed.set(known_conversion(&error_client, &error_url, standard::Plan::ConvertVideo));
                            } else if matches!(current, Feed::Direct(_)) {
                                status.set("Starting playback…".into());
                                native_failed.set(true);
                                feed.set(if *rust_sound.peek() { Feed::Rust } else { Feed::Pending });
                            } else if matches!(current, Feed::Converted(_)) {
                                status.set("The converted stream stopped: the source may have ended".into());
                            }
                        }
                    },
                    onvolumechange: move |_| {
                        if let Some(v) = video_el() {
                            if muted() != v.muted() {
                                muted.set(v.muted());
                            }
                            let level = (v.volume() * 100.0).round() as u32;
                            if volume() != level {
                                volume.set(level);
                            }
                        }
                    },
                    ontimeupdate: {
                        let (sound_client, sound_url) = (sound_client.clone(), sound_url.clone());
                        move |_| {
                        let Some(v) = video_el() else { return };
                        // The picture is there as soon as a frame has been decoded: no polling.
                        if !*picture_ready.peek() && v.get_video_playback_quality().total_video_frames() > 0 {
                            picture_ready.set(true);
                            if matches!(status.peek().as_str(), "Starting playback…" | "Waiting for picture…" | STILL_STARTING) {
                                status.set("Live".into());
                            }
                        }
                        // A backstop: the sniff catches known sound formats up front. Chrome's
                        // decoded-byte count lags a little, so give it a few seconds.
                        let quiet = no_audio_decoded(&v, 5.0);
                        if quiet && matches!(*feed.peek(), Feed::Direct(_)) {
                            // Surround sound (AC-3, E-AC-3) the browser's own HLS player can't
                            // decode: ffmpeg converts the sound and leaves the picture alone.
                            standard::remember(id, standard::Plan::ConvertSound);
                            native_failed.set(true);
                            status.set("Trying a compatible stream…".into());
                            feed.set(known_conversion(&sound_client, &sound_url, standard::Plan::ConvertSound));
                        } else if quiet != *silent.peek() && !matches!(*feed.peek(), Feed::Pending) {
                            // Also clears the note once sound arrives (after a switch, say).
                            silent.set(quiet);
                        }
                    }},
                    onclick: move |_| toggle_play(),
                    ondoubleclick: move |_| toggle_fullscreen("live-player", expanded),
                }
                if show_stats() {
                    div { class: "stats", "{stats}" }
                }
                if audio_only() {
                    div { class: "audio-only", role: "status",
                        span { class: "audio-orb", "♫" }
                        strong { "Audio only" }
                        small { "This channel isn't sending a video track." }
                    }
                }
                if !dial().is_empty() { div { class: "channel-dial", "{dial}" } }
                if let Some(why) = note() {
                    div { class: "sound-note", "No sound: {why}" }
                } else if silent() {
                    div { class: "sound-note", "No sound: this stream's audio format can't be decoded by your browser" }
                }
                if status() != "Live" && status() != "Audio only" {
                    div { class: "hud",
                        div { class: "loading-group",
                            if status() == "Starting playback…" || status() == "Waiting for picture…" || status() == STILL_STARTING { i { class: "spinner" } }
                            span { "{status}" }
                        }
                    }
                }
                if buffering() && !paused() && status() == "Live" { div { class: "hud", i { class: "spinner" } } }
                if paused() {
                    button { class: "bigplay", aria_label: "Play", onclick: move |_| toggle_play(), Icon { d: PLAY } }
                }
                div { class: "controls",
                    button { class: "ctl", aria_label: "Play or pause", onclick: move |_| toggle_play(),
                        Icon { d: if paused() { PLAY } else { PAUSE } }
                    }
                    div { class: "volume",
                        button {
                            class: "ctl",
                            aria_label: "Mute",
                            onclick: move |_| muted.set(toggle_mute().unwrap_or(false)),
                            Icon { d: if muted() { MUTED } else { VOLUME } }
                        }
                        input {
                            class: "vol",
                            r#type: "range",
                            min: "0",
                            max: "100",
                            aria_label: "Volume",
                            value: "{volume}",
                            oninput: move |e| {
                                let level: u32 = e.value().parse().unwrap_or(100);
                                volume.set(level);
                                muted.set(level == 0);
                                preferences::update(|p| p.volume = level.min(100) as u8);
                                if let Some(v) = video_el() {
                                    v.set_volume(f64::from(level) / 100.0);
                                    v.set_muted(level == 0);
                                }
                            }
                        }
                    }
                    button { class: "live-pill", title: "Jump to live", onclick: move |_| { go_live(); if paused() { toggle_play(); } }, "LIVE" }
                    span { class: "grow" }
                    button { class: "ctl", aria_label: "Stream info", title: "Stream info (I)", onclick: move |_| show_stats.set(!show_stats()), Icon { d: INFO } }
                    button { class: "ctl", aria_label: "Picture in picture", title: "Picture in picture", onclick: move |_| toggle_pip(video_el()), Icon { d: PIP } }
                    button { class: "ctl", aria_label: "Fullscreen", title: "Fullscreen (F)", onclick: move |_| toggle_fullscreen("live-player", expanded), Icon { d: FULLSCREEN } }
                }
            }
            Guide { id, title }
        }
    }
}

/// The channel's schedule on a timeline: hour ticks, one block per programme, a marker for now.
/// It scrolls to now on load and moves the marker every 30 seconds.
#[component]
fn Guide(id: u64, title: String) -> Element {
    let session = use_context::<Signal<Option<Client>>>();
    let client = use_hook(|| session.read().clone().expect("logged in"));
    let mut tick = use_signal(|| 0_u32);
    use_future(move || async move {
        loop {
            rstreamkit::mse::sleep(Duration::from_secs(30)).await;
            tick += 1;
        }
    });
    let table = use_resource(move || {
        let c = client.clone();
        async move {
            let now = now_secs();
            match c.epg_table(id).await {
                Ok(t) if xtream::guide::has_nearby(&t, now) => Ok(t),
                _ => c.short_epg(id, 8).await,
            }
        }
    });
    // Once the schedule arrives, start with the current programme in view. The timeline isn't
    // laid out the moment the data lands, so wait a moment.
    use_effect(move || {
        if table.read().is_some() {
            spawn(async move {
                rstreamkit::mse::sleep(Duration::from_millis(60)).await;
                let timeline = web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.get_element_by_id("timeline"));
                if let Some(el) = timeline
                    && let Some(at) = el
                        .first_element_child()
                        .and_then(|c| c.get_attribute("data-now"))
                        .and_then(|a| a.parse::<i32>().ok())
                {
                    el.set_scroll_left((at - 160).max(0));
                }
            });
        }
    });

    let _ = tick(); // re-render now and then so the marker moves
    let now = now_secs();
    let mut current = None::<String>;
    let body = match &*table.read() {
        None => rsx! { p { class: "dim", "Loading the guide…" } },
        Some(Err(_)) => rsx! { p { class: "dim", "The guide isn't available for this channel." } },
        Some(Ok(t)) => {
            let slots = xtream::guide::normalize(t);
            current = slots
                .iter()
                .find(|slot| slot.start <= now && now < slot.end)
                .map(|slot| slot.listing.title.clone());
            if slots.is_empty() {
                rsx! { p { class: "dim", "This provider has no guide for the channel." } }
            } else {
                // From up to eight hours back to a day ahead, on whole hours.
                let lo = slots[0].start.max(now.saturating_sub(8 * 3600)).min(now) / 3600 * 3600;
                let last = slots.last().map_or(now, |slot| slot.end);
                let hi = last.max(now + 3600).min(now + 24 * 3600).div_ceil(3600) * 3600;
                let scale = xtream::guide::seconds_per_px(&slots, lo, hi);
                let px = |t: u64| t.clamp(lo, hi).saturating_sub(lo) / scale;
                let (width, here) = (px(hi), px(now));
                let visible: Vec<_> = slots
                    .into_iter()
                    .filter(|slot| slot.end > lo && slot.start < hi)
                    .map(|slot| {
                        let block_width = px(slot.end).saturating_sub(px(slot.start)).max(3);
                        (slot.start, slot.end, slot.listing, block_width)
                    })
                    .collect();
                if visible.is_empty() {
                    rsx! { p { class: "dim", "No current guide data for this channel." } }
                } else {
                    rsx! {
                        div {
                            class: "timeline",
                            id: "timeline",
                            div { class: "tl", style: "width:{width}px", "data-now": "{here}",
                                for hour in (lo..hi).step_by(3600) {
                                    span { class: "tick", key: "{hour}", style: "left:{px(hour)}px", "{clock(hour)}" }
                                }
                                for (start, end, l, block_width) in visible {
                                    div {
                                        key: "{start}",
                                        class: if start <= now && now < end { if block_width < 90 { "block now compact" } else { "block now" } } else if block_width < 90 { "block compact" } else { "block" },
                                        style: "left:{px(start)}px;width:{block_width}px",
                                        title: "{clock(start)} – {clock(end)} · {l.title} · {l.description}",
                                        div { class: "txt",
                                            time { "{clock(start)} – {clock(end)}" }
                                            strong { if l.title.is_empty() { "Untitled programme" } else { "{l.title}" } }
                                            if !l.description.is_empty() { p { "{l.description}" } }
                                        }
                                        if start <= now && now < end {
                                            i { class: "prog", style: "width:{(now - start) * 100 / (end - start)}%" }
                                        }
                                    }
                                }
                                div { class: "now-line", style: "left:{here}px", span { "{clock(now)}" } }
                            }
                        }
                    }
                }
            }
        }
    };

    rsx! {
        section { class: "guide",
            div { class: "guide-head",
                strong { "{title}" }
                if let Some(now_title) = current { small { "Now: {now_title}" } }
            }
            {body}
        }
    }
}
