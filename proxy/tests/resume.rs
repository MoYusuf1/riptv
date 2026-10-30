#[path = "../../app/src/resume.rs"]
mod resume;

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

#[test]
fn identity_and_epoch_cache_avoids_repeated_storage_lookups() {
    let episodes = Rc::new((0..10_000_u32).collect::<Vec<_>>());
    let mut cache = resume::ResumeMemo::new();
    let lookups = Cell::new(0_usize);
    let scan = || {
        resume::last_unfinished(&episodes, |_, _| {
            lookups.set(lookups.get() + 1);
            None::<(f64, f64)>
        })
    };

    assert_eq!(cache.get_or_compute(&episodes, 1, scan), (0, None));
    assert_eq!(lookups.get(), 10_000);

    assert_eq!(
        cache.get_or_compute(&episodes, 1, || panic!("cache hit must not scan")),
        (0, None)
    );
    assert_eq!(lookups.get(), 10_000);

    assert_eq!(cache.get_or_compute(&episodes, 2, scan), (0, None));
    assert_eq!(lookups.get(), 20_000);

    // Same values in a new Rc snapshot still invalidate by allocation identity.
    let refreshed = Rc::new((0..10_000_u32).collect::<Vec<_>>());
    assert_eq!(cache.get_or_compute(&refreshed, 2, scan), (0, None));
    assert_eq!(lookups.get(), 30_000);
}

#[test]
fn progress_scan_and_episode_bars_share_one_lookup_per_episode() {
    let episodes = Rc::new((0..10_000_u32).collect::<Vec<_>>());
    let mut resume_cache = resume::ResumeMemo::new();
    let progress_cache = RefCell::new(resume::ProgressCache::new());
    let lookups = Cell::new(0_usize);

    let scan = |epoch| {
        resume::last_unfinished(&episodes, |index, _| {
            progress_cache
                .borrow_mut()
                .get_or_load(&episodes, epoch, index, || {
                    lookups.set(lookups.get() + 1);
                    None::<(f64, f64)>
                })
        })
    };
    assert_eq!(
        resume_cache.get_or_compute(&episodes, 1, || scan(1)),
        (0, None)
    );
    assert_eq!(lookups.get(), episodes.len());

    // Rendering all episode bars reads the same indices and should hit the populated cache.
    for index in 0..episodes.len() {
        assert_eq!(
            progress_cache
                .borrow_mut()
                .get_or_load(&episodes, 1, index, || {
                    lookups.set(lookups.get() + 1);
                    None::<(f64, f64)>
                }),
            None
        );
    }
    assert_eq!(lookups.get(), episodes.len());

    assert_eq!(
        resume_cache.get_or_compute(&episodes, 1, || scan(1)),
        (0, None)
    );
    for index in 0..episodes.len() {
        let _ = progress_cache
            .borrow_mut()
            .get_or_load(&episodes, 1, index, || {
                lookups.set(lookups.get() + 1);
                None::<(f64, f64)>
            });
    }
    assert_eq!(lookups.get(), episodes.len());
}

#[test]
fn progress_cache_invalidates_new_equal_snapshot_without_retaining_old_list() {
    let old = Rc::new(vec![7_u32]);
    let old_weak = Rc::downgrade(&old);
    let mut cache = resume::ProgressCache::new();
    assert_eq!(cache.get_or_load(&old, 3, 0, || None::<(f64, f64)>), None);
    drop(old);
    assert!(old_weak.upgrade().is_none());

    let refreshed = Rc::new(vec![7_u32]);
    let loads = Cell::new(0);
    assert_eq!(
        cache.get_or_load(&refreshed, 3, 0, || {
            loads.set(loads.get() + 1);
            Some((45.0, 120.0))
        }),
        Some((45.0, 120.0))
    );
    assert_eq!(loads.get(), 1);
}

#[test]
fn last_unfinished_preserves_play_order_and_empty_fallback() {
    let episodes = [0, 1, 2, 3, 4, 5];
    let selected = resume::last_unfinished(&episodes, |_, episode| match episode {
        2 => Some((80.0, 100.0)),
        5 => Some((35.0, 100.0)),
        _ => None,
    });
    assert_eq!(selected, (5, Some((35.0, 100.0))));
    assert_eq!(
        resume::last_unfinished::<u32, (f64, f64)>(&[], |_, _| None),
        (0, None)
    );
}

#[test]
fn progress_changes_are_seen_on_epoch_change_and_finished_episode_falls_back() {
    let episodes = Rc::new(vec![1_u32, 2, 3, 4]);
    let progress = RefCell::new(HashMap::from([(2, (40.0, 100.0)), (3, (60.0, 100.0))]));
    let mut cache = resume::ResumeMemo::new();
    let values = RefCell::new(resume::ProgressCache::new());
    let resolve = |epoch| {
        resume::last_unfinished(&episodes, |index, episode| {
            values
                .borrow_mut()
                .get_or_load(&episodes, epoch, index, || {
                    progress.borrow().get(episode).copied()
                })
        })
    };

    assert_eq!(
        cache.get_or_compute(&episodes, 7, || resolve(7)),
        (2, Some((60.0, 100.0)))
    );

    // A write during playback does not change the answer while DetailPage remains memoized.
    progress.borrow_mut().insert(4, (25.0, 100.0));
    assert_eq!(
        cache.get_or_compute(&episodes, 7, || resolve(7)),
        (2, Some((60.0, 100.0)))
    );
    assert_eq!(
        cache.get_or_compute(&episodes, 8, || resolve(8)),
        (3, Some((25.0, 100.0)))
    );

    // Finishing/removing the latest episode reveals the earlier unfinished episode.
    progress.borrow_mut().remove(&4);
    assert_eq!(
        cache.get_or_compute(&episodes, 8, || resolve(8)),
        (3, Some((25.0, 100.0)))
    );
    assert_eq!(
        cache.get_or_compute(&episodes, 9, || resolve(9)),
        (2, Some((60.0, 100.0)))
    );
}
