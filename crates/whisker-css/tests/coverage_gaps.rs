//! Coverage gap closers — exercises paths the per-module tests miss
//! (derive-only paths, less-common conversions, every Debug/PartialEq
//! invocation we need for round-tripping in user code).

use whisker_css::data_type::{
    Angle, CalcExpr, Color, ColorStop, CssString, FitContent, Gradient, Length, LengthPercentage,
    LinearDirection, MaxContent, NamedColor, Number, Percentage, RadialShape, StopPosition, Time,
};
use whisker_css::data_type_ext::{
    EasingFunction, Integer, Position, PositionKeyword, StepPosition,
};
use whisker_css::ext::*;
use whisker_css::shorthand::padding_margin::MarginValue;
use whisker_css::shorthand::{Animation, Background, BackgroundLayer, Border, Transition};
use whisker_css::value::{
    BorderRadius, FlexBasis, GridLine, GridTemplate, ImageRef, LineHeight, Repeated, Size,
};
use whisker_css::{Css, ToCss};

#[test]
// Whole-point of the test is to exercise the derived `Clone` impl
// on every public type, including the ones that are also `Copy`.
#[allow(clippy::clone_on_copy)]
fn debug_and_clone_pass_through() {
    // Exercise auto-derived `Clone` + `Debug` on every public type.
    let n = Number(1.0);
    let p = Percentage(50.0);
    let l = Length::Px(8.0);
    let lp = LengthPercentage::Length(l);
    let a = Angle::Deg(45.0);
    let t = Time::Ms(300.0);
    let cs = CssString::new("x");
    let c = Color::Named(NamedColor::Red);
    let g = Gradient::linear_to_bottom([ColorStop::new(c)]);
    let f = FitContent::with_limit(l);
    let m = MaxContent;
    let calc = CalcExpr::value(l);
    let i = Integer(2);
    let pos = Position::Keyword(PositionKeyword::Center);
    let stop = StopPosition::Number(0.5);
    let easing = EasingFunction::Steps(2, StepPosition::JumpEnd);
    let stop_pos = StepPosition::JumpStart;
    let lin = LinearDirection::ToBottom;
    let radial = RadialShape::Circle;

    // Force `Clone` to fire on every variant.
    let _ = (
        n.clone(),
        p.clone(),
        l,
        lp.clone(),
        a,
        t,
        cs.clone(),
        c,
        g.clone(),
        f.clone(),
        m,
        calc.clone(),
        i,
        pos.clone(),
        stop.clone(),
        easing,
        stop_pos,
        lin,
        radial.clone(),
    );

    // Force `Debug` (drops the verbose result on the floor).
    let _ = format!(
        "{:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?}",
        n, p, l, lp, a, t, cs, c, g, f, m, calc, i, pos, stop, easing, stop_pos, lin, radial,
    );
}

#[test]
fn length_zero_via_constructor_helpers() {
    // ZERO constant and is_zero detection.
    let z = whisker_css::ext::ZERO;
    assert!(z.is_zero());
    // Construct via `0.px()` — non-Zero variant whose value is 0.
    assert!(0.px().is_zero());
}

#[test]
fn length_percentage_calc_all_operators() {
    let e = CalcExpr::value(px(10))
        .add(CalcExpr::value(20.percent()))
        .sub(CalcExpr::value(px(5)))
        .mul(CalcExpr::number(2.0))
        .div(CalcExpr::number(4.0));
    let css = LengthPercentage::calc(e).to_css_string();
    assert!(css.starts_with("calc("));
    assert!(css.ends_with(")"));
}

#[test]
fn color_hex_alpha_opaque_path() {
    // alpha = 255 → opaque rgb form.
    assert_eq!(
        Color::hex_alpha(0xFF0000FF).to_css_string(),
        "rgb(255, 0, 0)"
    );
}

#[test]
fn color_rgb_constructor_path() {
    assert_eq!(Color::rgb(1, 2, 3).to_css_string(), "rgb(1, 2, 3)");
}

#[test]
fn color_named_round_trip_via_from() {
    let c: Color = NamedColor::Red.into();
    assert_eq!(c.to_css_string(), "red");
}

#[test]
fn gradient_radial_with_shape_keywords() {
    // Exercise Circle / Ellipse keyword paths.
    let g = Gradient::Radial {
        shape: RadialShape::Ellipse,
        stops: vec![ColorStop::new(Color::Named(NamedColor::Red))],
    };
    assert!(g.to_css_string().contains("ellipse"));
}

#[test]
fn gradient_color_stop_at_constructor() {
    let s = ColorStop::at(Color::Named(NamedColor::Red), Percentage(20.0));
    assert_eq!(s.to_css_string(), "red 20%");
}

#[test]
fn css_string_via_string_from() {
    let s: CssString = String::from("y").into();
    assert_eq!(s.to_css_string(), "\"y\"");
    let s2: CssString = "z".into();
    assert_eq!(s2.to_css_string(), "\"z\"");
    assert_eq!(s2.as_str(), "z");
}

#[test]
fn integer_constructor_paths() {
    assert_eq!(Integer::new(5).value(), 5);
    assert_eq!(Integer::from(3).to_css_string(), "3");
}

#[test]
fn percentage_constructor_paths() {
    assert_eq!(Percentage::new(33.0).value(), 33.0);
    let from_i: Percentage = 4.into();
    let from_f: Percentage = 4.0_f32.into();
    assert_eq!(from_i.to_css_string(), "4%");
    assert_eq!(from_f.to_css_string(), "4%");
}

#[test]
fn easing_step_position_legacy_aliases() {
    let s = EasingFunction::Steps(2, StepPosition::Start);
    assert_eq!(s.to_css_string(), "steps(2, start)");
    let s = EasingFunction::Steps(2, StepPosition::End);
    assert_eq!(s.to_css_string(), "steps(2, end)");
}

#[test]
fn position_mixed_keyword_offset() {
    let p = Position::Mixed(PositionKeyword::Bottom, px(8).into());
    assert_eq!(p.to_css_string(), "bottom 8px");
}

#[test]
fn position_keyword_each_variant() {
    use PositionKeyword::*;
    for k in [Left, Right, Top, Bottom, Center] {
        assert!(!Position::Keyword(k).to_css_string().is_empty());
    }
}

#[test]
fn image_ref_url_then_gradient_then_none() {
    let url = ImageRef::Url(CssString::new("a.png"));
    let gradient: ImageRef =
        Gradient::linear_to_bottom([ColorStop::new(NamedColor::Red.into())]).into();
    let none = ImageRef::None;
    assert!(url.to_css_string().contains("url"));
    assert!(gradient.to_css_string().contains("linear-gradient"));
    assert!(none.to_css_string() == "none");
}

#[test]
fn flex_basis_from_paths() {
    let from_lp: FlexBasis = LengthPercentage::Length(px(8)).into();
    let from_p: FlexBasis = 20.percent().into();
    let from_l: FlexBasis = px(8).into();
    for f in [from_lp, from_p, from_l] {
        assert!(!f.to_css_string().is_empty());
    }
}

#[test]
fn line_height_from_paths() {
    let from_lp: LineHeight = LengthPercentage::Length(px(20)).into();
    let from_p: LineHeight = 150.percent().into();
    let from_l: LineHeight = px(20).into();
    let from_f: LineHeight = 1.5_f32.into();
    for h in [from_lp, from_p, from_l, from_f] {
        assert!(!h.to_css_string().is_empty());
    }
}

#[test]
fn size_from_paths_all() {
    let from_l: Size = px(8).into();
    let from_p: Size = 50.percent().into();
    let from_lp: Size = LengthPercentage::Length(px(8)).into();
    let from_mc: Size = MaxContent.into();
    let from_fc: Size = FitContent::keyword().into();
    for s in [from_l, from_p, from_lp, from_mc, from_fc] {
        assert!(!s.to_css_string().is_empty());
    }
    assert_eq!(Size::None.to_css_string(), "none");
}

#[test]
fn margin_value_all_from_impls() {
    let from_l: MarginValue = px(8).into();
    let from_p: MarginValue = 25.percent().into();
    let from_lp: MarginValue = LengthPercentage::Length(px(8)).into();
    let auto = MarginValue::Auto;
    assert_eq!(from_l.to_css_string(), "8px");
    assert_eq!(from_p.to_css_string(), "25%");
    assert_eq!(from_lp.to_css_string(), "8px");
    assert_eq!(auto.to_css_string(), "auto");
}

#[test]
fn border_radius_all_constructor() {
    let r = BorderRadius::all(px(4));
    assert!(r.to_css_string().contains("4px"));
}

#[test]
fn grid_line_all_variants() {
    assert!(matches!(GridLine::Auto, GridLine::Auto));
    assert_eq!(GridLine::Number(0).to_css_string(), "0");
    assert_eq!(GridLine::Span(1).to_css_string(), "span 1");
}

#[test]
fn grid_template_empty_then_extend() {
    let tracks: Vec<String> = Vec::new();
    let t1 = GridTemplate::tracks(tracks);
    assert!(t1.to_css_string().is_empty());
    let t2 = GridTemplate::tracks(["1fr".to_string(), "2fr".to_string()]);
    assert_eq!(t2.to_css_string(), "1fr 2fr");
}

#[test]
fn repeated_empty_collection() {
    let v: Vec<Length> = Vec::new();
    let r: Repeated<Length> = Repeated::new(v);
    assert!(r.to_css_string().is_empty());
}

#[test]
fn animation_only_a_few_fields_set() {
    // Exercises individual setters being absent without crashing.
    let s = Css::new().animation(Animation::new("x"));
    assert_eq!(s.to_string(), "animation: x;");
    // Now exercise the path with delay set but iteration_count omitted.
    let s = Css::new().animation(Animation::new("y").delay(50.ms()));
    assert!(s.to_string().contains("50ms"));
}

#[test]
fn transition_skip_some_fields() {
    let t = Transition::new(whisker_css::keyword::TransitionPropertyKind::All).delay(0.s());
    let s = Css::new().transition(t);
    assert!(s.to_string().contains("0s"));
}

#[test]
fn background_color_only_via_struct() {
    let s = Css::new().background(Background::new().color(Color::Named(NamedColor::Black)));
    assert_eq!(s.to_string(), "background: black;");
}

#[test]
fn background_layer_image_only_then_color() {
    let s = Css::new().background(
        Background::new()
            .layer(BackgroundLayer::new(ImageRef::None))
            .color(Color::Named(NamedColor::Black)),
    );
    assert!(s.to_string().contains("none"));
}

#[test]
fn border_constructor_default() {
    let b = Border::new();
    assert!(b.width.is_none() && b.style.is_none() && b.color.is_none());
}

#[test]
fn calc_expr_value_and_number_constructors() {
    let v = CalcExpr::value(px(4));
    let n = CalcExpr::number(2.0);
    let _ = LengthPercentage::calc(v.clone().mul(n.clone()));
    // Smoke check on stand-alone formatting.
    assert!(v.to_css_string().contains("4px"));
    assert!(n.to_css_string() == "2");
}

#[test]
fn linear_direction_keyword_directions_in_gradient() {
    use LinearDirection::*;
    for d in [
        ToTop,
        ToRight,
        ToBottom,
        ToLeft,
        ToTopRight,
        ToTopLeft,
        ToBottomRight,
        ToBottomLeft,
    ] {
        let g = Gradient::Linear {
            direction: d,
            stops: vec![ColorStop::new(Color::Named(NamedColor::Red))],
        };
        assert!(g.to_css_string().contains("linear-gradient"));
    }
    let g = Gradient::Linear {
        direction: LinearDirection::Angle(45.deg()),
        stops: vec![ColorStop::new(Color::Named(NamedColor::Red))],
    };
    assert!(g.to_css_string().contains("45deg"));
}

#[test]
fn fit_content_keyword_path() {
    // FitContent::keyword() variant — no limit.
    let fc = FitContent::keyword();
    assert_eq!(fc.to_css_string(), "fit-content");
}

#[test]
fn length_each_unit_zero_path() {
    // is_zero for every variant where value == 0.0.
    assert!(Length::Px(0.0).is_zero());
    assert!(Length::Rpx(0.0).is_zero());
    assert!(Length::Ppx(0.0).is_zero());
    assert!(Length::Em(0.0).is_zero());
    assert!(Length::Rem(0.0).is_zero());
    assert!(Length::Vh(0.0).is_zero());
    assert!(Length::Vw(0.0).is_zero());
}
