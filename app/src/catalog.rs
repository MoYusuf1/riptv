//! Index-only catalog operations: title data is immutable for this cache's lifetime.
use std::{cell::OnceCell, rc::Rc};

#[derive(Default)]
pub(crate) struct OrderCache([OnceCell<Rc<Vec<u32>>>; 2]);
impl OrderCache {
    pub(crate) fn get<'a>(
        &self,
        len: usize,
        descending: bool,
        name: impl Fn(u32) -> &'a str,
    ) -> Rc<Vec<u32>> {
        let ascending = self.0[0].get_or_init(|| {
            let mut order: Vec<_> = (0..len as u32).collect();
            order.sort_by_cached_key(|&i| name(i).to_lowercase());
            Rc::new(order)
        });
        if !descending {
            return ascending.clone();
        }
        self.0[1]
            .get_or_init(|| {
                let mut reversed = ascending.as_ref().clone();
                reversed.reverse();
                Rc::new(reversed)
            })
            .clone()
    }
}

pub(crate) fn page(order: &[u32], skip: usize, size: usize) -> &[u32] {
    let remaining = order.get(skip..).unwrap_or_default();
    &remaining[..remaining.len().min(size)]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NavigationAction {
    Previous,
    Next,
    Number(usize),
}

/// Resolve a channel action in an already filtered order. The result contains the item's
/// zero-based position in that order and its index into the original catalog.
pub(crate) fn navigate(
    filtered: &[u32],
    current_id: u64,
    action: NavigationAction,
    id: impl Fn(u32) -> Option<u64>,
) -> Option<(usize, u32)> {
    if let NavigationAction::Number(number) = action {
        return number.checked_sub(1).and_then(|position| {
            filtered
                .get(position)
                .copied()
                .map(|index| (position, index))
        });
    }

    let mut first = None;
    let mut last = None;
    let mut previous = None;
    let mut next = None;
    let mut found_current = false;
    for (position, &index) in filtered.iter().enumerate() {
        let item = (position, index);
        if first.is_none() {
            first = Some(item);
        }
        if found_current && next.is_none() {
            next = Some(item);
        }
        if id(index)? == current_id {
            found_current = true;
            previous = last;
        }
        last = Some(item);
    }
    match action {
        NavigationAction::Previous => previous.or(last),
        NavigationAction::Next => next.or(first),
        NavigationAction::Number(_) => unreachable!("number action returned above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sorting_is_cached_and_descending_reuses_the_ascending_result() {
        let titles = ["Zulu", "Alpha", "alpha", "Éclair"];
        let cache = OrderCache::default();
        let calls = std::cell::Cell::new(0);
        let name = |i: u32| {
            calls.set(calls.get() + 1);
            titles[i as usize]
        };
        let first = cache.get(titles.len(), false, name);
        let count = calls.get();
        assert_eq!(*first, [1, 2, 0, 3]);
        assert!(Rc::ptr_eq(&first, &cache.get(titles.len(), false, name)));
        assert_eq!(*cache.get(titles.len(), true, name), [3, 0, 2, 1]);
        assert_eq!(calls.get(), count);
        assert_eq!(page(&first, 1, 2), [2, 0]);
        assert!(page(&first, usize::MAX, 2).is_empty());
    }
}
