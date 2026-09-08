use whisker::prelude::*;

pub const INK: u32 = 0x202629;
pub const MUTED: u32 = 0x73807e;
pub const SURFACE: u32 = 0xf6f7f4;
pub const ACCENT: u32 = 0x176957;

pub fn column() -> Css {
    Css::new()
        .display_flex()
        .flex_direction(FlexDirection::Column)
}
pub fn row() -> Css {
    Css::new()
        .display_flex()
        .flex_direction(FlexDirection::Row)
        .align_items(AlignItems::Center)
}
pub fn fill() -> Css {
    column().flex_grow(1.0).flex_shrink(1.0).min_height(px(0))
}
pub fn text(size: f32) -> Css {
    Css::new()
        .font_size(px(size))
        .color(Color::hex(INK))
        .flex_shrink(1.0)
}
pub fn muted() -> Css {
    text(13.0).color(Color::hex(MUTED))
}
pub fn field() -> Css {
    text(16.0)
        .background_color(Color::hex(0xffffff))
        .border_radius(px(12))
        .padding(px(12))
        .height(px(48))
        .flex_shrink(0.0)
}
