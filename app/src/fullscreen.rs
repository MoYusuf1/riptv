//! Native fullscreen with a cancellable page-fill fallback for browsers that ignore the request.
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use dioxus::prelude::{Signal, WritableExt};
use wasm_bindgen_futures::JsFuture;
use web_sys::wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{Document, Element, Event, js_sys};

use crate::fullscreen_state::{Action, NativeSettlement, State};

struct PendingListener {
    token: u64,
    document: Document,
    callback: Closure<dyn FnMut(Event)>,
    attached: bool,
}

struct Inner {
    state: RefCell<State>,
    listener: RefCell<Option<PendingListener>>,
    cancelled: Rc<Cell<Option<u64>>>,
}

impl Inner {
    fn detach_listener(&self, token: u64) {
        if let Some(pending) = self
            .listener
            .borrow()
            .as_ref()
            .filter(|pending| pending.token == token && pending.attached)
        {
            let _ = pending.document.remove_event_listener_with_callback(
                "fullscreenchange",
                pending.callback.as_ref().unchecked_ref(),
            );
        }
    }

    fn clear_listener(&self, token: Option<u64>) {
        let mut listener = self.listener.borrow_mut();
        if listener
            .as_ref()
            .is_some_and(|pending| token.is_none_or(|token| pending.token == token))
            && let Some(pending) = listener.take()
            && pending.attached
        {
            let _ = pending.document.remove_event_listener_with_callback(
                "fullscreenchange",
                pending.callback.as_ref().unchecked_ref(),
            );
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        let state = self.state.get_mut();
        state.dispose();
        self.cancelled.set(state.cancelled_token());
        if let Some(pending) = self.listener.get_mut().take()
            && pending.attached
        {
            let _ = pending.document.remove_event_listener_with_callback(
                "fullscreenchange",
                pending.callback.as_ref().unchecked_ref(),
            );
        }
    }
}

/// Keep one controller per player component and call `dispose` from its `use_drop` hook. Timer
/// tasks hold only a weak controller reference, so they cannot keep a disposed signal alive.
#[derive(Clone)]
pub(crate) struct Controller {
    inner: Rc<Inner>,
}

impl Default for Controller {
    fn default() -> Self {
        Self {
            inner: Rc::new(Inner {
                state: RefCell::new(State::new()),
                listener: RefCell::new(None),
                cancelled: Rc::new(Cell::new(None)),
            }),
        }
    }
}

impl Controller {
    /// Toggle native fullscreen, or page-fill if the request is denied or ignored.
    pub(crate) fn toggle(&self, id: &'static str, mut expanded: Signal<bool>) {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let native = document.fullscreen_element().is_some();
        let action = {
            let mut state = self.inner.state.borrow_mut();
            let action = state.toggle(native, expanded());
            self.inner.cancelled.set(state.cancelled_token());
            action
        };
        match action {
            Action::None => {}
            Action::ExitNative => {
                self.inner.clear_listener(None);
                document.exit_fullscreen();
            }
            Action::ClearFallback => {
                self.inner.clear_listener(None);
                expanded.set(false);
            }
            Action::CancelPending => self.inner.clear_listener(None),
            Action::Start(token) => {
                // A new attempt replaces any listener left behind by a completed/cancelled timer.
                self.inner.clear_listener(None);
                let Some(player) = document.get_element_by_id(id) else {
                    self.inner.state.borrow_mut().cancel();
                    return;
                };
                self.start_request(document, player, id, token, expanded);
            }
        }
    }

    /// Cancel a pending native request, for example when Escape is pressed.
    pub(crate) fn cancel(&self) {
        let mut state = self.inner.state.borrow_mut();
        state.cancel();
        self.inner.cancelled.set(state.cancelled_token());
        drop(state);
        self.inner.clear_listener(None);
    }

    /// Escape must close both the browser fullscreen surface and the page-fill fallback.
    pub(crate) fn exit(&self, id: &'static str, mut expanded: Signal<bool>) {
        self.cancel();
        expanded.set(false);
        if let Some(document) = web_sys::window().and_then(|window| window.document())
            && document
                .fullscreen_element()
                .is_some_and(|player| player.id() == id)
        {
            document.exit_fullscreen();
        }
    }

    /// Invalidate any delayed write and detach its listener before the player is unmounted.
    pub(crate) fn dispose(&self) {
        let mut state = self.inner.state.borrow_mut();
        state.dispose();
        self.inner.cancelled.set(state.cancelled_token());
        drop(state);
        self.inner.clear_listener(None);
    }

    fn start_request(
        &self,
        document: Document,
        player: Element,
        id: &'static str,
        token: u64,
        mut expanded: Signal<bool>,
    ) {
        let weak = Rc::downgrade(&self.inner);
        let event_document = document.clone();
        let callback_weak = weak.clone();
        let callback = Closure::<dyn FnMut(Event)>::new(move |_| {
            if let Some(inner) = callback_weak.upgrade()
                && inner.state.borrow_mut().fullscreen_changed(token)
            {
                // Keep the Closure alive until the timeout owns cleanup, but stop observing now.
                inner.detach_listener(token);
            }
        });
        let attached = document
            .add_event_listener_with_callback("fullscreenchange", callback.as_ref().unchecked_ref())
            .is_ok();
        self.inner.listener.replace(Some(PendingListener {
            token,
            document: event_document,
            callback,
            attached,
        }));

        // Calling through Reflect keeps the native Promise, which lets a cancelled late success
        // leave fullscreen again. Await it even when rejected so the browser sees no unhandled
        // rejection; the bounded fallback timer below handles the rejected and ignored cases.
        let promise = js_sys::Reflect::get(player.as_ref(), &"requestFullscreen".into())
            .ok()
            .and_then(|method| method.dyn_into::<js_sys::Function>().ok())
            .and_then(|method| method.call0(player.as_ref()).ok())
            .and_then(|result| result.dyn_into::<js_sys::Promise>().ok());
        if let Some(promise) = promise {
            let promise_weak = weak.clone();
            let promise_cancelled = Rc::clone(&self.inner.cancelled);
            let promise_document = document.clone();
            let promise_player = player.clone();
            let mut promise_expanded = expanded;
            wasm_bindgen_futures::spawn_local(async move {
                let _ = JsFuture::from(promise).await;
                let mut settlement = NativeSettlement {
                    current: false,
                    alive: false,
                    cancelled: false,
                };
                if let Some(inner) = promise_weak.upgrade() {
                    let mut state = inner.state.borrow_mut();
                    settlement = state.settle_native(token);
                    promise_cancelled.set(state.cancelled_token());
                } else if promise_cancelled.get() == Some(token) {
                    // The owner was dropped; retain only this token long enough to clean up a
                    // late success. There is no live signal to update.
                    settlement = NativeSettlement {
                        current: true,
                        alive: false,
                        cancelled: true,
                    };
                    promise_cancelled.set(None);
                }
                let is_native_player = promise_document
                    .fullscreen_element()
                    .is_some_and(|current| current.is_same_node(Some(&promise_player)));
                if is_native_player && settlement.cancelled {
                    promise_document.exit_fullscreen();
                } else if is_native_player && settlement.current && settlement.alive {
                    // A late native success replaces fallback fill; clear its old state.
                    promise_expanded.set(false);
                }
            });
        }
        wasm_bindgen_futures::spawn_local(async move {
            rstreamkit::mse::sleep(Duration::from_millis(600)).await;
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let no_native_fullscreen = document.fullscreen_element().is_none();
            let same_player = document
                .get_element_by_id(id)
                .is_some_and(|current| current.is_same_node(Some(&player)));
            let should_expand =
                inner
                    .state
                    .borrow_mut()
                    .finish(token, no_native_fullscreen, same_player);
            inner.clear_listener(Some(token));
            if should_expand {
                expanded.set(true);
            }
        });
    }
}
