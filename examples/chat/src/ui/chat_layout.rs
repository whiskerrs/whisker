use super::theme::{self, space};
use whisker::css::{AlignSelf, PositionKind};
use whisker::prelude::*;

pub const HEADER_HEIGHT: f32 = 64.0;
pub const COMPOSER_HEIGHT: f32 = 192.0;
pub const CONTENT_TOP: f32 = HEADER_HEIGHT + space::XL;
pub const CONTENT_BOTTOM: f32 = COMPOSER_HEIGHT + space::LG;

pub fn header() -> Css {
    theme::row()
        .position(PositionKind::Absolute)
        .top(px(space::MD))
        .left(px(space::MD))
        .right(px(space::MD))
        .height(px(HEADER_HEIGHT))
        .padding(px(space::SM))
        .gap(px(space::SM))
        .z_index(2)
}

pub fn composer() -> Css {
    theme::column()
        .position(PositionKind::Absolute)
        .left(px(0))
        .right(px(0))
        .bottom(px(0))
        .height(px(COMPOSER_HEIGHT))
        .align_items(AlignItems::Center)
        .z_index(2)
}

pub fn latest() -> Css {
    theme::row()
        .position(PositionKind::Absolute)
        .bottom(px(CONTENT_BOTTOM))
        .align_self(AlignSelf::Center)
        .z_index(3)
}

pub fn spacer(height: f32) -> Element {
    View::builder()
        .style(theme::column().height(px(height)).flex_shrink(0.0))
        .build()
}
