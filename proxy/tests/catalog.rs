#[path = "../../app/src/catalog.rs"]
mod catalog;

use catalog::NavigationAction::{Next, Number, Previous};

#[derive(Clone)]
struct Title {
    id: u64,
    category: u64,
    name: String,
}

fn reference_target(
    titles: &[Title],
    order: &[u32],
    category: Option<u64>,
    query: &str,
    current: u64,
    action: catalog::NavigationAction,
) -> Option<(usize, u32)> {
    let mut first = None;
    let mut last = None;
    let mut previous = None;
    let mut next = None;
    let mut found_current = false;
    let mut position = 0;
    for &index in order {
        let title = &titles[index as usize];
        if category.is_some_and(|wanted| title.category != wanted)
            || !xtream::contains_lowercase(&title.name, query)
        {
            continue;
        }
        let item = (position, index);
        if first.is_none() {
            first = Some(item);
        }
        if let Number(number) = action
            && number == position + 1
        {
            return Some(item);
        }
        if found_current && next.is_none() {
            next = Some(item);
        }
        if title.id == current {
            found_current = true;
            previous = last;
        }
        last = Some(item);
        position += 1;
    }
    match action {
        Previous => previous.or(last),
        Next => next.or(first),
        Number(_) => None,
    }
}

fn filtered_indices(
    titles: &[Title],
    order: &[u32],
    category: Option<u64>,
    query: &str,
) -> Vec<u32> {
    order
        .iter()
        .copied()
        .filter(|&index| {
            let title = &titles[index as usize];
            category.is_none_or(|wanted| title.category == wanted)
                && xtream::contains_lowercase(&title.name, query)
        })
        .collect()
}

#[test]
fn cached_navigation_matches_filtering_reference_for_edges_and_actions() {
    let titles = [
        Title {
            id: 10,
            category: 1,
            name: "Alpha".into(),
        },
        Title {
            id: 11,
            category: 2,
            name: "Beta".into(),
        },
        Title {
            id: 12,
            category: 1,
            name: "Gamma".into(),
        },
        Title {
            id: 13,
            category: 1,
            name: "Delta".into(),
        },
    ];
    let order = [0, 1, 2, 3];
    let cases = [
        (Some(1), "a", 10, Previous), // first wraps to last
        (Some(1), "a", 13, Next),     // last wraps to first
        (Some(1), "a", 12, Previous),
        (Some(1), "a", 12, Next),
        (Some(1), "a", 99, Previous), // missing current wraps backward
        (Some(1), "a", 99, Next),     // missing current wraps forward
        (Some(1), "a", 10, Number(2)),
        (Some(1), "a", 10, Number(0)),
        (Some(1), "a", 10, Number(4)),
        (Some(1), "z", 10, Previous), // empty filtered set
        (None, "", 10, Number(3)),
    ];

    for (category, query, current, action) in cases {
        let cached = filtered_indices(&titles, &order, category, query);
        let expected = reference_target(&titles, &order, category, query, current, action);
        let actual = catalog::navigate(&cached, current, action, |index| {
            titles.get(index as usize).map(|title| title.id)
        });
        assert_eq!(
            actual, expected,
            "{category:?} / {query:?} / {current} / {action:?}"
        );
    }

    let stale = [0, u32::MAX];
    for action in [Previous, Next] {
        assert_eq!(
            catalog::navigate(&stale, 10, action, |index| {
                titles.get(index as usize).map(|title| title.id)
            }),
            None,
            "stale catalog indexes return no target"
        );
    }
}

#[test]
#[ignore = "synthetic catalog navigation comparison; not a browser benchmark"]
fn navigation_cost_30k() {
    use std::time::Instant;

    let titles: Vec<_> = (0..30_000_u64)
        .map(|i| Title {
            id: i + 1,
            category: i % 12,
            name: if i % 10 == 0 {
                format!("Match channel {i}")
            } else {
                format!("Other channel {i}")
            },
        })
        .collect();
    let order: Vec<_> = (0..titles.len() as u32).collect();
    let (category, query) = (Some(0), "match");
    let cached = filtered_indices(&titles, &order, category, query);
    let actions = [Previous, Next, Number(150), Number(1), Previous, Next];
    let current = 271;

    let start = Instant::now();
    let reference: Vec<_> = (0..100)
        .flat_map(|_| {
            actions
                .iter()
                .map(|&action| reference_target(&titles, &order, category, query, current, action))
        })
        .collect();
    let reference_elapsed = start.elapsed();

    let start = Instant::now();
    let optimized: Vec<_> = (0..100)
        .flat_map(|_| {
            actions.iter().map(|&action| {
                catalog::navigate(&cached, current, action, |index| {
                    titles.get(index as usize).map(|title| title.id)
                })
            })
        })
        .collect();
    let cached_elapsed = start.elapsed();
    assert_eq!(optimized, reference);
    println!(
        "30,000 titles, {} cached hits, 600 channel actions: refilter-each-action={reference_elapsed:?}; cached-filtered-index={cached_elapsed:?} (synthetic native measurement)",
        cached.len()
    );
}
