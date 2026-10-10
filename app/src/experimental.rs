//! The experimental player: rstreamkit reads the stream and feeds the `<video>` itself, in the
//! page. It is the only place that starts rstreamkit playback, and it never hands over to the
//! standard player (`standard.rs`, the proxy's ffmpeg); the screens decide what to do with what it
//! reports.
use crate::fetch::Proxied;
use rstreamkit::{Unsupported, mse};
use std::rc::Rc;
use web_sys::HtmlVideoElement;
use xtream::Client;

/// Playback stops when this is dropped.
pub(crate) type Player = mse::Player;

/// What the experimental player reports, in the app's terms.
pub(crate) enum Event {
    Playing,
    /// Something to tell the viewer while playback goes on.
    Note(String),
    /// It can't play the sound; the picture can still play without it.
    NoSound(String),
    /// It can't play this at all, and why.
    Unsupported(String),
    Ended,
    Failed(String),
}

fn event(status: mse::Status) -> Option<Event> {
    Some(match status {
        mse::Status::Playing => Event::Playing,
        mse::Status::Note(n) => Event::Note(n),
        mse::Status::Unsupported(why @ Unsupported::Sound(_)) => Event::NoSound(why.to_string()),
        mse::Status::Unsupported(why) => Event::Unsupported(why.to_string()),
        mse::Status::Ended => Event::Ended,
        mse::Status::Failed(e) => Event::Failed(e),
        _ => return None,
    })
}

/// A live channel's HLS playlist. `partial` plays the picture only; `sound` decodes sound the
/// browser can't (AC-3, E-AC-3, MP2) in Rust.
pub(crate) fn live(
    video: HtmlVideoElement,
    playlist: String,
    client: Client,
    partial: bool,
    sound: bool,
    mut on: impl FnMut(Event) + 'static,
) -> Player {
    mse::start(video, playlist, Proxied(client), partial, sound, move |s| {
        if let Some(e) = event(s) {
            on(e)
        }
    })
}

/// A movie or episode whose index has been read, from `at` seconds.
pub(crate) fn movie(
    video: HtmlVideoElement,
    movie: Rc<rstreamkit::vod::Movie>,
    url: String,
    client: Client,
    at: f64,
    mut on: impl FnMut(Event) + 'static,
) -> Player {
    mse::play_movie(video, movie, url, Proxied(client), at, move |s| {
        if let Some(e) = event(s) {
            on(e)
        }
    })
}
