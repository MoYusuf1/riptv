//! A live channel: the player (standard: the proxy's `/live` stream; experimental: rstreamkit,
//! which never falls back), its controls, and the channel's guide.

use super::*;

const LIVE_MEDIA_ACTIONS: &[(&str, media_session::Action)] = &[
    ("play", media_session::Action::Play),
    ("pause", media_session::Action::Pause),
    ("stop", media_session::Action::Stop),
];

/// Shown, with the progress ring, while a dropped live stream is opened again.
pub(crate) const RECONNECTING: &str = "Reconnecting…";

/// Shown, with the spinner, when a channel takes unusually long to start.
/// Whether a status means "on its way" (the ring), as opposed to playing or having given up.
pub(crate) fn starting(status: &str) -> bool {
    matches!(
        status,
        "Starting playback…"
            | "Waiting for picture…"
            | "Trying another playback method…"
            | "Trying a compatible stream…"
            | "Re-encoding the video…"
            | RECONNECTING
            | STILL_STARTING
    )
}

pub(crate) const STILL_STARTING: &str = "Still starting: your provider is slow to answer…";

/// A live stream through the Rust HLS player, with its own controls and the channel's guide. The
/// player stops when this component goes away (its handle is dropped), so picking another channel
/// ends the download loop.
/// What is driving a live channel's `<video>`: the Rust player, the Rust player with the sound
/// given up on (when the proxy can't convert it), or the proxy's ffmpeg-converted stream.
#[derive(Clone, PartialEq)]
pub(crate) enum Feed {
    Pending,
    Direct(String),
    Rust,
    Partial,
    Converted(String),
}

#[component]
pub(crate) fn LivePlayer(
    id: u64,
    title: String,
    url: String,
    /// The channel's logo, shown while it starts and for radio.
    icon: Option<String>,
    onchannel: EventHandler<ChannelAction>,
) -> Element {
    let mut paused = use_signal(|| false);
    let mut holding = use_signal(|| false);
    let session = use_context::<Signal<Option<Client>>>();
    let rust_sound = use_context::<Signal<bool>>();
    let client = use_hook(|| session.read().clone().expect("logged in"));
    let media = use_hook(|| Rc::new(RefCell::new(None::<media_session::Session>)));
    let media_for_install = media.clone();
    let media_title = title.clone();
    use_effect(move || {
        let installed = media_session::Session::install(
            &media_title,
            "Live TV · RIPTV",
            LIVE_MEDIA_ACTIONS,
            move |action| match action {
                media_session::Action::Play => {
                    if let Some(v) = video_el() {
                        let _ = v.play();
                    }
                }
                media_session::Action::Pause | media_session::Action::Stop => {
                    holding.set(false);
                    paused.set(true);
                    if let Some(v) = video_el() {
                        let _ = v.pause();
                    }
                }
                _ => {}
            },
        );
        if let (Some(session), Some(video)) = (installed.as_ref(), video_el()) {
            session.playing(!video.paused());
        }
        *media_for_install.borrow_mut() = installed;
    });
    let media_on_play = media.clone();
    let media_on_pause = media.clone();
    use_drop(move || {
        media.borrow_mut().take();
    });
    let saved_volume = u32::from(use_hook(preferences::load).volume);
    let mut status = use_signal(|| "Starting playback…".to_string());
    let mut picture_ready = use_signal(|| false);
    // How far the current start has got, for the progress ring (see `LoadRing`).
    let mut load_stage = use_signal(|| 0_u8);
    let mut audio_only = use_signal(|| false);
    let mut muted = use_signal(move || saved_volume == 0);
    let mut volume = use_signal(move || saved_volume);
    let mut buffering = use_signal(|| false);
    // A user pause cancels automatic buffer recovery; controls must always win.
    let mut toggle_play = move || {
        if *holding.peek() {
            holding.set(false);
            paused.set(true);
            if let Some(video) = video_el() {
                let _ = video.pause();
            }
        } else {
            crate::toggle_play();
        }
    };
    let mut expanded = use_signal(|| false);
    // Why there is no sound, when the player knows: its own note, or what the browser reports.
    let mut note = use_signal(|| None::<String>);
    let mut silent = use_signal(|| false);
    // The controls fade out after a moment of no mouse movement while playing.
    let active = use_signal(|| true);
    let idle = use_hook(|| IdleHide::new(active, 2500.0));
    let handle = use_hook(|| Rc::new(RefCell::new(None::<rstreamkit::mse::Player>)));
    let live_stream = use_hook(|| Rc::new(RefCell::new(None::<standard::Stream>)));
    {
        let live_stream = live_stream.clone();
        use_drop(move || {
            live_stream.borrow_mut().take();
        });
    }
    // With `riptv --logs`: what this channel's player does, for the proxy's diagnostics log.
    let trace = use_hook(|| {
        let upstream = xtream::Url::parse(&url)
            .map(|u| client.upstream(&u).to_string())
            .unwrap_or_default();
        diag::Trace::start(
            &upstream,
            serde_json::json!({
                "kind": "live",
                "channel": id,
                "title": title,
                "experimental": *rust_sound.peek(),
            }),
        )
    });

    // Standard: the proxy's live stream, from the first moment (an iPhone, which can't play one,
    // uses its own HLS player). Experimental: rstreamkit, and nothing else.
    let mut feed = use_signal(|| {
        if *rust_sound.peek() {
            Feed::Rust
        } else if standard::plays_live_streams() {
            standard::live(&client, &url, false).map_or(Feed::Pending, Feed::Converted)
        } else {
            native_hls_source(&client, &url).map_or(Feed::Pending, Feed::Direct)
        }
    });
    // Reconnections in a row (a live stream that drops is reopened, a few times).
    let mut reconnects = use_signal(|| 0_u32);
    let mut reconnect = move || {
        let tries = *reconnects.peek() + 1;
        let current = feed.peek().clone();
        let Feed::Converted(src) = current else {
            return;
        };
        if tries > 3 {
            status.set("This channel keeps dropping".into());
            return;
        }
        reconnects.set(tries);
        status.set(RECONNECTING.into());
        // A new address (same stream) so the player opens it afresh.
        let base = src.split("&again=").next().unwrap_or(&src).to_owned();
        feed.set(Feed::Converted(format!("{base}&again={tries}")));
    };
    {
        let trace = trace.clone();
        use_future(move || {
            let trace = trace.clone();
            async move {
                let mut held_since = None::<f64>;
                let mut healthy_since = None::<f64>;
                let mut source = String::new();
                loop {
                    rstreamkit::mse::sleep(Duration::from_millis(250)).await;
                    let Feed::Converted(current) = feed.peek().clone() else {
                        continue;
                    };
                    if current != source {
                        source = current;
                        held_since = None;
                        healthy_since = None;
                    }
                    let Some(video) = video_el() else {
                        continue;
                    };
                    let now = js_sys::Date::now();
                    let ahead = buffer_ahead(&video);
                    if *holding.peek() && !*paused.peek() {
                        healthy_since = None;
                        let since = *held_since.get_or_insert(now);
                        if !video.paused() {
                            let _ = video.pause();
                        }
                        // Accumulate a reserve before starting or resuming. A finite wait also
                        // accommodates browsers that stop preloading before six seconds.
                        if ahead >= 6.0 || (now - since >= 15_000.0 && ahead >= 1.0) {
                            holding.set(false);
                            buffering.set(false);
                            held_since = None;
                            trace.event(
                                "buffer_resume",
                                serde_json::json!({"ahead_s": ahead, "wait_ms": now - since}),
                            );
                            let _ = video.play();
                        } else if now - since >= 20_000.0 {
                            holding.set(false);
                            held_since = None;
                            reconnect();
                        }
                    } else {
                        held_since = None;
                        if !*paused.peek() && ahead >= 2.0 && video.ready_state() >= 3 {
                            let since = *healthy_since.get_or_insert(now);
                            if now - since >= 60_000.0 && *reconnects.peek() != 0 {
                                reconnects.set(0);
                            }
                        } else {
                            healthy_since = None;
                        }
                    }
                }
            }
        });
    }
    // Only where neither of those applies (an iPhone whose own player failed): the proxy checks
    // the stream and converts it.
    {
        let (client, url) = (client.clone(), url.clone());
        let _convert = use_resource(move || {
            let pending = matches!(feed(), Feed::Pending);
            let (client, url) = (client.clone(), url.clone());
            async move {
                if !pending {
                    return;
                }
                let Ok(media) = xtream::Url::parse(&url) else {
                    status.set("This channel's address is invalid".into());
                    return;
                };
                match client.convert(&media).await {
                    Ok(converted) => feed.set(Feed::Converted(converted.at(0).to_string())),
                    Err(e) => status.set(channel_trouble(&e)),
                }
            }
        });
    }
    // A readout for telling a slow stream from a slow decoder: what the picture really is,
    // frames per second actually shown, frames dropped, and how much is buffered.
    let mut show_stats = use_signal(|| false);
    let mut stats = use_signal(String::new);
    {
        let trace = trace.clone();
        use_effect(move || {
            let engine = match feed() {
                Feed::Pending => "starting_ffmpeg",
                Feed::Direct(_) => "native_hls",
                Feed::Rust => "rust",
                Feed::Partial => "rust_no_sound",
                Feed::Converted(_) => "ffmpeg",
            };
            trace.event("engine", serde_json::json!({ "engine": engine }));
        });
    }
    {
        let trace = trace.clone();
        use_effect(move || {
            let text = status();
            let name = if diag::is_failure(&text) {
                "failure"
            } else {
                "status"
            };
            trace.event(name, serde_json::json!({ "text": text }));
        });
    }
    {
        let trace = trace.clone();
        use_effect(move || {
            if let Some(why) = note() {
                trace.event("no_sound", serde_json::json!({ "text": why }));
            } else if silent() {
                trace.event(
                    "no_sound",
                    serde_json::json!({ "text": "browser decoded no audio" }),
                );
            }
        });
    }
    {
        let trace = trace.clone();
        use_drop(move || trace.close());
    }
    let mut channel_action = use_signal(|| None::<ChannelAction>);
    let mut dial = use_signal(String::new);
    let digits = use_hook(|| Rc::new(RefCell::new(String::new())));
    let dial_epoch = use_hook(|| Rc::new(Cell::new(0_u64)));
    use_effect(move || {
        if let Some(action) = channel_action() {
            channel_action.set(None);
            onchannel.call(action);
        }
    });
    // A resource, not a future: it starts again when `show_stats` changes (a future runs once),
    // so nothing polls while the readout is off.
    let _stats_task = use_resource(move || async move {
        if !show_stats() {
            return;
        }
        let mut last = (0_u64, js_sys::Date::now());
        let mut frame_counter = None;
        let mut frame_counter_checked = false;
        loop {
            rstreamkit::mse::sleep(Duration::from_secs(1)).await;
            let Some(v) = video_el() else {
                continue;
            };
            if !frame_counter_checked {
                frame_counter = frame_stats::Counter::start(&v);
                frame_counter_checked = true;
                last = (0, js_sys::Date::now());
            }
            let quality = v.get_video_playback_quality();
            let (frames, now) = (
                frame_counter.as_ref().map_or(
                    u64::from(quality.total_video_frames()),
                    frame_stats::Counter::presented,
                ),
                js_sys::Date::now(),
            );
            let fps = (frames.saturating_sub(last.0) as f64 * 1000.0 / (now - last.1).max(1.0))
                .round() as u32;
            last = (frames, now);
            let buffered = v.buffered();
            let ahead = buffered
                .length()
                .checked_sub(1)
                .and_then(|i| buffered.end(i).ok())
                .map_or(0.0, |end| (end - v.current_time()).max(0.0))
                as u32;
            let source = match feed() {
                Feed::Pending => "starting compatibility mode",
                Feed::Direct(_) => "native HLS",
                Feed::Rust => "Rust player",
                Feed::Partial => "Rust player, no sound",
                Feed::Converted(_) => "converted by ffmpeg",
            };
            stats.set(format!(
                "{}×{} · {fps} fps · {} dropped · {ahead}s buffered · {source}",
                v.video_width(),
                v.video_height(),
                quality.dropped_video_frames()
            ));
        }
    });
    let watchdog_client = client.clone();
    let watchdog_url = url.clone();
    let player_video = use_hook(|| Rc::new(RefCell::new(None::<web_sys::HtmlVideoElement>)));
    {
        let player_video = player_video.clone();
        use_drop(move || {
            if let Some(video) = player_video.borrow_mut().take() {
                release(&video);
            }
        });
    }
    use_future(move || async move {
        rstreamkit::mse::sleep(Duration::from_secs(12)).await;
        if status.try_peek().is_ok_and(|s| *s == "Starting playback…") {
            status.set(STILL_STARTING.into());
        }
    });
    let (sound_client, sound_url) = (client.clone(), url.clone());
    let trace_for_player = trace.clone();
    use_effect(move || {
        let Some(video) = video_el() else {
            status.set("Could not start the player".into());
            return;
        };
        *player_video.borrow_mut() = Some(video.clone());
        load_stage.set(0); // a new source starts over
        trace_for_player.follow(&video);
        video.set_volume(f64::from(*volume.peek()) / 100.0);
        video.set_muted(*muted.peek());
        live_stream.borrow_mut().take();
        let partial = match feed() {
            Feed::Pending => return,
            Feed::Direct(src) => {
                handle.borrow_mut().take();
                video.set_src(&src);
                let _ = video.play();
                return;
            }
            Feed::Converted(src) => {
                // Stop the Rust player and let the plain `<video>` play the converted stream.
                handle.borrow_mut().take();
                note.set(None);
                paused.set(false);
                holding.set(true);
                buffering.set(true);
                let fallback_client = client.clone();
                let fallback_url = url.clone();
                let stream = standard::start(video, src, move |why| {
                    if why.starts_with("browser:") {
                        let already_transcoding = matches!(&*feed.peek(), Feed::Converted(src) if src.contains("video=transcode"));
                        if !already_transcoding
                            && let Some(src) = standard::live(&fallback_client, &fallback_url, true)
                        {
                            status.set("Re-encoding the video…".into());
                            feed.set(Feed::Converted(src));
                        } else {
                            holding.set(false);
                            paused.set(true);
                            status.set(why);
                        }
                    } else {
                        reconnect();
                    }
                });
                match stream {
                    Ok(stream) => *live_stream.borrow_mut() = Some(stream),
                    Err(why) => status.set(why),
                }
                return;
            }
            Feed::Partial => true,
            Feed::Rust => false,
        };
        let Ok(playlist) = xtream::Url::parse(&url) else {
            status.set("Could not start the player".into());
            return;
        };
        let c = client.clone();
        let upstream = client.upstream(&playlist);
        let unsupported_trace = trace_for_player.clone();
        *handle.borrow_mut() = Some(rstreamkit::mse::start(
            video,
            upstream.to_string(),
            Proxied(c),
            partial,
            // `peek`: changing the setting must not restart a channel that is playing.
            *rust_sound.peek(),
            move |s| match s {
                rstreamkit::mse::Status::Playing => {
                    status.set(
                        if *picture_ready.peek() {
                            "Live"
                        } else {
                            "Waiting for picture…"
                        }
                        .into(),
                    );
                }
                rstreamkit::mse::Status::Note(n) => note.set(Some(n)),
                rstreamkit::mse::Status::Unsupported(reason) => {
                    unsupported_trace.event(
                        "unsupported",
                        serde_json::json!({ "reason": reason.to_string() }),
                    );
                    // The experimental player stays experimental: it says what it can't do
                    // rather than quietly handing over to the standard player.
                    if matches!(reason, Unsupported::Sound(_)) {
                        note.set(Some(reason.to_string()));
                        feed.set(Feed::Partial);
                    } else {
                        status.set("Not supported by the experimental player".into());
                    }
                }
                rstreamkit::mse::Status::Ended => status.set("Stream ended".into()),
                rstreamkit::mse::Status::Failed(_) => status.set("Playback failed".into()),
                _ => {}
            },
        ));
    });

    // Some providers append data successfully but never deliver a decodable picture. In that
    // case MSE can say "Playing" while the screen stays black. Watch only until the first frame;
    // each fallback gets one chance, so a broken upstream does not loop forever.
    use_future(move || {
        let client = watchdog_client.clone();
        let url = watchdog_url.clone();
        async move {
            let mut mode = None::<Feed>;
            let mut since = js_sys::Date::now();
            loop {
                rstreamkit::mse::sleep(Duration::from_secs(2)).await;
                let current = feed.peek().clone();
                if mode.as_ref() != Some(&current) {
                    mode = Some(current.clone());
                    since = js_sys::Date::now();
                }
                // `ontimeupdate` notices the first decoded frame.
                if *picture_ready.peek() || *audio_only.peek() {
                    break;
                }
                let Some(video) = video_el() else { continue };
                if video.paused()
                    || web_sys::window()
                        .and_then(|w| w.document())
                        .is_some_and(|d| d.hidden())
                {
                    since = js_sys::Date::now();
                    continue;
                }
                // Time moving with no frame decoded means the video can't be decoded here: act
                // soon. Nothing moving at all is a slow provider, which another method from the
                // same provider won't fix quickly: give it longer.
                let wait_ms = if video.current_time() > 1.0 {
                    5_000.0
                } else {
                    20_000.0
                };
                if js_sys::Date::now() - since < wait_ms {
                    continue;
                }
                match current {
                    Feed::Pending => continue,
                    Feed::Direct(_) => {
                        status.set("Trying another playback method…".into());
                        feed.set(Feed::Pending);
                    }
                    Feed::Rust | Feed::Partial => {
                        status.set("No picture from the experimental player".into());
                        break;
                    }
                    // Time moves but no frame: a stream with no video track plays as radio; a
                    // picture this browser won't decode gets re-encoded, once.
                    Feed::Converted(src) => {
                        if video.video_width() == 0 && video.ready_state() >= 2 {
                            audio_only.set(true);
                            picture_ready.set(true);
                            status.set("Audio only".into());
                            break;
                        }
                        match (!src.contains("video=transcode"))
                            .then(|| standard::live(&client, &url, true))
                            .flatten()
                        {
                            Some(again) => {
                                status.set("Re-encoding the video…".into());
                                feed.set(Feed::Converted(again));
                            }
                            None => {
                                status.set("No picture from this channel".into());
                                break;
                            }
                        }
                    }
                }
            }
        }
    });

    // A document listener also works after a channel-row click leaves focus outside the player.
    // Ignore search fields so typing a category or title never changes channel.
    let keys = use_hook(|| {
        Rc::new(RefCell::new(
            None::<Closure<dyn FnMut(web_sys::KeyboardEvent)>>,
        ))
    });
    let keys_for_install = keys.clone();
    let digits_for_keys = digits.clone();
    let epoch_for_keys = dial_epoch.clone();
    use_effect(move || {
        let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let digits = digits_for_keys.clone();
        let epoch = epoch_for_keys.clone();
        let on_key =
            Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
                if e.alt_key() || e.ctrl_key() || e.meta_key() {
                    return;
                }
                if e.target()
                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                    .is_some_and(|el| {
                        matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT")
                    })
                {
                    return;
                }
                match e.key().to_ascii_lowercase().as_str() {
                    " " => {
                        e.prevent_default();
                        toggle_play();
                    }
                    "arrowup" => {
                        e.prevent_default();
                        digits.borrow_mut().clear();
                        dial.set(String::new());
                        epoch.set(epoch.get().wrapping_add(1));
                        channel_action.set(Some(ChannelAction::Up));
                    }
                    "arrowdown" => {
                        e.prevent_default();
                        digits.borrow_mut().clear();
                        dial.set(String::new());
                        epoch.set(epoch.get().wrapping_add(1));
                        channel_action.set(Some(ChannelAction::Down));
                    }
                    "f" => toggle_fullscreen("live-player", expanded),
                    "i" => show_stats.set(!show_stats()),
                    "escape" => {
                        digits.borrow_mut().clear();
                        dial.set(String::new());
                        epoch.set(epoch.get().wrapping_add(1));
                        expanded.set(false);
                    }
                    "m" => muted.set(toggle_mute().unwrap_or(false)),
                    "enter" => {
                        let number = digits.borrow().parse().ok();
                        digits.borrow_mut().clear();
                        dial.set(String::new());
                        epoch.set(epoch.get().wrapping_add(1));
                        if let Some(number) = number {
                            channel_action.set(Some(ChannelAction::Number(number)));
                        }
                    }
                    key if key.len() == 1 && key.as_bytes()[0].is_ascii_digit() => {
                        e.prevent_default();
                        let typed = {
                            let mut value = digits.borrow_mut();
                            if value.len() >= 5 {
                                value.clear();
                            }
                            value.push_str(key);
                            value.clone()
                        };
                        dial.set(typed);
                        let revision = epoch.get().wrapping_add(1);
                        epoch.set(revision);
                        let (digits, epoch) = (digits.clone(), epoch.clone());
                        wasm_bindgen_futures::spawn_local(async move {
                            rstreamkit::mse::sleep(Duration::from_millis(1400)).await;
                            if epoch.get() == revision {
                                let number = digits.borrow().parse().ok();
                                digits.borrow_mut().clear();
                                dial.set(String::new());
                                if let Some(number) = number {
                                    channel_action.set(Some(ChannelAction::Number(number)));
                                }
                            }
                        });
                    }
                    _ => {}
                }
            });
        let _ = doc.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
        *keys_for_install.borrow_mut() = Some(on_key);
    });
    use_drop(move || {
        dial_epoch.set(dial_epoch.get().wrapping_add(1));
        if let (Some(on_key), Some(doc)) = (
            keys.borrow_mut().take(),
            web_sys::window().and_then(|w| w.document()),
        ) {
            let _ =
                doc.remove_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
        }
    });

    let class = format!(
        "player{}{}{}{}",
        if paused() { " paused" } else { "" },
        if active() { " active" } else { "" },
        if expanded() { " fill" } else { "" },
        // Shown on its first frame (or once playing, where a browser can't count frames).
        if (picture_ready() || load_stage() == 4) && !audio_only() {
            " showing"
        } else {
            ""
        }
    );
    // Only a channel that gave up says so; everything on its way is the ring (the words go to
    // the `--logs` trace).
    let failed = !starting(&status()) && !matches!(status().as_str(), "Live" | "Audio only");
    let logo = icon.clone().filter(|src| src.starts_with("http"));
    rsx! {
        div { class: "live-stage",
            div {
                class: "{class}",
                id: "live-player",
                tabindex: "0",
                onmousemove: move |_| idle.wake(),
                video {
                    id: "live-video",
                    autoplay: !matches!(*feed.peek(), Feed::Converted(_)),
                    playsinline: true,
                    onplay: move |_| {
                        paused.set(false);
                        if let Some(s) = media_on_play.borrow().as_ref() { s.playing(true); }
                    },
                    onpause: move |_| {
                        if !*holding.peek() {
                            paused.set(true);
                            if let Some(s) = media_on_pause.borrow().as_ref() { s.playing(false); }
                        }
                    },
                    onwaiting: move |_| {
                        buffering.set(true);
                        if matches!(*feed.peek(), Feed::Converted(_)) && !*paused.peek() {
                            holding.set(true);
                        }
                    },
                    // A live channel never ends: the connection did. Open it again.
                    onended: move |_| reconnect(),
                    onloadstart: move |_| if *load_stage.peek() < 1 { load_stage.set(1) },
                    onloadedmetadata: move |_| {
                        if *load_stage.peek() < 2 {
                            load_stage.set(2);
                        }
                        // No video track at all (radio): say so at once, not after waiting.
                        if let Some(v) = video_el()
                            && v.video_width() == 0
                            && matches!(*feed.peek(), Feed::Converted(_))
                        {
                            audio_only.set(true);
                            picture_ready.set(true);
                            status.set("Audio only".into());
                        }
                    },
                    onloadeddata: move |_| if *load_stage.peek() < 3 { load_stage.set(3) },
                    onplaying: move |_| {
                        load_stage.set(4);
                        if !*holding.peek() { buffering.set(false); }
                        paused.set(false);
                        if matches!(feed(), Feed::Direct(_) | Feed::Converted(_)) {
                            status.set(if audio_only() { "Audio only" } else if picture_ready() { "Live" } else { "Waiting for picture…" }.into());
                        }
                    },
                    onerror: {
                        let (client, url) = (sound_client.clone(), sound_url.clone());
                        move |_| {
                            // MEDIA_ERR_DECODE: the browser's decoder rejects this picture: have
                            // the proxy re-encode it, once.
                            let error = video_el()
                                .and_then(|v| js_sys::Reflect::get(&v, &"error".into()).ok())
                                .filter(|e| !e.is_null() && !e.is_undefined());
                            let field = |name: &str| error.as_ref().and_then(|e| js_sys::Reflect::get(e, &name.into()).ok());
                            let decoding = field("code").and_then(|c| c.as_f64()) == Some(3.0);
                            // A video decode error (an audio one is a damaged packet: reconnect).
                            let decode_error = decoding
                                && !field("message").and_then(|m| m.as_string()).is_some_and(|m| m.contains("audio"));
                            let current = feed.peek().clone();
                            match current {
                                Feed::Converted(src) if decode_error && !src.contains("video=transcode") => {
                                    if let Some(again) = standard::live(&client, &url, true) {
                                        status.set("Re-encoding the video…".into());
                                        feed.set(Feed::Converted(again));
                                    }
                                }
                                // It was playing, or got as far as decoding: a dropped
                                // connection or a damaged packet. Open it again.
                                Feed::Converted(_) if *picture_ready.peek() || decoding => reconnect(),
                                // The proxy said why it couldn't (offline, refused, ...): ask it.
                                Feed::Converted(src) => {
                                    status.set("Starting playback…".into());
                                    let client = client.clone();
                                    wasm_bindgen_futures::spawn_local(async move {
                                        let why = match xtream::Url::parse(&src) {
                                            Ok(u) => client.refusal(&u).await,
                                            Err(_) => None,
                                        };
                                        if status.try_peek().is_ok() {
                                            status.set(match why {
                                                Some(why) => channel_trouble(&xtream::Error::Proxy(why)),
                                                None => "The stream stopped".into(),
                                            });
                                        }
                                    });
                                }
                                Feed::Direct(_) => {
                                    status.set("Starting playback…".into());
                                    feed.set(Feed::Pending);
                                }
                                _ => {}
                            }
                        }
                    },
                    onvolumechange: move |_| {
                        if let Some(v) = video_el() {
                            if muted() != v.muted() {
                                muted.set(v.muted());
                            }
                            let level = (v.volume() * 100.0).round() as u32;
                            if volume() != level {
                                volume.set(level);
                            }
                        }
                    },
                    ontimeupdate: move |_| {
                        let Some(v) = video_el() else { return };
                        // The picture is there as soon as a frame has been decoded: no polling.
                        if !*picture_ready.peek() && v.get_video_playback_quality().total_video_frames() > 0 {
                            picture_ready.set(true);
                            if matches!(status.peek().as_str(), "Starting playback…" | "Waiting for picture…" | STILL_STARTING | RECONNECTING) {
                                status.set("Live".into());
                            }
                        }
                        // Playing steadily again: the next drop gets its full set of retries.
                        if v.current_time() > 15.0 && *reconnects.peek() > 0 {
                            reconnects.set(0);
                        }
                        // Chrome's decoded-byte count lags a little: give it a few seconds. Clears
                        // itself once sound arrives.
                        let quiet = no_audio_decoded(&v, 5.0);
                        if quiet != *silent.peek() {
                            silent.set(quiet);
                        }
                    },
                    onclick: move |_| toggle_play(),
                    ondoubleclick: move |_| toggle_fullscreen("live-player", expanded),
                }
                if show_stats() {
                    div { class: "stats", "{stats}" }
                }
                if audio_only() {
                    div { class: if paused() { "radio paused" } else { "radio" },
                        if let Some(src) = &logo {
                            img { class: "radio-glow", src: "{src}", alt: "" }
                        }
                        div { class: "radio-art",
                            if let Some(src) = &logo { img { src: "{src}", alt: "" } } else { Icon { d: MUSIC } }
                        }
                        strong { "{title}" }
                        div { class: "eq", i {} i {} i {} i {} i {} }
                    }
                }
                if !dial().is_empty() { div { class: "channel-dial", "{dial}" } }
                if note().is_some() || silent() {
                    div { class: "sound-note", title: note().unwrap_or_default(), Icon { d: MUTED } "No sound" }
                }
                if starting(&status()) {
                    div { class: "hud", LoadRing { stage: load_stage(), icon: logo.clone() } }
                } else if failed {
                    div { class: "hud fail",
                        p { "{status}" }
                        button { class: "retry", onclick: move |_| { reconnects.set(0); reconnect(); }, "Try again" }
                    }
                } else if buffering() && !paused() && !audio_only() {
                    div { class: "hud", LoadRing { stage: 2 } }
                }
                if paused() && !failed && !starting(&status()) {
                    button { class: "bigplay", aria_label: "Play", onclick: move |_| toggle_play(), Icon { d: PLAY } }
                }
                div { class: "controls",
                    button { class: "ctl", aria_label: "Play or pause", onclick: move |_| toggle_play(),
                        Icon { d: if paused() { PLAY } else { PAUSE } }
                    }
                    div { class: "volume",
                        button {
                            class: "ctl",
                            aria_label: "Mute",
                            onclick: move |_| muted.set(toggle_mute().unwrap_or(false)),
                            Icon { d: if muted() { MUTED } else { VOLUME } }
                        }
                        input {
                            class: "vol",
                            r#type: "range",
                            min: "0",
                            max: "100",
                            aria_label: "Volume",
                            value: "{volume}",
                            style: "--level:{volume}%",
                            oninput: move |e| {
                                let level: u32 = e.value().parse().unwrap_or(100);
                                volume.set(level);
                                muted.set(level == 0);
                                preferences::update(|p| p.volume = level.min(100) as u8);
                                if let Some(v) = video_el() {
                                    v.set_volume(f64::from(level) / 100.0);
                                    v.set_muted(level == 0);
                                }
                            }
                        }
                    }
                    button { class: "live-pill", title: "Jump to live", onclick: move |_| { go_live(); if paused() { toggle_play(); } }, "LIVE" }
                    span { class: "grow" }
                    button { class: "ctl", aria_label: "Stream info", title: "Stream info (I)", onclick: move |_| show_stats.set(!show_stats()), Icon { d: INFO } }
                    button { class: "ctl", aria_label: "Picture in picture", title: "Picture in picture", onclick: move |_| toggle_pip(video_el()), Icon { d: PIP } }
                    button { class: "ctl", aria_label: "Fullscreen", title: "Fullscreen (F)", onclick: move |_| toggle_fullscreen("live-player", expanded), Icon { d: FULLSCREEN } }
                }
            }
            Guide { id, playing: picture_ready() && !failed }
        }
    }
}

/// The channel's schedule on a timeline: hour ticks, one block per programme, a marker for now.
fn buffer_ahead(video: &web_sys::HtmlVideoElement) -> f64 {
    let ranges = video.buffered();
    let at = video.current_time();
    (0..ranges.length())
        .find_map(|i| {
            let (start, end) = (ranges.start(i).ok()?, ranges.end(i).ok()?);
            (start <= at + 0.1 && end > at).then_some(end - at)
        })
        .unwrap_or(0.0)
}

/// It scrolls to now on load and moves the marker every 30 seconds.
#[component]
fn Guide(id: u64, playing: bool) -> Element {
    let session = use_context::<Signal<Option<Client>>>();
    let client = use_hook(|| session.read().clone().expect("logged in"));
    let mut tick = use_signal(|| 0_u32);
    use_future(move || async move {
        loop {
            rstreamkit::mse::sleep(Duration::from_secs(30)).await;
            tick += 1;
        }
    });
    let refresh = use_memo(move || tick() / 10);
    let table = use_resource(move || {
        let _ = refresh();
        let c = client.clone();
        async move {
            let now = now_secs();
            match c.epg_table(id).await {
                Ok(t) if xtream::guide::has_nearby(&t, now) => Ok(t),
                _ => c.short_epg(id, 8).await,
            }
        }
    });
    // Once the schedule arrives, start with the current programme in view. The timeline isn't
    // laid out the moment the data lands, so wait a moment.
    use_effect(move || {
        if table.read().is_some() {
            spawn(async move {
                rstreamkit::mse::sleep(Duration::from_millis(60)).await;
                let timeline = web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.get_element_by_id("timeline"));
                if let Some(el) = timeline
                    && let Some(at) = el
                        .first_element_child()
                        .and_then(|c| c.get_attribute("data-now"))
                        .and_then(|a| a.parse::<i32>().ok())
                {
                    el.set_scroll_left((at - 160).max(0));
                }
            });
        }
    });

    let _ = tick(); // re-render now and then so the marker moves
    let now = now_secs();
    let body = match &*table.read() {
        // While it loads, the shape of a schedule; without one, nothing at all.
        None => {
            return rsx! { section { class: "guide", div { class: "tl-skeleton", i {} i {} i {} } } };
        }
        Some(Err(_)) => return rsx! {},
        Some(Ok(t)) => {
            let slots = xtream::guide::normalize(t);
            if playing
                && slots.iter().any(|slot| {
                    slot.start <= now
                        && now < slot.end
                        && slot.listing.title.to_ascii_uppercase().contains("OFFLINE")
                })
            {
                return rsx! { section { class: "guide guide-note", p { "This stream is playing, but the provider's guide lists it as offline." } } };
            }
            if slots.is_empty() {
                return rsx! {};
            } else {
                // From up to eight hours back to a day ahead, on whole hours.
                let lo = slots[0].start.max(now.saturating_sub(8 * 3600)).min(now) / 3600 * 3600;
                let last = slots.last().map_or(now, |slot| slot.end);
                let hi = last.max(now + 3600).min(now + 24 * 3600).div_ceil(3600) * 3600;
                let scale = xtream::guide::seconds_per_px(&slots, lo, hi);
                let px = |t: u64| t.clamp(lo, hi).saturating_sub(lo) / scale;
                let (width, here) = (px(hi), px(now));
                let visible: Vec<_> = slots
                    .into_iter()
                    .filter(|slot| slot.end > lo && slot.start < hi)
                    .map(|slot| {
                        let block_width = px(slot.end).saturating_sub(px(slot.start)).max(3);
                        (slot.start, slot.end, slot.listing, block_width)
                    })
                    .collect();
                if visible.is_empty() {
                    return rsx! {};
                } else {
                    rsx! {
                        div {
                            class: "timeline",
                            id: "timeline",
                            div { class: "tl", style: "width:{width}px", "data-now": "{here}",
                                for hour in (lo..hi).step_by(3600) {
                                    span { class: "tick", key: "{hour}", style: "left:{px(hour)}px", "{clock(hour)}" }
                                }
                                for (start, end, l, block_width) in visible {
                                    div {
                                        key: "{start}",
                                        class: if start <= now && now < end { if block_width < 90 { "block now compact" } else { "block now" } } else if block_width < 90 { "block compact" } else { "block" },
                                        style: "left:{px(start)}px;width:{block_width}px",
                                        title: "{clock(start)} – {clock(end)} · {l.title} · {l.description}",
                                        div { class: "txt",
                                            time { "{clock(start)} – {clock(end)}" }
                                            strong { if l.title.is_empty() { "Untitled programme" } else { "{l.title}" } }
                                            if !l.description.is_empty() { p { "{l.description}" } }
                                        }
                                        if start <= now && now < end {
                                            i { class: "prog", style: "width:{(now - start) * 100 / (end - start)}%" }
                                        }
                                    }
                                }
                                div { class: "now-line", style: "left:{here}px", span { "{clock(now)}" } }
                            }
                        }
                    }
                }
            }
        }
    };

    rsx! {
        section { class: "guide",
            div { class: "guide-heading", strong { "Programme guide" } small { "Local time · {clock(now)}" } }
            {body}
        }
    }
}
