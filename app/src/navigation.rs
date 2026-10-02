//! Section navigation stays outside the top bar; labels work with keyboard and touch.
use super::*;

pub(crate) const CSS: &str = r#"
.rail{position:fixed;z-index:25;top:50%;left:.7rem;display:flex;flex-direction:column;gap:.35rem;width:4.2rem;padding:.35rem;border:1px solid var(--hair);border-radius:20px;background:linear-gradient(150deg,#21161c,#140e12);box-shadow:0 12px 34px rgba(0,0,0,.25);transform:translateY(-50%)}
.rail button{display:flex;flex-direction:column;align-items:center;justify-content:center;gap:.35rem;height:3.8rem;padding:.4rem .2rem;border-radius:14px;color:var(--dim);transition:background .18s,color .18s}
.rail button:hover{background:rgba(255,255,255,.04);color:var(--text)}
.rail button.on{background:var(--soft);color:var(--accent);box-shadow:inset 0 0 0 1px rgba(255,125,146,.12)}
.rail svg{width:1.35rem;height:1.35rem}.rail span{font-size:.62rem;font-weight:600;white-space:nowrap}
@media(max-width:820px){.rail{top:auto;bottom:.8rem;left:50%;width:auto;flex-direction:row;transform:translateX(-50%);background:rgba(24,16,20,.95);backdrop-filter:blur(24px)}.rail button{min-width:4.6rem;height:3.2rem;gap:.2rem;padding:0 .8rem}.topbar{height:3.3rem;gap:.6rem}.topbar .brand{display:grid;width:2.2rem;height:2.2rem;border-radius:11px}.topbar .rust-mark{width:1.6rem;height:1.6rem}.search input{height:2.3rem}.workspace{inset:4.6rem 0 0}}
"#;

#[component]
pub(crate) fn LibraryNav(kind: Kind, playlist: bool, onpick: EventHandler<Kind>) -> Element {
    let class = |k| if kind == k { "on" } else { "" };
    let current = |k| if kind == k { "page" } else { "false" };
    rsx! {
        nav { class: "rail", aria_label: "Library",
            button { class: class(Kind::Live), title: "Live TV", aria_current: current(Kind::Live),
                onclick: move |_| onpick.call(Kind::Live), Icon { d: LIVE_TV } span { "Live TV" } }
            if !playlist {
                button { class: class(Kind::Movies), title: "Movies", aria_current: current(Kind::Movies),
                    onclick: move |_| onpick.call(Kind::Movies), Icon { d: MOVIE } span { "Movies" } }
                button { class: class(Kind::Series), title: "TV Shows", aria_current: current(Kind::Series),
                    onclick: move |_| onpick.call(Kind::Series), Icon { d: SERIES } span { "TV Shows" } }
            }
        }
    }
}
