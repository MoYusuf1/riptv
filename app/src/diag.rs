//! Playback tracing for `riptv --logs`. Each stream the app opens gets a session with the proxy,
//! and the player reports what happens to it: which engine plays it, status changes, stalls and
//! how long they last, media errors, and a health sample now and then. The proxy writes it all
//! (redacted) to its diagnostics log and probes the stream itself when something goes wrong.
//!
//! Without `--logs` the proxy answers 404 to the first session, and the app stops asking: no
//! further requests, timers or listeners. Everything here is safe to call from raw JS callbacks
//! (no Dioxus `spawn`).

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::{Rc, Weak},
    time::Duration,
};

use serde_json::{Map, Value, json};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    HtmlVideoElement, js_sys,
    wasm_bindgen::{JsCast, JsValue, closure::Closure},
};

thread_local! {
    /// Whether the proxy keeps diagnostics: unknown until the first session is asked for.
    static AVAILABLE: Cell<Option<bool>> = const { Cell::new(None) };
}

/// A wait longer than this is reported as `stuck` (and the proxy probes the stream).
const STUCK_MS: f64 = 8_000.0;
/// Not playing this long after opening is reported as `stuck` too.
const STARTUP_MS: f64 = 15_000.0;
/// A health sample this often while something plays.
const HEALTH_EVERY: u32 = 6; // ticks of 5 s

/// A `<video>` event name and what handles it, kept so it can be removed again.
type Listener = (&'static str, Closure<dyn FnMut()>);

enum Link {
    Opening,
    Open(String),
    Off,
}

struct Inner {
    link: RefCell<Link>,
    queue: RefCell<VecDeque<Map<String, Value>>>,
    sending: Cell<bool>,
    closed: Cell<bool>,
    born: f64,
    video: RefCell<Option<HtmlVideoElement>>,
    listeners: RefCell<Vec<Listener>>,
    first_play: Cell<bool>,
    waiting_since: Cell<Option<f64>>,
    stuck_reported: Cell<bool>,
    stalls: Cell<u32>,
    stalled_ms: Cell<f64>,
}

/// One stream's trace. Cheap to clone; every clone reports to the same session.
#[derive(Clone)]
pub struct Trace(Rc<Inner>);

fn now() -> f64 {
    js_sys::Date::now()
}

async fn post(path: &str, body: &str) -> Option<(u16, String)> {
    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_body(&JsValue::from_str(body));
    let headers = web_sys::Headers::new().ok()?;
    headers.set("content-type", "application/json").ok()?;
    init.set_headers(&headers);
    let request = web_sys::Request::new_with_str_and_init(path, &init).ok()?;
    let response: web_sys::Response =
        JsFuture::from(web_sys::window()?.fetch_with_request(&request))
            .await
            .ok()?
            .unchecked_into();
    let text = JsFuture::from(response.text().ok()?).await.ok()?;
    Some((response.status(), text.as_string().unwrap_or_default()))
}

impl Trace {
    /// Opens a session for `upstream` (the provider's address, which the proxy keeps in memory
    /// only) and reports `open` with `fields`.
    pub fn start(upstream: &str, fields: Value) -> Self {
        let off = AVAILABLE.with(Cell::get) == Some(false);
        let trace = Self(Rc::new(Inner {
            link: RefCell::new(if off { Link::Off } else { Link::Opening }),
            queue: RefCell::default(),
            sending: Cell::new(false),
            closed: Cell::new(false),
            born: now(),
            video: RefCell::default(),
            listeners: RefCell::default(),
            first_play: Cell::new(true),
            waiting_since: Cell::new(None),
            stuck_reported: Cell::new(false),
            stalls: Cell::new(0),
            stalled_ms: Cell::new(0.0),
        }));
        if off {
            return trace;
        }
        let mut open = fields;
        if let Some(o) = open.as_object_mut()
            && let Some(agent) = js_sys::Reflect::get(&js_sys::global(), &"navigator".into())
                .and_then(|n| js_sys::Reflect::get(&n, &"userAgent".into()))
                .ok()
                .and_then(|a| a.as_string())
        {
            o.insert("browser".into(), agent.into());
        }
        trace.event("open", open);
        let body = json!({ "url": upstream }).to_string();
        let weak = Rc::downgrade(&trace.0);
        spawn_local(async move {
            let answer = post("/diagnostics", &body).await;
            let Some(inner) = weak.upgrade() else { return };
            let id = match answer {
                Some((200, text)) => serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v["id"].as_str().map(str::to_owned)),
                Some((404, _)) => {
                    AVAILABLE.with(|a| a.set(Some(false)));
                    None
                }
                _ => None,
            };
            match id {
                Some(id) => {
                    AVAILABLE.with(|a| a.set(Some(true)));
                    *inner.link.borrow_mut() = Link::Open(id);
                    Self(inner).pump();
                }
                None => {
                    *inner.link.borrow_mut() = Link::Off;
                    inner.queue.borrow_mut().clear();
                    Self(inner).unfollow();
                }
            }
        });
        trace
    }

    fn off(&self) -> bool {
        matches!(*self.0.link.borrow(), Link::Off)
    }

    /// Reports `name` with `fields` (an object), stamped with milliseconds since the start.
    pub fn event(&self, name: &str, fields: Value) {
        if self.off() {
            return;
        }
        let mut entry = match fields {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        entry.insert("event".into(), name.into());
        entry.insert("t_ms".into(), ((now() - self.0.born) as u64).into());
        let mut queue = self.0.queue.borrow_mut();
        if queue.len() < 200 {
            queue.push_back(entry);
        }
        drop(queue);
        self.pump();
    }

    /// Sends queued events one at a time, so the log keeps their order.
    fn pump(&self) {
        let Link::Open(id) = &*self.0.link.borrow() else {
            return;
        };
        if self.0.sending.replace(true) {
            return;
        }
        let (path, inner) = (format!("/diagnostics/{id}/event"), self.0.clone());
        spawn_local(async move {
            loop {
                let Some(entry) = inner.queue.borrow_mut().pop_front() else {
                    break;
                };
                let answer = post(&path, &Value::Object(entry).to_string()).await;
                if !answer.is_some_and(|(status, _)| (200..300).contains(&status)) {
                    // The session expired, or the proxy restarted or is older: stop talking to it.
                    *inner.link.borrow_mut() = Link::Off;
                    inner.queue.borrow_mut().clear();
                    break;
                }
            }
            inner.sending.set(false);
        });
    }

    /// Watches `video` for stalls, errors and health. Calling it again for the same element
    /// does nothing; a new element replaces the old one.
    pub fn follow(&self, video: &HtmlVideoElement) {
        if self.off() || self.0.video.borrow().as_ref() == Some(video) {
            return;
        }
        self.unfollow();
        *self.0.video.borrow_mut() = Some(video.clone());
        let weak = Rc::downgrade(&self.0);
        let on = |what: &'static str| {
            let weak = weak.clone();
            Closure::<dyn FnMut()>::new(move || {
                if let Some(inner) = weak.upgrade() {
                    Self(inner).seen(what);
                }
            })
        };
        let mut listeners = self.0.listeners.borrow_mut();
        for what in [
            "waiting", "playing", "stalled", "error", "ended", "pause", "play",
        ] {
            let callback = on(what);
            let _ = video.add_event_listener_with_callback(what, callback.as_ref().unchecked_ref());
            listeners.push((what, callback));
        }
        drop(listeners);
        Self::health(weak);
    }

    fn unfollow(&self) {
        if let Some(video) = self.0.video.borrow_mut().take() {
            for (what, callback) in self.0.listeners.borrow_mut().drain(..) {
                let _ = video
                    .remove_event_listener_with_callback(what, callback.as_ref().unchecked_ref());
            }
        }
    }

    /// Where the player is: its position, how much is buffered past it, and its states.
    fn state(&self) -> Map<String, Value> {
        let mut out = Map::new();
        if let Some(v) = self.0.video.borrow().as_ref() {
            let (b, at) = (v.buffered(), v.current_time());
            let ahead = (0..b.length())
                .filter_map(|i| Some((b.start(i).ok()?, b.end(i).ok()?)))
                .find(|(s, e)| *s <= at + 0.5 && at <= *e)
                .map_or(0.0, |(_, e)| e - at);
            out.insert("at_s".into(), json!((at * 10.0).round() / 10.0));
            out.insert("ahead_s".into(), json!((ahead * 10.0).round() / 10.0));
            out.insert("ready".into(), v.ready_state().into());
            out.insert("network".into(), v.network_state().into());
            out.insert("paused".into(), v.paused().into());
        }
        out
    }

    fn seen(&self, what: &str) {
        let t = now();
        match what {
            "waiting" => {
                if self.0.waiting_since.get().is_none() {
                    self.0.waiting_since.set(Some(t));
                    self.0.stuck_reported.set(false);
                }
            }
            "playing" => {
                let waited = self.0.waiting_since.take().map(|since| t - since);
                if self.0.first_play.replace(false) {
                    // The first wait is start-up: reported as how long until it played.
                    let mut fields = self.state();
                    fields.insert("startup_ms".into(), ((t - self.0.born) as u64).into());
                    self.event("playing", Value::Object(fields));
                } else if let Some(ms) = waited.filter(|ms| *ms >= 250.0) {
                    self.0.stalls.set(self.0.stalls.get() + 1);
                    self.0.stalled_ms.set(self.0.stalled_ms.get() + ms);
                    let mut fields = self.state();
                    fields.insert("ms".into(), (ms as u64).into());
                    fields.insert("count".into(), self.0.stalls.get().into());
                    self.event("stall", Value::Object(fields));
                }
            }
            // The browser fires `stalled` whenever a download idles for 3 s, which HLS does
            // between segments: only a short buffer makes it news.
            "stalled" => {
                let state = self.state();
                if state.get("ahead_s").and_then(Value::as_f64).unwrap_or(0.0) < 2.0 {
                    self.event("net_stalled", Value::Object(state));
                }
            }
            "error" => {
                let error = self
                    .0
                    .video
                    .borrow()
                    .as_ref()
                    .and_then(|v| js_sys::Reflect::get(v, &"error".into()).ok())
                    .filter(|e| !e.is_null() && !e.is_undefined());
                let get = |key: &str| {
                    error
                        .as_ref()
                        .and_then(|e| js_sys::Reflect::get(e, &key.into()).ok())
                };
                let mut fields = self.state();
                fields.insert(
                    "code".into(),
                    get("code").and_then(|c| c.as_f64()).unwrap_or(0.0).into(),
                );
                fields.insert(
                    "message".into(),
                    get("message")
                        .and_then(|m| m.as_string())
                        .unwrap_or_default()
                        .into(),
                );
                // Code 0 is no error: a source being swapped out fires `error` too.
                if fields.get("code").and_then(Value::as_f64).unwrap_or(0.0) > 0.0 {
                    self.event("media_error", Value::Object(fields));
                }
            }
            "ended" => self.event("ended", Value::Object(self.state())),
            // Context for everything else: a pause is the viewer's (or the browser's, for a
            // muted video in a background tab), not a stall.
            "pause" | "play" => self.event(what, Value::Object(self.state())),
            _ => {}
        }
    }

    /// Every 5 s: a wait that has gone on too long becomes `stuck`; every 30 s, a health sample.
    fn health(weak: Weak<Inner>) {
        spawn_local(async move {
            let mut tick = 0_u32;
            let mut last = (0_u32, 0_u32, now());
            loop {
                rstreamkit::mse::sleep(Duration::from_secs(5)).await;
                let Some(inner) = weak.upgrade() else { return };
                let trace = Self(inner);
                if trace.0.closed.get() || trace.off() {
                    return;
                }
                let Some(video) = trace.0.video.borrow().clone() else {
                    return;
                };
                // Waiting mid-play, or never having started at all.
                let since = trace.0.waiting_since.get().or_else(|| {
                    trace
                        .0
                        .first_play
                        .get()
                        .then_some(trace.0.born + STARTUP_MS - STUCK_MS)
                });
                // (Paused mid-play is the viewer's choice; never having started is not.)
                if let Some(since) = since
                    && now() - since > STUCK_MS
                    && (!video.paused() || trace.0.first_play.get())
                    // An error was already reported: not stuck, failed.
                    && js_sys::Reflect::get(&video, &"error".into()).is_ok_and(|e| e.is_null())
                    && !trace.0.stuck_reported.replace(true)
                {
                    let mut fields = trace.state();
                    fields.insert("ms".into(), ((now() - since) as u64).into());
                    fields.insert("starting".into(), trace.0.first_play.get().into());
                    trace.event("stuck", Value::Object(fields));
                }
                tick += 1;
                if !tick.is_multiple_of(HEALTH_EVERY) || video.paused() {
                    continue;
                }
                let quality = video.get_video_playback_quality();
                let (frames, dropped, t) = (
                    quality.total_video_frames(),
                    quality.dropped_video_frames(),
                    now(),
                );
                let secs = ((t - last.2) / 1000.0).max(1.0);
                let mut fields = trace.state();
                fields.insert(
                    "fps".into(),
                    json!((f64::from(frames.saturating_sub(last.0)) / secs).round()),
                );
                fields.insert("dropped".into(), dropped.saturating_sub(last.1).into());
                fields.insert("width".into(), video.video_width().into());
                fields.insert("height".into(), video.video_height().into());
                fields.insert(
                    "audio_bytes".into(),
                    js_sys::Reflect::get(&video, &"webkitAudioDecodedByteCount".into())
                        .ok()
                        .and_then(|n| n.as_f64())
                        .map_or(Value::Null, |n| json!(n)),
                );
                fields.insert(
                    "hidden".into(),
                    web_sys::window()
                        .and_then(|w| w.document())
                        .is_some_and(|d| d.hidden())
                        .into(),
                );
                last = (frames, dropped, t);
                trace.event("health", Value::Object(fields));
            }
        });
    }

    /// The viewer left: a summary, then nothing more.
    pub fn close(&self) {
        if self.0.closed.replace(true) {
            return;
        }
        let mut fields = self.state();
        fields.insert("stalls".into(), self.0.stalls.get().into());
        fields.insert("stalled_ms".into(), (self.0.stalled_ms.get() as u64).into());
        self.event("closed", Value::Object(fields));
        self.unfollow();
    }
}

/// Whether a status line the player shows means it has given up (as opposed to working on it).
pub fn is_failure(status: &str) -> bool {
    !matches!(
        status,
        "Starting playback…"
            | "Waiting for picture…"
            | "Live"
            | "Audio only"
            | "Trying another playback method…"
            | "Trying a compatible stream…"
            | "Re-encoding the video…"
            | crate::RECONNECTING
            | "Stream ended"
            | crate::STILL_STARTING
    )
}
