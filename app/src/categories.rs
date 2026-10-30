//! Category browsing and per-profile pins. No extra provider requests or stream URLs.
use super::*;

pub(crate) const CSS: &str = r#"
.category-item{position:relative;display:flex;align-items:center;border-radius:10px}.category-item:hover{background:var(--row)}.category-item.selected{background:var(--soft)}
.category-item.is-pinned:not(.selected){background:color-mix(in srgb,var(--accent) 5%,transparent)}
.category-item{min-height:3.5rem;gap:.2rem}.category-item .cat{flex:1;min-width:0;min-height:3.5rem;padding:.55rem .35rem .55rem .7rem;text-align:left}.category-item .cat span{display:-webkit-box;overflow:hidden;white-space:normal;overflow-wrap:anywhere;-webkit-box-orient:vertical;-webkit-line-clamp:2}.category-item .cat:hover,.category-item .cat.on{background:transparent}
.category-pin{position:static;display:grid;place-items:center;flex:none;width:2.75rem;height:2.75rem;border-radius:9px;color:var(--dim);opacity:.6;transition:opacity .15s,background .15s}
.category-pin svg{width:1.2rem;height:1.2rem}.category-pin:hover{background:rgba(255,255,255,.08);color:var(--text)}
.category-pin.pinned{color:var(--accent);opacity:1}.category-item:hover .category-pin,.category-item:focus-within .category-pin{opacity:1}
.category-all{min-height:3.5rem;padding-left:.7rem}.category-note{display:grid;gap:.55rem;padding:.75rem .7rem;color:var(--dim);font-size:.8rem}
.category-list{display:flex;flex:1;flex-direction:column;min-height:0}.category-list>.category-filter,.category-list>.category-all{flex:none}.category-items{flex:1;min-height:0;overflow-y:auto;overscroll-behavior:contain;scrollbar-width:thin}
aside.sidebar:has(.category-list){display:flex;flex-direction:column;overflow:hidden}
@media(max-width:820px){aside.sidebar:has(.category-list){display:none}}
.category-dialog .sheet-body:has(.category-list){display:flex;flex-direction:column;min-height:0;overflow:hidden}
.category-dialog .category-list,.sidebar .category-list{min-height:0}
.category-dialog .category-item .category-pin{position:static;width:2.75rem;min-width:2.75rem;height:2.75rem;opacity:.8}
.category-dialog .category-item .cat,.category-dialog .cat.category-all{min-height:3.5rem}
.category-filter{position:relative;display:flex;align-items:center;height:2.5rem;margin:0 0 .5rem;border:1px solid transparent;border-radius:10px;background:var(--row);transition:border-color .2s,background .2s,box-shadow .2s}
.category-filter:focus-within{border-color:color-mix(in srgb,var(--accent) 40%,transparent);background:color-mix(in srgb,var(--accent) 4%,var(--row));box-shadow:0 0 0 2px color-mix(in srgb,var(--accent) 12%,transparent)}
.category-filter>svg{position:absolute;left:.75rem;width:.95rem;height:.95rem;color:var(--dim);pointer-events:none}
.category-filter input{width:100%;height:100%;padding:0 2.55rem 0 2.2rem;border:0;outline:0;background:transparent;font-size:.85rem}
.category-filter input::placeholder{color:var(--faint)}
.category-filter button{position:absolute;right:.3rem;display:grid;place-items:center;width:1.9rem;height:1.9rem;border-radius:7px;color:var(--dim);transition:background .15s,color .15s}
.category-filter button:hover{background:var(--soft);color:var(--text)}.category-filter button svg{width:.85rem;height:.85rem}
.category-filter button[aria-hidden="true"]{visibility:hidden;pointer-events:none}
.category-note button{justify-self:start;padding:.38rem .65rem;border-radius:8px;background:var(--soft);color:var(--text);font-size:.78rem;font-weight:600}
.category-note button:hover{background:color-mix(in srgb,var(--accent) 16%,var(--soft))}
@media(hover:none){.category-pin{opacity:.8}}
"#;

/// Compare immutable catalogue snapshots by identity, not by walking thousands of titles.
#[derive(Clone)]
pub(crate) struct CategoryData {
    pub list: Rc<Vec<xtream::Category>>,
}

impl PartialEq for CategoryData {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.list, &other.list)
    }
}

#[component]
pub(crate) fn CategoryList(
    data: CategoryData,
    selected: Option<u64>,
    pins: Vec<u64>,
    query: Signal<String>,
    #[props(default)] in_dialog: bool,
    onselect: EventHandler<Option<u64>>,
    onpin: EventHandler<u64>,
) -> Element {
    let CategoryData { list } = data;
    let mut query = query;
    let q = query().to_lowercase();
    let filter_id = if in_dialog {
        "category-dialog-filter"
    } else {
        "category-sidebar-filter"
    };
    let pins: std::collections::HashSet<_> = pins.into_iter().collect();
    let matches =
        |c: &&xtream::Category| q.is_empty() || xtream::contains_lowercase(&c.category_name, &q);
    let has_matches = q.is_empty()
        || list
            .iter()
            .any(|c| xtream::contains_lowercase(&c.category_name, &q));
    rsx! {
        div { class: "category-list",
            div { class: "category-filter",
                Icon { d: SEARCH }
                input {
                    id: "{filter_id}",
                    aria_label: "Filter categories", placeholder: "Filter categories",
                    value: "{query}", oninput: move |e| query.set(e.value())
                }
                button {
                    aria_label: "Clear category filter", aria_hidden: q.is_empty(),
                    tabindex: if q.is_empty() { -1 } else { 0 }, onclick: move |_| {
                        crate::controls::focus(filter_id);
                        query.set(String::new());
                    },
                    Icon { d: CLOSE }
                }
            }
            button {
                class: if selected.is_none() { "cat category-all on" } else { "cat category-all" },
                aria_pressed: selected.is_none(),
                onclick: move |_| onselect.call(None), span { "All" }
            }
            div { class: "category-items",
                // Pinned first, preserving provider order within both groups and each row's identity.
                for c in list.iter().filter(matches).filter(|c| pins.contains(&c.category_id))
                    .chain(list.iter().filter(matches).filter(|c| !pins.contains(&c.category_id))) {
                    CategoryItem { key: "category-{c.category_id}", category: c.clone(),
                        selected: selected == Some(c.category_id), pinned: pins.contains(&c.category_id), onselect, onpin }
                }
                if !q.is_empty() && !has_matches {
                    div { class: "category-note",
                        span { "No categories match ‘{query}’." }
                        button { aria_label: "Clear category filter", onclick: move |_| {
                            crate::controls::focus(filter_id);
                            query.set(String::new());
                        }, "Clear filter" }
                    }
                }
            }
        }
    }
}

#[component]
fn CategoryItem(
    category: xtream::Category,
    selected: bool,
    pinned: bool,
    onselect: EventHandler<Option<u64>>,
    onpin: EventHandler<u64>,
) -> Element {
    let id = category.category_id;
    let action = if pinned { "Unpin" } else { "Pin" };
    rsx! {
        div { class: format!("category-item{}{}", if selected { " selected" } else { "" }, if pinned { " is-pinned" } else { "" }),
            button {
                class: if selected { "cat on" } else { "cat" },
                aria_pressed: selected,
                title: "{category.category_name}", onclick: move |_| onselect.call(Some(id)),
                span { "{category.category_name}" }
            }
            button {
                class: if pinned { "category-pin pinned" } else { "category-pin" },
                aria_label: "{action} {category.category_name}", title: "{action} category",
                aria_pressed: pinned, onclick: move |_| onpin.call(id), Icon { d: PIN }
            }
        }
    }
}
