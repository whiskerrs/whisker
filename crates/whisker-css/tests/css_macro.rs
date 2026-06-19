//! Tests for the [`css!`] macro.

use whisker_css::ext::*;
use whisker_css::{
    AnimationDirection, Border, BorderStyle, Color, Css, FlexDirection, JustifyContent, NamedColor,
    PositionKind, ToCss, TransformFn, css,
};

#[test]
fn empty_macro_produces_empty_css() {
    let s = css!();
    assert_eq!(s.to_css_string(), "");
}

#[test]
fn empty_macro_is_equivalent_to_new() {
    assert_eq!(
        css!().to_css_string(),
        whisker_css::Css::new().to_css_string()
    );
}

#[test]
fn single_property() {
    let s = css!(background_color: Color::hex(0xFF0000));
    assert_eq!(s.to_css_string(), "background-color: rgb(255, 0, 0);");
}

#[test]
fn multiple_properties_emit_in_order() {
    let s = css!(
        background_color: Color::hex(0x1A1330),
        color: Color::Named(NamedColor::White),
        padding: px(12),
        border_radius: px(10)
    );
    let out = s.to_css_string();
    // Padding expands to four longhands; border-radius too. We assert
    // the salient pieces rather than the full ordering.
    assert!(out.contains("background-color: rgb(26, 19, 48)"));
    assert!(out.contains("color: white"));
    assert!(out.contains("padding-top: 12px"));
    assert!(out.contains("padding-right: 12px"));
    assert!(out.contains("padding-bottom: 12px"));
    assert!(out.contains("padding-left: 12px"));
    assert!(out.contains("border-top-left-radius: 10px"));
}

#[test]
fn trailing_comma_is_accepted() {
    let with = css!(color: Color::Named(NamedColor::Red),);
    let without = css!(color: Color::Named(NamedColor::Red));
    assert_eq!(with.to_css_string(), without.to_css_string());
}

#[test]
fn shorthand_builder_in_value_position() {
    let s = css!(
        border: Border::new().width(px(1)).style(BorderStyle::Solid).color(Color::hex(0xCCCCCC)),
    );
    let out = s.to_css_string();
    assert!(out.contains("border-top-width: 1px"));
    assert!(out.contains("border-top-style: solid"));
    assert!(out.contains("border-top-color: rgb(204, 204, 204)"));
}

#[test]
fn tuple_value_for_padding_shorthand() {
    let s = css!(padding: (px(8), px(16)));
    let out = s.to_css_string();
    assert!(out.contains("padding-top: 8px"));
    assert!(out.contains("padding-right: 16px"));
    assert!(out.contains("padding-bottom: 8px"));
    assert!(out.contains("padding-left: 16px"));
}

#[test]
fn four_value_padding_tuple() {
    let s = css!(padding: (px(1), px(2), px(3), px(4)));
    assert_eq!(
        s.to_css_string(),
        "padding-top: 1px; padding-right: 2px; padding-bottom: 3px; padding-left: 4px;"
    );
}

#[test]
fn keyword_enum_value() {
    let s = css!(
        position: PositionKind::Absolute,
        flex_direction: FlexDirection::Column,
        justify_content: JustifyContent::SpaceBetween,
    );
    let out = s.to_css_string();
    assert!(out.contains("position: absolute"));
    assert!(out.contains("flex-direction: column"));
    assert!(out.contains("justify-content: space-between"));
}

#[test]
fn method_call_chain_value() {
    let s = css!(
        background_color: Color::hex(0xABCDEF),
    );
    assert_eq!(s.to_css_string(), "background-color: rgb(171, 205, 239);");
}

#[test]
fn array_value_for_transform() {
    let s = css!(
        transform: [
            TransformFn::TranslateX(px(10).into()),
            TransformFn::Rotate(45.deg()),
        ],
    );
    assert_eq!(
        s.to_css_string(),
        "transform: translateX(10px) rotate(45deg);"
    );
}

#[test]
fn block_expr_value() {
    let dark = true;
    let s = css!(
        color: if dark { Color::Named(NamedColor::White) } else { Color::Named(NamedColor::Black) },
    );
    assert_eq!(s.to_css_string(), "color: white;");
}

#[test]
fn match_expr_value() {
    #[derive(Copy, Clone)]
    #[allow(dead_code)] // `Light` is reached only through the match arm.
    enum Theme {
        Dark,
        Light,
    }
    let theme = Theme::Dark;
    let s = css!(
        background_color: match theme {
            Theme::Dark => Color::hex(0x111111),
            Theme::Light => Color::hex(0xFFFFFF),
        },
    );
    assert_eq!(s.to_css_string(), "background-color: rgb(17, 17, 17);");
}

#[test]
fn computed_inside_block() {
    fn dark() -> Color {
        Color::Named(NamedColor::Red)
    }
    fn light() -> Color {
        Color::Named(NamedColor::Blue)
    }
    // The block holds a load-bearing local + a conditional — the
    // `{ … }` is genuinely a block expr, not a redundant brace
    // around a single call (which would trip `unused_braces`).
    let theme = "dark";
    let s = css!(color: {
        // Two statements + the trailing expr so the block expression
        // form is actually load-bearing (and `unused_braces` /
        // `let_and_return` stay silent).
        let primary = dark();
        let secondary = light();
        if theme == "dark" { primary } else { secondary }
    });
    assert_eq!(s.to_css_string(), "color: red;");
}

#[test]
fn css_macro_returns_css_type_for_chaining() {
    // The macro result is `Css`, so it can be `.<method>()`-chained
    // afterwards as a regular builder value.
    let s = css!(color: Color::Named(NamedColor::Red)).padding(px(8));
    let out = s.to_css_string();
    assert!(out.contains("color: red"));
    assert!(out.contains("padding-top: 8px"));
}

#[test]
fn css_macro_value_is_clonable() {
    let s = css!(color: Color::Named(NamedColor::Red));
    let copy = s.clone();
    assert_eq!(s.to_css_string(), copy.to_css_string());
}

#[test]
fn flex_keyword_method_via_no_arg_path() {
    // Methods that take no value (`display_flex()`, etc.) aren't
    // expressible as kwargs here because `css!` requires `name:
    // value`. The follow-up chain handles them:
    let s = css!(color: Color::Named(NamedColor::Red))
        .display_flex()
        .flex_direction(FlexDirection::Row);
    let out = s.to_css_string();
    assert!(out.contains("color: red"));
    assert!(out.contains("display: flex"));
    assert!(out.contains("flex-direction: row"));
}

#[test]
fn shared_with_animation_direction_keyword() {
    let s = css!(
        animation_duration: 300.ms(),
        animation_direction: AnimationDirection::Alternate,
    );
    let out = s.to_css_string();
    assert!(out.contains("animation-duration: 300ms"));
    assert!(out.contains("animation-direction: alternate"));
}

#[test]
fn nested_shorthand_within_kwarg() {
    let s = css!(
        padding: (px(4), px(8), px(12), px(16)),
        border: Border::new().width(px(2)).solid().color(Color::hex(0x000000)),
        background_color: Color::Named(NamedColor::Black),
    );
    let out = s.to_css_string();
    assert!(out.contains("padding-top: 4px"));
    assert!(out.contains("border-top-width: 2px"));
    assert!(out.contains("background-color: black"));
}

#[test]
fn many_properties_in_one_call() {
    // A larger call exercising the `+` repetition path.
    let s = css!(
        display: whisker_css::Display::Flex,
        flex_direction: FlexDirection::Column,
        padding: px(16),
        margin: px(8),
        background_color: Color::hex(0x1A1A2E),
        color: Color::Named(NamedColor::White),
        border_radius: px(10),
        opacity: 0.95,
        font_size: 14.px(),
    );
    let out = s.to_css_string();
    assert!(out.contains("display: flex"));
    assert!(out.contains("flex-direction: column"));
    assert!(out.contains("padding-top: 16px"));
    assert!(out.contains("margin-top: 8px"));
    assert!(out.contains("background-color: rgb(26, 26, 46)"));
    assert!(out.contains("color: white"));
    assert!(out.contains("border-top-left-radius: 10px"));
    assert!(out.contains("opacity: 0.95"));
    assert!(out.contains("font-size: 14px"));
}
