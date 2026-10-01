//! Browser/OS media keys, without a player dependency or polling loop.

use std::{cell::RefCell, rc::Rc};

use web_sys::{
    js_sys,
    wasm_bindgen::{JsCast, JsValue, closure::Closure},
};

#[derive(Clone, Copy)]
pub enum Action {
    Play,
    Pause,
    Back,
    Forward,
    Previous,
    Next,
    Stop,
}

type ActionHandler = Closure<dyn FnMut(JsValue)>;

pub struct Session {
    media: JsValue,
    handlers: Vec<(&'static str, ActionHandler)>,
}

fn method(target: &JsValue, name: &str) -> Option<js_sys::Function> {
    js_sys::Reflect::get(target, &JsValue::from_str(name))
        .ok()?
        .dyn_into()
        .ok()
}

impl Session {
    pub fn install(
        title: &str,
        artist: &str,
        actions: &'static [(&'static str, Action)],
        on_action: impl FnMut(Action) + 'static,
    ) -> Option<Self> {
        let navigator =
            js_sys::Reflect::get(web_sys::window()?.as_ref(), &"navigator".into()).ok()?;
        let media = js_sys::Reflect::get(&navigator, &"mediaSession".into()).ok()?;
        if media.is_null() || media.is_undefined() {
            return None;
        }
        if let Ok(ctor) = js_sys::Reflect::get(&js_sys::global(), &"MediaMetadata".into())
            && let Ok(ctor) = ctor.dyn_into::<js_sys::Function>()
        {
            let details = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&details, &"title".into(), &title.into());
            let _ = js_sys::Reflect::set(&details, &"artist".into(), &artist.into());
            let args = js_sys::Array::new();
            args.push(&details);
            if let Ok(metadata) = js_sys::Reflect::construct(&ctor, &args) {
                let _ = js_sys::Reflect::set(&media, &"metadata".into(), &metadata);
            }
        }
        let set_handler = method(&media, "setActionHandler")?;
        let on_action: Rc<RefCell<dyn FnMut(Action)>> = Rc::new(RefCell::new(on_action));
        let mut handlers = Vec::new();
        for &(name, action) in actions {
            let callback = on_action.clone();
            let handler =
                Closure::<dyn FnMut(JsValue)>::new(move |_| callback.borrow_mut()(action));
            if set_handler
                .call2(&media, &name.into(), handler.as_ref())
                .is_ok()
            {
                handlers.push((name, handler));
            }
        }
        Some(Self { media, handlers })
    }

    pub fn playing(&self, playing: bool) {
        let _ = js_sys::Reflect::set(
            &self.media,
            &"playbackState".into(),
            &if playing { "playing" } else { "paused" }.into(),
        );
    }

    pub fn position(&self, at: f64, duration: f64, speed: f64) {
        if !at.is_finite() || !duration.is_finite() || duration <= 0.0 {
            return;
        }
        let Some(set_position) = method(&self.media, "setPositionState") else {
            return;
        };
        let state = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&state, &"duration".into(), &duration.into());
        let _ = js_sys::Reflect::set(&state, &"position".into(), &at.clamp(0.0, duration).into());
        let _ = js_sys::Reflect::set(&state, &"playbackRate".into(), &speed.into());
        let _ = set_position.call1(&self.media, &state);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(set_handler) = method(&self.media, "setActionHandler") {
            for (name, _) in &self.handlers {
                let _ = set_handler.call2(&self.media, &(*name).into(), &JsValue::NULL);
            }
        }
        let _ = js_sys::Reflect::set(&self.media, &"playbackState".into(), &"none".into());
    }
}
