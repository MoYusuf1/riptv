//! Category browsing and per-profile pins. No extra provider requests or stream URLs.
use super::*;

pub(crate) const CSS: &str = r#"
.category-item{display:flex;align-items:center;border-radius:10px}.category-item:hover{background:var(--row)}.category-item.selected{background:var(--soft)}
.category-item .cat{flex:1;min-width:0;padding-right:.2rem}.category-item .cat:hover,.category-item .cat.on{background:transparent}
.category-pin{display:grid;place-items:center;flex:none;width:1.8rem;height:1.8rem;margin-right:.2rem;border-radius:8px;color:var(--dim);opacity:0;transition:opacity .15s,background .15s}
.category-pin svg{width:.9rem;height:.9rem}.category-pin:hover{background:rgba(255,255,255,.08);color:var(--text)}
.category-pin.pinned{color:var(--accent);opacity:1}.category-item:hover .category-pin,.category-item:focus-within .category-pin{opacity:1}
.category-label{display:flex;align-items:center;gap:.4rem;margin:1rem .7rem .35rem;color:var(--faint);font-size:.65rem;font-weight:600;text-transform:uppercase;letter-spacing:.09em}
.category-label svg{width:.75rem;height:.75rem;color:var(--accent)}.category-note{padding:.5rem .7rem;color:var(--dim);font-size:.8rem}
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
    let pinned: Vec<_> = list
        .iter()
        .filter(|c| pins.contains(&c.category_id))
        .filter(matches)
        .collect();
    let others: Vec<_> = list
        .iter()
        .filter(|c| !pins.contains(&c.category_id))
        .filter(matches)
        .collect();
    let total = library.as_ref().map_or(0, |l| l.len());
    rsx! {
        input {
            class: "filter", aria_label: "Filter categories", placeholder: "Filter categories",
            value: "{query}", oninput: move |e| query.set(e.value())
        }
        button {
            class: if selected.is_none() { "cat on" } else { "cat" },
            onclick: move |_| onselect.call(None), span { "All" } small { "{thousands(total)}" }
        }
        if !pinned.is_empty() {
            h2 { class: "category-label", Icon { d: PIN } "Pinned" }
            for c in pinned {
                CategoryItem { key: "pinned-{c.category_id}", category: c.clone(), count: count(c.category_id),
                    selected: selected == Some(c.category_id), pinned: true, onselect, onpin }
            }
            h2 { class: "category-label", "Categories" }
        }
        for c in others {
            CategoryItem { key: "category-{c.category_id}", category: c.clone(), count: count(c.category_id),
                selected: selected == Some(c.category_id), pinned: false, onselect, onpin }
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
        div { class: if selected { "category-item selected" } else { "category-item" },
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
