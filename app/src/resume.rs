//! Identity-keyed memo for episode progress in the currently displayed play list.
use std::{
    collections::HashMap,
    rc::{Rc, Weak},
};

/// Cache one calculation for an `Rc` snapshot and a caller-provided invalidation epoch.
/// `Weak` compares allocation identity without retaining the old snapshot's contents.
pub(crate) struct ResumeMemo<T, R> {
    identity: Option<Weak<T>>,
    epoch: Option<u64>,
    value: Option<R>,
}

/// Cache present and missing progress lookups by play index for one play-list snapshot/epoch.
/// Progress values are copied on hits; the cache never stores or clones episode keys.
pub(crate) struct ProgressCache<T, R> {
    identity: Option<Weak<T>>,
    epoch: Option<u64>,
    values: HashMap<usize, Option<R>>,
}

impl<T, R: Copy> ProgressCache<T, R> {
    pub(crate) fn new() -> Self {
        Self {
            identity: None,
            epoch: None,
            values: HashMap::new(),
        }
    }

    pub(crate) fn get_or_load(
        &mut self,
        identity: &Rc<T>,
        epoch: u64,
        index: usize,
        load: impl FnOnce() -> Option<R>,
    ) -> Option<R> {
        let next_identity = Rc::downgrade(identity);
        let same_snapshot = self
            .identity
            .as_ref()
            .is_some_and(|previous| Weak::ptr_eq(previous, &next_identity));
        if !same_snapshot {
            self.identity = Some(next_identity);
            self.epoch = Some(epoch);
            self.values = HashMap::new();
        } else if self.epoch != Some(epoch) {
            self.epoch = Some(epoch);
            self.values.clear();
        }
        if let Some(value) = self.values.get(&index) {
            return *value;
        }
        let value = load();
        self.values.insert(index, value);
        value
    }
}

impl<T, R> ResumeMemo<T, R> {
    pub(crate) fn new() -> Self {
        Self {
            identity: None,
            epoch: None,
            value: None,
        }
    }

    pub(crate) fn get_or_compute(
        &mut self,
        identity: &Rc<T>,
        epoch: u64,
        compute: impl FnOnce() -> R,
    ) -> R
    where
        R: Clone,
    {
        let next_identity = Rc::downgrade(identity);
        if self.epoch == Some(epoch)
            && self
                .identity
                .as_ref()
                .is_some_and(|previous| Weak::ptr_eq(previous, &next_identity))
            && let Some(value) = &self.value
        {
            return value.clone();
        }

        let value = compute();
        self.identity = Some(next_identity);
        self.epoch = Some(epoch);
        self.value = Some(value.clone());
        value
    }
}

/// Find the last unfinished item in playback order, checking from the end as the UI does today.
pub(crate) fn last_unfinished<T, R>(
    items: &[T],
    mut progress: impl FnMut(usize, &T) -> Option<R>,
) -> (usize, Option<R>) {
    items
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, item)| progress(index, item).map(|saved| (index, Some(saved))))
        .unwrap_or((0, None))
}
