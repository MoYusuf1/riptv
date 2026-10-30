//! Exercise fullscreen request cancellation and fallback gating without a browser runtime.
#[path = "../../app/src/fullscreen_state.rs"]
mod fullscreen_state;

use fullscreen_state::{Action, State};

#[test]
fn a_second_toggle_cancels_instead_of_starting_a_second_request() {
    let mut state = State::new();
    let Action::Start(token) = state.toggle(false, false) else {
        panic!("first toggle should start a request");
    };

    assert_eq!(state.toggle(false, false), Action::CancelPending);
    assert!(state.should_exit_late_native(token));
    assert_eq!(state.cancelled_token(), Some(token));
    assert!(!state.finish(token, true, true));
    let settled = state.settle_native(token);
    assert!(settled.current && settled.cancelled && settled.alive);
}

#[test]
fn a_new_request_clears_the_previous_late_success_cancellation() {
    let mut state = State::new();
    let Action::Start(old) = state.toggle(false, false) else {
        panic!("first toggle should start a request");
    };
    assert!(state.cancel());
    let Action::Start(current) = state.toggle(false, false) else {
        panic!("new toggle should start a request");
    };

    assert_ne!(old, current);
    assert!(!state.should_exit_late_native(old));
    assert!(!state.should_exit_late_native(current));
    assert!(!state.settle_native(old).current);
}

#[test]
fn a_new_request_after_fallback_supersedes_the_old_late_promise() {
    let mut state = State::new();
    let Action::Start(old) = state.toggle(false, false) else {
        panic!("first toggle should start a request");
    };
    assert!(state.finish(old, true, true));
    assert!(state.cancel());
    let Action::Start(current) = state.toggle(false, false) else {
        panic!("new toggle should start a request");
    };

    assert_ne!(old, current);
    assert!(!state.settle_native(old).current);
    assert!(!state.should_exit_late_native(old));
}

#[test]
fn fullscreenchange_entry_or_exit_cancels_the_delayed_fallback() {
    for _change_to in [true, false] {
        let mut state = State::new();
        let Action::Start(token) = state.toggle(false, false) else {
            panic!("toggle should start a request");
        };

        assert!(state.fullscreen_changed(token));
        assert!(!state.should_exit_late_native(token));
        assert!(!state.finish(token, true, true));
        let settled = state.settle_native(token);
        assert!(settled.current && settled.alive && !settled.cancelled);
    }
}

#[test]
fn cancellation_and_disposal_prevent_late_fallback() {
    let mut cancelled = State::new();
    let Action::Start(token) = cancelled.toggle(false, false) else {
        panic!("toggle should start a request");
    };
    assert!(cancelled.cancel());
    assert!(!cancelled.finish(token, true, true));

    let mut disposed = State::new();
    let Action::Start(token) = disposed.toggle(false, false) else {
        panic!("toggle should start a request");
    };
    disposed.dispose();
    assert!(disposed.should_exit_late_native(token));
    assert!(!disposed.finish(token, true, true));
    let settled = disposed.settle_native(token);
    assert!(settled.current && settled.cancelled && !settled.alive);
    assert_eq!(disposed.toggle(false, false), Action::None);
}

#[test]
fn cancelling_after_fallback_marks_the_unresolved_native_request() {
    let mut state = State::new();
    let Action::Start(token) = state.toggle(false, false) else {
        panic!("toggle should start a request");
    };

    assert!(state.finish(token, true, true));
    assert!(state.cancel());
    let settled = state.settle_native(token);
    assert!(settled.current && settled.cancelled && settled.alive);
}

#[test]
fn disposing_after_fallback_marks_the_unresolved_native_request() {
    let mut state = State::new();
    let Action::Start(token) = state.toggle(false, false) else {
        panic!("toggle should start a request");
    };

    assert!(state.finish(token, true, true));
    state.dispose();
    let settled = state.settle_native(token);
    assert!(settled.current && settled.cancelled && !settled.alive);
}

#[test]
fn late_native_success_after_fallback_is_current_and_not_cancelled() {
    let mut state = State::new();
    let Action::Start(token) = state.toggle(false, false) else {
        panic!("toggle should start a request");
    };

    assert!(state.finish(token, true, true));
    let settled = state.settle_native(token);
    assert!(settled.current && settled.alive && !settled.cancelled);
}

#[test]
fn rejected_or_ignored_request_falls_back_only_for_the_current_player() {
    let mut current = State::new();
    let Action::Start(token) = current.toggle(false, false) else {
        panic!("toggle should start a request");
    };
    assert!(current.finish(token, true, true));

    let mut native = State::new();
    let Action::Start(token) = native.toggle(false, false) else {
        panic!("toggle should start a request");
    };
    assert!(!native.finish(token, false, true));

    let mut replaced = State::new();
    let Action::Start(token) = replaced.toggle(false, false) else {
        panic!("toggle should start a request");
    };
    assert!(!replaced.finish(token, true, false));
}

#[test]
fn native_exit_and_page_fill_close_keep_their_toggle_behavior() {
    let mut native = State::new();
    assert_eq!(native.toggle(true, false), Action::ExitNative);

    let mut fallback = State::new();
    assert_eq!(fallback.toggle(false, true), Action::ClearFallback);
}
