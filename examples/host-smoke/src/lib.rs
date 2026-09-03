//! Minimal application used to verify Whisker Host bootstrapping.

use whisker::css::BorderStyle;
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_svg::Svg;

#[component]
fn external_module() -> Element {
    render! {
        Svg(
            content: r#"<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="10" fill="currentColor"/></svg>"#,
            color: "#0EA5E9",
            style: css!(
                width: px(48),
                height: px(48),
                margin_top: px(16),
            ),
        )
    }
}

#[whisker::main]
pub fn app() -> Element {
    render! {
        View(style: css!(
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            background_color: Color::hex(0x20242A),
            padding: px(24),
        )) {
            Text(
                style: css!(
                    color: Color::hex(0xF5F7FA),
                    font_size: px(18),
                ),
                value: "Whisker Host is running",
            )
            Text(
                style: css!(
                    color: Color::hex(0x94A3B8),
                    font_size: px(12),
                    margin_top: px(4),
                ),
                value: "External SVG module linked",
            )
            ExternalModule()
            View(style: css!(
                width: percent(100),
                height: px(88),
                margin_top: px(24),
                border_radius: px(24),
                background_color: Color::hex(0x2563EB),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            )) {
                Text(
                    style: css!(
                        color: Color::hex(0xFFFFFF),
                        font_size: px(16),
                    ),
                    value: "24px radius",
                )
            }
            View(style: css!(
                width: percent(100),
                height: px(88),
                margin_top: px(16),
                border_top_width: px(3),
                border_right_width: px(3),
                border_bottom_width: px(3),
                border_left_width: px(3),
                border_top_style: BorderStyle::Solid,
                border_right_style: BorderStyle::Solid,
                border_bottom_style: BorderStyle::Solid,
                border_left_style: BorderStyle::Solid,
                border_top_color: Color::hex(0xFDE68A),
                border_right_color: Color::hex(0xFDE68A),
                border_bottom_color: Color::hex(0xFDE68A),
                border_left_color: Color::hex(0xFDE68A),
                border_top_left_radius: px(40),
                border_top_right_radius: px(8),
                border_bottom_right_radius: px(40),
                border_bottom_left_radius: px(8),
                background_color: Color::hex(0x15803D),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            )) {
                Text(
                    style: css!(
                        color: Color::hex(0xFFFFFF),
                        font_size: px(16),
                    ),
                    value: "Asymmetric radius + border",
                )
            }
            View(style: css!(
                width: percent(100),
                height: px(88),
                margin_top: px(16),
                border_radius: percent(50),
                background_color: Color::hex(0x7C3AED),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            )) {
                Text(
                    style: css!(
                        color: Color::hex(0xFFFFFF),
                        font_size: px(16),
                    ),
                    value: "50% radius",
                )
            }
        }
    }
}
