//! [`Style`] — input wrapper for the `style:` attribute on every
//! built-in element tag.
//!
//! The element builder's `style(...)` method accepts any value that
//! converts into a [`Style`], which absorbs three source families:
//!
//! 1. A [`whisker_css::Css`] builder value (`Css::new().padding(8.px())`).
//! 2. A raw CSS string (`String` or `&str` / `&String`).
//! 3. A reactive [`ReadSignal<T>`] / [`RwSignal<T>`] of either form.
//!
//! One wrapper lets the same `view(style: ...)` keyword accept every
//! shape without callers calling `.to_css_string()` themselves.
//! Reactive paths re-fire the attribute apply inside the element's
//! `effect`, matching every other `Signal<T>`-driven prop.
//!
//! `Style` is defined in the `whisker` umbrella crate (rather than
//! in `whisker-css`) so the `Css` crate stays `whisker-runtime`-free
//! and reusable in standalone contexts.

use std::rc::Rc;

use whisker_css::{Css, ToCss};
use whisker_engine::whisker_style::{
    DisplayValue, FlexDirectionValue, GridAutoFlowValue, GridMaxTrackSizingValue,
    GridMinTrackSizingValue, GridRepetitionCountValue, GridTemplateComponentValue,
    GridTemplateValue, GridTrackSizingValue, LengthPercentageValue, LengthUnit, LengthValue,
    SizeValue, SpecifiedStyle, StyleProperty, StyleValue,
};
use whisker_runtime::reactive::{ReadSignal, RwSignal, effect};
use whisker_runtime::view::{Element, ScrollAxis, VirtualGridLayout, VirtualListLayout};
use whisker_runtime::view::{set_inline_styles, set_specified_style};

/// Value the `style:` builder method receives.
///
/// `Clone` is cheap: dynamic variants hold an [`Rc`], so a clone
/// shares the same closure rather than re-boxing it. This lets
/// the `#[component]` / `#[module_component]` macros store a `Style`
/// prop and re-clone it on every re-invoke (hot-reload remount path).
#[derive(Clone)]
pub enum Style {
    /// Typed declarations that can flow directly into the new scene engine.
    Typed(Css),
    /// CSS source the builder applies once, at element-construction
    /// time. Raw string inputs use this compatibility variant.
    Static(String),
    /// Typed declarations produced by a reactive subscription.
    DynamicTyped(Rc<dyn Fn() -> Css + 'static>),
    /// CSS source produced by a reactive subscription. The shared
    /// closure is called inside an `effect` and re-fires whenever
    /// any signal it reads changes.
    Dynamic(Rc<dyn Fn() -> String + 'static>),
}

impl Default for Style {
    /// An empty static style — what an element would see if no
    /// `style:` prop were declared. Lets the macros emit
    /// `self.style.unwrap_or_default()` for an omitted style prop.
    fn default() -> Self {
        Style::Static(String::new())
    }
}

// ---- Static sources --------------------------------------------------------

impl From<Css> for Style {
    fn from(s: Css) -> Self {
        Style::Typed(s)
    }
}

impl From<&Css> for Style {
    fn from(s: &Css) -> Self {
        Style::Typed(s.clone())
    }
}

impl From<String> for Style {
    fn from(s: String) -> Self {
        Style::Static(s)
    }
}

impl From<&str> for Style {
    fn from(s: &str) -> Self {
        Style::Static(s.to_string())
    }
}

impl From<&String> for Style {
    fn from(s: &String) -> Self {
        Style::Static(s.clone())
    }
}

// ---- Reactive sources -------------------------------------------------------
//
// One impl per (`ReadSignal` × `RwSignal`) × (`Css` × `String`) pair.
// Hand-written rather than blanket to keep coherence out of it and the
// type-inference error on an unsupported `T` sharp.

impl From<ReadSignal<Css>> for Style {
    fn from(sig: ReadSignal<Css>) -> Self {
        Style::DynamicTyped(Rc::new(move || sig.get()))
    }
}

impl From<ReadSignal<String>> for Style {
    fn from(sig: ReadSignal<String>) -> Self {
        Style::Dynamic(Rc::new(move || sig.get()))
    }
}

impl From<RwSignal<Css>> for Style {
    fn from(sig: RwSignal<Css>) -> Self {
        Style::from(sig.read_only())
    }
}

impl From<RwSignal<String>> for Style {
    fn from(sig: RwSignal<String>) -> Self {
        Style::from(sig.read_only())
    }
}

/// Apply a [`Style`] to a Lynx element. The `Static` branch sets the
/// inline-styles attribute once; the `Dynamic` branch wraps the
/// closure in an `effect` so it re-applies whenever any signal it
/// reads fires.
pub fn apply_style(h: Element, v: impl Into<Style>) {
    match v.into() {
        Style::Typed(css) => apply_typed_or_legacy(h, &css),
        Style::Static(css) => set_inline_styles(h, &css),
        Style::DynamicTyped(f) => {
            effect(move || apply_typed_or_legacy(h, &f()));
        }
        Style::Dynamic(f) => {
            effect(move || set_inline_styles(h, &f()));
        }
    }
}

/// Applies the two-layer List content style and returns the private
/// virtualization policy derived from it. A virtual Grid deliberately accepts
/// only a fixed, source-order subset; more global Grid algorithms cannot be
/// represented without materializing every item.
pub(crate) fn apply_list_content_style(
    element: Element,
    axis: ScrollAxis,
    style: Option<Style>,
) -> VirtualListLayout {
    let direction = match axis {
        ScrollAxis::Vertical => FlexDirectionValue::Column,
        ScrollAxis::Horizontal => FlexDirectionValue::Row,
    };
    let base = SpecifiedStyle::new()
        .push(
            StyleProperty::Display,
            StyleValue::Display(DisplayValue::Flex),
        )
        .push(
            StyleProperty::FlexDirection,
            StyleValue::FlexDirection(direction),
        );

    match style {
        None => {
            set_specified_style(element, &base);
            VirtualListLayout::Linear
        }
        Some(Style::Typed(css)) => {
            let specified = css
                .to_specified_style()
                .unwrap_or_else(|error| panic!("List content_style must use typed CSS: {error}"));
            match virtual_grid_styles(&specified, axis) {
                Ok(Some((outer, grid))) => {
                    set_specified_style(element, &base.merge(outer));
                    VirtualListLayout::Grid(grid)
                }
                Ok(None) => {
                    set_specified_style(element, &base.merge(specified));
                    VirtualListLayout::Linear
                }
                Err(reason) => panic!("unsupported virtualized Grid: {reason}"),
            }
        }
        Some(Style::DynamicTyped(f)) => {
            effect(move || {
                let css = f();
                let specified = css.to_specified_style().unwrap_or_else(|error| {
                    panic!("List content_style must use typed CSS: {error}")
                });
                if specified.resolved().iter().any(|declaration| {
                    declaration.property() == StyleProperty::Display
                        && declaration.value() == &StyleValue::Display(DisplayValue::Grid)
                }) {
                    panic!(
                        "unsupported virtualized Grid: reactive Grid configuration is not supported; use a stable typed content_style"
                    );
                }
                set_specified_style(element, &base.clone().merge(specified));
            });
            VirtualListLayout::Linear
        }
        Some(Style::Static(css)) => {
            set_inline_styles(element, &css);
            VirtualListLayout::Linear
        }
        Some(Style::Dynamic(f)) => {
            effect(move || set_inline_styles(element, &f()));
            VirtualListLayout::Linear
        }
    }
}

fn virtual_grid_styles(
    style: &SpecifiedStyle,
    axis: ScrollAxis,
) -> Result<Option<(SpecifiedStyle, VirtualGridLayout)>, String> {
    let resolved = style.resolved();
    let is_grid = resolved.iter().any(|declaration| {
        declaration.property() == StyleProperty::Display
            && declaration.value() == &StyleValue::Display(DisplayValue::Grid)
    });
    if !is_grid {
        return Ok(None);
    }

    let (cross_template_property, main_template_property, cross_gap_property, main_gap_property) =
        match axis {
            ScrollAxis::Vertical => (
                StyleProperty::GridTemplateColumns,
                StyleProperty::GridTemplateRows,
                StyleProperty::ColumnGap,
                StyleProperty::RowGap,
            ),
            ScrollAxis::Horizontal => (
                StyleProperty::GridTemplateRows,
                StyleProperty::GridTemplateColumns,
                StyleProperty::RowGap,
                StyleProperty::ColumnGap,
            ),
        };
    let expected_flow = match axis {
        ScrollAxis::Vertical => GridAutoFlowValue::Row,
        ScrollAxis::Horizontal => GridAutoFlowValue::Column,
    };

    let declaration = |property| {
        resolved
            .iter()
            .copied()
            .find(|declaration| declaration.property() == property)
    };
    if declaration(main_template_property).is_some() {
        return Err(format!(
            "`{}` controls the virtualized axis; omit it and size item content instead",
            main_template_property.css_name()
        ));
    }
    if let Some(flow) = declaration(StyleProperty::GridAutoFlow) {
        match flow.value() {
            StyleValue::GridAutoFlow(value) if *value == expected_flow => {}
            StyleValue::GridAutoFlow(GridAutoFlowValue::RowDense)
            | StyleValue::GridAutoFlow(GridAutoFlowValue::ColumnDense) => {
                return Err(
                    "`grid-auto-flow: dense` can move later items into earlier tracks".into(),
                );
            }
            StyleValue::GridAutoFlow(_) => {
                return Err(format!(
                    "`grid-auto-flow` must follow the List {:?} axis",
                    axis
                ));
            }
            _ => return Err("`grid-auto-flow` has an invalid typed value".into()),
        }
    }
    for property in [
        StyleProperty::GridAutoRows,
        StyleProperty::GridAutoColumns,
        StyleProperty::GridTemplateAreas,
    ] {
        if declaration(property).is_some() {
            return Err(format!(
                "`{}` is not supported by virtualized Grid",
                property.css_name()
            ));
        }
    }

    let template_declaration = declaration(cross_template_property).ok_or_else(|| {
        format!(
            "`{}` must declare a fixed number of tracks",
            cross_template_property.css_name()
        )
    })?;
    let StyleValue::GridTemplate(template) = template_declaration.value() else {
        return Err(format!(
            "`{}` must use a typed Grid template",
            cross_template_property.css_name()
        ));
    };
    let items_per_track = fixed_grid_track_count(template)?;
    if items_per_track == 0 {
        return Err("the cross-axis Grid template must contain at least one track".into());
    }

    let main_gap = match declaration(main_gap_property) {
        None => 0.0,
        Some(declaration) => fixed_pixel_gap(declaration.value()).ok_or_else(|| {
            format!(
                "`{}` must be a non-negative px value in a virtualized Grid",
                main_gap_property.css_name()
            )
        })?,
    };

    let grid_properties = [
        StyleProperty::Display,
        StyleProperty::GridTemplateColumns,
        StyleProperty::GridTemplateRows,
        StyleProperty::GridAutoRows,
        StyleProperty::GridAutoColumns,
        StyleProperty::GridAutoFlow,
        StyleProperty::GridTemplateAreas,
        StyleProperty::RowGap,
        StyleProperty::ColumnGap,
        StyleProperty::AlignItems,
        StyleProperty::AlignContent,
        StyleProperty::JustifyItems,
        StyleProperty::JustifyContent,
    ];
    let mut outer = SpecifiedStyle::new();
    for declaration in &resolved {
        if !grid_properties.contains(&declaration.property()) {
            outer = outer.push(declaration.property(), declaration.value().clone());
        }
    }
    for declaration in style.resolved_custom() {
        outer = outer.push_custom(declaration.name().clone(), declaration.value().clone());
    }

    let mut track = SpecifiedStyle::new()
        .push(
            StyleProperty::Display,
            StyleValue::Display(DisplayValue::Grid),
        )
        .push(
            cross_template_property,
            StyleValue::GridTemplate(template.clone()),
        )
        .push(
            StyleProperty::GridAutoFlow,
            StyleValue::GridAutoFlow(expected_flow),
        );
    for property in [
        cross_gap_property,
        StyleProperty::AlignItems,
        StyleProperty::AlignContent,
        StyleProperty::JustifyItems,
        StyleProperty::JustifyContent,
    ] {
        if let Some(declaration) = declaration(property) {
            track = track.push(property, declaration.value().clone());
        }
    }

    let zero = StyleValue::Size(SizeValue::LengthPercentage(LengthPercentageValue::Length(
        LengthValue::Zero,
    )));
    let cell = SpecifiedStyle::new()
        .push(StyleProperty::MinWidth, zero.clone())
        .push(StyleProperty::MinHeight, zero);

    Ok(Some((
        outer,
        VirtualGridLayout {
            items_per_track,
            track_style: track,
            cell_style: cell,
            main_gap,
        },
    )))
}

fn fixed_grid_track_count(template: &GridTemplateValue) -> Result<usize, String> {
    if template.line_names.iter().any(|names| !names.is_empty()) {
        return Err("named Grid lines are not supported by virtualized Grid".into());
    }
    let mut count = 0_usize;
    for component in &template.components {
        match component {
            GridTemplateComponentValue::Track(track) => {
                validate_virtual_track(track)?;
                count = count.saturating_add(1);
            }
            GridTemplateComponentValue::Repeat(repetition) => {
                if repetition.line_names.iter().any(|names| !names.is_empty()) {
                    return Err("named Grid lines are not supported by virtualized Grid".into());
                }
                let GridRepetitionCountValue::Count(repetitions) = repetition.count else {
                    return Err(
                        "`auto-fill` and `auto-fit` are not supported by virtualized Grid".into(),
                    );
                };
                for track in &repetition.tracks {
                    validate_virtual_track(track)?;
                }
                count = count.saturating_add(usize::from(repetitions) * repetition.tracks.len());
            }
        }
    }
    Ok(count)
}

fn validate_virtual_track(track: &GridTrackSizingValue) -> Result<(), String> {
    let supported = matches!(
        (&track.min, &track.max),
        (
            GridMinTrackSizingValue::Fixed(_),
            GridMaxTrackSizingValue::Fixed(_)
        ) | (
            GridMinTrackSizingValue::Fixed(_),
            GridMaxTrackSizingValue::Fraction(_)
        ) | (
            GridMinTrackSizingValue::Auto,
            GridMaxTrackSizingValue::Fraction(_)
        )
    );
    if supported {
        Ok(())
    } else {
        Err(
            "intrinsic Grid tracks (`auto`, `min-content`, `max-content`, `fit-content`) are not supported by virtualized Grid"
                .into(),
        )
    }
}

fn fixed_pixel_gap(value: &StyleValue) -> Option<f32> {
    let StyleValue::LengthPercentage(LengthPercentageValue::Length(length)) = value else {
        return None;
    };
    let value = match length {
        LengthValue::Zero => 0.0,
        LengthValue::Dimension {
            value,
            unit: LengthUnit::Px,
        } => value.get(),
        LengthValue::Dimension { .. } => return None,
    };
    value.is_finite().then_some(value.max(0.0))
}

fn apply_typed_or_legacy(h: Element, css: &Css) {
    if let Ok(specified) = css.to_specified_style()
        && whisker_runtime::view::set_specified_style(h, &specified)
    {
        return;
    }
    set_inline_styles(h, &css.to_css_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_css::ext::*;

    fn css(d: Style) -> String {
        match d {
            Style::Typed(s) => s.to_css_string(),
            Style::Static(s) => s,
            Style::DynamicTyped(f) => f().to_css_string(),
            Style::Dynamic(f) => f(),
        }
    }

    #[test]
    fn from_css_serializes_via_to_css_string() {
        let s = Css::new().padding(px(8));
        let out = css(s.into());
        assert!(out.contains("padding-top: 8px"));
    }

    #[test]
    fn from_borrowed_css_keeps_owner_alive() {
        let s = Css::new().padding(px(8));
        let style: Style = (&s).into();
        let out = css(style);
        assert!(out.contains("padding-top: 8px"));
        assert!(!s.is_empty());
    }

    #[test]
    fn from_str_passes_through_verbatim() {
        let out = css("color: red;".into());
        assert_eq!(out, "color: red;");
    }

    #[test]
    fn from_string_consumes_and_returns_same_text() {
        let out = css(String::from("color: blue;").into());
        assert_eq!(out, "color: blue;");
    }

    #[test]
    fn from_string_ref_clones() {
        let owner = String::from("color: green;");
        let out = css((&owner).into());
        assert_eq!(out, "color: green;");
        assert_eq!(owner, "color: green;");
    }

    // The reactive `From` impls need the runtime arena, which this
    // standalone test environment doesn't bootstrap; the static cases
    // above cover the discriminant + serialization paths.
}
