//! Dioxus web UI. It talks to IPTV servers only through the local proxy
//! (`xtream::Client::via_proxy`), because servers send no CORS headers.
//!
//! ponytail: credentials live in memory only (reload = log in again), no favourites, downloads are
//! plain browser downloads (no queue), no adaptive bitrate, and live streams must be HLS with
//! MPEG-TS segments (H.264 + AAC).

use dioxus::prelude::*;
use std::{cell::RefCell, collections::HashMap, rc::Rc, time::Duration};

use web_sys::{
    js_sys,
    wasm_bindgen::{JsCast, JsValue, closure::Closure},
};
use xtream::{Client, Details, EpgListing, LiveStream, Season, VodStream};

/// Rows built per "page"; scrolling near the bottom adds another page.
const SHOW: usize = 200;

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
"#,
    // Sign-in
    r#".login-page{display:grid;place-items:center;min-height:100vh;padding:1.4rem;background:radial-gradient(circle at 50% 0,rgba(225,29,72,.22),transparent 45%),var(--bg)}
.login{display:flex;flex-direction:column;gap:.75rem;width:min(100%,24rem);padding:2rem;border-radius:22px;background:var(--panel);box-shadow:0 30px 80px rgba(0,0,0,.5)}
.login .brand{align-self:center}
.login h1{margin:.4rem 0 .5rem;font-size:1.9rem;letter-spacing:-.03em}
.login input{width:100%;padding:.85rem 1rem;border:0;border-radius:12px;outline:0;background:var(--row)}
.login input:focus{box-shadow:0 0 0 2px var(--accent)}
.login button{padding:.85rem;border-radius:12px;background:var(--fill);color:#fff;font-weight:600;text-align:center}
.login button:disabled{opacity:.6;cursor:wait}
.login button.ghost{background:none;color:var(--accent)}
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
.head{display:flex;align-items:flex-end;justify-content:space-between;gap:1rem;padding:.4rem 1rem .7rem .4rem}
.head h1{margin:0;font-size:1.6rem;letter-spacing:-.03em;line-height:1.1}
.head span{color:var(--dim);font-size:.8rem}
.tools{display:flex;align-items:center;gap:.5rem}
.sortwrap{position:relative}
.sort-btn{display:inline-flex;align-items:center;gap:.4rem;padding:.4rem .8rem;border-radius:999px;background:var(--row);font-size:.8rem;color:var(--dim)}
.sort-btn:hover{color:var(--text)}
.sort-menu{min-width:11rem}
.menu button.on{color:var(--accent);font-weight:600}
.menu button:has(svg){display:flex;align-items:center;justify-content:space-between}
.channels .sort-btn span{display:none}
.channels .sort-btn{padding:.45rem}
.cats-btn{display:none;align-items:center;gap:.4rem;flex:none;padding:.4rem .8rem;border-radius:999px;background:var(--row);font-size:.8rem}
.scroll{flex:1;min-height:0;padding:0 .4rem 2rem;overflow:auto;scrollbar-width:thin}
"#,
    // Posters and channel tiles
    r#".grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(8.6rem,1fr));gap:1.5rem 1rem}
.card{min-width:0;text-align:center;transition:transform .15s}
.card:hover{transform:scale(1.03)}
.art{position:relative;display:grid;place-items:center;aspect-ratio:2/3;overflow:hidden;border-radius:12px;background:var(--row);box-shadow:0 6px 18px rgba(0,0,0,.35);color:var(--faint);font-size:1.6rem;font-weight:700}
.tiles .art{aspect-ratio:16/10}
.art img{width:100%;height:100%;object-fit:cover}
.tiles .art img{padding:.7rem;object-fit:contain}
.score{position:absolute;top:.4rem;right:.4rem;padding:.1rem .45rem;border-radius:999px;background:rgba(0,0,0,.55);color:#fff;font-size:.68rem;font-weight:600;backdrop-filter:blur(8px)}
.score::before{content:'★ ';color:#ffcc4d}
.card strong{display:-webkit-box;margin-top:.5rem;overflow:hidden;color:var(--dim);font-size:.74rem;font-weight:500;-webkit-box-orient:vertical;-webkit-line-clamp:2}
"#,
    // Movie and series pages
    r#".detail{position:absolute;z-index:3;inset:0;overflow:hidden auto;border-radius:16px;background:var(--bg);scrollbar-width:thin}
.backdrop{position:absolute;inset:0 0 auto;width:100%;height:26rem;object-fit:cover;filter:blur(30px) brightness(.5) saturate(1.2);transform:scale(1.15);-webkit-mask-image:linear-gradient(#000 40%,transparent);mask-image:linear-gradient(#000 40%,transparent)}
.back{position:absolute;z-index:2;top:1rem;left:1rem;background:rgba(0,0,0,.4)}
.hero{position:relative;display:flex;align-items:flex-start;gap:1.8rem;padding:4rem 2.2rem 1.2rem}
.poster-lg{flex:none;width:11.5rem;aspect-ratio:2/3;object-fit:cover;border-radius:14px;background:var(--row);box-shadow:0 18px 50px rgba(0,0,0,.55)}
.info{min-width:0;max-width:48rem}
.info h1{margin:0 0 .7rem;font-size:2.4rem;letter-spacing:-.03em;line-height:1.05}
.chips{display:flex;flex-wrap:wrap;gap:.4rem;margin-bottom:.9rem}
.chip{padding:.15rem .6rem;border-radius:7px;background:rgba(255,255,255,.1);font-size:.75rem}
.plot{margin:0;color:#e6dbe0;line-height:1.55}
.plot.clamp{display:-webkit-box;overflow:hidden;-webkit-box-orient:vertical;-webkit-line-clamp:4}
.more{margin-top:.3rem;font-size:.72rem;font-weight:600;letter-spacing:.05em}
.facts{display:flex;flex-wrap:wrap;gap:.6rem 2.2rem;margin:1rem 0}
.facts small{display:block;color:var(--faint);font-size:.72rem}
.facts strong{font-weight:500}
.actions{display:flex;align-items:center;gap:.6rem;margin-top:1.2rem}
.play{display:inline-flex;align-items:center;gap:.5rem;padding:.7rem 1.4rem;border-radius:14px;background:#fff;color:#111;font-weight:600}
.actions .icon-btn{width:2.9rem;height:2.9rem;border-radius:14px}
.episodes{position:relative;padding:0 1.4rem 2rem}
.episodes h3{margin:1rem 0 .4rem;color:var(--dim);font-size:.75rem;letter-spacing:.06em;text-transform:uppercase}
.ep{display:flex;align-items:center;gap:.5rem;padding-right:.4rem;border-radius:10px}
.ep:hover{background:var(--row)}
.ep button{flex:1;padding:.7rem .6rem}
"#,
    // Live TV: channel list, player, timeline guide
    r#".live{display:grid;flex:1;grid-template-columns:18rem minmax(0,1fr);min-width:0;min-height:0;overflow:hidden;border:1px solid var(--hair);border-radius:16px}
.channels{border-right:1px solid var(--hair);background:var(--panel)}
.channels .head{padding:1rem 1rem .6rem}
.channels .head h1{font-size:1.15rem}
.channels .scroll{padding:0 .6rem 1rem}
.row{display:flex;align-items:center;gap:.7rem;width:100%;padding:.5rem .6rem;border-radius:12px}
.row:hover{background:var(--row)}
.row.on{background:var(--soft)}
.logo{display:grid;place-items:center;flex:none;width:2.5rem;height:2.5rem;overflow:hidden;border-radius:9px;background:var(--row);color:var(--faint);font-size:.6rem;font-weight:700}
.logo img{width:100%;height:100%;padding:.2rem;object-fit:contain}
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
.sound-note{position:absolute;left:1rem;bottom:4.6rem;max-width:calc(100% - 2rem);padding:.35rem .8rem;border-radius:10px;background:rgba(0,0,0,.68);color:#ffd48a;font-size:.78rem;backdrop-filter:blur(10px)}
.sound-note.inline{position:static;max-width:none;padding:.7rem 1rem;border-radius:0;background:rgba(255,180,84,.12);backdrop-filter:none}
.stats{position:absolute;top:.8rem;left:.8rem;padding:.3rem .7rem;border-radius:8px;background:rgba(0,0,0,.66);color:#fff;font:600 .72rem ui-monospace,monospace;backdrop-filter:blur(8px)}
.hud{position:absolute;inset:0;display:grid;place-items:center;color:#fff;pointer-events:none}
.hud span{padding:.4rem .9rem;border-radius:999px;background:rgba(0,0,0,.6);backdrop-filter:blur(10px)}
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
.block time{display:block;color:var(--dim);font-size:.7rem}
.block strong{display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.block p{display:-webkit-box;margin:.25rem 0 0;overflow:hidden;color:var(--dim);font-size:.75rem;-webkit-box-orient:vertical;-webkit-line-clamp:2}
.prog{position:absolute;bottom:0;left:0;height:3px;background:var(--accent)}
.now-line{position:absolute;z-index:2;top:0;bottom:0;width:2px;background:var(--fill)}
.now-line span{position:absolute;top:0;left:-1.3rem;padding:.02rem .35rem;border-radius:5px;background:var(--fill);color:#fff;font-size:.65rem;font-weight:600}
"#,
    // Sheets: movie player and the category picker
    r#".overlay{position:fixed;z-index:50;inset:0;display:grid;place-items:center;padding:4vh 4vw;background:rgba(0,0,0,.6);backdrop-filter:blur(10px)}
.overlay.bottom{place-items:end center;padding:0}
.sheet{width:min(100%,64rem);max-height:92vh;overflow:auto;border-radius:20px;background:var(--panel);box-shadow:0 30px 90px rgba(0,0,0,.6)}
.sheet video{display:block;width:100%;max-height:64vh;background:#000}
.sheet.cats{width:100%;max-height:78vh;border-radius:22px 22px 0 0}
.sheet-body{padding:0 .8rem 1rem}
.vod{position:relative;background:#000}
.vod.fill{position:fixed;z-index:60;inset:0}
.vod.fill video,.vod:fullscreen video{height:100%;max-height:none}
.vod .controls{opacity:1}
.scrub{flex:1;min-width:4rem;margin:0 .5rem;accent-color:var(--accent)}
.time{margin:0 .3rem;color:#fff;font-size:.78rem;font-variant-numeric:tabular-nums;white-space:nowrap}
.bar{display:flex;align-items:center;justify-content:space-between;gap:1rem;padding:.8rem 1rem}
.bar strong{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.bar div{display:flex;align-items:center;gap:.5rem}
"#,
    // Narrow screens: the rail becomes a floating tab bar, categories a button
    r#"@media(max-width:820px){.brand span,.who b{display:none}.topbar{gap:.5rem}.rail{top:auto;bottom:.8rem;left:50%;flex-direction:row;transform:translateX(-50%)}.rail button{width:auto;min-width:4.4rem;height:3rem;padding:0 .8rem}.rail span{display:block}.workspace{inset:4.2rem 0 0;grid-template-columns:1fr;padding:0 .6rem .6rem}.sidebar{display:none}.cats-btn{display:inline-flex}.scroll,.episodes{padding-bottom:5.5rem}.hero{flex-direction:column;align-items:center;padding:4rem 1rem 1rem;text-align:center}.poster-lg{width:9rem}.info h1{font-size:1.7rem}.chips,.facts,.actions{justify-content:center}}
/* Live TV on a phone, like a video app: the picture on top and full width, its guide under it, the channel list below. */
@media(max-width:820px){.live{display:flex;flex-direction:column;margin:0 -.6rem;border:0;border-radius:0}.stage{order:-1;flex:none}.stage.idle{display:none}.live-stage{display:block;height:auto}.player{margin:0;border-radius:0;aspect-ratio:16/9}.player.fill{aspect-ratio:auto}.controls{padding:2.2rem .5rem .3rem}.ctl{width:2.8rem;height:2.8rem}.vol{display:none}.bigplay{width:4.2rem;height:4.2rem}.sound-note{bottom:3.9rem}.guide{min-height:0;padding:.7rem .9rem .8rem;border-top:0;background:var(--bg)}.tl{height:5.6rem}.channels{flex:1;border:0;background:none}.channels .head{padding:.8rem 1rem .4rem}.channels .scroll{padding-bottom:5.5rem}}
/* A phone turned sideways while watching: just the picture. */
@media(max-height:500px) and (orientation:landscape){body:has(.live.watching) .topbar,body:has(.live.watching) .rail,.workspace:has(.live.watching) .sidebar,.live.watching .channels,.live.watching .guide{display:none}.workspace:has(.live.watching){inset:0;grid-template-columns:1fr;padding:0}.live.watching{grid-template-columns:1fr;margin:0}.live.watching .stage{flex:1}.live.watching .live-stage{height:100%}.live.watching .player{height:100%;aspect-ratio:auto}}
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
const USER: &str = "M20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2M12 11a4 4 0 100-8 4 4 0 000 8z";
const PIP: &str = "M4 5h16a1 1 0 011 1v7M3 6v11a1 1 0 001 1h6M13 14h7a1 1 0 011 1v3a1 1 0 01-1 1h-7a1 1 0 01-1-1v-3a1 1 0 011-1z";
const SORT: &str = "M3 6h11M3 12h7M3 18h4M17 6v12m0 0l-3-3m3 3l3-3";
const CHECK: &str = "M5 12l5 5 9-10";
const INFO: &str = "M12 3a9 9 0 100 18 9 9 0 000-18zM12 8h.01M11 12h1v5h1";
const CLOSE: &str = "M6 6l12 12M18 6L6 18";
const LIST: &str = "M4 6h16M4 12h16M4 18h10";

/// Pixels on the guide timeline are this many seconds wide (90 px per hour).
const SECS_PER_PX: u64 = 40;

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
    c.approve().await?;
    c.auth().await?;
    Ok(c)
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
fn toggle_pip() {
    let (Some(video), Some(doc)) = (video_el(), web_sys::window().and_then(|w| w.document()))
    else {
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

fn sheet_video() -> Option<web_sys::HtmlVideoElement> {
    web_sys::window()?
        .document()?
        .query_selector(".sheet video")
        .ok()??
        .dyn_into()
        .ok()
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
            rffmpeg::mse::sleep(Duration::from_millis(600)).await;
            if doc.fullscreen_element().is_none() {
                expanded.set(true);
            }
        });
    }
}

/// Chromium counts the audio bytes it has decoded. Zero after a few seconds of playing, with the
/// element not muted, means the stream's sound isn't in a format this browser can decode (AC-3,
/// DTS and friends) or it has none. Other browsers don't report it, and then this stays quiet.
fn no_audio_decoded(v: &web_sys::HtmlVideoElement) -> bool {
    let bytes = js_sys::Reflect::get(v, &JsValue::from_str("webkitAudioDecodedByteCount"))
        .ok()
        .and_then(|n| n.as_f64());
    v.current_time() > 4.0 && !v.muted() && bytes == Some(0.0)
}

#[component]
fn App() -> Element {
    let session = use_context_provider(|| Signal::new(None::<Client>));
    use_context_provider(|| Signal::new(String::new())); // the account name, if any
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

#[component]
fn Login() -> Element {
    let mut session = use_context::<Signal<Option<Client>>>();
    let mut playlist = use_context::<Signal<String>>();
    let mut url = use_signal(String::new);
    let mut user = use_signal(String::new);
    let mut pass = use_signal(String::new);
    let mut alias = use_signal(String::new);
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
            form { class: "login",
                onsubmit: move |e| {
                    e.prevent_default();
                    connect((url(), user(), pass(), alias().trim().to_string()));
                },
                div { class: "brand", RustMark {} span { "RIPTV" } }
                h1 { "Sign in" }
                input { aria_label: "Server URL", placeholder: "Server URL", value: "{url}", oninput: move |e| url.set(e.value()) }
                input { aria_label: "Username", autocomplete: "username", placeholder: "Username", value: "{user}", oninput: move |e| user.set(e.value()) }
                input { r#type: "password", aria_label: "Password", autocomplete: "current-password", placeholder: "Password", value: "{pass}", oninput: move |e| pass.set(e.value()) }
                input { aria_label: "Account name", placeholder: "Account name (optional)", value: "{alias}", oninput: move |e| alias.set(e.value()) }
                button { r#type: "submit", disabled: busy(), if busy() { "Connecting…" } else { "Connect" } }
                button { r#type: "button", class: "ghost",
                    onclick: move |_| connect(("http://127.0.0.1:8081".into(), "demo".into(), "demo".into(), "Demo".into())),
                    "Try the demo"
                }
                if let Some(msg) = status() { p { class: "err", "{msg}" } }
            }
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
        // What each kind of list sorts by, pulled out once so the sorting below isn't repeated
        // (and compiled) for every kind of list.
        let (names, dates, ratings): (Vec<&str>, Vec<Option<u64>>, Vec<u32>) = match &self.items {
            Items::Live(v) => (
                v.iter().map(|s| s.name.as_str()).collect(),
                vec![None; v.len()],
                vec![0; v.len()],
            ),
            Items::Movies(v) => (
                v.iter().map(|s| s.name.as_str()).collect(),
                v.iter().map(|s| s.added).collect(),
                v.iter().map(|s| tenths(&s.rating)).collect(),
            ),
            Items::Series(v) => (
                v.iter().map(|s| s.name.as_str()).collect(),
                v.iter().map(|s| s.last_modified).collect(),
                v.iter().map(|s| tenths(&s.rating)).collect(),
            ),
        };
        let mut order: Vec<u32> = (0..names.len() as u32).collect();
        // Every numeric sort is "smallest key first"; titles with no date go last either way.
        let number = |i: u32| -> u64 {
            let i = i as usize;
            match sort {
                Sort::Newest => dates[i].map_or(u64::MAX, |d| u64::MAX - d),
                Sort::Oldest => dates[i].unwrap_or(u64::MAX),
                _ => u64::from(u32::MAX - ratings[i]),
            }
        };
        match sort {
            Sort::Provider => {}
            Sort::AZ | Sort::ZA => {
                order.sort_by_cached_key(|&i| names[i as usize].to_lowercase());
                if sort == Sort::ZA {
                    order.reverse();
                }
            }
            _ => order.sort_by_key(|&i| number(i)),
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

/// The rows to show: items in `cat` (all if `None`) whose title contains `q` (lowercase), at most
/// `limit`, plus how many matched. Only the rows actually shown are built (that includes making a
/// stream URL), so a 30,000-title library stays cheap on every keystroke in the search box.
fn rows(
    lib: &Library,
    order: &[u32],
    c: &Client,
    cat: Option<u64>,
    q: &str,
    limit: usize,
) -> (Vec<Row>, usize) {
    fn take<T>(
        (list, order): (&[T], &[u32]),
        (cat, q, limit): (Option<u64>, &str, usize),
        info: impl Fn(&T) -> (&str, Option<u64>),
        row: impl Fn(&T) -> Row,
    ) -> (Vec<Row>, usize) {
        let mut hits = order.iter().map(|&i| &list[i as usize]).filter(|t| {
            let (title, category) = info(t);
            (cat.is_none() || category == cat) && (q.is_empty() || title.to_lowercase().contains(q))
        });
        let shown: Vec<Row> = hits.by_ref().take(limit).map(&row).collect();
        let total = shown.len() + hits.count();
        (shown, total)
    }
    let filter = (cat, q, limit);
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
    let mut open = use_signal(|| None::<Row>); // the movie or series page
    // A title whose sound the browser can't play: what we tell the viewer, and the converted
    // stream that replaces the original once the proxy has one.
    let mut movie_note = use_signal(|| None::<String>);
    let mut movie_conv = use_signal(|| None::<(xtream::Converted, u64)>);
    let mut sort = use_signal(|| Sort::Provider);
    let mut sort_open = use_signal(|| false);
    let mut account_open = use_signal(|| false);
    let mut cats_open = use_signal(|| false);
    let mut player_revision = use_signal(|| 0_u64);
    let mut refresh = use_signal(|| 0_u64);
    let mut limit = use_signal(|| SHOW);

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
        kind.set(k);
        sort.set(Sort::options(k)[0]);
        category.set(None);
        search.set(String::new());
        category_search.set(String::new());
        open.set(None);
        playing.set(None);
        live.set(None);
        limit.set(SHOW);
    };
    let mut select = move |cat: Option<u64>| {
        category.set(cat);
        search.set(String::new());
        open.set(None);
        cats_open.set(false);
        if cat.is_none() {
            live.set(None);
        }
        limit.set(SHOW);
    };
    let pick = move |row: Row| match row.target {
        Target::Live { id, url } => {
            playing.set(None);
            if !three_pane {
                category.set(row.category);
                limit.set(SHOW);
            }
            live.set(Some((id, row.title, url)));
        }
        Target::Movie { .. } | Target::Series(_) => open.set(Some(row)),
    };

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
                    for c in list.iter().filter(|c| q.is_empty() || c.category_name.to_lowercase().contains(&q)) {
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

    let scope = format!(
        "{}-{:?}-{}-{}-{:?}",
        kind() as u8,
        category(),
        search(),
        refresh(),
        sort()
    );
    let body = match &*library.read() {
        Some(Err(e)) => rsx! { p { class: "err", "{e}" } },
        Some(Ok(lib)) if lib.kind == kind() => {
            let order = order.read();
            let order = order
                .as_deref()
                .filter(|o| o.len() == lib.len())
                .unwrap_or(&[]);
            let (shown, total) = rows(
                lib,
                order,
                &client,
                category(),
                &search().to_lowercase(),
                limit(),
            );
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
            rsx! {
                div {
                    class: "scroll",
                    key: "{scope}",
                    onscroll: move |e| {
                        let d = e.data();
                        let bottom = d.scroll_top() + f64::from(d.client_height());
                        if bottom > f64::from(d.scroll_height()) - 800.0 && limit() < total {
                            limit += SHOW;
                        }
                    },
                    {list}
                }
            }
        }
        // Still loading, or showing the previous section's data: don't pass that off as this one.
        _ => rsx! { div { class: "empty", "Loading your library…" } },
    };

    let player = playing().map(|(title, url)| {
        let (converter, original) = (client.clone(), url.clone());
        rsx! {
            div { class: "overlay",
                div { class: "sheet",
                    div { class: "bar",
                        strong { "{title}" }
                        div {
                            Download { title: title.clone(), url: url.clone() }
                            button { class: "icon-btn", aria_label: "Close", onclick: move |_| playing.set(None), Icon { d: CLOSE } }
                        }
                    }
                    if let Some((converted, from)) = movie_conv() {
                        // Converted for sound: the browser's own bar can't seek in that, ours can.
                        ConvertedPlayer { key: "{converted.at(0)}", converted, from }
                    } else {
                        video {
                            key: "{url}",
                            controls: true,
                            autoplay: true,
                            src: "{url}",
                            ontimeupdate: move |_| {
                                if movie_note().is_none()
                                    && let Some(v) = sheet_video()
                                    && no_audio_decoded(&v)
                                {
                                    // Silent: have the proxy convert it, and carry on from here.
                                    let from = v.current_time() as u64;
                                    movie_note.set(Some("Fixing the sound: converting for your browser…".into()));
                                    let (converter, original) = (converter.clone(), original.clone());
                                    wasm_bindgen_futures::spawn_local(async move {
                                        match converter.convert(&xtream::Url::parse(&original).expect("ours")).await {
                                            Ok(converted) => {
                                                movie_note.set(None);
                                                movie_conv.set(Some((converted, from)));
                                            }
                                            Err(e) => movie_note.set(Some(format!(
                                                "No sound: this title's audio format (often AC-3 or DTS) can't be decoded by your browser. {e}"
                                            ))),
                                        }
                                    });
                                }
                            },
                        }
                        if let Some(note) = movie_note() {
                            p { class: "sound-note inline", "{note}" }
                        }
                    }
                }
            }
        }
    });
    let live_panel = live().map(|(id, title, url)| {
        let player_key = format!("{url}-{}", player_revision());
        rsx! { LivePlayer { key: "{player_key}", id, title, url } }
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
                                    limit.set(SHOW);
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
        header { class: "topbar",
            div { class: "brand", RustMark {} span { "RIPTV" } }
            label { class: "search",
                Icon { d: SEARCH }
                input {
                    aria_label: "Search this section",
                    placeholder: "Search {page_name}",
                    value: "{search}",
                    oninput: move |e| { search.set(e.value()); limit.set(SHOW); }
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
                    button { class: "scrim", aria_label: "Close menu", onclick: move |_| account_open.set(false) }
                    div { class: "menu",
                        button {
                            onclick: move |_| {
                                refresh += 1;
                                player_revision += 1;
                                limit.set(SHOW);
                                account_open.set(false);
                            },
                            "Refresh"
                        }
                        button { onclick: move |_| session.set(None), "Disconnect" }
                    }
                }
            }
        }
        nav { class: "rail", aria_label: "Library",
            button { class: tab(Kind::Live), title: "Live TV", onclick: move |_| pick_kind(Kind::Live), Icon { d: LIVE_TV } span { "Live TV" } }
            button { class: tab(Kind::Movies), title: "Movies", onclick: move |_| pick_kind(Kind::Movies), Icon { d: MOVIE } span { "Movies" } }
            button { class: tab(Kind::Series), title: "Series", onclick: move |_| pick_kind(Kind::Series), Icon { d: SERIES } span { "Series" } }
        }
        main { class: "workspace",
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
                if let Some(row) = open() {
                    DetailPage {
                        key: "{row.key}",
                        row,
                        onback: move |_| open.set(None),
                        onplay: move |t: (String, String)| {
                            movie_note.set(None);
                            movie_conv.set(None);
                            playing.set(Some(t));
                        },
                    }
                }
            }
            {player}
            {cats_sheet}
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

/// A movie or episode the proxy converted to fix its sound. That stream is one continuous piece
/// with no index, so the browser's own bar can't seek in it; this one can: picking a place starts a
/// new conversion from that second.
#[component]
fn ConvertedPlayer(converted: xtream::Converted, from: u64) -> Element {
    let mut start = use_signal(move || from); // the second of the movie where this stream begins
    let mut pos = use_signal(move || from); // the second being shown
    let mut scrubbing = use_signal(|| false);
    let mut paused = use_signal(|| false);
    let mut muted = use_signal(|| false);
    let mut volume = use_signal(|| 100_u32);
    let expanded = use_signal(|| false);
    let src = converted.at(start()).to_string();
    let total = converted.duration;

    // Always a new conversion from there: the browser treats this stream as unseekable, even the
    // part it has already loaded, so moving the playhead in place does nothing.
    let mut seek = move |t: u64| {
        pos.set(t);
        start.set(t);
    };

    rsx! {
        div { class: if expanded() { "vod fill" } else { "vod" }, id: "vod-player",
            video {
                key: "{src}",
                autoplay: true,
                src: "{src}",
                onplay: move |_| paused.set(false),
                onpause: move |_| paused.set(true),
                onclick: move |_| {
                    if let Some(v) = sheet_video() {
                        toggle(&v);
                    }
                },
                ontimeupdate: move |_| {
                    if !scrubbing()
                        && let Some(v) = sheet_video()
                    {
                        pos.set(start() + v.current_time() as u64);
                    }
                },
            }
            div { class: "controls",
                button {
                    class: "ctl",
                    aria_label: "Play or pause",
                    onclick: move |_| {
                        if let Some(v) = sheet_video() {
                            toggle(&v);
                        }
                    },
                    Icon { d: if paused() { PLAY } else { PAUSE } }
                }
                div { class: "volume",
                    button {
                        class: "ctl",
                        aria_label: "Mute",
                        onclick: move |_| {
                            if let Some(v) = sheet_video() {
                                v.set_muted(!v.muted());
                                muted.set(v.muted());
                            }
                        },
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
                            if let Some(v) = sheet_video() {
                                v.set_volume(f64::from(level) / 100.0);
                                v.set_muted(level == 0);
                            }
                        }
                    }
                }
                span { class: "time",
                    "{hms(pos())}"
                    if let Some(total) = total { " / {hms(total)}" }
                }
                if let Some(total) = total {
                    input {
                        class: "scrub",
                        r#type: "range",
                        min: "0",
                        max: "{total}",
                        aria_label: "Seek",
                        value: "{pos}",
                        oninput: move |e| {
                            scrubbing.set(true);
                            pos.set(e.value().parse().unwrap_or(0));
                        },
                        onchange: move |e| {
                            scrubbing.set(false);
                            seek(e.value().parse().unwrap_or(0));
                        }
                    }
                } else {
                    span { class: "grow" }
                }
                button {
                    class: "ctl",
                    aria_label: "Fullscreen",
                    onclick: move |_| toggle_fullscreen("vod-player", expanded),
                    Icon { d: FULLSCREEN }
                }
            }
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
                if let Some(src) = row.icon.as_deref().filter(|s| s.starts_with("http")) {
                    img { src: "{src}", loading: "lazy" }
                } else { "TV" }
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
    rsx! {
        button {
            class: "card",
            title: "{row.title}",
            onclick: move |_| onpick.call(r.clone()),
            span { class: "art",
                if let Some(src) = row.icon.as_deref().filter(|s| s.starts_with("http")) {
                    img { src: "{src}", loading: "lazy" }
                } else { "{fallback}" }
                if let Some(s) = &row.score { span { class: "score", "{s}" } }
            }
            strong { "{row.title}" }
        }
    }
}

/// The page for one movie or series: backdrop, poster, facts, and either Play or the episodes.
#[component]
fn DetailPage(
    row: Row,
    onback: EventHandler<()>,
    onplay: EventHandler<(String, String)>,
) -> Element {
    let session = use_context::<Signal<Option<Client>>>();
    let client = use_hook(|| session.read().clone().expect("logged in"));
    let mut expanded = use_signal(|| false);
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
    let backdrop = details.backdrop.clone().or_else(|| poster.clone());
    let genres = details.genre.as_ref().map(|g| {
        g.split(',')
            .take(2)
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(", ")
    });
    let chips: Vec<String> = [
        details.year.clone(),
        genres,
        details.duration.clone(),
        details.country.clone(),
        score(&details.rating).map(|r| format!("★ {r}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    let long = details
        .plot
        .as_ref()
        .is_some_and(|p| p.chars().count() > 280);
    let movie_url = match &row.target {
        Target::Movie { url, .. } => Some(url.clone()),
        _ => None,
    };

    rsx! {
        section { class: "detail",
            if let Some(src) = backdrop.as_deref().filter(|s| s.starts_with("http")) { img { class: "backdrop", src: "{src}" } }
            button { class: "icon-btn back", aria_label: "Back", onclick: move |_| onback.call(()), Icon { d: BACK } }
            div { class: "hero",
                if let Some(src) = poster.as_deref().filter(|s| s.starts_with("http")) { img { class: "poster-lg", src: "{src}" } }
                div { class: "info",
                    h1 { "{title}" }
                    if !chips.is_empty() {
                        div { class: "chips", for c in chips { span { class: "chip", key: "{c}", "{c}" } } }
                    }
                    if let Some(plot) = &details.plot {
                        p { class: if expanded() || !long { "plot" } else { "plot clamp" }, "{plot}" }
                        if long {
                            button { class: "more", onclick: move |_| expanded.set(!expanded()), if expanded() { "LESS" } else { "MORE" } }
                        }
                    }
                    match &*guard {
                        None => rsx! { p { class: "dim", "Loading…" } },
                        Some(Err(e)) => rsx! { p { class: "err", "{e}" } },
                        Some(Ok(_)) => rsx! {},
                    }
                    div { class: "facts",
                        if let Some(cast) = &details.cast { div { small { "Actors" } strong { "{cast}" } } }
                        if let Some(director) = &details.director { div { small { "Director" } strong { "{director}" } } }
                    }
                    if let Some(url) = movie_url {
                        div { class: "actions",
                            button {
                                class: "play",
                                onclick: {
                                    let (title, url) = (title.clone(), url.clone());
                                    move |_| onplay.call((title.clone(), url.clone()))
                                },
                                Icon { d: PLAY } "Play"
                            }
                            Download { title: title.clone(), url: url.clone() }
                        }
                    }
                }
            }
            if let Some(Ok((_, seasons))) = &*guard {
                div { class: "episodes",
                    for season in seasons.iter() {
                        h3 { key: "s{season.number}", "Season {season.number}" }
                        for (n, ep) in season.episodes.iter().enumerate() {
                            {
                                let num = ep.episode_num.unwrap_or(n as u64 + 1);
                                let ext = ep.container_extension.as_deref().filter(|e| !e.is_empty()).unwrap_or("mp4");
                                let url = client.episode_url(ep.id, ext).to_string();
                                let name = format!("{title} S{}E{num}: {}", season.number, ep.title);
                                rsx! {
                                    div { key: "{ep.id}", class: "ep",
                                        button {
                                            onclick: { let (name, url) = (name.clone(), url.clone()); move |_| onplay.call((name.clone(), url.clone())) },
                                            if ep.title.to_uppercase().contains(&format!("E{num:02}")) {
                                                "{ep.title}"
                                            } else {
                                                "{num}. {ep.title}"
                                            }
                                        }
                                        Download { title: name.clone(), url: url.clone() }
                                    }
                                }
                            }
                        }
                    }
                }
            }
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
    Native,
    Partial,
    Converted(String),
}

#[component]
fn LivePlayer(id: u64, title: String, url: String) -> Element {
    let session = use_context::<Signal<Option<Client>>>();
    let client = use_hook(|| session.read().clone().expect("logged in"));
    let mut status = use_signal(|| "Connecting…".to_string());
    let mut paused = use_signal(|| false);
    let mut muted = use_signal(|| false);
    let mut volume = use_signal(|| 100_u32);
    let mut buffering = use_signal(|| false);
    let mut expanded = use_signal(|| false);
    // Why there is no sound, when the player knows: its own note, or what the browser reports.
    let mut note = use_signal(|| None::<String>);
    let mut silent = use_signal(|| false);
    // The controls fade out after a moment of no mouse movement while playing.
    let mut active = use_signal(|| true);
    let mut activity = use_signal(|| 0_u32);
    let mut wake = move || {
        activity += 1;
        active.set(true);
        let mine = activity();
        spawn(async move {
            rffmpeg::mse::sleep(Duration::from_millis(2500)).await;
            if activity() == mine {
                active.set(false);
            }
        });
    };
    let handle = use_hook(|| Rc::new(RefCell::new(None::<rffmpeg::mse::Player>)));

    let mut feed = use_signal(|| Feed::Native);
    // A readout for telling a slow stream from a slow decoder: what the picture really is,
    // frames per second actually shown, frames dropped, and how much is buffered.
    let mut show_stats = use_signal(|| false);
    let mut stats = use_signal(String::new);
    use_future(move || async move {
        let mut last = (0_u32, js_sys::Date::now());
        loop {
            rffmpeg::mse::sleep(Duration::from_secs(1)).await;
            let Some(v) = video_el().filter(|_| show_stats()) else {
                continue;
            };
            let quality = v.get_video_playback_quality();
            let (frames, now) = (quality.total_video_frames(), js_sys::Date::now());
            let fps = (f64::from(frames.saturating_sub(last.0)) * 1000.0 / (now - last.1).max(1.0))
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
                Feed::Native => "Rust player",
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
    use_effect(move || {
        let Some(video) = video_el() else {
            status.set("Could not start the player".into());
            return;
        };
        let partial = match feed() {
            Feed::Converted(src) => {
                // Stop the Rust player and let the plain `<video>` play the converted stream.
                handle.borrow_mut().take();
                note.set(None);
                video.set_src(&src);
                let _ = video.play();
                return;
            }
            Feed::Partial => true,
            Feed::Native => false,
        };
        let Ok(playlist) = xtream::Url::parse(&url) else {
            status.set("Could not start the player".into());
            return;
        };
        let (c, converter, media) = (client.clone(), client.clone(), playlist.clone());
        *handle.borrow_mut() = Some(rffmpeg::mse::start(
            video,
            playlist,
            move |u| c.proxied(u),
            partial,
            move |s| match s {
                rffmpeg::mse::Status::Playing => status.set("Live".into()),
                rffmpeg::mse::Status::Note(n) => note.set(Some(n)),
                rffmpeg::mse::Status::NeedsConversion(reason) => {
                    status.set("Converting for your browser…".into());
                    let (converter, media) = (converter.clone(), media.clone());
                    // Not Dioxus's `spawn`: this callback runs outside its runtime.
                    wasm_bindgen_futures::spawn_local(async move {
                        match converter.convert(&media).await {
                            Ok(converted) => feed.set(Feed::Converted(converted.at(0).to_string())),
                            // No ffmpeg (or it can't read the stream): a sound problem still
                            // plays, without sound, and says why; anything else can't play.
                            Err(e) if reason.contains("sound") => {
                                note.set(Some(format!("{reason} ({e})")));
                                feed.set(Feed::Partial);
                            }
                            Err(e) => {
                                status.set(format!("Can't play this channel: {reason} ({e})"))
                            }
                        }
                    });
                }
                rffmpeg::mse::Status::Ended => status.set("Stream ended".into()),
                rffmpeg::mse::Status::Failed(e) => status.set(format!("Playback failed: {e}")),
            },
        ));
    });

    // Space, F and M. A plain browser listener: Dioxus's own keyboard events add ~26 KB of wasm.
    use_effect(move || {
        let player = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("live-player"));
        let Some(player) = player else { return };
        let on_key =
            Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
                match e.key().to_ascii_lowercase().as_str() {
                    " " => {
                        e.prevent_default();
                        toggle_play();
                    }
                    "f" => toggle_fullscreen("live-player", expanded),
                    "i" => show_stats.set(!show_stats()),
                    "escape" => expanded.set(false),
                    "m" => muted.set(toggle_mute().unwrap_or(false)),
                    _ => {}
                }
            });
        let _ = player.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
        // ponytail: leaks one small closure per channel opened; the element it listens on goes away.
        on_key.forget();
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
                onmousemove: move |_| wake(),
                video {
                    id: "live-video",
                    autoplay: true,
                    onplay: move |_| paused.set(false),
                    onpause: move |_| paused.set(true),
                    onwaiting: move |_| buffering.set(true),
                    onplaying: move |_| {
                        buffering.set(false);
                        paused.set(false);
                        if matches!(feed(), Feed::Converted(_)) {
                            status.set("Live".into());
                        }
                    },
                    onerror: move |_| {
                        if matches!(feed(), Feed::Converted(_)) {
                            status.set("The converted stream stopped: the source may have ended".into());
                        }
                    },
                    onvolumechange: move |_| {
                        if let Some(v) = video_el()
                            && muted() != v.muted()
                        {
                            muted.set(v.muted());
                        }
                    },
                    ontimeupdate: move |_| {
                        if !silent()
                            && let Some(v) = video_el()
                            && no_audio_decoded(&v)
                        {
                            silent.set(true);
                        }
                    },
                    onclick: move |_| toggle_play(),
                    ondoubleclick: move |_| toggle_fullscreen("live-player", expanded),
                }
                if show_stats() {
                    div { class: "stats", "{stats}" }
                }
                if let Some(why) = note() {
                    div { class: "sound-note", "No sound: {why}" }
                } else if silent() {
                    div { class: "sound-note", "No sound: this stream's audio format can't be decoded by your browser" }
                }
                if status() != "Live" { div { class: "hud", span { "{status}" } } }
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
                            onclick: move |_| { muted.set(toggle_mute().unwrap_or(false)); },
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
                    button { class: "ctl", aria_label: "Picture in picture", title: "Picture in picture", onclick: move |_| toggle_pip(), Icon { d: PIP } }
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
            rffmpeg::mse::sleep(Duration::from_secs(30)).await;
            tick += 1;
        }
    });
    let table = use_resource(move || {
        let c = client.clone();
        async move {
            match c.epg_table(id).await {
                Ok(t) if t.iter().any(|e| e.start_ts.is_some()) => Ok(t),
                _ => c.short_epg(id, 8).await,
            }
        }
    });
    // Once the schedule arrives, start with the current programme in view. The timeline isn't
    // laid out the moment the data lands, so wait a moment.
    use_effect(move || {
        if table.read().is_some() {
            spawn(async move {
                rffmpeg::mse::sleep(Duration::from_millis(60)).await;
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
            let mut slots: Vec<(u64, u64, &EpgListing)> = t
                .iter()
                .filter_map(|l| {
                    let (s, e) = (l.start_ts?, l.end_ts?);
                    (e > s).then_some((s, e, l))
                })
                .collect();
            slots.sort_by_key(|s| s.0);
            current = slots
                .iter()
                .find(|(s, e, _)| *s <= now && now < *e)
                .map(|(_, _, l)| l.title.clone());
            if slots.is_empty() {
                rsx! { p { class: "dim", "This provider has no guide for the channel." } }
            } else {
                // From up to eight hours back to a day ahead, on whole hours.
                let lo = slots[0].0.max(now.saturating_sub(8 * 3600)) / 3600 * 3600;
                let last = slots.last().map_or(now, |s| s.1);
                let hi = last.min(now + 24 * 3600).div_ceil(3600) * 3600;
                let px = |t: u64| t.clamp(lo, hi).saturating_sub(lo) / SECS_PER_PX;
                let (width, here) = (px(hi), px(now));
                rsx! {
                    div {
                        class: "timeline",
                        id: "timeline",
                        div { class: "tl", style: "width:{width}px", "data-now": "{here}",
                            for hour in (lo..hi).step_by(3600) {
                                span { class: "tick", key: "{hour}", style: "left:{px(hour)}px", "{clock(hour)}" }
                            }
                            for (start, end, l) in slots.into_iter().filter(|(s, e, _)| *e > lo && *s < hi) {
                                div {
                                    key: "{start}",
                                    class: if start <= now && now < end { "block now" } else { "block" },
                                    style: "left:{px(start)}px;width:{px(end).saturating_sub(px(start)).max(3)}px",
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
