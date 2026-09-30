//! Not a check of behaviour: what parsing a big provider list costs, for comparing changes by.
//!
//! `cargo test -p xtream --release -- --ignored --nocapture parse_cost`
//!
//! It counts every allocation, which is what the browser's (never shrinking) memory pays for.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering::Relaxed},
    time::Instant,
};

use crate::{Series, VodStream, stream::ListReader, vec_from_slice};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards to the system allocator and only counts.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            CALLS.fetch_add(1, Relaxed);
            let now = LIVE.fetch_add(layout.size(), Relaxed) + layout.size();
            PEAK.fetch_max(now, Relaxed);
        }
        p
    }

    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        unsafe { System.dealloc(p, layout) };
        LIVE.fetch_sub(layout.size(), Relaxed);
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// A list like a big provider's: `n` movies, the fields real panels send (numbers as text, some
/// missing), names with accents.
fn movies(n: usize) -> Vec<u8> {
    let mut out = String::from("[");
    for i in 0..n {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            r#"{{"num":{i},"name":"Última Crimson Éternel {i}","stream_type":"movie","stream_id":{},"stream_icon":"https://image.tmdb.org/t/p/w600_and_h900_bestv2/abcdefghijklmnop{i}.jpg","rating":"{}.{}","rating_5based":3.5,"added":"{}","category_id":"{}","container_extension":"mkv","custom_sid":"","direct_source":""}}"#,
            1000 + i,
            i % 10,
            i * 3 % 10,
            1_600_000_000 + i as u64 * 3601 % 190_000_000,
            i % 40 + 1
        ));
    }
    out.push(']');
    out.into_bytes()
}

fn series(n: usize) -> Vec<u8> {
    let mut out = String::from("[");
    for i in 0..n {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            r#"{{"num":{i},"name":"Silent Orchard {i}","series_id":{},"cover":"https://image.tmdb.org/t/p/w600_and_h900_bestv2/s{i}.jpg","plot":"A synthetic series, with a plot of a realistic length for a list.","cast":"A, B, C","director":"D","genre":"Drama","releaseDate":"2020-01-01","last_modified":"1700000000","rating":"7.5","rating_5based":"3.5","backdrop_path":[],"youtube_trailer":"","episode_run_time":"45","category_id":"{}"}}"#,
            5000 + i,
            i % 20 + 1
        ));
    }
    out.push(']');
    out.into_bytes()
}

/// What reading a list costs from the moment its first byte arrives, in 64 KB pieces like a
/// network: `held` keeps the whole body before parsing it (how it used to be read), otherwise each
/// piece is parsed as it comes and dropped.
fn cost<T: serde::de::DeserializeOwned>(label: &str, json: &[u8], held: bool) {
    let mut best = u128::MAX;
    let mut last = (0, 0, 0);
    for _ in 0..7 {
        let (base, calls) = (LIVE.load(Relaxed), CALLS.load(Relaxed));
        PEAK.store(base, Relaxed);
        let t = Instant::now();
        let list = if held {
            let mut body = Vec::new();
            for piece in json.chunks(64 << 10) {
                // (Each piece arrives as a buffer of its own, as from a network read.)
                let arrived = piece.to_vec();
                body.extend_from_slice(&arrived);
            }
            vec_from_slice::<T>(&body).unwrap()
        } else {
            let mut reader = ListReader::<T>::new();
            for piece in json.chunks(64 << 10) {
                let arrived = piece.to_vec();
                reader.feed(&arrived).unwrap();
            }
            reader.finish().unwrap()
        };
        best = best.min(t.elapsed().as_micros());
        last = (
            CALLS.load(Relaxed) - calls,
            PEAK.load(Relaxed) - base,
            LIVE.load(Relaxed) - base,
        );
        std::hint::black_box(&list);
    }
    println!(
        "{label} ({}): {} KB of JSON -> {:.1} ms, {} allocations, peak {} KB, kept {} KB",
        if held { "whole body held" } else { "streamed" },
        json.len() / 1024,
        best as f64 / 1000.0,
        last.0,
        last.1 / 1024,
        last.2 / 1024
    );
}

#[test]
#[ignore = "a measurement, not a test"]
fn parse_cost() {
    for held in [true, false] {
        cost::<VodStream>("30,000 movies", &movies(30_000), held);
        cost::<Series>("6,000 series", &series(6_000), held);
    }
}
