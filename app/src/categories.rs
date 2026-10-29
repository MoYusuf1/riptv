//! Category browsing and per-profile pins. No extra provider requests or stream URLs.
use super::*;

pub(crate) const CSS: &str = r#"
.category-item{position:relative;display:flex;align-items:center;border-radius:10px}.category-item:hover{background:var(--row)}.category-item.selected{background:var(--soft)}
.category-item.is-pinned:not(.selected){background:color-mix(in srgb,var(--accent) 5%,transparent)}
.category-item .cat{flex:1;min-width:0;padding-left:2.1rem}.category-item .cat:hover,.category-item .cat.on{background:transparent}
.category-pin{position:absolute;left:.3rem;display:grid;place-items:center;width:1.5rem;height:1.6rem;border-radius:7px;color:var(--dim);opacity:0;transition:opacity .15s,background .15s}
.category-pin svg{width:.9rem;height:.9rem}.category-pin:hover{background:rgba(255,255,255,.08);color:var(--text)}
.category-pin.pinned{color:var(--accent);opacity:1}.category-item:hover .category-pin,.category-item:focus-within .category-pin{opacity:1}
.category-all{padding-left:2.1rem}.category-note{display:grid;gap:.55rem;padding:.75rem .7rem;color:var(--dim);font-size:.8rem}
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
@media(hover:none){.category-pin{opacity:1}}
"#;

/// Compare immutable catalogue snapshots by identity, not by walking thousands of titles.
#[derive(Clone)]
pub(crate) struct CategoryData {
    pub list: Rc<Vec<xtream::Category>>,
    pub library: Option<Rc<Library>>,
}

impl PartialEq for CategoryData {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.list, &other.list)
            && match (&self.library, &other.library) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}

#[component]
pub(crate) fn CategoryList(
    data: CategoryData,
    selected: Option<u64>,
    pins: Vec<u64>,
    query: Signal<String>,
    onselect: EventHandler<Option<u64>>,
    onpin: EventHandler<u64>,
) -> Element {
    let CategoryData { list, library } = data;
    let mut query = query;
    let q = query().to_lowercase();
    let pins: std::collections::HashSet<_> = pins.into_iter().collect();
    let matches =
        |c: &&xtream::Category| q.is_empty() || xtream::contains_lowercase(&c.category_name, &q);
    let has_matches = list
        .iter()
        .any(|c| xtream::contains_lowercase(&c.category_name, &q));
    let count = |id: u64| {
        library
            .as_ref()
            .and_then(|l| l.counts.get(&id))
            .copied()
            .unwrap_or(0)
    };
    let total = library.as_ref().map_or(0, |l| l.len());
    rsx! {
        div { class: "category-filter",
            Icon { d: SEARCH }
            input {
                aria_label: "Filter categories", placeholder: "Filter categories",
                value: "{query}", oninput: move |e| query.set(e.value())
            }
            button {
                aria_label: "Clear category filter", aria_hidden: q.is_empty(),
                tabindex: if q.is_empty() { -1 } else { 0 }, onclick: move |_| query.set(String::new()),
                Icon { d: CLOSE }
            }
        }
        button {
            class: if selected.is_none() { "cat category-all on" } else { "cat category-all" },
            aria_pressed: selected.is_none(),
            onclick: move |_| onselect.call(None), span { "All" } small { "{thousands(total)}" }
        }
        // Pinned first, preserving provider order within both groups and each row's identity.
        for c in list.iter().filter(matches).filter(|c| pins.contains(&c.category_id))
            .chain(list.iter().filter(matches).filter(|c| !pins.contains(&c.category_id))) {
            CategoryItem { key: "category-{c.category_id}", category: c.clone(), count: count(c.category_id),
                selected: selected == Some(c.category_id), pinned: pins.contains(&c.category_id), onselect, onpin }
        }
        if !q.is_empty() && !has_matches {
            div { class: "category-note",
                span { "No categories match ‘{query}’." }
                button { aria_label: "Clear category filter", onclick: move |_| query.set(String::new()), "Clear filter" }
            }
        }
    }
}

#[component]
fn CategoryItem(
    category: xtream::Category,
    count: usize,
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
                span { "{category.category_name}" } small { "{thousands(count)}" }
            }
            button {
                class: if pinned { "category-pin pinned" } else { "category-pin" },
                aria_label: "{action} {category.category_name}", title: "{action} category",
                aria_pressed: pinned, onclick: move |_| onpin.call(id), Icon { d: PIN }
            }
        }
    }
}
