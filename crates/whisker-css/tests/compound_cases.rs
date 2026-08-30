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

use whisker_css::ext::*;
use whisker_css::keyword::{AlignItems, Overflow};
use whisker_css::{
    Animation, Background, BackgroundLayer, Border, BorderRadius, Color, ColorStop, Css, CssString,
    EasingFunction, Flex, FlexBasis, FlexDirection, Gradient, GridLine, GridTemplate, GridTrack,
    ImageRef, JustifyContent, LengthPercentage, NamedColor, Number, PositionKind, Size, ToCss,
    TransformFn, Transition, TransitionPropertyKind, Visibility,
};

#[test]
fn realistic_card_layout() {
    let s = Css::new()
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
    let s = Css::new().padding(px(16)).padding_top(px(4));
    let css = s.to_string();
    assert!(css.contains("padding-top: 4px"));
    assert!(css.contains("padding-right: 16px"));
    assert!(css.contains("padding-bottom: 16px"));
    assert!(css.contains("padding-left: 16px"));
}

#[test]
fn padding_longhand_then_shorthand_resets() {
    let s = Css::new().padding_top(px(4)).padding(px(16));
    assert_eq!(
        s.to_string(),
        "padding-top: 16px; padding-right: 16px; padding-bottom: 16px; padding-left: 16px;"
    );
}

#[test]
fn margin_auto_combined_with_explicit_top() {
    let s = Css::new()
        .margin(px(0))
        .margin_top(px(8))
        .margin_left(whisker_css::shorthand::padding_margin::MarginValue::Auto)
        .margin_right(whisker_css::shorthand::padding_margin::MarginValue::Auto);
    assert_eq!(
        s.to_string(),
        "margin-bottom: 0px; margin-top: 8px; margin-left: auto; margin-right: auto;"
    );
}

#[test]
fn border_full_then_per_side_override() {
    let s = Css::new()
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
    assert!(css.contains("border-bottom-width: 3px"));
    assert!(css.contains("border-bottom-style: solid"));
    assert!(css.contains("border-bottom-color: rgb(255, 0, 0)"));
    assert!(css.contains("border-top-width: 1px"));
    assert!(css.contains("border-right-color: rgb(204, 204, 204)"));
}

#[test]
fn flex_shorthand_then_per_axis_override() {
    let s = Css::new().flex(Flex::Auto).flex_basis(FlexBasis::Content);
    assert_eq!(
        s.to_string(),
        "flex-grow: 1; flex-shrink: 1; flex-basis: content;"
    );
}

#[test]
fn flex_number_then_grow_chain() {
    let s = Css::new().flex(Flex::Number(2.0)).flex_grow(3.0);
    let css = s.to_string();
    assert!(css.contains("flex-grow: 3"));
    assert!(css.contains("flex-shrink: 1"));
    assert!(css.contains("flex-basis: 0%"));
}

#[test]
fn position_absolute_overlay() {
    let s = Css::new()
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
    let s = Css::new()
        .overflow(Overflow::Hidden)
        .overflow_x(Overflow::Visible);
    assert_eq!(s.to_string(), "overflow-y: hidden; overflow-x: visible;");
}

#[test]
fn gap_then_row_gap_override_keeps_column() {
    let s = Css::new().gap(px(8)).row_gap(px(16));
    assert_eq!(s.to_string(), "column-gap: 8px; row-gap: 16px;");
}

#[test]
fn last_write_wins_over_many_repeats() {
    let s = Css::new()
        .color(Color::hex(0x111111))
        .color(Color::hex(0x222222))
        .color(Color::hex(0x333333))
        .color(Color::hex(0x444444))
        .color(Color::hex(0x555555));
    assert_eq!(s.to_string(), "color: rgb(85, 85, 85);");
}

#[test]
fn empty_style_is_empty_string() {
    let s = Css::new();
    assert_eq!(s.to_string(), "");
    assert!(s.is_empty());
}

#[test]
fn merge_overlays_other_onto_self() {
    let base = Css::new().padding(px(4)).color(Color::hex(0x000000));
    let overlay = Css::new().color(Color::hex(0xFFFFFF));
    let merged = base.merge(overlay);
    let css = merged.to_string();
    assert!(css.contains("color: rgb(255, 255, 255)"));
    assert!(css.contains("padding-top: 4px"));
}

#[test]
fn complete_animation_chain() {
    let s = Css::new()
        .animation(
            Animation::new("pulse")
                .duration(2.s())
                .timing(EasingFunction::EaseInOut)
                .iteration_count(whisker_css::keyword::AnimationIterationCount::Infinite)
                .direction(whisker_css::keyword::AnimationDirection::Alternate),
        )
        .opacity(0.8);
    let css = s.to_string();
    assert!(css.contains("animation: pulse 2s ease-in-out infinite alternate"));
    assert!(css.contains("opacity: 0.8"));
}

#[test]
fn transform_layered() {
    let s = Css::new().transform([
        TransformFn::TranslateX(px(10).into()),
        TransformFn::Scale(Number::new(1.5).into(), Number::new(1.5).into()),
        TransformFn::Rotate(45.deg().into()),
    ]);
    assert_eq!(
        s.to_string(),
        "transform: translateX(10px) scale(1.5, 1.5) rotate(45deg);"
    );
}

#[test]
fn transitions_multi() {
    let s = Css::new().transitions([
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
    let s = Css::new().background(
        Background::new()
            .layer(BackgroundLayer::new(Gradient::linear_to_bottom([
                ColorStop::new(Color::Named(NamedColor::Red)),
                ColorStop::new(Color::Named(NamedColor::Blue)),
            ])))
            .color(Color::Named(NamedColor::White)),
    );
    assert_eq!(
        s.to_string(),
        "background: linear-gradient(to bottom, red, blue) white;"
    );
}

#[test]
fn grid_definition_block() {
    let s = Css::new()
        .display_grid()
        .grid_template_columns(GridTemplate::tracks([
            GridTrack::fraction(1.0),
            GridTrack::auto(),
            GridTrack::fraction(1.0),
        ]))
        .grid_template_rows(GridTemplate::tracks([GridTrack::auto()]))
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
    let s = Css::new().width(Size::Auto).width(px(200));
    assert_eq!(s.to_string(), "width: 200px;");
}

#[test]
fn visibility_then_opacity() {
    let s = Css::new().visibility(Visibility::Hidden).opacity(0.0);
    assert_eq!(s.to_string(), "visibility: hidden; opacity: 0;");
}

#[test]
fn border_radius_full_elliptical_expands_to_semantic_corner_longhands() {
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
    let s = Css::new().border_radius_full(BorderRadius::elliptical(h, v));
    assert_eq!(
        s.to_string(),
        "border-top-left-radius: 2px 20px; border-top-right-radius: 4px 40px; border-bottom-right-radius: 6px 60px; border-bottom-left-radius: 8px 80px;"
    );
    assert!(s.resolved().into_iter().all(|declaration| matches!(
        declaration.style_value(),
        whisker_style::StyleValue::BorderRadius(_)
    )));
    let specified = s.to_specified_style();
    let resolved =
        whisker_style::resolve_style(&specified, None, whisker_style::StyleEnvironment::default())
            .unwrap();
    let radius = resolved.computed().paint().border_radii.top_left;
    assert_eq!(radius.horizontal.length(), 2.0);
    assert_eq!(radius.vertical.length(), 20.0);
}

#[test]
fn align_items_then_full_layout() {
    let s = Css::new()
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
    let s = Css::new().padding(px(8)).color(Color::hex(0xFF0000));
    let css: String = s.into();
    assert!(css.contains("padding-top: 8px"));
    assert!(css.contains("color: rgb(255, 0, 0)"));
}

#[test]
fn duplicate_then_late_repeat_keeps_late_position() {
    // A property re-declared later resolves at the position of its
    // LAST write, not its first.
    let s = Css::new()
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
    let s = Css::new().border_top(solid_only).border_bottom(composed);
    let css = s.to_string();
    assert!(css.contains("border-top-style: solid"));
    assert!(css.contains("border-bottom-style: dotted"));
    assert!(css.contains("border-bottom-width: 2px"));
    assert!(css.contains("border-bottom-color: blue"));
}

#[test]
fn padding_4tuple_resolves_each_side_independently() {
    let s = Css::new().padding((px(1), px(2), px(3), px(4)));
    assert_eq!(
        s.to_string(),
        "padding-top: 1px; padding-right: 2px; padding-bottom: 3px; padding-left: 4px;"
    );
}

#[test]
fn padding_2tuple_then_individual_side_override() {
    let s = Css::new().padding((px(8), px(16))).padding_right(px(32));
    let css = s.to_string();
    assert!(css.contains("padding-top: 8px"));
    assert!(css.contains("padding-bottom: 8px"));
    assert!(css.contains("padding-left: 16px"));
    assert!(css.contains("padding-right: 32px"));
}

#[test]
fn background_layer_min_image_only_renders() {
    let layer = BackgroundLayer::new(ImageRef::Url(CssString::new("a.png")));
    let s = Css::new().background(Background::new().layer(layer));
    assert_eq!(s.to_string(), "background: url(\"a.png\");");
}

#[test]
fn color_conversion_named_to_hex_round_trip_shape() {
    let s = Css::new()
        .color(Color::Named(NamedColor::Red))
        .background_color(Color::hex(0xFF0000));
    let css = s.to_string();
    // The caller's chosen form is preserved rather than normalized.
    assert!(css.contains("color: red"));
    assert!(css.contains("background-color: rgb(255, 0, 0)"));
}

#[test]
fn transform_then_secondary_transform_replaces() {
    let s = Css::new()
        .transform([TransformFn::TranslateX(px(10).into())])
        .transform([TransformFn::Rotate(45.deg().into())]);
    assert_eq!(s.to_string(), "transform: rotate(45deg);");
}

#[test]
fn entries_iteration_preserves_duplicates() {
    let s = Css::new()
        .color(Color::hex(0x000000))
        .color(Color::hex(0xFFFFFF));
    let names: Vec<&str> = s.entries().map(|p| p.name()).collect();
    assert_eq!(names, ["color", "color"]);
    let resolved: Vec<&str> = s.resolved().iter().map(|p| p.name()).collect();
    assert_eq!(resolved, ["color"]);
}

#[test]
fn style_to_css_via_trait_object() {
    let s = Css::new().padding(px(4));
    let mut buf = String::new();
    let dyn_to_css: &dyn ToCss = &s;
    dyn_to_css.to_css(&mut buf).unwrap();
    assert!(buf.contains("padding-top: 4px"));
}
