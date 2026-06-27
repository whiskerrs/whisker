//! Reusable, stateless UI for the Bluesky example. All values come from props;
//! these components read no context. Milestone 1 ships just the timeline's
//! `PostCard` (avatar + author + body + engagement counts).

use bsky_domain::FeedPost;
use bsky_theme as theme;
use whisker::css::{AlignItems, Display, FlexDirection, FontWeight};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_image::{Image, ImageMode};

/// One timeline row. Avatar on the left, author line + body + counts stacked on
/// the right, with a hairline separator below.
#[component]
pub fn post_card(post: FeedPost) -> Element {
    let avatar = post.author.avatar.clone().unwrap_or_default();
    let name = post.author.name();
    let handle = format!("@{}", post.author.handle);
    let meta = format!(
        "💬 {}   🔁 {}   ♥ {}",
        post.reply_count, post.repost_count, post.like_count
    );

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
                text(
                    style: css!(
                        font_size: theme::T_META,
                        color: theme::TEXT_SECONDARY,
                        margin_top: px(8),
                    ),
                    value: meta,
                )
            }
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
