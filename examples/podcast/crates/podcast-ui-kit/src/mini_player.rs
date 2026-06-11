//! Bottom mini-player bar.
//!
//! Floats above the scrolling content. Lays out as: leading artwork
//! square showing the now-playing podcast art, then a stacked
//! `episode title / show title` block, then a play / pause glyph
//! and a "skip 30 s" RotateCw glyph on the trailing edge.
//!
//! ## Reactivity
//!
//! - Reads [`NowPlayingSignal`] from context — when it's `None`,
//!   the whole bar is hidden (via `Show`), so a cold-start page
//!   doesn't show an empty player.
//! - Reads [`Player`] from context and pulls its [`PlaybackStatus`]
//!   to drive the play / pause icon swap and to compute the
//!   skip-30s target offset.
//! - The play / pause toggle calls `Player::play` /
//!   `Player::pause` based on the current `is_playing` flag, so a
//!   single tap covers both directions.
//!
//! ## Context contract
//!
//! Both contexts MUST be provided by an ancestor (the shell does
//! this in `#[whisker::main]`). If either is missing the component
//! panics on mount — a deliberate sharp edge because a silently-
//! noop mini-player would mask the wiring bug.

use podcast_domain::NowPlaying;
use podcast_theme as theme;
use whisker::css::{
    AlignItems, Color, Display, FlexDirection, FontWeight, JustifyContent, PositionKind,
    TextOverflow,
};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker::ArcRwSignal;
use whisker_audio::Player;
use whisker_icons::{lucide, Icon};
use whisker_image::{Image, ImageMode};

/// Mirror of the shell-side alias. Same TypeId across crates
/// because the resolver matches on `Rc`/`ArcRwSignal` instantiation,
/// not module path.
pub type NowPlayingSignal = ArcRwSignal<Option<NowPlaying>>;

#[component]
pub fn mini_player() -> Element {
    // Both contexts are required. Sharp-edge `expect` rather than
    // a graceful fall-through: a silent no-op would just shift the
    // bug down to "play button doesn't work" which is harder to
    // trace than a startup panic.
    let now_playing = use_context::<NowPlayingSignal>()
        .expect("mini_player requires NowPlayingSignal in context");
    let player = use_context::<Player>().expect("mini_player requires Player in context");
    let status = player.status();

    // Visibility — collapse to empty fragment when nothing's queued.
    let np_for_when = now_playing.clone();
    let visible = move || np_for_when.get().is_some();

    // Reactive bindings for the visible body. Each `computed` reads
    // the now-playing signal so a `set(Some(_))` in the detail
    // screen drives the artwork / title swap without a remount.
    let np_for_artwork = now_playing.clone();
    let artwork_src = computed(move || {
        np_for_artwork
            .get()
            .map(|np| np.artwork_url)
            .unwrap_or_default()
    });
    let np_for_title = now_playing.clone();
    let title_text = computed(move || {
        np_for_title
            .get()
            .map(|np| np.episode_title)
            .unwrap_or_default()
    });
    let np_for_show = now_playing.clone();
    let show_text = computed(move || {
        np_for_show
            .get()
            .map(|np| np.show_title)
            .unwrap_or_default()
    });

    // Play / pause glyph follows the live `is_playing` flag the
    // audio module pushes through `statusChanged`. The `Icon`'s
    // `svg:` prop is `Signal<String>` — a `computed` of
    // `&'static str → String` plugs straight in.
    let play_icon = computed(move || {
        if status.get().is_playing {
            lucide::Pause.to_string()
        } else {
            lucide::Play.to_string()
        }
    });

    // `Show`'s body is invoked under an outer `Fn` closure: a
    // visibility flip would re-run it, so the `on_tap` values
    // can't be `move`-captured once and reused. The render! body
    // below builds each handler inline, cloning the (Rc-backed)
    // `Player` per build — cheap, and lets every re-mount get a
    // fresh closure.
    let player_for_toggle = player.clone();
    let player_for_skip = player.clone();

    render! {
        Show(when: visible, fallback: || render! { fragment() }) {
            // Floats above content via absolute positioning. The
            // parent page must use `position: relative` (default).
            view(style: css!(
                position: PositionKind::Absolute,
                left: theme::GUTTER,
                right: theme::GUTTER,
                bottom: theme::MINI_PLAYER_BOTTOM,
                height: theme::MINI_PLAYER_HEIGHT,
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding_left: px(8),
                padding_right: px(8),
                border_radius: px(12),
                background_color: theme::MINI_PLAYER_BG,
            )) {
                // Leading artwork. `aspect-ratio` keeps Lynx from
                // collapsing the box before the image loads.
                Image(
                    style: css!(
                        width: px(40),
                        height: px(40),
                        border_radius: px(6),
                        background_color: Color::rgba(255, 255, 255, 0.15),
                    ).raw("aspect-ratio", "1 / 1").to_css_string(),
                    src: artwork_src,
                    mode: ImageMode::AspectFill,
                )
                // Stacked text — episode title on top, show title
                // below. Flex-grow lets it absorb the slack so the
                // trailing controls stay right-aligned.
                view(style: css!(
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    margin_left: px(10),
                    margin_right: px(10),
                )) {
                    text(
                        style: css!(
                            font_size: px(13),
                            color: theme::TEXT_PRIMARY,
                            font_weight: FontWeight::Numeric(600),
                            text_overflow: TextOverflow::Ellipsis,
                        ).raw("text-maxline", "1"),
                        value: title_text,
                    )
                    text(
                        style: css!(
                            font_size: px(11),
                            color: theme::TEXT_SECONDARY,
                            margin_top: px(2),
                            text_overflow: TextOverflow::Ellipsis,
                        ).raw("text-maxline", "1"),
                        value: show_text,
                    )
                }
                // Play / pause toggle.
                view(
                    style: css!(
                        width: px(36),
                        height: px(36),
                        display: Display::Flex,
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                    ),
                    on_tap: {
                        let p = player_for_toggle.clone();
                        move |_| {
                            if status.get().is_playing {
                                p.pause();
                            } else {
                                p.play();
                            }
                        }
                    },
                ) {
                    Icon(svg: play_icon, color: "#ffffff", size: "24")
                }
                // Skip-30s.
                view(
                    style: css!(
                        width: px(36),
                        height: px(36),
                        display: Display::Flex,
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        margin_left: px(4),
                    ),
                    on_tap: {
                        let p = player_for_skip.clone();
                        move |_| {
                            let pos = status.get().position;
                            p.seek_to(pos + 30.0);
                        }
                    },
                ) {
                    Icon(svg: lucide::RotateCw, color: "#ffffff", size: "22")
                }
            }
        }
    }
}
