//! Section navigation stays outside the top bar; labels work with keyboard and touch.
use super::*;

pub(crate) const CSS: &str = r#"
.rail{position:fixed;z-index:25;top:50%;left:.7rem;display:flex;flex-direction:column;gap:.35rem;width:4.2rem;padding:.35rem;border:1px solid var(--hair);border-radius:20px;background:linear-gradient(150deg,#21161c,#140e12);box-shadow:0 12px 34px rgba(0,0,0,.25);transform:translateY(-50%)}
.rail button{display:flex;flex-direction:column;align-items:center;justify-content:center;gap:.35rem;height:3.8rem;padding:.4rem .2rem;border-radius:14px;color:var(--dim);transition:background .18s,color .18s}
.rail button:hover{background:rgba(255,255,255,.04);color:var(--text)}
.rail button.on{background:var(--soft);color:var(--accent);box-shadow:inset 0 0 0 1px rgba(255,125,146,.12)}
.rail svg{width:1.35rem;height:1.35rem}.rail span{font-size:.62rem;font-weight:600;white-space:nowrap}
@media(max-width:820px){
  .topbar:not(.detail-mode){top:max(.6rem,env(safe-area-inset-top));right:max(.7rem,env(safe-area-inset-right));left:max(.7rem,env(safe-area-inset-left));display:grid;grid-template-columns:1fr auto;grid-template-rows:2.5rem auto;align-items:center;gap:.45rem .6rem;height:auto;padding:0;border:0;border-radius:0;background:transparent;box-shadow:none;backdrop-filter:none}
  .topbar:not(.detail-mode) .brand{display:grid;grid-column:1;grid-row:1;place-items:center;width:2.25rem;height:2.25rem;border:0;border-radius:0;background:transparent;box-shadow:none}
  .topbar:not(.detail-mode) .rust-mark{width:1.75rem;height:1.75rem}
  .topbar:not(.detail-mode) .search{grid-column:1/-1;grid-row:2;flex:none;width:100%;min-width:0;max-width:none;margin:0}
  .topbar:not(.detail-mode) .search input{height:2.55rem}
  .topbar:not(.detail-mode) .account{grid-column:2;grid-row:1;justify-self:end;flex:none}
  .topbar:not(.detail-mode) .who{height:2.5rem;padding:.2rem}
  .topbar:not(.detail-mode) .who i{width:2rem;height:2rem}
  .topbar.detail-mode{top:max(.6rem,env(safe-area-inset-top));right:max(.6rem,env(safe-area-inset-right));left:auto}
  .detail-return{top:max(.6rem,env(safe-area-inset-top));left:max(.6rem,env(safe-area-inset-left))}
  nav.rail{top:auto;right:auto;bottom:max(.65rem,env(safe-area-inset-bottom));left:50%;display:flex;justify-content:stretch;width:max-content;max-width:calc(100vw - 1.25rem);height:auto;flex-direction:row;gap:.15rem;padding:.25rem;transform:translateX(-50%);border:1px solid var(--hair);border-radius:999px;background:rgba(19,22,28,.96);box-shadow:0 8px 24px rgba(0,0,0,.3);backdrop-filter:none}
  nav.rail button{flex:1;min-width:4.3rem;min-height:2.75rem;height:auto;gap:.12rem;padding:.3rem .65rem;border:0;border-radius:999px;background:transparent}
  nav.rail button.on{background:var(--soft);box-shadow:inset 0 0 0 1px rgba(168,199,250,.16)}
  nav.rail svg{display:block;width:1.15rem;height:1.15rem}nav.rail span{font-size:.68rem;font-weight:600}
  body .workspace{inset:calc(max(.6rem,env(safe-area-inset-top)) + 6.1rem) 0 calc(5rem + env(safe-area-inset-bottom)) 0;grid-template-columns:1fr;gap:0;padding:0 .6rem .5rem}
  .scroll,.episodes{padding-bottom:1rem}
  .channels .scroll{padding-bottom:1rem}
  .pager{padding-bottom:.5rem}
  .head{align-items:flex-start;flex-wrap:wrap;gap:.45rem}
  .head h1{flex:1;min-width:0;max-width:100%;overflow:hidden;display:-webkit-box;-webkit-box-orient:vertical;-webkit-line-clamp:2;overflow-wrap:anywhere;font-size:1.6rem;font-weight:680;line-height:1.15}
  .head .tools{flex:0 1 auto;flex-wrap:wrap;gap:.25rem;min-width:0}
  .sort-btn,.cats-btn{min-height:2.5rem;max-width:9rem;padding:.4rem .55rem;font-size:.75rem;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
}
@media(max-width:360px){
  .topbar:not(.detail-mode){gap:.35rem .45rem}
  .topbar:not(.detail-mode) .search input{height:2.4rem;padding-right:2.4rem;padding-left:2.3rem}
  .topbar:not(.detail-mode) .search svg{left:.75rem}
  .topbar:not(.detail-mode) .who i{width:1.9rem;height:1.9rem}
  body .workspace{padding-inline:.35rem}
  .head{gap:.35rem}
  .head h1{font-size:1.4rem}
  .head .tools{justify-content:flex-end}
  .sort-btn,.cats-btn{min-height:2.5rem;max-width:8.5rem;padding:.38rem .45rem;font-size:.7rem}
}
"#;

#[component]
pub(crate) fn LibraryNav(
    kind: Kind,
    playlist: bool,
    onpick: EventHandler<Kind>,
) -> Element {
    let class = |k| if kind == k { "on" } else { "" };
    let current = |k| if kind == k { "page" } else { "false" };
    rsx! {
        nav { class: "rail", aria_label: "Library",
            button { class: class(Kind::Live), title: "Live", aria_current: current(Kind::Live),
                onclick: move |_| onpick.call(Kind::Live), Icon { d: LIVE_TV } span { "Live" } }
            if !playlist {
                button { class: class(Kind::Movies), title: "Movies", aria_current: current(Kind::Movies),
                    onclick: move |_| onpick.call(Kind::Movies), Icon { d: MOVIE } span { "Movies" } }
                button { class: class(Kind::Series), title: "Shows", aria_current: current(Kind::Series),
                    onclick: move |_| onpick.call(Kind::Series), Icon { d: SERIES } span { "Shows" } }
            }
        }
    }
}
