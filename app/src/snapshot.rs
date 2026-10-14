//! Immutable UI data compared by identity, without walking every episode on redraws.
use std::{ops::Deref, rc::Rc};

pub(crate) struct Snapshot<T>(Rc<T>);
impl<T> Snapshot<T> {
    pub(crate) fn from_rc(value: Rc<T>) -> Self {
        Self(value)
    }
    pub(crate) fn new(value: T) -> Self {
        Self(Rc::new(value))
    }
}
impl<T> Clone for Snapshot<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> PartialEq for Snapshot<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}
impl<T> Deref for Snapshot<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sharing_and_equality_do_not_require_clone_or_comparison_of_contents() {
        struct Large(Vec<u8>);
        let first = Snapshot::new(Large(vec![1; 100_000]));
        assert!(first == first.clone());
        let shared = Rc::new(Large(vec![1; 100_000]));
        assert!(Snapshot::from_rc(shared.clone()) == Snapshot::from_rc(shared));
        assert!(first != Snapshot::new(Large(vec![1; 100_000])));
        assert_eq!(first.0.0.len(), 100_000);
    }
}
