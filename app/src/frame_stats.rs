//! Frame presentation counter, active only while the stream-info overlay is visible.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use web_sys::{
    HtmlVideoElement, js_sys,
    wasm_bindgen::{JsCast, JsValue, closure::Closure},
};

type FrameCallback = Closure<dyn FnMut(f64, JsValue)>;

struct State {
    presented: Cell<u64>,
    request_id: Cell<Option<f64>>,
    callback: RefCell<Option<FrameCallback>>,
}

pub struct Counter {
    video: HtmlVideoElement,
    state: Rc<State>,
}

fn method(video: &HtmlVideoElement, name: &str) -> Option<js_sys::Function> {
    js_sys::Reflect::get(video.as_ref(), &name.into())
        .ok()?
        .dyn_into()
        .ok()
}

impl Counter {
    pub fn start(video: &HtmlVideoElement) -> Option<Self> {
        let request = method(video, "requestVideoFrameCallback")?;
        let state = Rc::new(State {
            presented: Cell::new(0),
            request_id: Cell::new(None),
            callback: RefCell::new(None),
        });
        let weak = Rc::downgrade(&state);
        let target = video.clone();
        let callback = Closure::<dyn FnMut(f64, JsValue)>::new(move |_, metadata| {
            let Some(state) = weak.upgrade() else { return };
            if let Ok(value) = js_sys::Reflect::get(&metadata, &"presentedFrames".into())
                && let Some(frames) = value.as_f64()
            {
                state.presented.set(frames as u64);
            }
            if let Some(callback) = state.callback.borrow().as_ref()
                && let Ok(id) = request.call1(target.as_ref(), callback.as_ref())
            {
                state.request_id.set(id.as_f64());
            }
        });
        *state.callback.borrow_mut() = Some(callback);
        let id = method(video, "requestVideoFrameCallback")?
            .call1(video.as_ref(), state.callback.borrow().as_ref()?.as_ref())
            .ok()?
            .as_f64()?;
        state.request_id.set(Some(id));
        Some(Self {
            video: video.clone(),
            state,
        })
    }

    pub fn presented(&self) -> u64 {
        self.state.presented.get()
    }
}

impl Drop for Counter {
    fn drop(&mut self) {
        if let (Some(cancel), Some(id)) = (
            method(&self.video, "cancelVideoFrameCallback"),
            self.state.request_id.take(),
        ) {
            let _ = cancel.call1(self.video.as_ref(), &id.into());
        }
        self.state.callback.borrow_mut().take();
    }
}
