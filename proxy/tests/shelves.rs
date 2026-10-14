//! The real shelf cache with a storage adapter, including cross-tab and profile changes.
#![allow(dead_code)]
use std::{cell::RefCell, collections::HashMap, rc::Rc};
thread_local! { static ITEMS: RefCell<HashMap<String,String>> = RefCell::default(); }
struct Storage;
impl Storage {
    fn get_item(&self, key: &str) -> Result<Option<String>, ()> {
        Ok(ITEMS.with(|s| s.borrow().get(key).cloned()))
    }
    fn set_item(&self, key: &str, value: &str) -> Result<(), ()> {
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
    assert!(Rc::ptr_eq(&first, &shelves::recent()));
    storage()
        .unwrap()
        .set_item(&key, r#"[{"kind":"movie","id":2,"title":"Two"}]"#)
        .unwrap();
    assert_eq!(shelves::recent()[0].id, 2);
    shelves::set_scope("b");
    assert!(shelves::recent().is_empty());
    shelves::set_scope("a");
    assert_eq!(shelves::recent()[0].id, 2);
}
