//! Test the browser preference model natively with an in-memory storage adapter.
//! This imports the actual app module; it does not duplicate preference logic.
use std::{cell::RefCell, collections::HashMap};

thread_local! {
    static ITEMS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    static PROFILE: RefCell<String> = RefCell::new("test-a".into());
}

struct MemoryStorage;
impl MemoryStorage {
    fn get_item(&self, name: &str) -> Result<Option<String>, ()> {
        Ok(ITEMS.with(|items| items.borrow().get(name).cloned()))
    }
    fn set_item(&self, name: &str, value: &str) -> Result<(), ()> {
        ITEMS.with(|items| items.borrow_mut().insert(name.into(), value.into()));
        Ok(())
    }
}
fn storage() -> Option<MemoryStorage> {
    Some(MemoryStorage)
}
mod shelves {
    pub fn scoped_key(name: &str) -> String {
        super::PROFILE.with(|profile| format!("{}.{}", profile.borrow(), name))
    }
}
#[path = "../../app/src/preferences.rs"]
mod preferences;

#[test]
fn old_preferences_default_to_no_pins() {
    storage()
        .unwrap()
        .set_item(
            &shelves::scoped_key("preferences"),
            r#"{"volume":67,"categories":[42,null,null],"sorts":[1,0,0]}"#,
        )
        .unwrap();
    let saved = preferences::load();
    assert_eq!(saved.volume, 67);
    assert_eq!(saved.categories[0], Some(42));
    assert!(saved.pinned_categories.iter().all(Vec::is_empty));
}

#[test]
fn pins_persist_and_are_separate_by_profile_and_section() {
    preferences::update(|p| preferences::toggle_pin(&mut p.pinned_categories[0], 19));
    assert_eq!(preferences::load().pinned_categories[0], vec![19]);
    assert!(preferences::load().pinned_categories[1].is_empty());
    PROFILE.with(|profile| *profile.borrow_mut() = "test-b".into());
    assert!(preferences::load().pinned_categories[0].is_empty());
    PROFILE.with(|profile| *profile.borrow_mut() = "test-a".into());
    preferences::update(|p| preferences::toggle_pin(&mut p.pinned_categories[0], 19));
    assert!(preferences::load().pinned_categories[0].is_empty());
}

#[test]
fn corrupt_or_excessive_preferences_are_bounded() {
    let mut pins = vec![1; 200];
    pins.extend(2..500);
    let saved = serde_json::json!({"volume":255,"speed":99,"pinned_categories":[pins,[],[]]});
    storage()
        .unwrap()
        .set_item(&shelves::scoped_key("preferences"), &saved.to_string())
        .unwrap();
    let saved = preferences::load();
    assert_eq!(saved.volume, 100);
    assert_eq!(saved.speed, 1.0);
    assert_eq!(saved.pinned_categories[0].len(), 100);
    storage()
        .unwrap()
        .set_item(&shelves::scoped_key("preferences"), "broken")
        .unwrap();
    assert!(preferences::load().pinned_categories[0].is_empty());
}

#[test]
fn pin_limit_still_allows_unpinning() {
    let mut pins: Vec<_> = (0..100).collect();
    preferences::toggle_pin(&mut pins, 200);
    assert_eq!(pins.len(), 100);
    preferences::toggle_pin(&mut pins, 5);
    assert_eq!(pins.len(), 99);
    preferences::toggle_pin(&mut pins, 200);
    assert_eq!(pins.len(), 100);
    assert!(pins.contains(&200));
}
