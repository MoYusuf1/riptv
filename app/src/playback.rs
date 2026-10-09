//! Shared movie/episode engine selection and file metadata, separate from player controls.
use super::{Client, Proxied};
use dioxus::prelude::*;
use rstreamkit::vod::Verdict;
use std::rc::Rc;

#[derive(Clone)]
pub(crate) enum Engine {
    /// Keep the indexed length even when the browser plays the file directly.
    Native(Option<f64>),
    Rust(Rc<rstreamkit::vod::Movie>, xtream::Url),
    Converted(xtream::Converted, String),
    Failed(String),
}

impl Engine {
    pub(crate) fn duration(&self) -> Option<f64> {
        let value = match self {
            Self::Native(d) => *d,
            Self::Rust(movie, _) => Some(movie.duration),
            Self::Converted(stream, _) => stream.duration.map(|d| d as f64),
            Self::Failed(_) => None,
        };
        value.filter(|d| d.is_finite() && *d > 0.0)
    }
}

pub(crate) async fn choose(c: &Client, url: &str, experimental: bool) -> Engine {
    let Ok(media) = xtream::Url::parse(url) else {
        return Engine::Failed("that address is not valid".into());
    };
    let media = c.upstream(&media);
    let Ok(movie) = rstreamkit::mse::probe(&Proxied(c.clone()), media.as_str()).await else {
        return Engine::Native(None);
    };
    match movie.verdict(&rstreamkit::mse::can_play) {
        Verdict::Native => Engine::Native(Some(movie.duration)),
        Verdict::Rust if experimental => Engine::Rust(movie, media),
        Verdict::Rust => convert(c, &media, "a format requiring conversion".into()).await,
        Verdict::Unsupported(why) => convert(c, &media, why.to_string()).await,
        _ => Engine::Failed("unsupported movie format".into()),
    }
}

async fn convert(c: &Client, media: &xtream::Url, why: String) -> Engine {
    match c.convert(media).await {
        Ok(converted) => Engine::Converted(converted, why),
        Err(e) => Engine::Failed(format!(
            "This has {why}, which your browser can't play, and it could not be converted: {e}"
        )),
    }
}

pub(crate) async fn probe_duration(c: &Client, url: &str) -> Option<f64> {
    let media = xtream::Url::parse(url).ok()?;
    // convert() only checks metadata and returns a URL; conversion starts when that URL is read.
    let checked = c.convert(&media).await.ok()?;
    checked.duration.filter(|d| *d > 0).map(|d| d as f64)
}

pub(crate) fn switch_to_convert(
    c: Client,
    url: String,
    why: String,
    at: f64,
    mut engine: Signal<Option<Engine>>,
    mut start: Signal<u64>,
) {
    wasm_bindgen_futures::spawn_local(async move {
        let Ok(media) = xtream::Url::parse(&url) else {
            return;
        };
        let converted = convert(&c, &media, why).await;
        if engine.try_peek().is_ok() {
            start.set(at as u64);
            engine.set(Some(converted));
        }
    });
}
