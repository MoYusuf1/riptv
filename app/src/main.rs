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

mod categories;
mod channel_preview;
mod controls;
mod diag;
mod fetch;
mod frame_stats;
mod live;
mod media_session;
mod navigation;
mod preferences;
mod profiles;
mod shelves;
mod standard;

use controls::{IdleHide, LoadRing, Skip};
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
    r#":root{color-scheme:dark;--bg:#0b0709;--panel:#130d10;--row:#1c1418;--hair:rgba(255,255,255,.08);--text:#f5eef1;--dim:#a0919a;--faint:#6f6068;--accent:#ff7d92;--fill:#e11d48;--soft:rgba(255,125,146,.14);--glass:rgba(24,16,20,.72);--ease:cubic-bezier(.2,.8,.2,1)}
*{box-sizing:border-box}
*{scrollbar-width:thin;scrollbar-color:#59414c transparent}
::-webkit-scrollbar{width:9px;height:9px}
::-webkit-scrollbar-track{background:transparent}
::-webkit-scrollbar-thumb{border:2px solid var(--panel);border-radius:999px;background:#59414c}
::-webkit-scrollbar-thumb:hover{background:#946373}
::-webkit-scrollbar-corner{background:transparent}
button:focus-visible,a:focus-visible{outline:2px solid var(--accent);outline-offset:3px}
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
    // Who's watching, and the profile form: quiet, centred, round avatars.
    r#".login-page{display:grid;place-items:center;min-height:100vh;min-height:100dvh;padding:2rem 1.25rem;background:radial-gradient(120% 70% at 50% -10%,#24141b 0%,var(--bg) 62%)}
.chooser{display:flex;flex-direction:column;align-items:center;gap:2.5rem;width:min(100%,48rem);animation:fade .4s var(--ease)}
.chooser h1{margin:0;font-size:clamp(1.7rem,4vw,2.4rem);font-weight:600;letter-spacing:-.03em}
.profiles{display:flex;flex-wrap:wrap;justify-content:center;gap:1.8rem 1.5rem}
.profile{display:flex;text-align:center;flex-direction:column;align-items:center;gap:.8rem;width:7.6rem;color:var(--dim);animation:rise .45s var(--ease) backwards;animation-delay:calc(var(--i,0) * 45ms);transition:color .2s}
.profile:hover,.profile:focus-visible{color:var(--text);outline:0}
.profile:disabled{opacity:.55}
.avatar{position:relative;display:grid;place-items:center;width:6.6rem;aspect-ratio:1;border-radius:50%;background:linear-gradient(150deg,color-mix(in srgb,var(--c) 82%,#fff),color-mix(in srgb,var(--c) 72%,#000));color:#fff;font-size:2.5rem;font-weight:600;letter-spacing:-.02em;box-shadow:0 14px 34px color-mix(in srgb,var(--c) 28%,transparent);transition:transform .3s var(--ease),box-shadow .3s var(--ease)}
.profile:hover .avatar,.profile:focus-visible .avatar{transform:scale(1.06);box-shadow:0 0 0 3px var(--bg),0 0 0 5px rgba(255,255,255,.9),0 18px 40px color-mix(in srgb,var(--c) 38%,transparent)}
.profile:active .avatar{transform:scale(.97)}
.avatar svg{width:40%;height:40%}
.avatar.add{background:rgba(255,255,255,.06);box-shadow:inset 0 0 0 1.5px rgba(255,255,255,.12);color:var(--dim)}
.profile:hover .avatar.add{box-shadow:inset 0 0 0 1.5px rgba(255,255,255,.3);color:var(--text)}
.avatar .spinner{width:2rem;height:2rem}
.avatar .edit{position:absolute;inset:0;display:grid;place-items:center;border-radius:50%;background:rgba(0,0,0,.42);animation:fade .2s var(--ease)}
.avatar .edit svg{width:30%;height:30%}
.profile .name{max-width:100%;overflow:hidden;font-size:.92rem;font-weight:500;text-overflow:ellipsis;white-space:nowrap}
.text-btn{padding:.45rem .95rem;text-align:center;border-radius:999px;color:var(--dim);font-size:.88rem;font-weight:500;transition:color .2s,background .2s}
.text-btn:hover{color:var(--text);background:rgba(255,255,255,.07)}
.text-btn.danger{color:#ff8a9a}
.chooser .err,.profile-form .err{margin:0;color:#ff9aa8;font-size:.86rem;text-align:center}
.profile-form{display:flex;flex-direction:column;gap:.7rem;width:min(100%,22rem);animation:rise .35s var(--ease)}
.profile-form h1{margin:0 0 .5rem;text-align:center;font-size:1.55rem;font-weight:600;letter-spacing:-.025em}
.profile-form .avatar{align-self:center;width:5.6rem;margin-bottom:.6rem;font-size:2.1rem;cursor:pointer}
.profile-form .avatar:hover{transform:scale(1.05)}.profile-form .avatar:active{transform:scale(.96)}
.segmented{display:grid;grid-template-columns:1fr 1fr;padding:3px;border-radius:12px;background:rgba(255,255,255,.06)}
.segmented button{padding:.5rem;text-align:center;border-radius:9px;color:var(--dim);font-size:.86rem;font-weight:600;transition:background .25s var(--ease),color .2s}
.segmented button.on{background:rgba(255,255,255,.13);color:var(--text)}
.profile-form input{width:100%;padding:.82rem 1rem;border:0;border-radius:12px;background:rgba(255,255,255,.06);color:var(--text);font:inherit;outline:0;transition:background .2s,box-shadow .2s}
.profile-form input::placeholder{color:var(--faint)}
.profile-form input:focus{background:rgba(255,255,255,.09);box-shadow:0 0 0 2px color-mix(in srgb,var(--accent) 55%,transparent)}
.profile-form .primary{margin-top:.5rem;text-align:center;padding:.82rem;border-radius:12px;background:var(--fill);color:#fff;font-weight:600;transition:filter .2s,transform .15s var(--ease)}
.profile-form .primary:hover{filter:brightness(1.1)}.profile-form .primary:active{transform:scale(.98)}.profile-form .primary:disabled{opacity:.6}
.row-actions{display:flex;justify-content:center;flex-wrap:wrap;gap:.3rem}
.profile-form .note{text-align:center;color:var(--faint);font-size:.72rem}
@media(max-width:520px){.profiles{gap:1.4rem 1rem}.profile{width:6.2rem}.avatar{width:5.5rem;font-size:2.1rem}}
"#,
    // Top chrome: logo, search, account. Section navigation lives in navigation.rs.
    r#".topbar{position:fixed;z-index:20;display:flex;border:1px solid rgba(255,255,255,.1);background:linear-gradient(115deg,rgba(42,25,34,.88),rgba(20,14,18,.94));box-shadow:0 8px 32px rgba(0,0,0,.22),inset 0 1px 0 rgba(255,255,255,.03);backdrop-filter:blur(24px) saturate(150%);inset:.7rem .7rem auto;align-items:center;gap:1rem;height:3.7rem;padding:0 .65rem;border-radius:20px}
.brand{display:grid;place-items:center;flex:none;width:2.55rem;height:2.55rem;border:1px solid rgba(255,125,146,.13);border-radius:14px;background:linear-gradient(145deg,rgba(255,125,146,.12),rgba(255,255,255,.02));box-shadow:inset 0 1px 0 rgba(255,255,255,.04)}
.rust-mark{width:1.85rem;height:1.85rem;color:var(--accent)}
.search{position:relative;display:flex;align-items:center;flex:1;min-width:4rem;max-width:42rem;margin-inline:auto}
.search svg{position:absolute;left:.95rem;width:1rem;height:1rem;color:var(--dim);pointer-events:none}
.search input{width:100%;height:2.55rem;padding:0 1rem 0 2.6rem;border:1px solid rgba(255,255,255,.05);border-radius:13px;outline:0;background:rgba(255,255,255,.035);transition:border-color .2s,background .2s,box-shadow .2s}
.search input:hover{background:rgba(255,255,255,.055)}
.search input:focus{border-color:rgba(255,125,146,.35);background:rgba(255,125,146,.035);box-shadow:0 0 0 3px rgba(255,125,146,.06)}
.search input::placeholder{color:var(--faint)}
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
    r#".workspace{position:fixed;inset:5.1rem 0 0 5.1rem;display:grid;grid-template-columns:14rem minmax(0,1fr);gap:.7rem;padding:0 .7rem .7rem}
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
.row .eq{flex:none;height:12px;margin:0 .35rem 0 auto;gap:2px}
.row .eq i{width:2.5px;background:var(--accent)}
.channel-preview{position:fixed;z-index:45;display:flex;flex-direction:column;gap:.5rem;width:min(320px,calc(100vw - 24px));padding:1rem 1.1rem;border:1px solid rgba(255,125,146,.22);border-radius:16px;background:#22171e;box-shadow:0 16px 48px rgba(0,0,0,.5);text-align:left;pointer-events:none;animation:fade .16s var(--ease)}
.channel-preview .preview-label{color:var(--accent);font-size:.61rem;font-weight:700;letter-spacing:.09em}
.channel-preview strong{font-size:.98rem;line-height:1.35;white-space:normal}
.channel-preview time{font-size:.75rem;color:var(--dim);font-variant-numeric:tabular-nums}
.channel-preview .preview-description{display:-webkit-box;overflow:hidden;-webkit-box-orient:vertical;-webkit-line-clamp:4;font-size:.8rem;color:var(--dim);line-height:1.5}
.stage{display:flex;flex-direction:column;min-width:0;min-height:0;background:var(--bg)}
.stage-empty{display:grid;flex:1;place-content:center;justify-items:center;gap:.8rem;color:var(--dim)}
.stage-empty svg{width:3.4rem;height:3.4rem;opacity:.55}
.live-stage{display:flex;flex-direction:column;height:100%;min-height:0}
.live-stage>.player{flex:1;margin-bottom:.7rem}
.live-stage:has(>.guide)>.player{max-height:calc(100cqw * .5625);margin-bottom:0}
.live-stage:has(>.guide){container-type:inline-size}
.live-stage>.guide{flex:1;min-height:clamp(11rem,25vh,20rem);max-height:50%}
.live-stage>.guide.guide-note{flex:none;min-height:0;max-height:none}
.player{position:relative;min-height:0;margin:.7rem .7rem 0;overflow:hidden;border-radius:16px;background:#000;outline:0}
.player:fullscreen,.player.fill{margin:0;border-radius:0}
.player.fill{position:fixed;z-index:60;inset:0}
.player:not(.active):not(.paused){cursor:none}
.player video{width:100%;height:100%;object-fit:contain;opacity:0;transition:opacity .45s var(--ease)}
.player.showing video{opacity:1}
.radio{position:absolute;inset:0;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:1.1rem;overflow:hidden;background:#0a0709;color:#fff;pointer-events:none;animation:rise .5s var(--ease)}
.radio-glow{position:absolute;inset:-25%;width:150%;height:150%;object-fit:cover;opacity:.32;filter:blur(48px) saturate(1.6);transform:scale(1.1)}
.radio-art{position:relative;display:grid;place-items:center;width:clamp(6.5rem,26vh,10rem);aspect-ratio:1;border-radius:24%;background:rgba(255,255,255,.07);box-shadow:0 24px 60px rgba(0,0,0,.55),inset 0 0 0 1px rgba(255,255,255,.08);overflow:hidden}
.radio-art img{width:76%;height:76%;object-fit:contain}
.radio-art svg{width:38%;height:38%;opacity:.7}
.radio strong{position:relative;max-width:80%;overflow:hidden;font-size:1.05rem;font-weight:600;letter-spacing:-.01em;text-overflow:ellipsis;white-space:nowrap}
.eq{position:relative;display:flex;align-items:flex-end;gap:3px;height:16px}
.eq i{width:3px;height:100%;border-radius:2px;background:#fff;opacity:.85;transform-origin:bottom;animation:eq 1.1s var(--ease) infinite}
.eq i:nth-child(2){animation-duration:.85s;animation-delay:-.4s}
.eq i:nth-child(3){animation-duration:1.3s;animation-delay:-.2s}
.eq i:nth-child(4){animation-duration:.95s;animation-delay:-.7s}
.eq i:nth-child(5){animation-duration:1.2s;animation-delay:-.1s}
.radio.paused .eq i{animation-play-state:paused}
@keyframes eq{0%,100%{transform:scaleY(.2)}50%{transform:scaleY(1)}}
.sound-note{position:absolute;top:1rem;left:1rem;display:flex;align-items:center;gap:.4rem;padding:.35rem .7rem;border-radius:999px;background:rgba(0,0,0,.55);color:#fff;font-size:.75rem;backdrop-filter:blur(14px);animation:rise .3s var(--ease)}
.sound-note svg{width:.95rem;height:.95rem}
.stats{position:absolute;top:.8rem;left:.8rem;padding:.3rem .7rem;border-radius:8px;background:rgba(0,0,0,.66);color:#fff;font:600 .72rem ui-monospace,monospace;backdrop-filter:blur(8px)}
.channel-dial{position:absolute;top:1rem;right:1rem;min-width:3rem;padding:.45rem .75rem;border-radius:12px;background:rgba(0,0,0,.6);color:#fff;text-align:center;font-weight:700;font-variant-numeric:tabular-nums;backdrop-filter:blur(14px)}
.hud{position:absolute;inset:0;display:grid;place-items:center;color:#fff;pointer-events:none;animation:fade .3s var(--ease)}
.hud.fail{align-content:center;gap:.9rem;padding:2rem;text-align:center;pointer-events:auto;background:rgba(0,0,0,.35)}
.hud.fail p{max-width:22rem;margin:0;color:rgba(255,255,255,.82);font-size:.9rem;line-height:1.45}
.retry{justify-self:center;padding:.5rem 1.1rem;border-radius:999px;background:rgba(255,255,255,.14);color:#fff;font-size:.85rem;font-weight:600;backdrop-filter:blur(14px);transition:background .2s,transform .2s var(--ease)}
.retry:hover{background:rgba(255,255,255,.24)}.retry:active{transform:scale(.96)}
.loader{position:relative;display:grid;place-items:center;width:4.6rem;height:4.6rem}
.loader svg{position:absolute;inset:0;width:100%;height:100%;animation:spin 1.6s linear infinite}
.loader circle{fill:none;stroke-width:2.5}
.ring-track{stroke:rgba(255,255,255,.1)}
.ring-fill{stroke:url(#ring-grad);stroke-linecap:round;stroke-dasharray:138.23;transition:stroke-dashoffset 2.6s var(--ease)}
@starting-style{.ring-fill{stroke-dashoffset:138.23}}
.loader-logo{width:54%;height:54%;object-fit:contain;animation:breathe 2.4s ease-in-out infinite}
.loader-dot{width:.5rem;height:.5rem;border-radius:50%;background:#fff;animation:breathe 1.6s ease-in-out infinite}
@keyframes breathe{50%{opacity:.55;transform:scale(.92)}}
.spinner{width:2.6rem;height:2.6rem;border:2.5px solid rgba(255,255,255,.18);border-top-color:#fff;border-radius:50%;animation:spin .9s linear infinite}
@keyframes spin{to{transform:rotate(1turn)}}
.bigplay{position:absolute;top:50%;left:50%;display:grid;place-items:center;width:4.4rem;height:4.4rem;border-radius:50%;background:rgba(255,255,255,.14);color:#fff;transform:translate(-50%,-50%);backdrop-filter:blur(20px);animation:pop-center .25s var(--ease);transition:transform .2s var(--ease),background .2s}
.bigplay:hover{background:rgba(255,255,255,.22)}.bigplay:active{transform:translate(-50%,-50%) scale(.94)}
.bigplay svg{width:1.6rem;height:1.6rem;margin-left:.15rem;fill:currentColor}
@keyframes pop-center{from{opacity:0;transform:translate(-50%,-50%) scale(.85)}}
.controls{position:absolute;inset:auto 0 0;display:flex;align-items:center;gap:.25rem;padding:3.5rem .9rem .75rem;background:linear-gradient(transparent,rgba(0,0,0,.72));opacity:0;transform:translateY(6px);transition:opacity .3s var(--ease),transform .3s var(--ease)}
.player.active .controls,.player.paused .controls,.controls:focus-within{opacity:1;transform:none}
.ctl{display:grid;place-items:center;width:2.6rem;height:2.6rem;border-radius:50%;color:#fff;transition:background .2s,transform .15s var(--ease)}
.ctl:hover{background:rgba(255,255,255,.14)}.ctl:active{transform:scale(.9)}
.ctl svg{width:1.25rem;height:1.25rem}
.volume{display:flex;align-items:center}
.vol{width:0;height:4px;margin:0;opacity:0;appearance:none;border-radius:2px;background:linear-gradient(90deg,#fff var(--level,100%),rgba(255,255,255,.25) 0);transition:width .25s var(--ease),opacity .25s}
.vol::-webkit-slider-thumb{width:12px;height:12px;appearance:none;border-radius:50%;background:#fff}
.vol::-moz-range-thumb{width:12px;height:12px;border:0;border-radius:50%;background:#fff}
.volume:hover .vol,.vol:focus-visible{width:5.5rem;margin:0 .6rem 0 .2rem;opacity:1}
.live-pill{display:inline-flex;align-items:center;gap:.4rem;margin-left:.35rem;padding:.28rem .7rem;border-radius:999px;background:var(--fill);color:#fff;font-size:.68rem;font-weight:700;letter-spacing:.08em;transition:background .2s}
.live-pill::before{content:'';width:.42rem;height:.42rem;border-radius:50%;background:#fff}
.player:not(.paused) .live-pill::before{animation:pulse 1.8s ease-in-out infinite}
.player.paused .live-pill{background:rgba(255,255,255,.16)}
@media(prefers-reduced-motion:reduce){.loader svg,.loader-logo,.loader-dot,.eq i,.live-pill::before{animation:none!important}}
.guide{display:flex;flex-direction:column;gap:.8rem;min-width:0;padding:1rem;border-top:1px solid var(--hair);background:var(--panel);animation:fade .3s var(--ease)}
.guide-heading{display:flex;align-items:center;justify-content:space-between;gap:.75rem}
.guide-heading strong{font-size:.8rem;font-weight:600}
.guide-heading small{color:var(--faint);font-size:.68rem}
.tl-skeleton{display:flex;gap:.4rem;height:6.4rem;padding-top:1.5rem}
.tl-skeleton i{flex:1;border-radius:10px;background:var(--row);animation:pulse 1.4s ease-in-out infinite}
.tl-skeleton i:first-child{flex:.6}
.timeline{flex:1;min-height:0;overflow-x:auto;padding-bottom:.4rem;scrollbar-width:thin}
.tl{position:relative;height:100%;min-height:6.4rem}
.tick{position:absolute;top:0;height:100%;padding-left:.4rem;border-left:1px solid var(--hair);color:var(--faint);font-size:.7rem}
.block{position:absolute;top:1.5rem;bottom:0;padding:.5rem .7rem;border-right:3px solid var(--panel);border-radius:10px;background:var(--row)}
.block.now{background:var(--soft);outline:1px solid rgba(255,125,146,.45);outline-offset:-1px}
.block .txt{position:sticky;left:.7rem;max-width:min(100%,18rem);overflow:hidden}
.block.compact{padding:.5rem .35rem}.block.compact time,.block.compact p{display:none}.block.compact strong{font-size:.72rem}
.block time{display:block;color:var(--dim);font-size:.7rem}
.block strong{display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.block p{display:-webkit-box;margin:.5rem 0 0;overflow:hidden;color:var(--dim);font-size:.8rem;line-height:1.55;-webkit-box-orient:vertical;-webkit-line-clamp:5}
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
.scroll{animation:fade .2s var(--ease)}
.detail{animation:rise .3s cubic-bezier(.2,.8,.2,1)}
.d-main{animation:rise .45s .06s cubic-bezier(.2,.8,.2,1) backwards}
.watch{animation:fade .22s var(--ease)}
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
    r#"@media(max-width:1080px){.topbar{gap:.7rem}.who b{display:none}.rail button{padding:0 .65rem}}
@media(max-width:820px){.brand{display:none}.who b{display:none}.topbar{gap:.5rem;height:3rem;inset:.6rem .6rem auto}.rail{position:fixed;z-index:25;top:auto;bottom:.8rem;left:50%;flex-direction:row;transform:translateX(-50%);background:rgba(24,16,20,.94);backdrop-filter:blur(24px);box-shadow:0 10px 30px rgba(0,0,0,.4);border-radius:18px}.rail button{flex-direction:column;gap:.15rem;width:auto;min-width:4.4rem;height:3rem;padding:0 .8rem}.rail span{font-size:.65rem}.workspace{inset:4.2rem 0 0;grid-template-columns:1fr;padding:0 .6rem .6rem}.sidebar{display:none}.cats-btn{display:inline-flex}.scroll,.episodes{padding-bottom:5.5rem}.d-hero{min-height:auto;padding:7rem 1rem 1.6rem}.d-facts{position:relative;inset:auto;width:auto;margin-top:1.4rem}.d-sec{padding:1rem 1rem .8rem}.w-top{padding:.8rem .8rem 2.5rem}.w-bottom{padding:3rem .8rem .6rem}.w-row .vol,.w-row .volume{display:none}.w-menu{right:.8rem;bottom:5.2rem}}
@media(max-width:820px){.topbar{backdrop-filter:none;background:#181014}}
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
const SERIES: &str = "M7 3h10M5 6h14M5 9h14a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2v-8a2 2 0 012-2zM10 12l5 3-5 3v-6z";
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
const PIN: &str = "M9 3h6M10 3v6l-4 4v2h12v-2l-4-4V3M12 15v6";
const SORT: &str = "M3 6h11M3 12h7M3 18h4M17 6v12m0 0l-3-3m3 3l3-3";
const CHECK: &str = "M5 12l5 5 9-10";
const INFO: &str = "M12 3a9 9 0 100 18 9 9 0 000-18zM12 8h.01M11 12h1v5h1";
const HEART: &str =
    "M12 20s-7.5-4.6-7.5-10.2A4.3 4.3 0 0112 7.3a4.3 4.3 0 017.5 2.5C19.5 15.4 12 20 12 20z";
const CLOSE: &str = "M6 6l12 12M18 6L6 18";
const MUSIC: &str =
    "M9 18V5l12-2v13M9 18a3 3 0 11-6 0 3 3 0 016 0zM21 16a3 3 0 11-6 0 3 3 0 016 0z";
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
    // A few words each: the detail is in the proxy's `--logs` trace.
    if has("404") || has("Not Found") {
        "This channel is offline"
    } else if has("401") || has("403") || has("Forbidden") || has("Unauthorized") {
        "Your provider refused this channel"
    } else if has("429") || has("458") || has("509") || has("Bandwidth Limit") {
        "Too many streams on your account"
    } else if has("timed out") || has("took too long") {
        "Your provider isn't responding"
    } else if has("ffmpeg isn't installed") {
        "ffmpeg isn't installed"
    } else {
        "This channel isn't available right now"
    }
    .into()
}

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
        style { "{navigation::CSS}{categories::CSS}" }
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
            role: "img",
            "aria-label": "Rust logo",
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

/// A section's data kept after it loads (see `Browse`), shared rather than copied.
#[derive(Default)]
struct Loaded {
    categories: Option<Rc<Vec<xtream::Category>>>,
    library: Option<Rc<Library>>,
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
    let mut pinned_categories = use_signal(|| remembered.pinned_categories.clone());
    let mut playing = use_signal(|| None::<Play>);
    let mut queue = use_signal(Vec::<Play>::new); // the episodes after the one playing
    let mut live = use_signal(|| None::<(u64, String, String, Option<String>)>); // (id, title, playlist url, logo)
    let mut open = use_signal(|| None::<Row>); // the movie or series page
    let mut sort = use_signal(|| Sort::restored(Kind::Live, remembered.sorts[0]));
    let mut sort_open = use_signal(|| false);
    let mut account_open = use_signal(|| false);
    let mut cats_open = use_signal(|| false);
    let mut player_revision = use_signal(|| 0_u64);
    let mut refresh = use_signal(|| 0_u64);
    let mut page = use_signal(|| 0_usize);

    let (c_cats, c_lib) = (client.clone(), client.clone());
    // Each section, once loaded, stays: switching back to it is instant. Refresh starts over.
    let loaded = use_hook(|| Rc::new(RefCell::new(HashMap::<(u8, u64), Loaded>::new())));
    let cats_loaded = loaded.clone();
    let cats = use_resource(move || {
        let (c, k, generation) = (c_cats.clone(), kind(), refresh());
        let cache = cats_loaded.clone();
        async move {
            let key = (k as u8, generation);
            if let Some(list) = cache.borrow().get(&key).and_then(|l| l.categories.clone()) {
                return Ok::<_, xtream::Error>((k, list));
            }
            let list = match k {
                Kind::Live => c.live_categories().await,
                Kind::Movies => c.vod_categories().await,
                Kind::Series => c.series_categories().await,
            }
            .map(Rc::new)?;
            let mut cache = cache.borrow_mut();
            cache.retain(|(_, g), _| *g == generation);
            cache.entry(key).or_default().categories = Some(list.clone());
            Ok((k, list))
        }
    });
    let library = use_resource(move || {
        let (c, k, generation) = (c_lib.clone(), kind(), refresh());
        let cache = loaded.clone();
        async move {
            let key = (k as u8, generation);
            if let Some(lib) = cache.borrow().get(&key).and_then(|l| l.library.clone()) {
                return Ok::<_, xtream::Error>(lib);
            }
            let lib = Rc::new(load(&c, k).await?);
            let mut cache = cache.borrow_mut();
            cache.retain(|(_, g), _| *g == generation);
            cache.entry(key).or_default().library = Some(lib.clone());
            Ok(lib)
        }
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
            live.set(Some((id, row.title, url, row.icon)));
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
                rsx! {
                    categories::CategoryList {
                        data: categories::CategoryData { list: list.clone(), library: lib.cloned() }, selected: category(),
                        pins: pinned_categories()[kind() as usize].clone(), query: category_search,
                        onselect: move |id| select(id),
                        onpin: move |id| {
                            let mut pins = pinned_categories();
                            preferences::toggle_pin(&mut pins[kind() as usize], id);
                            pinned_categories.set(pins.clone());
                            preferences::update(|p| p.pinned_categories = pins);
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
                            active: live().as_ref().map(|(id, ..)| *id) == Some(r.key),
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
    let live_panel = live().map(|(id, title, url, icon)| {
        let player_key = format!("{url}-{}", player_revision());
        rsx! {
            live::LivePlayer {
                key: "{player_key}",
                id,
                title,
                url,
                icon,
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
                                stream.stream_icon.clone(),
                            ))
                    };
                    if let Some((position, next_id, name, icon)) = selected
                        && next_id != id
                    {
                        page.set(position / PAGE_LIST);
                        live.set(Some((next_id, name, channel_client.live_url(next_id, "m3u8").to_string(), icon)));
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
                span { class: "brand", title: "Rust", RustMark {} }
                label { class: "search",
                    Icon { d: SEARCH }
                    input {
                        aria_label: "Search this section",
                        placeholder: "Search",
                        value: "{search}",
                        oninput: move |e| { search.set(e.value()); page.set(0); }
                    }
                }
            }
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
        if open().is_none() {
            navigation::LibraryNav { kind: kind(), playlist: client.is_playlist(), onpick: move |k| pick_kind(k) }
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
    let anchor = format!("channel-{}", row.key);
    let channel_preview::Preview {
        mut pointer,
        mut focused,
        mut escaped,
        content,
    } = channel_preview::use_preview(Some(row.key), anchor.clone());
    rsx! {
        button {
            id: "{anchor}",
            class: if active { "row on" } else { "row" },
            aria_describedby: if (pointer() || focused()) && !escaped() { Some(format!("{anchor}-guide")) } else { None },
            onmouseenter: move |_| { escaped.set(false); pointer.set(true); },
            onmouseleave: move |_| pointer.set(false),
            onfocus: move |_| { escaped.set(false); focused.set(true); },
            onblur: move |_| focused.set(false),
            onkeydown: move |e| { if e.key() == Key::Escape { escaped.set(true); } },
            onclick: move |_| onpick.call(r.clone()),
            span { class: "logo",
                span { class: "ph", "TV" }
                if let Some(src) = row.icon.as_deref().filter(|s| s.starts_with("http")) {
                    img { src: "{src}", loading: "lazy", decoding: "async" }
                }
            }
            strong { "{row.title}" }
            if active { span { class: "eq", i {} i {} i {} } }
        }
        {content}
    }
}

/// A poster (movies, series) or a logo tile (channels), depending on the grid it sits in.
#[component]
fn Card(row: Row, onpick: EventHandler<Row>) -> Element {
    let r = row.clone();
    let id = match row.target {
        Target::Live { id, .. } => Some(id),
        _ => None,
    };
    let anchor = format!("tile-{}", row.key);
    let channel_preview::Preview {
        mut pointer,
        mut focused,
        mut escaped,
        content,
    } = channel_preview::use_preview(id, anchor.clone());
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
            id: "{anchor}",
            class: "card",
            title: if id.is_none() { Some(row.title.clone()) } else { None },
            aria_describedby: if id.is_some() && (pointer() || focused()) && !escaped() { Some(format!("{anchor}-guide")) } else { None },
            onmouseenter: move |_| { escaped.set(false); pointer.set(true); },
            onmouseleave: move |_| pointer.set(false),
            onfocus: move |_| { escaped.set(false); focused.set(true); },
            onblur: move |_| focused.set(false),
            onkeydown: move |e| { if e.key() == Key::Escape { escaped.set(true); } },
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
        {content}
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
    // How far the current source has got with starting, for the progress ring.
    let mut load_stage = use_signal(|| 0_u8);
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
                    div { class: "w-load", LoadRing { stage: 0 } }
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
                    // A new source (its own element) starts the ring over.
                    onloadstart: move |_| load_stage.set(1),
                    onloadeddata: move |_| if *load_stage.peek() < 3 { load_stage.set(3) },
                    onplaying: move |_| { load_stage.set(4); waiting.set(false); paused.set(false); },
                    oncanplay: move |_| waiting.set(false),
                    onloadedmetadata: move |_| {
                        if *load_stage.peek() < 2 {
                            load_stage.set(2);
                        }
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
                if waiting() && !paused() {
                    div { class: "hud",
                        if load_stage() < 4 { LoadRing { stage: load_stage() } } else { i { class: "spinner" } }
                    }
                }
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
