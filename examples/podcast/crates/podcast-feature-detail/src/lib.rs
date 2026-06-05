//! Podcast detail screen — `/podcast/:id`.
//!
//! Layout:
//!
//! ```text
//!   ┌───────────────────────────┐
//!   │  ← back   title           │   top bar (safe-area inset)
//!   ├───────────────────────────┤
//!   │       ╔═══════════╗       │
//!   │       ║           ║       │   large artwork
//!   │       ║  artwork  ║       │
//!   │       ║           ║       │
//!   │       ╚═══════════╝       │
//!   │      Show title           │
//!   │      Artist name          │
//!   │      Genre · 42 ep        │
//!   │     [ + Follow ]          │   placeholder pill
//!   ├───────────────────────────┤
//!   │  Episodes                 │
//!   │  ─────────                │
//!   │  Episode list placeholder │
//!   └───────────────────────────┘
//! ```
//!
//! The screen reads the shared [`PodcastIndex`] context (set up by
//! the top-level `podcast` crate) to look up the podcast by `id`
//! at render time — no re-fetch, no prop-drilling. If the id is
//! missing (e.g. a deep-link to a podcast Browse hasn't cached
//! yet), the screen renders a "not found" pane. A future PR can
//! plumb the data layer's `fetch_podcast_by_id` through `resource`
//! to handle the deep-link case.
//!
//! ## Playback wiring
//!
//! The detail screen reads the shared [`Player`] handle and the
//! [`NowPlayingSignal`] from context (both provided by the shell)
//! and writes both on tap: `Player::set_source(url) + play()`
//! starts audio, and updating the now-playing signal flips the
//! mini-player into "showing a track" mode.
//!
//! Episodes themselves come from a `resource()`-driven iTunes
//! lookup that runs once per detail-screen mount and re-fires when
//! a new id lands.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use podcast_data::fetch_episodes;
use podcast_domain::{Episode, NowPlaying, Podcast};
use podcast_routing::Navigator;
use podcast_theme as theme;
use whisker::css::{
    AlignItems, Display, FlexDirection, FontWeight, JustifyContent, TextAlign, TextOverflow,
};
use whisker::prelude::*;
use whisker::runtime::tasks::run_blocking;
use whisker::runtime::view::Element;
use whisker::ArcRwSignal;
use whisker_audio::Player;
use whisker_icons::{lucide, Icon, IconProps};
use whisker_image::{Image, ImageProps};
use whisker_safe_area::safe_area_insets;

/// `Rc<RefCell<HashMap<u64, Podcast>>>` — same alias the top-level
/// `podcast` crate defines. Duplicated here so this crate stays
/// router-agnostic (no dep on the top-level shell) but can still
/// match the context type the host provides.
pub type PodcastIndex = Rc<RefCell<HashMap<u64, Podcast>>>;

/// Mirror of the shell-side `NowPlayingSignal` alias. TypeId-
/// matched, so `use_context::<NowPlayingSignal>()` here finds the
/// signal the shell provided without taking a dep on the shell.
pub type NowPlayingSignal = ArcRwSignal<Option<NowPlaying>>;

/// Show detail screen.
///
/// `id` is the iTunes `collectionId` of the podcast to display.
/// The back chevron pulls [`Navigator::go_back`] from context — the
/// shell wires it to `router::back()`.
#[component]
pub fn detail_screen(id: u64) -> Element {
    let podcast: Option<Podcast> =
        use_context::<PodcastIndex>().and_then(|index| index.borrow().get(&id).cloned());

    match podcast {
        Some(podcast) => detail_body(podcast),
        None => not_found(),
    }
}

/// The populated detail body. Split out so the top-level component
/// stays a thin "lookup + branch" wrapper.
fn detail_body(podcast: Podcast) -> Element {
    let artwork_src = podcast.artwork_url_600.clone();
    let title = podcast.collection_name.clone();
    let artist = podcast.artist_name.clone();
    let genre = podcast.primary_genre_name.clone();
    let track_count = podcast.track_count;
    let is_explicit = podcast.is_explicit();

    // Episodes resource — fires the iTunes lookup on mount, re-fires
    // if the screen is re-rendered with a different id (resource()
    // is owner-scoped, the outer `match podcast` discriminant means
    // a navigate-to-different-show remounts this body).
    let id_for_fetch = podcast.id;
    let episodes = resource(move || async move {
        run_blocking(move || fetch_episodes(id_for_fetch, 50))
            .await
            .map_err(|e| e.to_string())
    });

    // Snapshot of podcast metadata for the episode-tap closures —
    // they need to populate `NowPlaying` without re-reading the
    // signal map.
    let show_title = podcast.collection_name.clone();
    let show_artwork = podcast.artwork_url_600.clone();

    // Meta line: "Genre · N episodes". Built once here so the
    // `render!` body doesn't fan out into three nested `text`s.
    let meta_line = build_meta_line(genre, track_count, is_explicit);

    render! {
        view(style: css!(
            flex_grow: 1.0,
            flex_shrink: 1.0,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            background_color: theme::BG,
        )) {
            top_bar(title: "Show".to_string())
            scroll_view(
                style: css!(
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    width: percent(100),
                ),
                scroll_orientation: "vertical",
                scroll_bar_enable: false,
                bounces: true,
            ) {
                // Hero block — artwork + title metadata. Padded
                // horizontally by the page gutter so it visually
                // aligns with the section content below.
                view(style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    padding_left: theme::GUTTER,
                    padding_right: theme::GUTTER,
                    padding_top: px(16),
                    padding_bottom: px(24),
                )) {
                    Image(
                        style: css!(
                            width: px(220),
                            height: px(220),
                            border_radius: theme::ARTWORK_RADIUS,
                            background_color: theme::SURFACE,
                        ).raw("aspect-ratio", "1 / 1").to_css_string(),
                        src: artwork_src,
                        mode: "aspectFill",
                    )
                    text(
                        style: css!(
                            font_size: px(22),
                            color: theme::TEXT_PRIMARY,
                            font_weight: FontWeight::Bold,
                            margin_top: px(16),
                            text_align: TextAlign::Center,
                            text_overflow: TextOverflow::Ellipsis,
                        ).raw("text-maxline", "3"),
                        value: title,
                    )
                    text(
                        style: css!(
                            font_size: px(15),
                            color: theme::ACCENT,
                            margin_top: px(6),
                            text_align: TextAlign::Center,
                        ),
                        value: artist,
                    )
                    text(
                        style: css!(
                            font_size: px(13),
                            color: theme::TEXT_SECONDARY,
                            margin_top: px(8),
                            text_align: TextAlign::Center,
                        ),
                        value: meta_line,
                    )
                    follow_pill()
                }
                // Episodes section header + list. `Show` toggles
                // between the loading / error placeholder and the
                // populated list once the resource resolves.
                view(style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    padding_left: theme::GUTTER,
                    padding_right: theme::GUTTER,
                    padding_top: px(8),
                    padding_bottom: px(96),
                )) {
                    text(
                        style: css!(
                            font_size: theme::T_SECTION,
                            color: theme::TEXT_PRIMARY,
                            font_weight: FontWeight::Bold,
                            margin_bottom: px(8),
                        ),
                        value: "Episodes".to_string(),
                    )
                    Show(
                        when: move || episodes.get().is_some(),
                        fallback: move || render! {
                            episode_status(message: if episodes.error().is_some() {
                                "Couldn't load episodes.".to_string()
                            } else {
                                "Loading…".to_string()
                            })
                        },
                    ) {
                        episode_list(
                            episodes: episodes.get().unwrap_or_default(),
                            show_title: show_title.clone(),
                            show_artwork: show_artwork.clone(),
                        )
                    }
                }
            }
        }
    }
}

/// Compose the "Genre · N episodes · E" line. Reused for both
/// the populated and not-found bodies (the latter just gets a
/// generic version).
fn build_meta_line(genre: Option<String>, track_count: u32, is_explicit: bool) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(3);
    if let Some(g) = genre {
        parts.push(g);
    }
    if track_count > 0 {
        // Singular vs plural — small touch, surfaces a single
        // "1 episode" entry the same way iTunes shows it.
        if track_count == 1 {
            parts.push("1 episode".to_string());
        } else {
            parts.push(format!("{} episodes", track_count));
        }
    }
    if is_explicit {
        parts.push("Explicit".to_string());
    }
    parts.join(" · ")
}

/// Top bar with a leading chevron + centered title. Pads the
/// status-bar inset via the shared `safe-area` context, same as
/// the browse screen's `top_nav` — keeps the bars visually flush
/// across the two screens. The back chevron pulls
/// [`Navigator::go_back`] from context (provided by the host).
#[component]
fn top_bar(title: String) -> Element {
    let insets = safe_area_insets();
    let on_back = use_context::<Navigator>()
        .expect("top_bar requires Navigator in context")
        .go_back;
    let wrapper_style = computed(move || {
        css!(
            width: percent(100),
            padding_top: px(insets.get().top as f32),
            flex_shrink: 0.0,
            background_color: theme::BG,
        )
        .to_css_string()
    });

    render! {
        view(style: wrapper_style) {
            view(style: css!(
                width: percent(100),
                min_height: theme::NAV_HEIGHT,
                flex_shrink: 0.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding_left: theme::GUTTER,
                padding_right: theme::GUTTER,
            )) {
                view(
                    style: css!(
                        flex_grow: 1.0,
                        flex_shrink: 1.0,
                        flex_basis: percent(0),
                        display: Display::Flex,
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::FlexStart,
                    ),
                    on_tap: move |_| (on_back)(),
                ) {
                    Icon(
                        svg: lucide::ChevronLeft,
                        color: "#a78bfa",
                        size: "26",
                    )
                }
                view(style: css!(
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    flex_basis: percent(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                )) {
                    text(
                        style: css!(
                            font_size: theme::T_NAV_TITLE,
                            color: theme::TEXT_PRIMARY,
                            font_weight: FontWeight::Numeric(600),
                        ),
                        value: title.clone(),
                    )
                }
                // Right-side spacer keeps the title genuinely
                // centered — without it the title slides leftward
                // by the chevron's width.
                view(style: css!(
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    flex_basis: percent(0),
                ))
            }
        }
    }
}

/// Visual-only "+ Follow" pill. No state, no callbacks — the
/// playback / library PR adds the real subscribe flow.
#[component]
fn follow_pill() -> Element {
    render! {
        view(style: css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            margin_top: px(20),
            padding_left: px(20),
            padding_right: px(20),
            padding_top: px(10),
            padding_bottom: px(10),
            border_radius: px(20),
            background_color: theme::SURFACE,
        )) {
            Icon(svg: lucide::Plus, color: "#ffffff", size: "16")
            text(
                style: css!(
                    font_size: px(14),
                    color: theme::TEXT_PRIMARY,
                    font_weight: FontWeight::Numeric(600),
                    margin_left: px(6),
                ),
                value: "Follow".to_string(),
            )
        }
    }
}

/// Centred "Loading…" / error strip. Same surface card the prior
/// placeholder used so layout doesn't shift between resource states.
#[component]
fn episode_status(message: String) -> Element {
    render! {
        view(style: css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding_top: px(40),
            padding_bottom: px(40),
            border_radius: px(12),
            background_color: theme::SURFACE,
        )) {
            text(
                style: css!(
                    font_size: px(14),
                    color: theme::TEXT_SECONDARY,
                    text_align: TextAlign::Center,
                ),
                value: message.clone(),
            )
        }
    }
}

/// Vertical stack of episode rows. The `show_*` strings get
/// captured into each tap closure so the now-playing signal can
/// surface the show title + artwork without a back-channel lookup.
#[component]
fn episode_list(episodes: Vec<Episode>, show_title: String, show_artwork: String) -> Element {
    render! {
        view(style: css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        )) {
            ForEach(
                each: {
                    let items = episodes.clone();
                    move || items.clone()
                },
                key: |ep: &Episode| ep.id,
                children: {
                    let show_title = show_title.clone();
                    let show_artwork = show_artwork.clone();
                    move |ep: Episode| render! {
                        episode_row(
                            episode: ep,
                            show_title: show_title.clone(),
                            show_artwork: show_artwork.clone(),
                        )
                    }
                },
            )
        }
    }
}

/// One episode row. Title on the leading edge, release date + a
/// "•" separator + duration on the trailing edge.
///
/// Tapping anywhere on the row drives playback. The handler pulls
/// the shared `Player` + `NowPlayingSignal` from context so this
/// component stays usable without the shell wiring (a standalone
/// harness gets a `None` and the tap is a no-op).
#[component]
fn episode_row(episode: Episode, show_title: String, show_artwork: String) -> Element {
    let title = episode.track_name.clone();
    let meta = build_episode_meta(episode.release_date.as_deref(), episode.track_time_millis);
    let audio_url = episode.episode_url.clone().unwrap_or_default();
    let title_for_now_playing = title.clone();

    let player = use_context::<Player>();
    let now_playing = use_context::<NowPlayingSignal>();
    // Component bodies are re-invoked under a `FnMut` wrapper (see
    // `whisker::call`), so anything captured into the tap closure
    // must be `Clone`d out of the component params first — moving
    // the original `String`s straight in would consume the outer
    // FnMut's captures on the first body run.
    let show_title_for_tap = show_title.clone();
    let show_artwork_for_tap = show_artwork.clone();
    let on_tap = move |_: _| {
        if audio_url.is_empty() {
            return;
        }
        if let Some(player) = player.as_ref() {
            player.set_source(audio_url.clone());
            player.play();
        }
        if let Some(np) = now_playing.as_ref() {
            np.set(Some(NowPlaying {
                episode_title: title_for_now_playing.clone(),
                show_title: show_title_for_tap.clone(),
                artwork_url: show_artwork_for_tap.clone(),
                audio_url: audio_url.clone(),
            }));
        }
    };

    render! {
        view(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                padding_top: px(12),
                padding_bottom: px(12),
                border_radius: px(8),
            ),
            on_tap: on_tap,
        ) {
            text(
                style: css!(
                    font_size: px(15),
                    color: theme::TEXT_PRIMARY,
                    font_weight: FontWeight::Numeric(600),
                    text_overflow: TextOverflow::Ellipsis,
                ).raw("text-maxline", "2"),
                value: title.clone(),
            )
            text(
                style: css!(
                    font_size: px(12),
                    color: theme::TEXT_SECONDARY,
                    margin_top: px(4),
                ),
                value: meta,
            )
        }
    }
}

/// "Sep 12 · 32 min"-style meta line. Either side is optional —
/// older RSS feeds sometimes omit the duration; iTunes itself
/// rarely omits the release date. Empty pieces are dropped so a
/// partial row doesn't get an orphan separator.
fn build_episode_meta(release_date: Option<&str>, track_ms: Option<u64>) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(2);
    if let Some(date) = release_date.and_then(short_date) {
        parts.push(date);
    }
    if let Some(ms) = track_ms.filter(|ms| *ms > 0) {
        parts.push(short_duration(ms));
    }
    parts.join(" · ")
}

/// ISO-8601 → "Sep 12" (year omitted unless it's not the current
/// one). Defensive: anything not matching `YYYY-MM-DDT...` falls
/// through to a verbatim copy so a wire-shape surprise still
/// renders something readable instead of `None`.
fn short_date(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    if bytes.len() < 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return Some(raw.to_string());
    }
    let month = &raw[5..7];
    let day = &raw[8..10];
    let month_name = match month {
        "01" => "Jan",
        "02" => "Feb",
        "03" => "Mar",
        "04" => "Apr",
        "05" => "May",
        "06" => "Jun",
        "07" => "Jul",
        "08" => "Aug",
        "09" => "Sep",
        "10" => "Oct",
        "11" => "Nov",
        "12" => "Dec",
        _ => return Some(raw.to_string()),
    };
    let day_n: u32 = day.parse().ok()?;
    Some(format!("{month_name} {day_n}"))
}

/// "32 min" or "1 h 12 min". Anything under a minute reads as
/// "<1 min" rather than "0 min" so very short trailers don't
/// appear empty.
fn short_duration(ms: u64) -> String {
    let total_min = ms / 60_000;
    if total_min == 0 {
        return "<1 min".to_string();
    }
    let hours = total_min / 60;
    let mins = total_min % 60;
    if hours == 0 {
        format!("{mins} min")
    } else {
        format!("{hours} h {mins} min")
    }
}

/// Rendered when the registry doesn't have `id` (e.g. cold deep-
/// link before browse populated the cache, or the id is bogus).
fn not_found() -> Element {
    render! {
        view(style: css!(
            flex_grow: 1.0,
            flex_shrink: 1.0,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            background_color: theme::BG,
        )) {
            top_bar(title: "Show".to_string())
            view(style: css!(
                flex_grow: 1.0,
                flex_shrink: 1.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding_left: theme::GUTTER,
                padding_right: theme::GUTTER,
            )) {
                text(
                    style: css!(
                        font_size: px(16),
                        color: theme::TEXT_SECONDARY,
                        text_align: TextAlign::Center,
                    ),
                    value: "Couldn't find this podcast in the local cache. \
                            Open it from the browse screen first."
                        .to_string(),
                )
            }
        }
    }
}
