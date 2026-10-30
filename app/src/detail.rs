//! A movie's or series' page, and the small formatting helpers it uses.
use super::*;

/// The container a stream address ends in (`mkv`), if it looks like one.
pub(crate) fn extension(url: &str) -> Option<String> {
    url.rsplit('.')
        .next()
        .filter(|e| (1..=4).contains(&e.len()) && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(str::to_owned)
}

/// What a shelf calls this title.
pub(crate) fn identity(row: &Row) -> (shelves::Title, u64) {
    match &row.target {
        Target::Series(id) => (shelves::Title::Series, *id),
        Target::Movie { id, .. } | Target::Live { id, .. } => (shelves::Title::Movie, *id),
    }
}

/// A title as the shelves remember it.
pub(crate) fn entry_of(
    row: &Row,
    title: &str,
    icon: Option<String>,
    ext: Option<String>,
    sub: Option<String>,
) -> shelves::Entry {
    let (kind, id) = identity(row);
    shelves::Entry {
        kind,
        id,
        title: title.to_owned(),
        icon,
        ext,
        sub,
        at: 0.0,
        total: 0.0,
    }
}

/// "2h 53m" or "42m".
pub(crate) fn runtime_label(secs: u64) -> String {
    let (h, m) = (secs / 3600, secs / 60 % 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{}m", m.max(1))
    }
}

/// The local time, `secs` from now: "2:23 PM".
pub(crate) fn ends_at(secs: u64) -> String {
    let d = js_sys::Date::new(&JsValue::from_f64(
        js_sys::Date::now() + secs as f64 * 1000.0,
    ));
    let h = d.get_hours();
    format!(
        "{}:{:02} {}",
        if h.is_multiple_of(12) { 12 } else { h % 12 },
        d.get_minutes(),
        if h < 12 { "AM" } else { "PM" }
    )
}

/// "Jul 14, 2026" from "2026-07-14"; anything else as it came.
pub(crate) fn long_date(raw: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut parts = raw.get(..10).unwrap_or(raw).split('-');
    if let (Some(y), Some(m), Some(d)) = (parts.next(), parts.next(), parts.next())
        && y.len() == 4
        && let (Ok(m @ 1..=12), Ok(d)) = (m.parse::<usize>(), d.parse::<u32>())
    {
        return format!("{} {d}, {y}", MONTHS[m - 1]);
    }
    raw.to_owned()
}

/// The names in a cast list ("A, B / C").
pub(crate) fn people(list: &str) -> Vec<&str> {
    list.split([',', '/', ';'])
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .take(24)
        .collect()
}

/// The id in a YouTube address or a bare id.
pub(crate) fn youtube_id(text: &str) -> Option<String> {
    let ok = |s: &str| {
        s.len() == 11
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };
    let text = text.trim();
    [
        text,
        text.split("v=").nth(1).unwrap_or(""),
        text.rsplit('/').next().unwrap_or(""),
    ]
    .iter()
    .map(|s| s.split(['&', '?']).next().unwrap_or(""))
    .find(|s| ok(s))
    .map(str::to_owned)
}

/// "3. Title", the title alone when it already says which episode it is, or "Episode 3".
pub(crate) fn episode_name(ep: &Episode, nth: usize) -> String {
    let num = ep.episode_num.unwrap_or(nth as u64 + 1);
    let name = ep.name();
    if name.is_empty() {
        format!("Episode {num}")
    } else if name.to_uppercase().contains(&format!("E{num:02}")) {
        name.to_owned()
    } else {
        format!("{num}. {name}")
    }
}

thread_local! {
    /// Performers already asked Wikipedia about this session (`None`: no page about them).
    static PORTRAITS: RefCell<HashMap<String, Option<xtream::wiki::Performer>>> = RefCell::default();
}

/// The page for one movie or series: backdrop, title, facts, cast, trailer, and Play or the episodes.
#[component]
pub(crate) fn DetailPage(
    row: Row,
    inert: bool,
    #[props(default)] resume_epoch: u64,
    onback: EventHandler<()>,
    onplay: EventHandler<(Rc<Vec<Play>>, usize)>,
) -> Element {
    let session = use_context::<Signal<Option<Client>>>();
    let client = use_hook(|| session.read().clone().expect("logged in"));
    let mut expanded = use_signal(|| false);
    let mut chosen = use_signal(|| None::<u64>);
    let mut seasons_open = use_signal(|| false);
    let mut listed = use_signal(|| {
        let (kind, id) = identity(&row);
        shelves::is_mine(kind, id)
    });
    let mut backdrop_failed = use_signal(|| false);
    let mut poster_failed = use_signal(|| false);
    let (c, target) = (client.clone(), row.target.clone());
    let info = use_resource(move || {
        let (c, target) = (c.clone(), target.clone());
        async move {
            match target {
                Target::Movie { id, .. } => c.vod_info(id).await.map(|d| (d, Vec::<Season>::new())),
                Target::Series(id) => c.series_info(id).await.map(|s| (s.details, s.seasons)),
                Target::Live { .. } => Ok((Details::default(), vec![])),
            }
        }
    });

    // What the provider left out of a series (episode pictures and summaries, cast photos), from
    // TVmaze, once the provider has said what the show is called. The page doesn't wait for it.
    let (c, target) = (client.clone(), row.target.clone());
    let listing = row.title.clone();
    let guide = use_resource(move || {
        let ask = match (&target, &*info.read()) {
            (Target::Series(_), Some(Ok((d, _)))) => Some((
                if d.name.is_empty() {
                    listing.clone()
                } else {
                    d.name.clone()
                },
                d.year
                    .as_deref()
                    .and_then(|y| y.get(..4)?.parse::<u32>().ok()),
            )),
            _ => None,
        };
        let c = c.clone();
        async move {
            let (name, year) = ask?;
            c.guide(&name, year).await.map(Rc::new)
        }
    });
    let guide_now = guide.read().clone().flatten();
    // A movie's cast photos (TVmaze covers series only), each filled in as Wikipedia answers.
    let mut photos = use_signal(HashMap::<String, xtream::wiki::Performer>::new);
    // Each actor's IMDb profile when Wikidata knows it, otherwise IMDb's search for their name.
    let imdb = move |name: &str| {
        photos
            .read()
            .get(name)
            .and_then(|p| p.imdb.clone())
            .unwrap_or_else(|| xtream::wiki::imdb_search(name))
    };
    let (c, target) = (client.clone(), row.target.clone());
    use_effect(move || {
        let Target::Movie { .. } = target else { return };
        let Some(Ok((d, _))) = &*info.read() else {
            return;
        };
        let cast = d.cast.clone().unwrap_or_default();
        let c = c.clone();
        spawn(async move {
            for name in people(&cast).into_iter().take(12).map(str::to_owned) {
                let known = PORTRAITS.with(|p| p.borrow().get(&name).cloned());
                match known {
                    Some(Some(found)) => {
                        photos.write().insert(name, found);
                    }
                    Some(None) => {}
                    None => {
                        // One lookup at a time, cancelled with this detail page.
                        let found = c.performer(&name).await;
                        PORTRAITS.with(|p| {
                            let mut cache = p.borrow_mut();
                            if cache.len() >= 256
                                && let Some(old) = cache.keys().next().cloned()
                            {
                                cache.remove(&old);
                            }
                            cache.insert(name.clone(), found.clone())
                        });
                        if let Some(found) = found
                            && photos.try_peek().is_ok()
                        {
                            photos.write().insert(name, found);
                        }
                    }
                }
            }
        });
    });

    let seasons = use_memo(move || {
        let guard = info.read();
        let guide = guide.read();
        let mut seasons = guard
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .map(|(_, s)| s.clone())
            .unwrap_or_default();
        if let Some(g) = guide.as_ref().and_then(|g| g.as_ref()) {
            xtream::tvmaze::fill(&mut seasons, g);
        }
        Snapshot::new(seasons)
    });
    let seasons = seasons();
    let guard = info.read();
    let mut details = match &*guard {
        Some(Ok((d, _))) => d.clone(),
        _ => Details::default(),
    };
    // The provider's episodes, with the gaps filled in from the guide when it has arrived.
    if let Some(g) = &guide_now {
        details.plot = details.plot.take().or_else(|| g.plot.clone());
        details.poster = details.poster.take().or_else(|| g.poster.clone());
    }
    let title = if details.name.is_empty() {
        row.title.clone()
    } else {
        xtream::display_title(&details.name).to_owned()
    };
    let poster = details.poster.clone().or_else(|| row.icon.clone());
    let real_backdrop = details.backdrop.clone();
    let backdrop_src = real_backdrop
        .as_deref()
        .filter(|src| src.starts_with("http") && !backdrop_failed());
    let poster_src = poster
        .as_deref()
        .filter(|src| src.starts_with("http") && !poster_failed());
    // An episode with no picture of its own shows the series' art, dimmed, not an empty box.
    let series_still = backdrop_src
        .or(poster_src)
        .map(|src| xtream::sized_art(src, xtream::Art::Still));
    let genres: Vec<String> = details
        .genre
        .as_deref()
        .map(|g| {
            g.split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .take(3)
                .collect()
        })
        .unwrap_or_default();
    let quality = details
        .video
        .as_deref()
        .and_then(|v| v.split(" · ").next())
        .filter(|q| q.ends_with('p') || *q == "4K")
        .map(str::to_owned);
    let long = details
        .plot
        .as_ref()
        .is_some_and(|p| p.chars().count() > 240);
    let cast = details.cast.clone().unwrap_or_default();
    let trailer = details.trailer.as_deref().and_then(youtube_id);
    let is_movie = matches!(row.target, Target::Movie { .. });
    let for_list = entry_of(
        &row,
        &title,
        poster.clone(),
        match &row.target {
            Target::Movie { url, .. } => extension(url),
            _ => None,
        },
        None,
    );

    let mut facts: Vec<(&str, String)> = vec![];
    if let Some(secs) = details.runtime_secs {
        facts.push((
            "Runtime",
            if is_movie {
                format!("{} · Ends {}", runtime_label(secs), ends_at(secs))
            } else {
                format!("{} per episode", runtime_label(secs))
            },
        ));
    }
    if let Some(date) = &details.release_date {
        facts.push(("Release date", long_date(date)));
    }
    if let Some(country) = &details.country {
        facts.push(("Country", country.clone()));
    }
    if let Some(v) = &details.video {
        facts.push(("Video", v.clone()));
    }
    if let Some(a) = &details.audio {
        facts.push(("Audio", a.clone()));
    }

    // Everything there is to watch, in order: a movie is one, a series is its episodes.
    let runtime = details.runtime_secs;
    let build_row = row.clone();
    let build_client = client.clone();
    let built = use_memo(use_reactive(
        (&title, &poster, &seasons, &runtime),
        move |(title, poster, seasons, runtime)| {
            let row = &build_row;
            let client = &build_client;
            let mut all: Vec<Play> = Vec::with_capacity(
                seasons
                    .iter()
                    .map(|s| s.episodes.len())
                    .sum::<usize>()
                    .max(1),
            );
            let mut spans: Vec<(u64, std::ops::Range<usize>)> = vec![];
            match &row.target {
                Target::Movie { id, url } => all.push(Play {
                    key: format!("movie:{id}"),
                    title: title.clone(),
                    subtitle: None,
                    file: title.clone(),
                    url: url.clone(),
                    duration: runtime,
                    entry: Some(entry_of(row, &title, poster.clone(), extension(url), None)),
                }),
                Target::Series(_) => {
                    for season in seasons.iter() {
                        let from = all.len();
                        for (n, ep) in season.episodes.iter().enumerate() {
                            let num = ep.episode_num.unwrap_or(n as u64 + 1);
                            let ext = ep
                                .container_extension
                                .as_deref()
                                .filter(|e| !e.is_empty())
                                .unwrap_or("mp4");
                            let name = match ep.name() {
                                "" => format!("Episode {num}"),
                                name => name.to_owned(),
                            };
                            all.push(Play {
                                key: format!("episode:{}", ep.id),
                                title: title.clone(),
                                subtitle: Some(format!("S{} E{num} · {name}", season.number)),
                                file: format!("{title} S{}E{num}: {name}", season.number),
                                url: client.episode_url(ep.id, ext).to_string(),
                                duration: ep.info.runtime_secs,
                                entry: Some(entry_of(
                                    row,
                                    &title,
                                    poster.clone(),
                                    None,
                                    Some(format!("S{} E{num}", season.number)),
                                )),
                            });
                        }
                        spans.push((season.number, from..all.len()));
                    }
                }
                _ => {}
            }
            (Rc::new(all), Rc::new(spans))
        },
    ));
    let (all, spans) = built();
    let play_at = {
        let all = all.clone();
        move |i: usize| onplay.call((all.clone(), i))
    };
    // The main button plays the episode last left half watched (or the movie), else the first.
    // Re-scan only when this immutable play list changes or playback closes.
    let resume_cache = use_hook(|| Rc::new(RefCell::new(resume::ResumeMemo::new())));
    let progress_cache = use_hook(|| Rc::new(RefCell::new(resume::ProgressCache::new())));
    let (start_at, resume) = resume_cache
        .borrow_mut()
        .get_or_compute(&all, resume_epoch, || {
            resume::last_unfinished(&all, |index, play| {
                progress_cache
                    .borrow_mut()
                    .get_or_load(&all, resume_epoch, index, || shelves::progress(&play.key))
            })
        });
    let episode_now = all
        .get(start_at)
        .and_then(|p| p.subtitle.as_deref())
        .and_then(|s| s.split(" · ").next())
        .map(str::to_owned);
    let season_now = chosen().or_else(|| spans.first().map(|s| s.0));

    rsx! {
        section { class: "detail", inert: inert.then_some(""),
            // At the page's root: inside a section, content-visibility would confine it to that section.
            if seasons_open() {
                button { class: "scrim", aria_label: "Close menu", onclick: move |_| seasons_open.set(false) }
            }
            div { class: "detail-return",
                button { class: "icon-btn back", aria_label: "Back", title: "Back (Esc)", onclick: move |_| onback.call(()), Icon { d: BACK } }
            }
            div { class: if backdrop_src.is_none() && poster_src.is_none() { "d-hero no-art" } else { "d-hero" },
                if let Some(src) = backdrop_src {
                    img { class: "d-art", src: "{xtream::sized_art(src, xtream::Art::Backdrop)}", decoding: "async", fetchpriority: "high", onerror: move |_| backdrop_failed.set(true) }
                } else if let Some(src) = poster_src {
                    img { class: "d-poster", src: "{xtream::sized_art(src, xtream::Art::Poster)}", decoding: "async", fetchpriority: "high", onerror: move |_| poster_failed.set(true) }
                }
                div { class: "d-shade" }
                div { class: "d-main",
                    h1 { class: "d-title", "{title}" }
                    if !genres.is_empty() {
                        div { class: "d-genres", for g in genres.iter() { span { key: "{g}", "{g}" } } }
                    }
                    div { class: "d-actions",
                        if !all.is_empty() {
                            button { id: "title-play", class: "play", onclick: { let play_at = play_at.clone(); move |_| play_at(start_at) },
                                Icon { d: PLAY }
                                if let Some((at, total)) = resume {
                                    "Resume"
                                    if let Some(ep) = &episode_now { " {ep}" }
                                    if total > at { " · {runtime_label((total - at) as u64)} left" }
                                } else { "Play" }
                            }
                        }
                        button {
                            class: if listed() { "icon-btn on" } else { "icon-btn" },
                            title: if listed() { "Remove from My List" } else { "Add to My List" },
                            aria_label: "My List",
                            aria_pressed: "{listed()}",
                            onclick: move |_| listed.set(shelves::toggle_mine(for_list.clone())),
                            Icon { d: HEART }
                        }
                        if let Some(id) = &trailer {
                            a { class: "icon-btn", href: "https://www.youtube.com/watch?v={id}", target: "_blank", rel: "noopener noreferrer", title: "Trailer on YouTube", aria_label: "Trailer on YouTube", Icon { d: EXTERNAL } }
                        }
                        if is_movie && let Some(p) = all.first() { Download { title: p.file.clone(), url: p.url.clone() } }
                    }
                    div { class: "d-meta",
                        if let Some(y) = &details.year { span { "{y}" } }
                        if let Some(secs) = details.runtime_secs { span { "{runtime_label(secs)}" } }
                        if let Some(age) = &details.age { span { class: "badge", "{age}" } }
                        if let Some(q) = &quality { span { class: "badge", "{q}" } }
                        if let Some(r) = score(&details.rating) { span { span { class: "star", "★ " } "{r}" } }
                    }
                    if let Some(director) = &details.director { p { class: "d-by", "Director: " b { "{director}" } } }
                    if let Some(plot) = &details.plot {
                        p { class: if expanded() || !long { "plot" } else { "plot clamp" }, "{plot}" }
                        if long {
                            button { class: "more", onclick: move |_| expanded.set(!expanded()), if expanded() { "Show less" } else { "Read more" } }
                        }
                    }
                    match &*guard {
                        None => rsx! { p { class: "dim", "Loading…" } },
                        Some(Err(e)) => rsx! { p { class: "err", "{e}" } },
                        Some(Ok(_)) => rsx! {},
                    }
                }
                if !facts.is_empty() {
                    dl { class: "d-facts",
                        for (name, value) in facts { div { key: "{name}", dt { "{name}" } dd { "{value}" } } }
                    }
                }
            }
            if !spans.is_empty() {
                // Raised above the page's scrim while the season menu is open, so it stays clickable.
                div { class: if seasons_open() { "d-sec raised" } else { "d-sec" },
                    div { class: "ep-heading",
                        h2 { "Episodes" }
                        div { class: "ep-tools",
                            span { "{spans.iter().find(|s| Some(s.0) == season_now).map_or(0, |s| s.1.len())} episodes" }
                            if spans.len() > 1 {
                                // The app's own menu, not the browser's dropdown.
                                div { class: "sortwrap",
                                    onkeydown: move |e| {
                                        if e.key() == Key::Escape && seasons_open() {
                                            e.stop_propagation();
                                            e.prevent_default();
                                            crate::controls::focus("season-trigger");
                                            seasons_open.set(false);
                                        }
                                    },
                                    button {
                                        id: "season-trigger",
                                        class: "sort-btn season-btn",
                                        aria_label: "Season",
                                        aria_haspopup: "menu",
                                        aria_expanded: seasons_open(),
                                        onclick: move |_| seasons_open.set(!seasons_open()),
                                        "Season {season_now.unwrap_or_default()}"
                                        Icon { d: CHEVRON }
                                    }
                                    if seasons_open() {
                                        div { class: "menu season-menu", role: "menu",
                                            for (number, _) in spans.iter().cloned() {
                                                button {
                                                    key: "{number}",
                                                    role: "menuitemradio",
                                                    aria_checked: Some(number) == season_now,
                                                    class: if Some(number) == season_now { "on" } else { "" },
                                                    onclick: move |_| {
                                                        crate::controls::focus("season-trigger");
                                                        chosen.set(Some(number));
                                                        seasons_open.set(false);
                                                    },
                                                    span { "Season {number}" }
                                                    if Some(number) == season_now { Icon { d: CHECK } }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    div { class: "eps",
                        if let Some((_, range)) = spans.iter().find(|s| Some(s.0) == season_now) {
                            for (i, ep) in seasons.iter().flat_map(|s| s.episodes.iter()).enumerate().skip(range.start).take(range.len()) {
                                div { class: "epc", key: "{ep.id}",
                                    button { class: "thumb", aria_label: "Play {episode_name(ep, i - range.start)}", onclick: { let play_at = play_at.clone(); move |_| play_at(i) },
                                        if let Some(img) = ep.info.image.as_deref().filter(|s| s.starts_with("http")) {
                                            img { src: "{xtream::sized_art(img, xtream::Art::Still)}", loading: "lazy", decoding: "async" }
                                        } else if let Some(src) = &series_still {
                                            img { class: "series-still", src: "{src}", loading: "lazy", decoding: "async", alt: "" }
                                        }
                                        if let Some(p) = progress_cache.borrow_mut().get_or_load(&all, resume_epoch, i, || shelves::progress(&all[i].key))
                                            .and_then(|(at, total)| shelves::percent_of(at, total))
                                        { span { class: "prog", i { style: "width:{p}%" } } }
                                        span { class: "ep-num", "E{ep.episode_num.unwrap_or((i - range.start + 1) as u64)}" }
                                        span { class: "go", Icon { d: PLAY } }
                                        if let Some(secs) = ep.info.runtime_secs { span { class: "len", "{runtime_label(secs)}" } }
                                    }
                                    div { class: "epc-head",
                                        div {
                                            h4 { "{episode_name(ep, i - range.start)}" }
                                            if let Some(date) = &ep.info.release_date { small { "{long_date(date)}" } }
                                        }
                                        Download { title: all[i].file.clone(), url: all[i].url.clone() }
                                    }
                                    if let Some(plot) = &ep.info.plot { p { "{plot}" } }
                                }
                            }
                        }
                    }
                }
            }
            if let Some(g) = guide_now.as_ref().filter(|g| g.cast.iter().any(|p| p.photo.is_some())) {
                div { class: "d-sec",
                    h2 { "Cast" }
                    div { class: "cast portraits",
                        for p in g.cast.iter().take(24) {
                            a { class: "face", key: "{p.name}", href: "{xtream::wiki::imdb_search(&p.name)}", target: "_blank", rel: "noopener noreferrer", title: "{p.name} on IMDb",
                                if let Some(photo) = &p.photo {
                                    img { src: "{photo}", alt: "", loading: "lazy", decoding: "async" }
                                } else {
                                    span { class: "initial", "{p.name.chars().next().unwrap_or('?')}" }
                                }
                                strong { "{p.name}" }
                                if let Some(role) = p.role.as_ref().filter(|r| **r != p.name) { small { "{role}" } }
                            }
                        }
                    }
                }
            } else if photos.read().values().any(|p| p.photo.is_some()) {
                div { class: "d-sec",
                    h2 { "Cast" }
                    div { class: "cast portraits",
                        for name in people(&cast).into_iter().take(12) {
                            a { class: "face", key: "{name}", href: "{imdb(name)}", target: "_blank", rel: "noopener noreferrer", title: "{name} on IMDb",
                                if let Some(photo) = photos.read().get(name).and_then(|p| p.photo.clone()) {
                                    img { src: "{photo}", alt: "", loading: "lazy", decoding: "async" }
                                } else {
                                    span { class: "initial", "{name.chars().next().unwrap_or('?')}" }
                                }
                                strong { "{name}" }
                            }
                        }
                    }
                }
                p { class: "d-sec credit", "Cast photos from "
                    a { href: "https://en.wikipedia.org", target: "_blank", rel: "noopener noreferrer", "Wikipedia" }
                }
            } else if !cast.is_empty() {
                div { class: "d-sec",
                    h2 { "Cast" }
                    div { class: "cast",
                        for name in people(&cast) {
                            a { class: "person", key: "{name}", href: "{imdb(name)}", target: "_blank", rel: "noopener noreferrer", title: "{name} on IMDb",
                                strong { "{name}" }
                            }
                        }
                    }
                }
            }
            // TVmaze's data is CC BY-SA: credited wherever it fills something in.
            if let Some(g) = &guide_now {
                p { class: "d-sec credit", "Episode details and cast from "
                    a { href: "{g.page}", target: "_blank", rel: "noopener noreferrer", "TVmaze" }
                }
            }
            if let Some(id) = &trailer {
                div { class: "d-sec",
                    h2 { "Trailer" }
                    a { class: "trailer", href: "https://www.youtube.com/watch?v={id}", target: "_blank", rel: "noopener noreferrer",
                        img { src: "https://img.youtube.com/vi/{id}/hqdefault.jpg", loading: "lazy" }
                        span { "Watch the trailer" }
                    }
                }
            }
            div { class: "d-sec" }
        }
    }
}
