//! Reusable, presentational UI for the Bluesky example.
//!
//! Components take their display state and callbacks via props — they hold
//! no networking and read no context. The stateful wrapper that owns the
//! like / repost signals and calls `bsky-auth` lives in the app
//! (`post_row`); `PostCard` just renders what it's given and fires
//! `on_open` / `on_like` / `on_repost`.

use std::rc::Rc;

use bsky_domain::FeedPost;
use bsky_theme as theme;
use whisker::css::{AlignItems, Display, FlexDirection, FontWeight};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_icons::{Icon, lucide};
use whisker_image::{Image, ImageMode};

/// Active-state colours for the engagement actions.
const REPOST_ON: &str = "#43D17A";
const LIKE_ON: &str = "#EC4899";
const META_OFF: &str = "#8B98A5";

/// One timeline / thread row. Avatar on the left; author line + body +
/// engagement actions stacked on the right, with a hairline separator.
///
/// `liked` / `reposted` / `like_count` / `repost_count` are reactive so
/// the owning `post_row` can update them optimistically. Tapping the
/// author/body fires `on_open`; the repost / like glyphs fire their
/// callbacks (reply is intentionally display-only here).
#[component]
pub fn post_card(
    post: FeedPost,
    liked: Signal<bool>,
    reposted: Signal<bool>,
    like_count: Signal<i64>,
    repost_count: Signal<i64>,
    on_open: Rc<dyn Fn()>,
    on_like: Rc<dyn Fn()>,
    on_repost: Rc<dyn Fn()>,
    on_author: Rc<dyn Fn()>,
) -> Element {
    let avatar = post.author.avatar.clone().unwrap_or_default();
    let name = post.author.name();
    let handle = format!("@{}", post.author.handle);
    let reply_count = post.reply_count;
    let body = post.text.clone();
    // Clone the `Rc` callbacks per (re-invokable) body run so they can be
    // moved into the child components / tap closure.
    let open = on_open.clone();
    let like_cb = on_like.clone();
    let repost_cb = on_repost.clone();
    let author_cb = on_author.clone();

    render! {
        view(style: css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            padding: theme::GUTTER,
            border_bottom_width: px(1),
            border_bottom_color: theme::BORDER,
        )) {
            view(on_tap: move |_| (author_cb)()) {
                avatar_view(src: avatar)
            }
            view(style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                margin_left: theme::ROW_GAP,
            )) {
                // Tappable header + body → open the post detail.
                view(
                    style: css!(display: Display::Flex, flex_direction: FlexDirection::Column),
                    on_tap: move |_| (open)(),
                ) {
                    view(style: css!(
                        display: Display::Flex,
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                    )) {
                        text(
                            style: css!(
                                font_size: theme::T_NAME,
                                font_weight: FontWeight::Bold,
                                color: theme::TEXT_PRIMARY,
                                margin_right: px(6),
                            ),
                            value: name,
                        )
                        text(
                            style: css!(font_size: theme::T_HANDLE, color: theme::TEXT_SECONDARY),
                            value: handle,
                        )
                    }
                    text(
                        style: css!(
                            font_size: theme::T_BODY,
                            color: theme::TEXT_PRIMARY,
                            margin_top: px(2),
                        ),
                        value: body,
                    )
                }
                view(style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    margin_top: px(8),
                )) {
                    static_metric(icon: lucide::MessageCircle, count: reply_count)
                    action_metric(
                        icon: lucide::Repeat2,
                        active: reposted,
                        active_color: REPOST_ON,
                        count: repost_count,
                        on_tap: repost_cb,
                    )
                    action_metric(
                        icon: lucide::Heart,
                        active: liked,
                        active_color: LIKE_ON,
                        count: like_count,
                        on_tap: like_cb,
                    )
                }
            }
        }
    }
}

/// A non-interactive count (e.g. replies): glyph + number, muted.
#[component]
fn static_metric(icon: Signal<String>, count: u64) -> Element {
    render! {
        view(style: css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            margin_right: px(20),
        )) {
            Icon(svg: icon, color: META_OFF, size: "15")
            text(
                style: css!(
                    font_size: theme::T_META,
                    color: theme::TEXT_SECONDARY,
                    margin_left: px(5),
                ),
                value: count.to_string(),
            )
        }
    }
}

/// A tappable count whose glyph + number switch to `active_color` when
/// `active` is set. Used for repost / like.
#[component]
fn action_metric(
    icon: Signal<String>,
    active: Signal<bool>,
    active_color: &'static str,
    count: Signal<i64>,
    on_tap: Rc<dyn Fn()>,
) -> Element {
    // Only the glyph recolours on active (the count stays muted) — keeps
    // the icon's string colour reactive without parsing a CSS colour back
    // into a typed `Color` for the text.
    let color = computed(move || {
        if active.get() {
            active_color.to_string()
        } else {
            META_OFF.to_string()
        }
    });
    let label = computed(move || count.get().to_string());
    let cb = on_tap.clone();
    render! {
        view(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                margin_right: px(20),
            ),
            on_tap: move |_| (cb)(),
        ) {
            Icon(svg: icon, color: color, size: "15")
            text(
                style: css!(
                    font_size: theme::T_META,
                    color: theme::TEXT_SECONDARY,
                    margin_left: px(5),
                ),
                value: label,
            )
        }
    }
}

/// Circular avatar — the CDN image when `src` is non-empty, otherwise a flat
/// accent disc.
#[component]
fn avatar_view(src: String) -> Element {
    if src.is_empty() {
        render! {
            view(style: css!(
                width: theme::AVATAR_SIDE,
                height: theme::AVATAR_SIDE,
                border_radius: px(theme::AVATAR_RADIUS_PX as f32),
                background_color: theme::ACCENT,
            )) {}
        }
    } else {
        render! {
            Image(
                style: css!(
                    width: theme::AVATAR_SIDE,
                    height: theme::AVATAR_SIDE,
                    border_radius: px(theme::AVATAR_RADIUS_PX as f32),
                    background_color: theme::SURFACE,
                ),
                src: src.clone(),
                mode: ImageMode::AspectFill,
            )
        }
    }
}
