//! Browser fetch through RIPTV's local proxy. Keep the upstream URL for HLS redirects.

use futures_core::Stream;
use rstreamkit::mse::{Fetch, FetchError, Response};
use std::{
    ops::Range,
    pin::Pin,
    task::{Context, Poll},
};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    Headers, ReadableStreamDefaultReader, Request, RequestInit, Response as WebResponse, js_sys,
    wasm_bindgen::{JsCast, JsValue},
};
use xtream::{Client, Url};

#[derive(Clone)]
pub struct Proxied(pub Client);

impl Fetch for Proxied {
    async fn get(&self, url: &str, range: Option<Range<u64>>) -> Result<Response, FetchError> {
        let upstream =
            Url::parse(url).map_err(|_| FetchError::Permanent("invalid stream URL".into()))?;
        let address = self.0.proxied(upstream).to_string();
        let failed = |e: JsValue| FetchError::Temporary(format!("stream request failed: {e:?}"));
        let headers = Headers::new().map_err(failed)?;
        if let Some(r) = range {
            headers
                .set("Range", &format!("bytes={}-{}", r.start, r.end - 1))
                .map_err(failed)?;
        }
        let init = RequestInit::new();
        init.set_headers(&headers);
        let request = Request::new_with_str_and_init(&address, &init).map_err(failed)?;
        let response: WebResponse = JsFuture::from(
            web_sys::window()
                .ok_or_else(|| FetchError::Permanent("no browser window".into()))?
                .fetch_with_request(&request),
        )
        .await
        .map_err(failed)?
        .unchecked_into();
        let header = |name: &str| response.headers().get(name).ok().flatten();
        // The proxy follows redirects; the browser sees only its own URL. Use the final upstream
        // URL it reports so that relative segment paths resolve against the right playlist.
        let final_url = header("x-upstream-url").unwrap_or_else(|| url.to_owned());
        Ok(Response {
            status: response.status(),
            url: final_url,
            content_type: header("content-type").unwrap_or_default(),
            content_length: header("content-length").and_then(|v| v.parse().ok()),
            range_total: header("content-range")
                .and_then(|v| v.rsplit('/').next().and_then(|s| s.parse().ok())),
            body: Box::pin(Chunks {
                reader: response.body().map(|b| b.get_reader().unchecked_into()),
                pending: None,
            }),
        })
    }
}

struct Chunks {
    reader: Option<ReadableStreamDefaultReader>,
    pending: Option<JsFuture>,
}

impl Stream for Chunks {
    type Item = Result<Vec<u8>, String>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        let Some(reader) = &this.reader else {
            return Poll::Ready(None);
        };
        let result = match Pin::new(
            this.pending
                .get_or_insert_with(|| JsFuture::from(reader.read())),
        )
        .poll(cx)
        {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(result) => result,
        };
        this.pending = None;
        Poll::Ready(match result {
            Err(e) => Some(Err(format!("stream read failed: {e:?}"))),
            Ok(chunk) => {
                let get = |key: &str| js_sys::Reflect::get(&chunk, &key.into()).ok();
                if get("done").and_then(|v| v.as_bool()).unwrap_or(true) {
                    None
                } else {
                    Some(
                        get("value")
                            .map(|v| js_sys::Uint8Array::new(&v).to_vec())
                            .ok_or_else(|| "stream chunk has no data".into()),
                    )
                }
            }
        })
    }
}
