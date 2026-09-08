//! Shared visual language. Screens compose these styles rather than own palettes.
mod tokens;
pub use tokens::{color, radius, size, space};
use whisker::css::FontWeight;
use whisker::prelude::*;

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
    column()
        .flex_grow(1.0)
        .flex_shrink(1.0)
        .min_height(px(0))
        .min_width(px(0))
}
pub fn screen() -> Css {
    fill().background_color(Color::hex(color::CANVAS))
}
pub fn text(size: f32) -> Css {
    Css::new()
        .font_size(px(size))
        .color(Color::hex(color::INK))
        .flex_shrink(1.0)
        .line_height(1.45_f32)
}
pub fn muted() -> Css {
    text(size::LABEL).color(Color::hex(color::MUTED))
}
pub fn title() -> Css {
    text(size::TITLE)
        .font_weight(FontWeight::Bold)
        .line_height(1.2_f32)
}
pub fn display() -> Css {
    text(size::DISPLAY)
        .font_weight(FontWeight::Numeric(500))
        .line_height(1.12_f32)
}
pub fn field() -> Css {
    text(size::BODY)
        .background_color(Color::hex(color::PAPER))
        .border_radius(px(radius::CONTROL))
        .border(
            whisker::css::Border::new()
                .width(px(1))
                .color(Color::hex(color::BORDER))
                .style(whisker::css::BorderStyle::Solid),
        )
        .padding(px(space::MD))
        .height(px(size::TOUCH + space::XS))
        .flex_shrink(0.0)
}
pub fn card() -> Css {
    column()
        .background_color(Color::hex(color::PAPER))
        .border_radius(px(radius::CARD))
        .border(
            whisker::css::Border::new()
                .width(px(1))
                .color(Color::hex(color::BORDER))
                .style(whisker::css::BorderStyle::Solid),
        )
        .padding(px(space::XL))
        .gap(px(space::LG))
}
