//! Shared visual language. Screens compose these styles rather than own palettes.
mod appearance;
mod tokens;
pub use appearance::{Appearance, Palette, provide_appearance, style, use_appearance};
pub use tokens::{radius, size, space};
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
impl Palette {
    pub fn screen(self) -> Css {
        fill().background_color(Color::hex(self.canvas))
    }
    pub fn text(self, size: f32) -> Css {
        Css::new()
            .font_size(px(size))
            .color(Color::hex(self.ink))
            .flex_shrink(1.0)
            .line_height(1.45_f32)
    }
    pub fn muted(self) -> Css {
        self.text(size::LABEL).color(Color::hex(self.muted))
    }
    pub fn title(self) -> Css {
        self.text(size::TITLE)
            .font_weight(FontWeight::Bold)
            .line_height(1.2_f32)
    }
    pub fn display(self) -> Css {
        self.text(size::DISPLAY)
            .font_weight(FontWeight::Numeric(500))
            .line_height(1.12_f32)
    }
    pub fn field(self) -> Css {
        self.text(size::BODY)
            .background_color(Color::hex(self.paper))
            .border_radius(px(radius::CONTROL))
            .border(
                whisker::css::Border::new()
                    .width(px(1))
                    .color(Color::hex(self.border))
                    .style(whisker::css::BorderStyle::Solid),
            )
            .padding(px(space::MD))
            .height(px(size::TOUCH + space::XS))
            .flex_shrink(0.0)
    }
    pub fn card(self) -> Css {
        column()
            .background_color(Color::hex(self.paper))
            .border_radius(px(radius::CARD))
            .border(
                whisker::css::Border::new()
                    .width(px(1))
                    .color(Color::hex(self.border))
                    .style(whisker::css::BorderStyle::Solid),
            )
            .padding(px(space::XL))
            .gap(px(space::LG))
    }
}
