//! Category browsing and per-profile pins. No extra provider requests or stream URLs.
use super::*;

pub(crate) const CSS: &str = r#"
.category-item{position:relative;display:flex;align-items:center;border-radius:10px}.category-item:hover{background:var(--row)}.category-item.selected{background:var(--soft)}
.category-item.is-pinned:not(.selected){background:color-mix(in srgb,var(--accent) 5%,transparent)}
.category-item .cat{flex:1;min-width:0;padding-left:2.1rem}.category-item .cat:hover,.category-item .cat.on{background:transparent}
.category-pin{position:absolute;left:.3rem;display:grid;place-items:center;width:1.5rem;height:1.6rem;border-radius:7px;color:var(--dim);opacity:0;transition:opacity .15s,background .15s}
.category-pin svg{width:.9rem;height:.9rem}.category-pin:hover{background:rgba(255,255,255,.08);color:var(--text)}
.category-pin.pinned{color:var(--accent);opacity:1}.category-item:hover .category-pin,.category-item:focus-within .category-pin{opacity:1}
.category-all{padding-left:2.1rem}.category-note{padding:.5rem .7rem;color:var(--dim);font-size:.8rem}
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
    let matches =
        |c: &&xtream::Category| q.is_empty() || xtream::contains_lowercase(&c.category_name, &q);
    let count = |id: u64| {
        library
            .as_ref()
            .and_then(|l| l.counts.get(&id))
            .copied()
            .unwrap_or(0)
    };
    let total = library.as_ref().map_or(0, |l| l.len());
    rsx! {
        input {
            class: "filter", aria_label: "Filter categories", placeholder: "Filter categories",
            value: "{query}", oninput: move |e| query.set(e.value())
        }
        button {
            class: if selected.is_none() { "cat category-all on" } else { "cat category-all" },
            onclick: move |_| onselect.call(None), span { "All" } small { "{thousands(total)}" }
        }
        // Pinned first, preserving provider order within both groups and each row's identity.
        for c in list.iter().filter(matches).filter(|c| pins.contains(&c.category_id))
            .chain(list.iter().filter(matches).filter(|c| !pins.contains(&c.category_id))) {
            CategoryItem { key: "category-{c.category_id}", category: c.clone(), count: count(c.category_id),
                selected: selected == Some(c.category_id), pinned: pins.contains(&c.category_id), onselect, onpin }
        }
        if !q.is_empty() && !list.iter().any(|c| xtream::contains_lowercase(&c.category_name, &q)) {
            p { class: "category-note", "No matching categories" }
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
