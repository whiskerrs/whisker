use super::theme::{self, space};
use whisker::css::{AlignSelf, PositionKind};
use whisker::prelude::*;

pub const HEADER_HEIGHT: f32 = 64.0;
pub const CONTENT_TOP: f32 = HEADER_HEIGHT + space::XL;

pub fn header() -> Css {
    theme::row()
        .position(PositionKind::Absolute)
        .top(px(space::MD))
        .left(px(space::MD))
        .right(px(space::MD))
        .height(px(HEADER_HEIGHT))
        .padding(px(space::SM))
        .z_index(2)
}

pub fn composer() -> Css {
    theme::column()
        .position(PositionKind::Absolute)
        .left(px(0))
        .right(px(0))
        .bottom(px(0))
        .align_items(AlignItems::Center)
        .z_index(2)
}

pub fn content_bottom(input_height: f32) -> f32 {
    input_height + 2.0 * space::SM + 2.0 + 2.0 * space::LG
}

pub fn latest(input_height: f32) -> Css {
    theme::row()
        .position(PositionKind::Absolute)
        .bottom(px(content_bottom(input_height)))
        .align_self(AlignSelf::Center)
        .z_index(3)
}

pub fn spacer(height: f32) -> Element {
    View::builder()
        .style(theme::column().height(px(height)).flex_shrink(0.0))
        .build()
}
