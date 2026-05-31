//! Compound, cross-module integration tests.
//!
//! These exercise behaviors that span multiple builder methods or
//! types and would not be caught by any single per-module unit
//! test:
//!
//! - Multi-family style declarations end-to-end (layout + colors +
//!   borders + transitions in one go).
//! - Shorthand + longhand interaction (`padding` followed by
//!   `padding_top`, etc.).
//! - Last-write-wins resolution across non-adjacent updates.
//! - Conversions from CSS data types into the property surfaces.
//! - Pathological edge cases (empty builds, repeated overrides).

use whisker_style::ext::*;
use whisker_style::{
    Animation, Background, BackgroundLayer, Border, BorderRadius, Color, ColorStop, CssString,
    EasingFunction, Flex, FlexBasis, FlexDirection, Gradient, GridLine, GridTemplate, ImageRef,
    JustifyContent, LengthPercentage, LinearOrientation, NamedColor, PositionKind, Size, Style,
    ToCss, TransformFn, Transition, TransitionPropertyKind, Visibility,
};
use whisker_style::keyword::{AlignItems, Display, Overflow};

#[test]
fn realistic_card_layout() {
    let s = Style::new()
        .display_flex()
        .flex_direction(FlexDirection::Column)
        .padding(px(16))
        .border_radius(px(10))
        .background_color(Color::hex(0x1A1A2E))
        .color(Color::Named(NamedColor::White));
    let css = s.to_string();
    assert!(css.contains("display: flex"));
    assert!(css.contains("flex-direction: column"));
    assert!(css.contains("padding-top: 16px"));
    assert!(css.contains("padding-right: 16px"));
    assert!(css.contains("padding-bottom: 16px"));
    assert!(css.contains("padding-left: 16px"));
    assert!(css.contains("border-top-left-radius: 10px"));
    assert!(css.contains("background-color: rgb(26, 26, 46)"));
    assert!(css.contains("color: white"));
}

#[test]
fn padding_shorthand_then_longhand_wins() {
    let s = Style::new().padding(px(16)).padding_top(px(4));
    let css = s.to_string();
    // padding_top is overridden by the explicit setter; the other
    // three sides remain at 16px.
    assert!(css.contains("padding-top: 4px"));
    assert!(css.contains("padding-right: 16px"));
    assert!(css.contains("padding-bottom: 16px"));
    assert!(css.contains("padding-left: 16px"));
}

#[test]
fn padding_longhand_then_shorthand_resets() {
    let s = Style::new().padding_top(px(4)).padding(px(16));
    // Shorthand follows longhand → all four sides should be 16px.
    assert_eq!(
        s.to_string(),
        "padding-top: 16px; padding-right: 16px; padding-bottom: 16px; padding-left: 16px;"
    );
}

#[test]
fn margin_auto_combined_with_explicit_top() {
    let s = Style::new()
        .margin(px(0))
        .margin_top(px(8))
        .margin_left(whisker_style::shorthand::padding_margin::MarginValue::Auto)
        .margin_right(whisker_style::shorthand::padding_margin::MarginValue::Auto);
    assert_eq!(
        s.to_string(),
        "margin-bottom: 0px; margin-top: 8px; margin-left: auto; margin-right: auto;"
    );
}

#[test]
fn border_full_then_per_side_override() {
    let s = Style::new()
        .border(
            Border::new()
                .width(px(1))
                .solid()
                .color(Color::hex(0xCCCCCC)),
        )
        .border_bottom(
            Border::new()
                .width(px(3))
                .solid()
                .color(Color::hex(0xFF0000)),
        );
    let css = s.to_string();
    // The bottom side should win for all three dimensions.
    assert!(css.contains("border-bottom-width: 3px"));
    assert!(css.contains("border-bottom-style: solid"));
    assert!(css.contains("border-bottom-color: rgb(255, 0, 0)"));
    // Other sides retain the 1px / #CCC.
    assert!(css.contains("border-top-width: 1px"));
    assert!(css.contains("border-right-color: rgb(204, 204, 204)"));
}

#[test]
fn flex_shorthand_then_per_axis_override() {
    let s = Style::new().flex(Flex::Auto).flex_basis(FlexBasis::Content);
    assert_eq!(
        s.to_string(),
        "flex-grow: 1; flex-shrink: 1; flex-basis: content;"
    );
}

#[test]
fn flex_number_then_grow_chain() {
    let s = Style::new().flex(Flex::Number(2.0)).flex_grow(3.0);
    let css = s.to_string();
    assert!(css.contains("flex-grow: 3"));
    assert!(css.contains("flex-shrink: 1"));
    assert!(css.contains("flex-basis: 0%"));
}

#[test]
fn position_absolute_overlay() {
    let s = Style::new()
        .position(PositionKind::Absolute)
        .top(px(0))
        .left(px(0))
        .right(px(0))
        .bottom(px(0))
        .z_index(10);
    let css = s.to_string();
    assert!(css.contains("position: absolute"));
    assert!(css.contains("top: 0px"));
    assert!(css.contains("z-index: 10"));
}

#[test]
fn overflow_then_axis_override_keeps_other_axis() {
    let s = Style::new()
        .overflow(Overflow::Hidden)
        .overflow_x(Overflow::Visible);
    assert_eq!(s.to_string(), "overflow-y: hidden; overflow-x: visible;");
}

#[test]
fn gap_then_row_gap_override_keeps_column() {
    let s = Style::new().gap(px(8)).row_gap(px(16));
    assert_eq!(
        s.to_string(),
        "column-gap: 8px; row-gap: 16px;"
    );
}

#[test]
fn last_write_wins_over_many_repeats() {
    // 5 rewrites of the same property; only the last appears.
    let s = Style::new()
        .color(Color::hex(0x111111))
        .color(Color::hex(0x222222))
        .color(Color::hex(0x333333))
        .color(Color::hex(0x444444))
        .color(Color::hex(0x555555));
    assert_eq!(s.to_string(), "color: rgb(85, 85, 85);");
}

#[test]
fn empty_style_is_empty_string() {
    let s = Style::new();
    assert_eq!(s.to_string(), "");
    assert!(s.is_empty());
}

#[test]
fn raw_escape_hatch_coexists_with_typed() {
    let s = Style::new()
        .padding(px(8))
        .raw("-webkit-tap-highlight-color", "transparent");
    let css = s.to_string();
    assert!(css.contains("padding-top: 8px"));
    assert!(css.contains("-webkit-tap-highlight-color: transparent"));
}

#[test]
fn merge_overlays_other_onto_self() {
    let base = Style::new().padding(px(4)).color(Color::hex(0x000000));
    let overlay = Style::new().color(Color::hex(0xFFFFFF));
    let merged = base.merge(overlay);
    let css = merged.to_string();
    // Color from overlay wins; padding from base remains.
    assert!(css.contains("color: rgb(255, 255, 255)"));
    assert!(css.contains("padding-top: 4px"));
}

#[test]
fn complete_animation_chain() {
    let s = Style::new()
        .animation(
            Animation::new("pulse")
                .duration(2.s())
                .timing(EasingFunction::EaseInOut)
                .iteration_count(whisker_style::keyword::AnimationIterationCount::Infinite)
                .direction(whisker_style::keyword::AnimationDirection::Alternate),
        )
        .opacity(0.8);
    let css = s.to_string();
    assert!(css.contains("animation: pulse 2s ease-in-out infinite alternate"));
    assert!(css.contains("opacity: 0.8"));
}

#[test]
fn transform_layered() {
    let s = Style::new().transform([
        TransformFn::TranslateX(px(10).into()),
        TransformFn::Scale(1.5, 1.5),
        TransformFn::Rotate(45.deg()),
    ]);
    assert_eq!(
        s.to_string(),
        "transform: translateX(10px) scale(1.5, 1.5) rotate(45deg);"
    );
}

#[test]
fn transitions_multi() {
    let s = Style::new().transitions([
        Transition::new(TransitionPropertyKind::name("opacity"))
            .duration(300.ms())
            .timing(EasingFunction::Linear),
        Transition::new(TransitionPropertyKind::name("transform"))
            .duration(400.ms())
            .delay(100.ms()),
    ]);
    assert_eq!(
        s.to_string(),
        "transition: opacity 300ms linear, transform 400ms 100ms;"
    );
}

#[test]
fn background_full_shorthand() {
    let s = Style::new().background(
        Background::new()
            .layer(BackgroundLayer::new(Gradient::linear_to_bottom([
                ColorStop::new(NamedColor::Red.into()),
                ColorStop::new(NamedColor::Blue.into()),
            ])))
            .color(Color::Named(NamedColor::White)),
    );
    assert_eq!(
        s.to_string(),
        "background: linear-gradient(to bottom, red, blue) white;"
    );
}

#[test]
fn linear_extension_block() {
    let s = Style::new()
        .display(Display::Linear)
        .linear_orientation(LinearOrientation::Vertical)
        .linear_weight(1.0);
    assert_eq!(
        s.to_string(),
        "display: linear; linear-orientation: vertical; linear-weight: 1;"
    );
}

#[test]
fn grid_definition_block() {
    let s = Style::new()
        .display_grid()
        .grid_template_columns(GridTemplate::tracks(["1fr", "auto", "1fr"]))
        .grid_template_rows(GridTemplate::tracks(["auto"]))
        .grid_row_start(GridLine::Number(1))
        .grid_column_end(GridLine::Span(2));
    let css = s.to_string();
    assert!(css.contains("display: grid"));
    assert!(css.contains("grid-template-columns: 1fr auto 1fr"));
    assert!(css.contains("grid-template-rows: auto"));
    assert!(css.contains("grid-row-start: 1"));
    assert!(css.contains("grid-column-end: span 2"));
}

#[test]
fn size_keyword_then_explicit_length_overrides() {
    let s = Style::new().width(Size::Auto).width(px(200));
    assert_eq!(s.to_string(), "width: 200px;");
}

#[test]
fn visibility_then_opacity() {
    let s = Style::new().visibility(Visibility::Hidden).opacity(0.0);
    assert_eq!(s.to_string(), "visibility: hidden; opacity: 0;");
}

#[test]
fn border_radius_full_elliptical_stays_shorthand() {
    let h = [
        LengthPercentage::Length(px(2)),
        LengthPercentage::Length(px(4)),
        LengthPercentage::Length(px(6)),
        LengthPercentage::Length(px(8)),
    ];
    let v = [
        LengthPercentage::Length(px(20)),
        LengthPercentage::Length(px(40)),
        LengthPercentage::Length(px(60)),
        LengthPercentage::Length(px(80)),
    ];
    let s = Style::new().border_radius_full(BorderRadius::elliptical(h, v));
    assert_eq!(
        s.to_string(),
        "border-radius: 2px 4px 6px 8px / 20px 40px 60px 80px;"
    );
}

#[test]
fn align_items_then_full_layout() {
    let s = Style::new()
        .display_flex()
        .flex_direction(FlexDirection::Row)
        .align_items(AlignItems::Center)
        .justify_content(JustifyContent::SpaceBetween);
    assert_eq!(
        s.to_string(),
        "display: flex; flex-direction: row; align-items: center; justify-content: space-between;"
    );
}

#[test]
fn into_string_yields_full_css() {
    let s = Style::new().padding(px(8)).color(Color::hex(0xFF0000));
    let css: String = s.into();
    assert!(css.contains("padding-top: 8px"));
    assert!(css.contains("color: rgb(255, 0, 0)"));
}

#[test]
fn duplicate_then_late_repeat_keeps_late_position() {
    // `color` is declared at index 0, then again at index 2; in the
    // resolved order it appears where the last write occurred (after
    // background-color).
    let s = Style::new()
        .color(Color::hex(0xFF0000))
        .background_color(Color::hex(0x00FF00))
        .color(Color::hex(0x0000FF));
    assert_eq!(
        s.to_string(),
        "background-color: rgb(0, 255, 0); color: rgb(0, 0, 255);"
    );
}

#[test]
fn border_style_constructors_compose_independently() {
    let solid_only = Border::new().solid();
    let composed = Border::new()
        .width(px(2))
        .color(Color::Named(NamedColor::Blue))
        .dotted();
    let s = Style::new().border_top(solid_only).border_bottom(composed);
    let css = s.to_string();
    assert!(css.contains("border-top-style: solid"));
    assert!(css.contains("border-bottom-style: dotted"));
    assert!(css.contains("border-bottom-width: 2px"));
    assert!(css.contains("border-bottom-color: blue"));
}

#[test]
fn padding_4tuple_resolves_each_side_independently() {
    let s = Style::new().padding((px(1), px(2), px(3), px(4)));
    assert_eq!(
        s.to_string(),
        "padding-top: 1px; padding-right: 2px; padding-bottom: 3px; padding-left: 4px;"
    );
}

#[test]
fn padding_2tuple_then_individual_side_override() {
    let s = Style::new().padding((px(8), px(16))).padding_right(px(32));
    let css = s.to_string();
    assert!(css.contains("padding-top: 8px"));
    assert!(css.contains("padding-bottom: 8px"));
    assert!(css.contains("padding-left: 16px"));
    assert!(css.contains("padding-right: 32px"));
}

#[test]
fn background_layer_min_image_only_renders() {
    let layer = BackgroundLayer::new(ImageRef::Url(CssString::new("a.png")));
    let s = Style::new().background(Background::new().layer(layer));
    assert_eq!(s.to_string(), "background: url(\"a.png\");");
}

#[test]
fn color_conversion_named_to_hex_round_trip_shape() {
    let s = Style::new()
        .color(Color::Named(NamedColor::Red))
        .background_color(Color::hex(0xFF0000));
    let css = s.to_string();
    // Both forms appear distinctly — Whisker preserves the form
    // chosen by the caller rather than normalizing internally.
    assert!(css.contains("color: red"));
    assert!(css.contains("background-color: rgb(255, 0, 0)"));
}

#[test]
fn transform_then_secondary_transform_replaces() {
    let s = Style::new()
        .transform([TransformFn::TranslateX(px(10).into())])
        .transform([TransformFn::Rotate(45.deg())]);
    // Last `transform` wins entirely; the first sequence is discarded.
    assert_eq!(s.to_string(), "transform: rotate(45deg);");
}

#[test]
fn entries_iteration_preserves_duplicates() {
    let s = Style::new()
        .color(Color::hex(0x000000))
        .color(Color::hex(0xFFFFFF));
    let names: Vec<&str> = s.entries().map(|p| p.name()).collect();
    assert_eq!(names, ["color", "color"]);
    let resolved: Vec<&str> = s.resolved().iter().map(|p| p.name()).collect();
    assert_eq!(resolved, ["color"]);
}

#[test]
fn style_to_css_via_trait_object() {
    // Verify ToCss can be exercised through a trait object.
    let s = Style::new().padding(px(4));
    let mut buf = String::new();
    let dyn_to_css: &dyn ToCss = &s;
    dyn_to_css.to_css(&mut buf).unwrap();
    assert!(buf.contains("padding-top: 4px"));
}
