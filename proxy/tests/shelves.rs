//! The real shelf cache with a storage adapter, including cross-tab and profile changes.
#![allow(dead_code)]
use std::{cell::RefCell, collections::HashMap, rc::Rc};
thread_local! { static ITEMS: RefCell<HashMap<String,String>> = RefCell::default(); }
thread_local! { static FAIL_SET: RefCell<bool> = const { RefCell::new(false) }; }
struct Storage;
impl Storage {
    fn get_item(&self, key: &str) -> Result<Option<String>, ()> {
        Ok(ITEMS.with(|s| s.borrow().get(key).cloned()))
    }
    fn set_item(&self, key: &str, value: &str) -> Result<(), ()> {
        if FAIL_SET.with(|fail| *fail.borrow()) {
            return Err(());
        }
        ITEMS.with(|s| s.borrow_mut().insert(key.into(), value.into()));
        Ok(())
    }
    fn remove_item(&self, key: &str) -> Result<(), ()> {
        ITEMS.with(|s| s.borrow_mut().remove(key));
        Ok(())
    }
}
fn storage() -> Option<Storage> {
    Some(Storage)
}
#[path = "../../app/src/shelves.rs"]
mod shelves;

#[test]
fn shelf_cache_reuses_data_but_observes_external_edits_and_profile_switches() {
    shelves::set_scope("a");
    let key = shelves::scoped_key("recent");
    storage()
        .unwrap()
        .set_item(&key, r#"[{"kind":"movie","id":1,"title":"One"}]"#)
        .unwrap();
    let first = shelves::recent();
    assert_eq!(shelves::parse_count(), 1);
    assert!(Rc::ptr_eq(&first, &shelves::recent()));
    assert_eq!(shelves::parse_count(), 1);
    storage()
        .unwrap()
        .set_item(&key, r#"[{"kind":"movie","id":2,"title":"Two"}]"#)
        .unwrap();
    assert_eq!(shelves::recent()[0].id, 2);
    assert_eq!(shelves::parse_count(), 2);
    shelves::set_scope("b");
    assert!(shelves::recent().is_empty());
    assert_eq!(shelves::parse_count(), 3);
    shelves::set_scope("a");
    assert_eq!(shelves::recent()[0].id, 2);
    assert_eq!(shelves::parse_count(), 4);
}

#[test]
fn successful_own_write_publishes_shared_cache_and_failed_write_does_not() {
    shelves::set_scope("own-write");
    let before = shelves::recent();
    let after_initial_load = shelves::parse_count();
    shelves::save(
        "series:1:1:1",
        120.0,
        600.0,
        Some(&shelves::Entry {
            kind: shelves::Title::Series,
            id: 1,
            title: "One".into(),
            icon: None,
            ext: None,
            sub: Some("S1 E1".into()),
            at: 0.0,
            total: 0.0,
        }),
    );
    let written = shelves::recent();
    assert_eq!(shelves::parse_count(), after_initial_load);
    assert!(!Rc::ptr_eq(&before, &written));
    assert!(Rc::ptr_eq(&written, &shelves::recent()));
    assert_eq!(written[0].id, 1);

    let before_failed_write = shelves::mine();
    let after_failed_list_load = shelves::parse_count();
    FAIL_SET.with(|fail| *fail.borrow_mut() = true);
    assert!(shelves::toggle_mine(shelves::Entry {
        kind: shelves::Title::Movie,
        id: 2,
        title: "Two".into(),
        icon: None,
        ext: Some("mkv".into()),
        sub: None,
        at: 0.0,
        total: 0.0,
    }));
    FAIL_SET.with(|fail| *fail.borrow_mut() = false);
    let after_failed_write = shelves::mine();
    assert_eq!(shelves::parse_count(), after_failed_list_load);
    assert!(Rc::ptr_eq(&before_failed_write, &after_failed_write));
    assert!(after_failed_write.is_empty());
}
