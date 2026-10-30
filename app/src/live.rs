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
const RECOVERING: &str = "Provider interrupted; recovering…";

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
            | RECOVERING
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
    catalog: Snapshot<Library>,
    onchannel: EventHandler<ChannelAction>,
) -> Element {
    let mut paused = use_signal(|| false);
    let mut holding = use_signal(|| false);
    // The player paused the video itself (holding for buffer): its `pause` event, which can
    // arrive after holding has ended, isn't the viewer pressing pause.
    let mut own_pause = use_signal(|| false);
    let session = use_context::<Signal<Option<Client>>>();
    let rust_sound = use_context::<Signal<bool>>();
    let client = use_hook(|| session.read().clone().expect("logged in"));
    let mut selected_id = use_signal(|| id);
    let mut selected_title = use_signal(|| title.clone());
    let mut selected_url = use_signal(|| url.clone());
    let mut selected_icon = use_signal(|| icon.clone());
    let media = use_hook(|| Rc::new(RefCell::new(None::<media_session::Session>)));
    let media_for_install = media.clone();
    use_effect(move || {
        let media_title = selected_title();
        let installed = media_session::Session::install(
            &media_title,
            "Live · RIPTV",
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
    let saved = use_hook(preferences::load);
    let saved_volume = u32::from(saved.volume);
    let mut status = use_signal(|| "Starting playback…".to_string());
    let mut picture_ready = use_signal(|| false);
    // How far the current start has got, for the progress ring (see `LoadRing`).
    let mut load_stage = use_signal(|| 0_u8);
    let mut audio_only = use_signal(|| false);
    let mut muted = use_signal(move || saved_volume == 0);
    let mut volume = use_signal(move || saved_volume);
    let mut buffering = use_signal(|| false);
    let mut smoother = use_signal(|| saved.smoother_live);
    let mut backup = use_signal(|| {
        saved
            .live_backups
            .iter()
            .find(|(primary, _)| *primary == id)
            .map(|(_, backup)| *backup)
    });
    let mut settings_open = use_signal(|| false);
    let dismissal = use_hook(|| Rc::new(RefCell::new(None::<controls::OutsideClick>)));
    {
        let listener = dismissal.clone();
        use_effect(move || {
            listener.borrow_mut().take();
            if settings_open() {
                *listener.borrow_mut() =
                    controls::OutsideClick::new(".live-settings,.settings-toggle", move || {
                        settings_open.set(false)
                    });
            }
        });
        use_drop(move || {
            dismissal.borrow_mut().take();
        });
    }
    let options = use_context::<playback_options::Options>();
    use_effect(move || {
        let _ = (options.changed)();
        backup.set(
            preferences::load()
                .live_backups
                .iter()
                .find(|(primary, _)| *primary == id)
                .map(|(_, target)| *target),
        );
    });
    let mut ladder = use_signal(recovery::Ladder::default);
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
    let expanded = use_signal(|| false);
    let fullscreen = use_hook(crate::fullscreen::Controller::default);
    {
        let fullscreen = fullscreen.clone();
        use_drop(move || fullscreen.dispose());
    }
    // Why there is no sound, when the player knows: its own note, or what the browser reports.
    let mut note = use_signal(|| None::<String>);
    let mut silent = use_signal(|| false);
    // The controls fade out after a moment of no mouse movement while playing.
    let active = use_signal(|| true);
    let idle = use_hook(|| IdleHide::new(active, 2500.0));
    let handle = use_hook(|| Rc::new(RefCell::new(None::<experimental::Player>)));
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
    let mut recovery = use_signal(recovery::Recovery::default);
    let mut retry_until = use_signal(|| 0_u64);
    let mut reconnect = move || {
        if *paused.peek() || !matches!(&*feed.peek(), Feed::Converted(_)) {
            return;
        }
        if recovery.write().request(
            standard::clock(),
            (js_sys::Math::random() * 250.0) as u64,
            retry_until.peek().saturating_sub(standard::clock()),
        ) {
            status.set(RECOVERING.into());
        }
    };
    {
        let trace = trace.clone();
        let live_stream = live_stream.clone();
        let recovery_client = client.clone();
        let recovery_catalog = catalog.clone();
        use_resource(move || {
            let running = !paused();
            let trace = trace.clone();
            let live_stream = live_stream.clone();
            let recovery_client = recovery_client.clone();
            let recovery_catalog = recovery_catalog.clone();
            async move {
                if !running {
                    recovery.write().poll(standard::clock(), true, 0.0);
                    if let Some(video) = video_el() {
                        let _ = video.pause();
                    }
                    return;
                }
                let mut held_since = None::<u64>;
                let mut source = String::new();
                let mut monitor = recovery::Monitor::new(standard::clock());
                let mut interrupted_since = None::<u64>;
                loop {
                    rstreamkit::mse::sleep(Duration::from_millis(250)).await;
                    let changed = {
                        let selected = feed.peek();
                        let Feed::Converted(current) = &*selected else {
                            continue;
                        };
                        if current == &source {
                            false
                        } else {
                            source.clone_from(current);
                            true
                        }
                    };
                    if changed {
                        held_since = None;
                        monitor = recovery::Monitor::new(standard::clock());
                    }
                    let Some(video) = video_el() else {
                        continue;
                    };
                    let now = standard::clock();
                    let ahead = buffer_ahead(&video);
                    if *holding.peek() && !*paused.peek() && ahead < 0.5 {
                        let ranges = video.buffered();
                        let target = (0..ranges.length()).find_map(|index| {
                            recovery::gap_target(
                                video.current_time(),
                                ranges.start(index).ok()?,
                                ranges.end(index).ok()?,
                            )
                        });
                        if let Some(target) = target
                            && ladder.write().skip_gap()
                        {
                            trace.event(
                                "recovery_gap_jump",
                                serde_json::json!({"seconds": target - video.current_time()}),
                            );
                            video.set_current_time(target);
                            held_since = None;
                            continue;
                        }
                    }
                    // Browser autoplay/background policies must not look like a broken provider.
                    if video.paused() && !video.ended() && !*holding.peek() && !*paused.peek() {
                        paused.set(true);
                        continue;
                    }
                    let progress = live_stream
                        .borrow()
                        .as_ref()
                        .map(standard::Stream::progress);
                    if let Some(p) = progress {
                        if p.terminal && !recovery.peek().manual() {
                            holding.set(false);
                            paused.set(true);
                            recovery.write().poll(now, true, ahead);
                            let _ = video.pause();
                            continue;
                        }
                        // A paused reader is deliberate backpressure, not a failed provider.
                        let health = monitor.observe(recovery::Sample {
                            now,
                            position: video.current_time(),
                            frames: u64::from(
                                video.get_video_playback_quality().total_video_frames(),
                            ),
                            has_video: video.video_width() > 0,
                            visible: web_sys::window()
                                .and_then(|w| w.document())
                                .is_none_or(|d| !d.hidden()),
                            ahead,
                            paused: *paused.peek(),
                            holding: *holding.peek(),
                            transport: p,
                        });
                        recovery.write().healthy(now, health.healthy);
                        if health.healthy
                            && let Some(since) = interrupted_since.take()
                        {
                            trace.event("recovery_success", serde_json::json!({"ms": now.saturating_sub(since), "ahead_s": ahead, "channel": *selected_id.peek()}));
                        }
                        if health.healthy && status.peek().as_str() == RECOVERING {
                            status.set(
                                if *audio_only.peek() {
                                    "Audio only"
                                } else {
                                    "Live"
                                }
                                .into(),
                            );
                        }
                        if !*paused.peek() && (p.finished || health.stalled || health.stuck) {
                            interrupted_since.get_or_insert(now);
                            if health.stuck && ladder.write().repair() {
                                // Seek within already buffered media to ask the decoder for a
                                // fresh frame. No parallel fetch, fragment cache, or new buffer.
                                video.set_current_time(
                                    video.current_time() + (ahead - 1.0).clamp(0.1, 1.0),
                                );
                                monitor = recovery::Monitor::new(now);
                                trace.event(
                                    "recovery_local_repair",
                                    serde_json::json!({"ahead_s": ahead}),
                                );
                                continue;
                            }
                            if !recovery.peek().pending() {
                                trace.event("recovery_detected", serde_json::json!({"reason": if health.stuck { "decoder_stuck" } else if p.finished { "stream_ended" } else { "no_incoming_data" }, "ahead_s": ahead, "bytes": p.received}));
                            }
                            reconnect();
                            if health.stuck {
                                recovery.write().decoder_stuck();
                            }
                        }
                    }
                    let decision = recovery.write().poll(now, *paused.peek(), ahead);
                    match decision {
                        recovery::Step::Retry { serial, attempt } => {
                            holding.set(true);
                            buffering.set(true);
                            // Abort the old fetch before the effect can open the replacement.
                            live_stream.borrow_mut().take();
                            status.set(RECONNECTING.into());
                            let approved = backup.peek().and_then(|backup| {
                                let Items::Live(channels) = &recovery_catalog.items else {
                                    return None;
                                };
                                channels
                                    .iter()
                                    .find(|c| c.stream_id == backup && backup != id)
                            });
                            let route = ladder.write().route(
                                attempt,
                                !recovery_client.is_playlist(),
                                approved.is_some(),
                            );
                            let transcode = source.contains("video=transcode");
                            let replacement = match route {
                                recovery::Route::Alternate => {
                                    let alternate = recovery_client
                                        .live_url(*selected_id.peek(), "ts")
                                        .to_string();
                                    selected_url.set(alternate.clone());
                                    standard::live(&recovery_client, &alternate, transcode)
                                }
                                recovery::Route::Backup => approved.and_then(|channel| {
                                    let backup_url = recovery_client
                                        .live_url(channel.stream_id, "m3u8")
                                        .to_string();
                                    selected_id.set(channel.stream_id);
                                    selected_title.set(channel.name.clone());
                                    selected_icon.set(channel.stream_icon.clone());
                                    selected_url.set(backup_url.clone());
                                    standard::live(&recovery_client, &backup_url, transcode)
                                }),
                                recovery::Route::Reconnect => None,
                            };
                            let base = replacement.as_deref().unwrap_or_else(|| {
                                source.split("&again=").next().unwrap_or(&source)
                            });
                            feed.set(Feed::Converted(format!("{base}&again={serial}")));
                            trace.event("recovery_retry", serde_json::json!({"attempt": attempt, "route": format!("{route:?}"), "channel": *selected_id.peek(), "reserve_s": recovery.peek().reserve_for(*smoother.peek()), "ahead_s": ahead}));
                            continue;
                        }
                        recovery::Step::GiveUp => {
                            trace.event(
                                "recovery_give_up",
                                serde_json::json!({"channel": *selected_id.peek()}),
                            );
                            holding.set(false);
                            buffering.set(false);
                            paused.set(true);
                            live_stream.borrow_mut().take();
                            let _ = video.pause();
                            // Never showed a picture: it isn't dropping, the provider isn't serving it.
                            status.set(
                                if *picture_ready.peek() {
                                    "This channel keeps dropping. Retry or choose another feed."
                                } else {
                                    "This channel isn't available right now. Retry or choose another feed."
                                }
                                .into(),
                            );
                            continue;
                        }
                        recovery::Step::Wait => {}
                    }
                    if *holding.peek() && !*paused.peek() {
                        let since = *held_since.get_or_insert(now);
                        if !video.paused() {
                            own_pause.set(true);
                            let _ = video.pause();
                        }
                        // Accumulate a reserve before starting or resuming. A finite wait also
                        // accommodates browsers that stop preloading before six seconds; a stream
                        // that has ended has nothing more to wait for.
                        let ended = live_stream
                            .borrow()
                            .as_ref()
                            .is_some_and(standard::Stream::ended);
                        if ahead >= recovery.peek().reserve_for(*smoother.peek())
                            || (ended && ahead > 0.0)
                            || (now.saturating_sub(since) >= 15_000 && ahead >= 3.0)
                        {
                            holding.set(false);
                            buffering.set(false);
                            held_since = None;
                            trace.event(
                                "buffer_resume",
                                serde_json::json!({"ahead_s": ahead, "wait_ms": now.saturating_sub(since)}),
                            );
                            let _ = video.play();
                        } else if now.saturating_sub(since) >= 20_000 {
                            reconnect();
                        }
                    } else {
                        held_since = None;
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
    let mut stats = use_signal(crate::stream_info::Readout::default);
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
        // The first readout at once (fps from the second, once there is an interval to count).
        let mut first = true;
        loop {
            if !first {
                rstreamkit::mse::sleep(Duration::from_secs(1)).await;
            }
            let Some(v) = video_el() else {
                first = false;
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
            let (width, height) = (v.video_width(), v.video_height());
            let source = match feed() {
                Feed::Pending => "Preparing",
                Feed::Direct(_) => "Browser",
                Feed::Rust | Feed::Partial => "Experimental",
                Feed::Converted(_) => "Compatibility mode",
            };
            let next = crate::stream_info::Readout {
                picture: (width > 0 && height > 0).then_some((width, height)),
                fps: (!first).then_some(fps),
                dropped: quality.dropped_video_frames(),
                buffer_seconds: buffer_ahead(&v).round() as u32,
                playback: source,
            };
            first = false;
            if *stats.peek() != next {
                stats.set(next);
            }
        }
    });
    let watchdog_client = client.clone();
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
    let sound_client = client.clone();
    let trace_for_player = trace.clone();
    use_effect(move || {
        let Some(video) = video_el() else {
            status.set("Could not start the player".into());
            return;
        };
        *player_video.borrow_mut() = Some(video.clone());
        load_stage.set(0); // a new source starts over
        picture_ready.set(false);
        audio_only.set(false);
        silent.set(false);
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
                let stream = standard::start(video, src, move |why, progress| {
                    retry_until.set(standard::clock().saturating_add(progress.retry_after));
                    if progress.terminal {
                        holding.set(false);
                        paused.set(true);
                        status.set(channel_trouble(&xtream::Error::Proxy(why)));
                    } else if why.starts_with("browser:") {
                        let already_transcoding = matches!(&*feed.peek(), Feed::Converted(src) if src.contains("video=transcode"));
                        if !already_transcoding
                            && let Some(src) =
                                standard::live(&fallback_client, &selected_url.peek(), true)
                        {
                            status.set("Re-encoding the video…".into());
                            feed.set(Feed::Converted(src));
                        } else {
                            holding.set(false);
                            paused.set(true);
                            status.set(why);
                        }
                    } else if ["401", "403"]
                        .iter()
                        .any(|c| why.contains(&format!("returned {c}")))
                    {
                        // The provider's final answer (gone, refused): reconnecting only asks again.
                        holding.set(false);
                        paused.set(true);
                        status.set(channel_trouble(&xtream::Error::Proxy(why)));
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
        *handle.borrow_mut() = Some(experimental::live(
            video,
            upstream.to_string(),
            c,
            partial,
            // `peek`: changing the setting must not restart a channel that is playing.
            *rust_sound.peek(),
            move |e| match e {
                experimental::Event::Playing => {
                    status.set(
                        if *picture_ready.peek() {
                            "Live"
                        } else {
                            "Waiting for picture…"
                        }
                        .into(),
                    );
                }
                experimental::Event::Note(n) => note.set(Some(n)),
                // The experimental player stays experimental: it says what it can't do rather
                // than quietly handing over to the standard player.
                experimental::Event::NoSound(reason) => {
                    unsupported_trace.event("unsupported", serde_json::json!({ "reason": reason }));
                    note.set(Some(reason));
                    feed.set(Feed::Partial);
                }
                experimental::Event::Unsupported(reason) => {
                    unsupported_trace.event("unsupported", serde_json::json!({ "reason": reason }));
                    status.set("Not supported by the experimental player".into());
                }
                experimental::Event::Ended => status.set("Stream ended".into()),
                experimental::Event::Failed(_) => status.set("Playback failed".into()),
            },
        ));
    });

    // Some providers append data successfully but never deliver a decodable picture. In that
    // case MSE can say "Playing" while the screen stays black. Watch only until the first frame;
    // each fallback gets one chance, so a broken upstream does not loop forever.
    use_resource(move || {
        let _ = feed(); // Each replacement needs its own first-picture check.
        let client = watchdog_client.clone();
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
                            .then(|| standard::live(&client, &selected_url.peek(), true))
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
    let fullscreen_for_keys = fullscreen.clone();
    use_effect(move || {
        let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let digits = digits_for_keys.clone();
        let epoch = epoch_for_keys.clone();
        let fullscreen = fullscreen_for_keys.clone();
        let on_key =
            Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
                if e.default_prevented() {
                    return;
                }
                if crate::controls::is_within_open_dialog(&e) {
                    return;
                }
                if e.alt_key() || e.ctrl_key() || e.meta_key() || e.shift_key() {
                    return;
                }
                let key = e.key().to_ascii_lowercase();
                if options.menu.peek().is_some() {
                    if key == "escape" {
                        e.prevent_default();
                        if let Some((target, ..)) = *options.menu.peek() {
                            crate::controls::focus(&format!("channel-{target}"));
                        }
                        let mut menu = options.menu;
                        menu.set(None);
                    }
                    return;
                }
                // The menu's own key handler can clear its signal before this document listener
                // runs. Still consume Escape here so one press cannot also leave fullscreen.
                let from_options_menu = e
                    .target()
                    .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
                    .is_some_and(|element| {
                        element
                            .closest(".playback-context")
                            .ok()
                            .flatten()
                            .is_some()
                    });
                if from_options_menu && key == "escape" {
                    e.prevent_default();
                    return;
                }
                if key == "escape" && *show_stats.peek() {
                    e.prevent_default();
                    crate::controls::focus("stream-info-toggle");
                    show_stats.set(false);
                    return;
                }
                if *settings_open.peek() {
                    if key == "escape" {
                        e.prevent_default();
                        crate::controls::focus("live-settings-toggle");
                        settings_open.set(false);
                    }
                    return;
                }
                if crate::controls::preserve_native_key(&e) {
                    return;
                }
                match key.as_str() {
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
                    "f" => fullscreen.toggle("live-player", expanded),
                    "i" => show_stats.set(!show_stats()),
                    "escape" => {
                        digits.borrow_mut().clear();
                        dial.set(String::new());
                        epoch.set(epoch.get().wrapping_add(1));
                        fullscreen.exit("live-player", expanded);
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

    let fullscreen_for_picture = fullscreen.clone();
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
    let logo = selected_icon().filter(|src| src.starts_with("http"));
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
                        if !*holding.peek() && !video_el().is_some_and(|v| v.ended()) {
                        if *own_pause.peek() {
                            own_pause.set(false);
                            return;
                        }
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
                        let client = sound_client.clone();
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
                                    if let Some(again) = standard::live(&client, &selected_url.peek(), true) {
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
                        // Chrome's decoded-byte count lags a little: give it a few seconds. Clears
                        // itself once sound arrives.
                        let quiet = no_audio_decoded(&v, 5.0);
                        if quiet != *silent.peek() {
                            silent.set(quiet);
                        }
                    },
                    onclick: move |_| { if settings_open() { settings_open.set(false); } else { toggle_play(); } },
                    ondoubleclick: move |_| fullscreen_for_picture.toggle("live-player", expanded),
                }
                if show_stats() {
                    crate::stream_info::StreamInfo { readout: stats() }
                }
                if audio_only() {
                    div { class: if paused() { "radio paused" } else { "radio" },
                        if let Some(src) = &logo {
                            img { class: "radio-glow", src: "{src}", alt: "" }
                        }
                        div { class: "radio-art",
                            if let Some(src) = &logo { img { src: "{src}", alt: "" } } else { Icon { d: MUSIC } }
                        }
                        strong { "{selected_title}" }
                        div { class: "eq", i {} i {} i {} i {} i {} }
                    }
                }
                if !dial().is_empty() { div { class: "channel-dial", "{dial}" } }
                if note().is_some() || silent() {
                    div { class: "sound-note", role: "img", aria_label: "No sound", title: note().unwrap_or_else(|| "No sound".into()), Icon { d: MUTED } }
                }
                // Recovering while the buffer still plays is silent (it's in the --logs trace);
                // the ring shows only once the picture actually waits.
                if starting(&status()) && (status() != RECOVERING || holding() || buffering()) {
                    div { class: "hud", LoadRing { stage: load_stage(), icon: logo.clone() } }
                } else if failed {
                    div { class: "hud fail",
                        p { "{status}" }
                        button { class: "retry", onclick: move |_| { paused.set(false); let now = standard::clock(); recovery.write().retry_now(now, retry_until.peek().saturating_sub(now)); status.set(RECONNECTING.into()); }, "Retry now" }
                    }
                } else if buffering() && !paused() && !audio_only() {
                    div { class: "hud", LoadRing { stage: 2 } }
                }
                if paused() && !failed && !starting(&status()) {
                    button { class: "bigplay", aria_label: "Play", onclick: move |_| toggle_play(), Icon { d: PLAY } }
                }
                if settings_open() {
                    crate::live_settings::LiveSettings {
                        catalog: catalog.clone(), smoother: smoother(), backup: backup(),
                        onclose: move |_| {
                            crate::controls::focus("live-settings-toggle");
                            settings_open.set(false);
                        },
                        onmode: move |on| { smoother.set(on); preferences::update(|p| p.smoother_live = on); },
                        onbackup: move |target| { backup.set(target); preferences::update(|p| preferences::set_backup(p, id, target)); },
                    }
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
                    if selected_id() != id { span { class: "backup-label", "Backup · {selected_title}" } }
                    span { class: "grow" }
                    button { id: "stream-info-toggle", class: "ctl", aria_label: "Stream info", aria_expanded: show_stats(), aria_controls: "stream-info", title: "Stream info (I)", onclick: move |_| { show_stats.set(!show_stats()); settings_open.set(false); }, Icon { d: INFO } }
                    button { id: "live-settings-toggle", class: "ctl settings-toggle", aria_label: "Live playback settings", aria_expanded: settings_open(), aria_controls: "live-settings", title: "Live playback settings", onclick: move |_| { settings_open.set(!settings_open()); show_stats.set(false); }, Icon { d: GEAR } }
                    button { class: "ctl", aria_label: "Picture in picture", title: "Picture in picture", onclick: move |_| toggle_pip(video_el()), Icon { d: PIP } }
                    button { class: "ctl", aria_label: "Fullscreen", title: "Fullscreen (F)", onclick: move |_| fullscreen.toggle("live-player", expanded), Icon { d: FULLSCREEN } }
                }
            }
            Guide { key: "{selected_id}", id: selected_id(), playing: picture_ready() && !failed }
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
    let mut peek = use_signal(|| None::<u64>);
    let mut hovering = use_signal(|| None::<u64>);
    // One cancellable timer for the guide, not one timer per program.
    use_resource(move || {
        let target = hovering();
        peek.set(None);
        async move {
            if let Some(target) = target {
                rstreamkit::mse::sleep(Duration::from_secs(2)).await;
                if *hovering.peek() == Some(target) {
                    peek.set(Some(target));
                }
            }
        }
    });
    let mut dismiss = move || {
        hovering.set(None);
        peek.set(None);
    };
    let mut reveal = move |start| {
        if *hovering.peek() != Some(start) {
            peek.set(None);
            hovering.set(Some(start));
        }
    };
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
                    let preview = peek().and_then(|selected| {
                        visible.iter().find(|(start, ..)| *start == selected).map(
                            |(start, end, listing, _)| {
                                channel_preview::program_peek(
                                    &format!("guide-{id}-{start}"),
                                    &listing.title,
                                    &listing.description,
                                    *start,
                                    *end,
                                )
                            },
                        )
                    });
                    rsx! {
                        div {
                            class: "timeline",
                            id: "timeline",
                            onscroll: move |_| dismiss(),
                            div { class: "tl", style: "width:{width}px", "data-now": "{here}",
                                for hour in (lo..hi).step_by(3600) {
                                    span { class: "tick", key: "{hour}", style: "left:{px(hour)}px", "{clock(hour)}" }
                                }
                                for (start, end, l, block_width) in visible {
                                    div {
                                        key: "{start}",
                                        id: "guide-{id}-{start}",
                                        tabindex: "0",
                                        aria_describedby: if peek() == Some(start) { Some(format!("guide-{id}-{start}-peek")) } else { None },
                                        onmouseenter: move |_| reveal(start),
                                        onmouseleave: move |_| dismiss(),
                                        onfocus: move |_| reveal(start),
                                        onblur: move |_| dismiss(),
                                        onkeydown: move |e| if e.key() == Key::Escape { dismiss(); },
                                        class: if start <= now && now < end { if block_width < 90 { "block now compact" } else { "block now" } } else if block_width < 90 { "block compact" } else { "block" },
                                        style: "left:{px(start)}px;width:{block_width}px",
                                        div { class: "txt",
                                            time { "{clock(start)} – {clock(end)}" }
                                            strong { if l.title.is_empty() { "Untitled program" } else { "{l.title}" } }
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
                        {preview}
                    }
                }
            }
        }
    };

    rsx! {
        section { class: "guide",
            div { class: "guide-heading", strong { "Program guide" } small { "Local time · {clock(now)}" } }
            {body}
        }
    }
}
