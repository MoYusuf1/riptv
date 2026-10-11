//! The full-page player for movies and episodes, and what it plays.
use super::*;

/// Something to watch: a movie or an episode.
#[derive(Clone, PartialEq)]
pub(crate) struct Play {
    /// Remembers where the viewer got to (`movie:12`, `episode:340`).
    pub(crate) key: String,
    pub(crate) title: String,
    /// "S1 E3 · Title" for an episode.
    pub(crate) subtitle: Option<String>,
    /// What a download is called.
    pub(crate) file: String,
    pub(crate) url: String,
    pub(crate) duration: Option<u64>,
    /// The title as a shelf remembers it (a series, for an episode).
    pub(crate) entry: Option<shelves::Entry>,
}

/// What a key press asks the player's page to do, from outside Dioxus (see `Watch`).
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Act {
    Close,
    Next,
}

pub(crate) const WATCH_MEDIA_ACTIONS: &[(&str, media_session::Action)] = &[
    ("play", media_session::Action::Play),
    ("pause", media_session::Action::Pause),
    ("seekbackward", media_session::Action::Back),
    ("seekforward", media_session::Action::Forward),
    ("previoustrack", media_session::Action::Previous),
    ("stop", media_session::Action::Stop),
];
pub(crate) const WATCH_MEDIA_ACTIONS_NEXT: &[(&str, media_session::Action)] = &[
    ("play", media_session::Action::Play),
    ("pause", media_session::Action::Pause),
    ("seekbackward", media_session::Action::Back),
    ("seekforward", media_session::Action::Forward),
    ("previoustrack", media_session::Action::Previous),
    ("nexttrack", media_session::Action::Next),
    ("stop", media_session::Action::Stop),
];

pub(crate) fn watch_video() -> Option<web_sys::HtmlVideoElement> {
    web_sys::window()?
        .document()?
        .get_element_by_id("watch-video")?
        .dyn_into()
        .ok()
}

/// Seconds buffered up to, in the range the playhead is in.
pub(crate) fn buffered_end(v: &web_sys::HtmlVideoElement) -> f64 {
    let (b, t) = (v.buffered(), v.current_time());
    (0..b.length())
        .filter_map(|i| Some((b.start(i).ok()?, b.end(i).ok()?)))
        .find(|(s, e)| *s <= t + 0.1 && t <= *e)
        .map_or(0.0, |r| r.1)
}

pub(crate) fn fullscreen_watch() {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    if doc.fullscreen_element().is_some() {
        doc.exit_fullscreen();
    } else if let Some(el) = doc.get_element_by_id("watch") {
        let _ = el.request_fullscreen();
    }
}

/// The full-page player for a movie or episode: the picture, a bar along the top with the title,
/// and one along the bottom with the seek bar. Which engine plays it is [`choose`]'s decision.
#[component]
pub(crate) fn Watch(
    play: Play,
    next: Option<String>,
    onclose: EventHandler<()>,
    onnext: EventHandler<()>,
) -> Element {
    let session = use_context::<Signal<Option<Client>>>();
    let client = use_hook(|| session.read().clone().expect("logged in"));
    let experimental = use_context::<Signal<bool>>();
    let key = play.key.clone();
    let resume = use_hook(|| shelves::position(&key));
    let remembered = use_hook(preferences::load);
    let saved_volume = u32::from(remembered.volume);
    let saved_speed = remembered.speed;
    let has_next = next.is_some();

    let mut engine = use_signal(|| None::<Engine>);
    // A converted stream has no index a `<video>` could seek in, so the page restarts it: this is
    // the second of the movie where it begins.
    let mut start = use_signal(move || resume as u64);
    let mut pos = use_signal(move || resume);
    let duration_hint = play.duration;
    let mut total = use_signal(move || duration_hint.unwrap_or(0) as f64);
    let mut fallback_duration = use_signal(|| None::<f64>);
    let mut duration_checked = use_signal(|| false);
    let mut ahead = use_signal(|| 0.0_f64);
    let mut paused = use_signal(|| false);
    let mut waiting = use_signal(|| true);
    // How far the current source has got with starting, for the progress ring.
    let mut load_stage = use_signal(|| 0_u8);
    // How close the title is to playing, as a percentage that only moves forward: deciding how
    // to play it, the request, then each second buffered (3 s is plenty to start), the first
    // frame, playing.
    let mut ready = use_signal(|| 0_u32);
    // Bumped by every seek: each one is a new wait, measured from zero.
    let mut seeks = use_signal(|| 0_u32);
    let ring_from = use_hook(|| Rc::new(Cell::new(u32::MAX)));
    // A moment after a start, to let the ring land on 100% instead of vanishing short of it.
    let mut landing = use_signal(|| false);
    use_effect(move || {
        let buffered = (ahead() - pos()).max(0.0);
        let target = match load_stage() {
            0 => 8,
            1 => 25,
            2 => 45 + ((buffered / 3.0).min(1.0) * 45.0) as u32,
            3 => 92,
            // Playing already: a wait here is a seek within the same stream or the buffer
            // running dry; 2 s of picture at the new position is ready to go.
            _ if waiting() => ((buffered / 2.0).min(1.0) * 95.0) as u32,
            _ => 100,
        };
        // A seek starts the ring again from where that seek's wait stands (from zero); a switch
        // of player (the experimental one stalling) carries on from where the ring was.
        let seek = seeks();
        let now = *ready.peek();
        if target > now || seek != ring_from.get() {
            ready.set(target);
        }
        ring_from.set(seek);
    });
    let mut muted = use_signal(move || saved_volume == 0);
    let mut volume = use_signal(move || saved_volume);
    let mut rate = use_signal(move || saved_speed);
    let mut scrubbing = use_signal(|| false);
    // Whether the pointer is over the controls (they stay up while it is).
    let mut over_controls = use_signal(|| false);
    // Where the current drag on the seek bar began (see `let_go`).
    let mut scrub_from = use_signal(|| 0.0_f64);
    let mut menu = use_signal(|| false);
    let mut resumed = use_signal(move || (resume > 0.0).then_some(resume as u64));
    let mut up_next = use_signal(|| None::<u32>);
    let active = use_signal(|| true);
    let idle = use_hook(|| IdleHide::new(active, 2800.0));
    let handle = use_hook(|| Rc::new(RefCell::new(None::<experimental::Player>)));
    let saved = use_hook(|| Rc::new(Cell::new(resume)));

    let watch_element = use_hook(|| Rc::new(RefCell::new(None::<web_sys::HtmlVideoElement>)));
    // With `riptv --logs`: what this title's player does, for the proxy's diagnostics log.
    let trace = use_hook(|| {
        let upstream = xtream::Url::parse(&play.url)
            .map(|u| client.upstream(&u).to_string())
            .unwrap_or_default();
        diag::Trace::start(
            &upstream,
            serde_json::json!({
                "kind": play.key.split(':').next().unwrap_or("title"),
                "title": play.title,
                "episode": play.subtitle,
                "resume_s": resume as u64,
                "experimental": *experimental.peek(),
            }),
        )
    });
    {
        let (trace, watch_element) = (trace.clone(), watch_element.clone());
        use_effect(move || {
            match engine() {
                None => return,
                Some(Engine::Native(duration)) => trace.event("engine", serde_json::json!({ "engine": "native", "duration_s": duration })),
                Some(Engine::Rust(..)) => trace.event("engine", serde_json::json!({ "engine": "rust" })),
                Some(Engine::Converted(c, why)) => trace.event(
                    "engine",
                    serde_json::json!({ "engine": "ffmpeg", "why": why, "from_s": *start.peek(), "duration_s": c.duration }),
                ),
                Some(Engine::Failed(why)) => trace.event("failure", serde_json::json!({ "text": why })),
            }
            if let Some(video) = watch_video() {
                trace.follow(&video);
                *watch_element.borrow_mut() = Some(video);
            }
        });
    }
    {
        let trace = trace.clone();
        let watch_element = watch_element.clone();
        use_drop(move || {
            trace.close();
            if let Some(video) = watch_element.borrow_mut().take() {
                release(&video);
            }
        });
    }

    // Decide what plays it, before any of it plays.
    let (c, url) = (client.clone(), play.url.clone());
    use_future(move || {
        let (c, url) = (c.clone(), url.clone());
        async move { engine.set(Some(choose(&c, &url, *experimental.peek()).await)) }
    });
    // Browser metadata can omit the length even when playback works. Try the server's
    // independent file probe once, in the background; don't convert or restart playback.
    let (duration_client, duration_url) = (client.clone(), play.url.clone());
    use_effect(move || {
        let selected = engine();
        let Some(selected_engine) = selected.as_ref() else {
            return;
        };
        let converted = matches!(selected_engine, Engine::Converted(..));
        let known = selected_engine.duration();
        let length = timeline::duration(converted, known, duration_hint, f64::NAN);
        if length > 0.0 {
            // Don't wait for the browser's metadata event to enable the full timeline.
            total.set(length);
            return;
        }
        if total() > 0.0
            || !matches!(selected.as_ref(), Some(Engine::Native(..)))
            || duration_checked()
        {
            return;
        }
        duration_checked.set(true);
        let (c, url) = (duration_client.clone(), duration_url.clone());
        spawn(async move {
            if let Some(seconds) = playback::probe_duration(&c, &url).await {
                fallback_duration.set(Some(seconds));
                total.set(seconds);
            }
        });
    });
    // The hint about resuming fades on its own.
    use_future(move || async move {
        rstreamkit::mse::sleep(Duration::from_secs(7)).await;
        resumed.set(None);
    });

    // The Rust engine drives a `<video>` of its own making; the others are plain attributes.
    let (c, url) = (client.clone(), play.url.clone());
    use_effect(move || {
        let Some(Engine::Rust(movie, media)) = engine() else {
            handle.borrow_mut().take();
            return;
        };
        let Some(video) = watch_video() else { return };
        let (c, url) = (c.clone(), url.clone());
        // A stall the experimental player doesn't report (it appended data, but no frame is ever
        // decoded): after 8 s, hand over to the standard player, as for a format it can't play.
        {
            let (c, url, this) = (c.clone(), url.clone(), movie.clone());
            wasm_bindgen_futures::spawn_local(async move {
                rstreamkit::mse::sleep(Duration::from_secs(8)).await;
                let still = engine
                    .try_peek()
                    .is_ok_and(|e| matches!(&*e, Some(Engine::Rust(m, _)) if Rc::ptr_eq(m, &this)));
                if still && *load_stage.peek() < 3 {
                    switch_to_convert(
                        c,
                        url,
                        "the experimental player stalled".into(),
                        *pos.peek(),
                        engine,
                        start,
                    );
                }
            });
        }
        *handle.borrow_mut() = Some(experimental::movie(
            video,
            movie,
            media.to_string(),
            c.clone(),
            *pos.peek(),
            move |e| match e {
                experimental::Event::Playing => waiting.set(false),
                experimental::Event::Unsupported(why) | experimental::Event::NoSound(why) => {
                    switch_to_convert(c.clone(), url.clone(), why, *pos.peek(), engine, start)
                }
                experimental::Event::Failed(e) => engine.set(Some(Engine::Failed(e))),
                experimental::Event::Note(_) | experimental::Event::Ended => {}
            },
        ));
    });

    // What the `<video>` reports, as seconds of the movie.
    let mut sync = move || {
        let Some(v) = watch_video() else { return };
        let known = engine.peek().as_ref().and_then(Engine::duration);
        let converted = matches!(engine.peek().as_ref(), Some(Engine::Converted(..)));
        let offset = if converted { *start.peek() as f64 } else { 0.0 };
        // Only real changes: `progress` fires for every chunk a converted stream delivers, and
        // each signal write re-renders the player.
        let now = offset + v.current_time();
        if !*scrubbing.peek() && (now - *pos.peek()).abs() >= 0.2 {
            pos.set(now);
        }
        let known = known
            .filter(|d| d.is_finite() && *d > 0.0)
            .or(*fallback_duration.peek());
        let length = timeline::retain(
            *total.peek(),
            timeline::duration(converted, known, duration_hint, v.duration()),
        );
        if length != *total.peek() {
            total.set(length);
        }
        let end = offset + buffered_end(&v);
        if (end - *ahead.peek()).abs() >= 0.25 {
            ahead.set(end);
        }
    };
    let mut seek_to = move |t: f64| {
        let length = *total.peek();
        let t = if length > 0.0 {
            t.clamp(0.0, length)
        } else {
            t.max(0.0)
        };
        pos.set(t);
        seeks += 1;
        if matches!(engine.peek().as_ref(), Some(Engine::Converted(..))) {
            // A new stream from `t`: count it up by its own stages, not the old stream's buffer.
            load_stage.set(0);
            start.set(t as u64);
            waiting.set(true);
        } else if let Some(v) = watch_video() {
            v.set_current_time(t);
        }
    };
    // A drag that ends where it began: the browser fires no `change` (the value didn't move), so
    // without this the bar would stay frozen while the film plays on. Release, a cancelled touch
    // or losing focus unfreeze it, but only when the bar is back where the drag began: a release
    // can arrive before the slider's last value, so it never seeks. A real move always ends in
    // `change`, which does.
    let mut let_go = move || {
        let (at, from) = (*pos.peek(), *scrub_from.peek());
        if *scrubbing.peek() && (at - from).abs() < 1.0 {
            scrubbing.set(false);
        }
    };
    let mut set_level = move |level: u32| {
        let level = level.min(100);
        volume.set(level);
        muted.set(level == 0);
        preferences::update(|p| p.volume = level as u8);
        if let Some(v) = watch_video() {
            v.set_volume(f64::from(level) / 100.0);
            v.set_muted(level == 0);
        }
    };
    let toggle_watch = move || {
        if let Some(v) = watch_video() {
            toggle(&v);
        }
    };

    // Keys, on the whole page (a plain listener: Dioxus's keyboard events add ~26 KB of wasm). The
    // listener runs outside Dioxus, where only signals may be touched: no `spawn`, no
    // `EventHandler`. So it asks, and an effect (inside Dioxus) does.
    let mut act = use_signal(|| None::<Act>);
    use_effect(move || {
        if let Some(what) = act() {
            act.set(None);
            match what {
                Act::Close => onclose.call(()),
                Act::Next => onnext.call(()),
            }
        }
    });
    let mut media_seek = use_signal(|| None::<f64>);
    use_effect(move || {
        if let Some(delta) = media_seek() {
            media_seek.set(None);
            seek_to(if delta.is_infinite() {
                0.0
            } else {
                *pos.peek() + delta
            });
        }
    });
    let media = use_hook(|| Rc::new(RefCell::new(None::<media_session::Session>)));
    let media_for_install = media.clone();
    let media_title = play.title.clone();
    let media_artist = play.subtitle.clone().unwrap_or_else(|| "RIPTV".into());
    use_effect(move || {
        let installed = media_session::Session::install(
            &media_title,
            &media_artist,
            if has_next {
                WATCH_MEDIA_ACTIONS_NEXT
            } else {
                WATCH_MEDIA_ACTIONS
            },
            move |action| match action {
                media_session::Action::Play => {
                    if let Some(v) = watch_video() {
                        let _ = v.play();
                    }
                }
                media_session::Action::Pause => {
                    if let Some(v) = watch_video() {
                        let _ = v.pause();
                    }
                }
                media_session::Action::Back => media_seek.set(Some(-15.0)),
                media_session::Action::Forward => media_seek.set(Some(15.0)),
                media_session::Action::Previous => media_seek.set(Some(f64::INFINITY)),
                media_session::Action::Next if has_next => act.set(Some(Act::Next)),
                media_session::Action::Stop => act.set(Some(Act::Close)),
                _ => {}
            },
        );
        if let (Some(session), Some(video)) = (installed.as_ref(), watch_video()) {
            session.playing(!video.paused());
        }
        *media_for_install.borrow_mut() = installed;
    });
    let media_on_play = media.clone();
    let media_on_pause = media.clone();
    let media_on_time = media.clone();
    use_drop(move || {
        media.borrow_mut().take();
    });
    let last_media_second = use_hook(|| Rc::new(Cell::new(u64::MAX)));
    let key_idle = idle.clone();
    let listener = use_hook(|| {
        Rc::new(RefCell::new(
            None::<Closure<dyn FnMut(web_sys::KeyboardEvent)>>,
        ))
    });
    let installed = listener.clone();
    use_effect(move || {
        let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let key_idle = key_idle.clone();
        let on_key =
            Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
                if e.ctrl_key() || e.meta_key() || e.alt_key() {
                    return;
                }
                let key = e.key().to_ascii_lowercase();
                // Values are read into locals first: a `peek()` guard lives to the end of its
                // statement, and these calls write the same signals.
                let (now, level) = (pos(), volume());
                match key.as_str() {
                    " " | "k" => toggle_watch(),
                    "arrowleft" | "j" => seek_to(now - 15.0),
                    "arrowright" | "l" => seek_to(now + 15.0),
                    "arrowup" => set_level((level + 10).min(100)),
                    "arrowdown" => set_level(level.saturating_sub(10)),
                    "m" => {
                        if let Some(v) = watch_video() {
                            v.set_muted(!v.muted());
                        }
                    }
                    "f" => fullscreen_watch(),
                    "n" if has_next => act.set(Some(Act::Next)),
                    "escape" => act.set(Some(Act::Close)),
                    _ => return,
                }
                e.prevent_default();
                key_idle.wake();
            });
        let _ = doc.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
        *installed.borrow_mut() = Some(on_key);
    });
    use_drop(move || {
        if let (Some(on_key), Some(doc)) = (
            listener.borrow_mut().take(),
            web_sys::window().and_then(|w| w.document()),
        ) {
            let _ =
                doc.remove_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
        }
    });

    let (engine_now, kind, src) = match engine() {
        Some(Engine::Native(..)) => (true, "native", Some(play.url.clone())),
        Some(Engine::Rust(..)) => (true, "rust", None),
        Some(Engine::Converted(c, _)) => (true, "converted", Some(c.at(start()).to_string())),
        _ => (false, "", None),
    };
    let (p, b) = {
        let t = total().max(1.0);
        // To 0.1% (1.4 px on a 1440 px bar): the bar's style is rewritten only when it would move.
        let pct = |x: f64| ((x / t * 1000.0).round() / 10.0).clamp(0.0, 100.0);
        (pct(pos()), pct(ahead()))
    };
    let shown_volume = if muted() { 0 } else { volume() };
    let class = format!(
        "watch{}{}",
        if paused() { " paused" } else { "" },
        if active() || scrubbing() || over_controls() {
            " active"
        } else {
            ""
        }
    );
    let (c, url, key, saved, entry) = (
        client.clone(),
        play.url.clone(),
        play.key.clone(),
        saved.clone(),
        play.entry.clone(),
    );
    let native_url = play.url.clone();
    rsx! {
        div {
            class: "{class}",
            id: "watch",
            tabindex: "0",
            onmousemove: move |_| idle.wake(),
            ondoubleclick: move |_| fullscreen_watch(),
            if !engine_now {
                if let Some(Engine::Failed(why)) = engine() {
                    div { class: "w-fail",
                        strong { "This can't be played" }
                        p { "{why}" }
                    }
                } else {
                    div { class: "w-load", LoadRing { stage: 0, percent: Some(8) } }
                }
            } else {
                video {
                    key: "{kind}:{src.as_deref().unwrap_or_default()}",
                    id: "watch-video",
                    autoplay: true,
                    playsinline: true,
                    src: src,
                    onclick: move |_| { menu.set(false); toggle_watch(); },
                    onplay: move |_| {
                        paused.set(false);
                        if let Some(s) = media_on_play.borrow().as_ref() { s.playing(true); }
                    },
                    onpause: move |_| {
                        paused.set(true);
                        if let Some(s) = media_on_pause.borrow().as_ref() { s.playing(false); }
                    },
                    onwaiting: move |_| waiting.set(true),
                    // A new source (its own element) starts the ring over.
                    onloadstart: move |_| load_stage.set(1),
                    onloadeddata: move |_| if *load_stage.peek() < 3 { load_stage.set(3) },
                    onplaying: move |_| {
                        // Whatever the ring was waiting for (a start, a seek, a refill): finish it.
                        if (*load_stage.peek() < 4 || *waiting.peek()) && *ready.peek() < 100 {
                            landing.set(true);
                            spawn(async move {
                                rstreamkit::mse::sleep(Duration::from_millis(1100)).await;
                                landing.set(false);
                            });
                        }
                        load_stage.set(4);
                        waiting.set(false);
                        paused.set(false);
                    },
                    oncanplay: move |_| waiting.set(false),
                    onloadedmetadata: move |_| {
                        if *load_stage.peek() < 2 {
                            load_stage.set(2);
                        }
                        if let Some(v) = watch_video() {
                            v.set_volume(f64::from(volume()) / 100.0);
                            v.set_muted(volume() == 0);
                            v.set_playback_rate(rate());
                        }
                        // A plain file starts where the viewer left off; the other engines were told.
                        let native = matches!(engine.peek().as_ref(), Some(Engine::Native(..)));
                        if resume > 0.0
                            && native
                            && let Some(v) = watch_video()
                        {
                            v.set_current_time(resume);
                        }
                        sync();
                    },
                    ondurationchange: move |_| sync(),
                    onprogress: move |_| sync(),
                    onratechange: move |_| {
                        if let Some(v) = watch_video() { rate.set(v.playback_rate()); }
                    },
                    onvolumechange: move |_| {
                        if let Some(v) = watch_video() {
                            muted.set(v.muted());
                            let level = (v.volume() * 100.0).round() as u32;
                            volume.set(level);
                        }
                    },
                    ontimeupdate: {
                        let (c, url, key, saved, entry) = (c.clone(), url.clone(), key.clone(), saved.clone(), entry.clone());
                        move |_| {
                            sync();
                            if let Some(s) = media_on_time.borrow().as_ref() {
                                let second = *pos.peek() as u64;
                                if last_media_second.get() != second {
                                    last_media_second.set(second);
                                    s.position(*pos.peek(), *total.peek(), *rate.peek());
                                }
                            }
                            // Some of the file's sound the browser can't decode after all: go
                            // round through the converter rather than leave the film silent.
                            let native = matches!(engine.peek().as_ref(), Some(Engine::Native(..)));
                            if native
                                && let Some(v) = watch_video()
                                && no_audio_decoded(&v, 4.0)
                            {
                                switch_to_convert(c.clone(), url.clone(), "sound the browser can't decode".into(), *pos.peek(), engine, start);
                            }
                            if (*pos.peek() - saved.get()).abs() >= 5.0 {
                                saved.set(*pos.peek());
                                shelves::save(&key, *pos.peek(), *total.peek(), entry.as_ref());
                            }
                        }
                    },
                    onerror: {
                        let (c, url) = (c.clone(), url.clone());
                        move |_| {
                            let native = matches!(engine.peek().as_ref(), Some(Engine::Native(..)));
                            let converted = matches!(engine.peek().as_ref(), Some(Engine::Converted(..)));
                            if native {
                                switch_to_convert(c.clone(), url.clone(), "a format the browser can't play".into(), *pos.peek(), engine, start);
                            } else if converted {
                                engine.set(Some(Engine::Failed("The converted stream stopped: the source may have ended.".into())));
                            }
                        }
                    },
                    onended: move |_| {
                        paused.set(true);
                        shelves::finish(&key, entry.as_ref());
                        if has_next {
                            up_next.set(Some(8));
                            spawn(async move {
                                while let Some(n) = up_next() {
                                    if n == 0 {
                                        onnext.call(());
                                        break;
                                    }
                                    rstreamkit::mse::sleep(Duration::from_secs(1)).await;
                                    if up_next().is_some() { up_next.set(Some(n - 1)); }
                                }
                            });
                        }
                    },
                }
                // One ring from the start to the picture: when it's ready the same ring sweeps on to
                // 100% and fades (a new element would have nothing to animate from).
                // Until it plays: `canplay` clears `waiting` a moment before `playing`, which would
                // drop the ring for an instant and bring back a new one. Never while dragging the
                // seek bar: the wait starts when the drag ends.
                if !scrubbing() && ((load_stage() < 4 && !paused()) || (waiting() && !paused()) || landing()) {
                    div { class: if landing() { "hud landing" } else { "hud" },
                        LoadRing { stage: load_stage(), percent: Some(if landing() { 100 } else { ready() }) }
                    }
                }
                if paused() && !waiting() && up_next().is_none() {
                    button { class: "bigplay", aria_label: "Play", onclick: move |_| toggle_watch(), Icon { d: PLAY } }
                }
            }
            div { class: "w-top",
                div { button { class: "icon-btn back", aria_label: "Back", title: "Back (Esc)", onclick: move |_| onclose.call(()), Icon { d: BACK } } }
                div { class: "w-title",
                    if engine_now {
                        strong { "{play.title}" }
                        if let Some(sub) = &play.subtitle { small { "{sub}" } }
                    }
                }
                // Kept empty so the title stays centred; downloading is in the bottom row.
                div { class: "end" }
            }
            if engine_now {
                if menu() {
                    button { class: "scrim", aria_label: "Close menu", onclick: move |_| menu.set(false) }
                    div { class: "w-menu",
                        for r in [0.5_f64, 0.75, 1.0, 1.25, 1.5, 2.0] {
                            button {
                                key: "{r}",
                                class: if rate() == r { "on" } else { "" },
                                onclick: move |_| {
                                    if let Some(v) = watch_video() { v.set_playback_rate(r); }
                                    rate.set(r);
                                    preferences::update(|p| p.speed = r);
                                },
                                span { if r == 1.0 { "Normal" } else { "{r}×" } }
                                if rate() == r { Icon { d: CHECK } }
                            }
                        }
                    }
                }
                if let Some(t) = resumed() {
                    div { class: "toast",
                        "Resumed at {hms(t)}"
                        button { onclick: move |_| { seek_to(0.0); resumed.set(None); }, "Start over" }
                    }
                }
                if let (Some(n), Some(label)) = (up_next(), next.clone()) {
                    div { class: "upnext",
                        small { "Up next in {n}" }
                        strong { "{label}" }
                        div {
                            button { onclick: move |_| up_next.set(None), "Cancel" }
                            button { class: "go", onclick: move |_| onnext.call(()), "Play now" }
                        }
                    }
                }
                div { class: "w-bottom",
                    // A resting pointer must not let the bar fade from under it: the next press
                    // would land on the picture (pausing it) instead of seeking.
                    onmouseenter: move |_| over_controls.set(true),
                    onmouseleave: move |_| over_controls.set(false),
                // The bar, and while it's dragged, where you'd land.
                div { class: "w-seekwrap",
                        input {
                            class: if scrubbing() { "w-seek dragging" } else { "w-seek" },
                            r#type: "range",
                            min: "0",
                            max: "{total() as u64}",
                            step: "1",
                            value: "{pos() as u64}",
                            style: "--sp:{p}%;--sb:{b}%",
                            disabled: total() <= 0.0,
                            aria_label: "Seek",
                            title: if total() > 0.0 { "Seek" } else { "Total length unavailable from this stream" },
                            oninput: move |e| {
                                if !*scrubbing.peek() {
                                    let from = *pos.peek();
                                    scrub_from.set(from);
                                    scrubbing.set(true);
                                }
                                pos.set(e.value().parse().unwrap_or(0.0));
                            },
                            onchange: move |e| {
                                scrubbing.set(false);
                                seek_to(e.value().parse().unwrap_or(0.0));
                                // Give the focus back (arrow keys never move this bar: the player's
                                // own keys take them), or the controls would stay up for the rest of
                                // the film: they show while anything in them has focus.
                                if let Some(bar) = web_sys::window()
                                    .and_then(|w| w.document())
                                    .and_then(|d| d.active_element())
                                    .filter(|el| el.class_name().split(' ').any(|c| c == "w-seek"))
                                    && let Ok(blur) = js_sys::Reflect::get(&bar, &"blur".into())
                                    && let Some(blur) = blur.dyn_ref::<js_sys::Function>()
                                {
                                    let _ = blur.call0(&bar);
                                }
                            },
                            onpointerup: move |_| let_go(),
                            onpointercancel: move |_| let_go(),
                            onblur: move |_| let_go(),
                        }
                    if scrubbing() {
                        span { class: "w-tip", style: "--sp:{p}%", "{hms(pos() as u64)}" }
                    }
                }
                    div { class: "w-row",
                        button { class: "ctl", aria_label: "Play or pause", title: "Play or pause (Space)", onclick: move |_| toggle_watch(),
                            Icon { d: if paused() { PLAY } else { PAUSE } }
                        }
                        button { class: "ctl skip", aria_label: "Back 15 seconds", title: "Back 15 seconds (←)", onclick: move |_| seek_to(pos() - 15.0), Skip { back: true } }
                        button { class: "ctl skip", aria_label: "Forward 15 seconds", title: "Forward 15 seconds (→)", onclick: move |_| seek_to(pos() + 15.0), Skip { back: false } }
                        div { class: "volume",
                            button {
                                class: "ctl",
                                aria_label: "Mute",
                                title: "Mute (M)",
                                onclick: move |_| {
                                    if let Some(v) = watch_video() { v.set_muted(!v.muted()); }
                                },
                                Icon { d: if muted() { MUTED } else { VOLUME } }
                            }
                            input {
                                class: "vol",
                                r#type: "range",
                                min: "0",
                                max: "100",
                                aria_label: "Volume",
                                value: "{shown_volume}",
                                oninput: move |e| set_level(e.value().parse().unwrap_or(100)),
                            }
                        }
                        span { class: "w-time",
                            "{hms(pos() as u64)}"
                            if total() > 0.0 { span { " / {hms(total().round() as u64)}" } }
                            else { span { title: "Total length unavailable from this stream", " / —" } }
                        }
                        span { class: "grow" }
                        if has_next {
                            button { class: "next-btn", title: "Next episode (N)", onclick: move |_| onnext.call(()),
                                Icon { d: NEXT } "Next"
                            }
                        }
                        if engine_now {
                            a {
                                class: "ctl",
                                href: "{native_url}",
                                download: "{file_name(&play.file, &native_url)}",
                                title: "Download",
                                aria_label: "Download",
                                Icon { d: DOWNLOAD }
                            }
                        }
                        button { class: "ctl", aria_label: "Settings", title: "Settings", onclick: move |_| menu.set(!menu()), Icon { d: GEAR } }
                        button { class: "ctl", aria_label: "Picture in picture", title: "Picture in picture", onclick: move |_| toggle_pip(watch_video()), Icon { d: PIP } }
                        button { class: "ctl", aria_label: "Fullscreen", title: "Fullscreen (F)", onclick: move |_| fullscreen_watch(), Icon { d: FULLSCREEN } }
                    }
                }
            }
        }
    }
}
