//! Reusable, stateless UI for the Bluesky example. All values come from props;
//! these components read no context. Milestone 1 ships just the timeline's
//! `PostCard` (avatar + author + body + engagement counts).

use bsky_domain::FeedPost;
use bsky_theme as theme;
use whisker::css::{AlignItems, Display, FlexDirection, FontWeight};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_icons::{Icon, lucide};
use whisker_image::{Image, ImageMode};

/// One timeline row. Avatar on the left, author line + body + counts stacked on
/// the right, with a hairline separator below.
#[component]
pub fn post_card(post: FeedPost) -> Element {
    let avatar = post.author.avatar.clone().unwrap_or_default();
    let name = post.author.name();
    let handle = format!("@{}", post.author.handle);

    render! {
        view(style: css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            padding: theme::GUTTER,
            border_bottom_width: px(1),
            border_bottom_color: theme::BORDER,
        )) {
            avatar_view(src: avatar)
            view(style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                margin_left: theme::ROW_GAP,
            )) {
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
                    value: post.text.clone(),
                )
                view(style: css!(
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    margin_top: px(8),
                )) {
                    metric(icon: lucide::MessageCircle, count: post.reply_count)
                    metric(icon: lucide::Repeat2, count: post.repost_count)
                    metric(icon: lucide::Heart, count: post.like_count)
                }
            }
        }
    }
}

/// One engagement stat — a Lucide glyph followed by its count, both in
/// the secondary text colour. Replaces the old emoji meta line so the
/// icons render identically across iOS / Android (no font-emoji drift).
#[component]
fn metric(icon: Signal<String>, count: u64) -> Element {
    render! {
        view(style: css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            margin_right: px(20),
        )) {
            Icon(
                svg: icon,
                // `whisker-icons` forwards `color` straight to Lynx as the
                // `currentColor` substitution; there's no typed `Color` prop
                // yet, so the secondary text hex (theme::TEXT_SECONDARY)
                // goes in as a string literal.
                color: "#8B98A5",
                size: "15",
            )
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
