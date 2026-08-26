//! Plain-text lowering shared by the future Text element provider and tests.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use whisker_protocol::{
    MeasureFontFamily, MeasureFontStyle, MeasureLineHeight, MeasureTextDirection,
    MeasureTextOverflow, MeasureTextWrap, MeasurementPayload, MeasurementSpec, PaintColor,
    PendingMeasurePolicy, TextContent, TextMeasurePayload, TextMeasureStyle, TextPaint, TextShadow,
};
use whisker_style::{
    ColorValue, ComputedLineHeight, FontFamilyValue, FontStyleValue, InheritedStyle,
    TextDecorationLineValue, TextDecorationStyleValue,
};

/// Plain UTF-8 text and the shaping behavior not supplied by inherited style.
#[derive(Clone, Debug, PartialEq)]
pub struct PlainTextInput {
    /// UTF-8 text after application-level transformations.
    pub text: String,
    /// Optional BCP-47 locale hint.
    pub locale: Option<String>,
    /// Base shaping direction.
    pub direction: MeasureTextDirection,
    /// Line-wrapping behavior.
    pub wrap: MeasureTextWrap,
    /// Maximum visible line count.
    pub max_lines: Option<u32>,
    /// Overflow behavior at the line limit.
    pub overflow: MeasureTextOverflow,
    /// Layout behavior if the Host cannot answer synchronously.
    pub pending_policy: PendingMeasurePolicy,
}

impl PlainTextInput {
    /// Creates ordinary wrapping text that blocks presentation until measured.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            locale: None,
            direction: MeasureTextDirection::Auto,
            wrap: MeasureTextWrap::Wrap,
            max_lines: None,
            overflow: MeasureTextOverflow::Clip,
            pending_policy: PendingMeasurePolicy::Block,
        }
    }
}

/// Measurement and presentation values produced from one plain-text input.
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredPlainText {
    content: TextContent,
    measurement: MeasurementSpec,
}

impl LoweredPlainText {
    /// Returns the final presentation before a prepared Host object is known.
    pub const fn content(&self) -> &TextContent {
        &self.content
    }

    /// Returns the intrinsic-measurement registration for the node.
    pub const fn measurement(&self) -> &MeasurementSpec {
        &self.measurement
    }

    pub(crate) fn into_parts(self) -> (TextContent, MeasurementSpec) {
        (self.content, self.measurement)
    }
}

/// Lowers computed inherited text style into the shared Host measurement model.
pub fn lower_plain_text(input: &PlainTextInput, style: &InheritedStyle) -> LoweredPlainText {
    let payload = TextMeasurePayload {
        text: input.text.clone(),
        style: TextMeasureStyle {
            font_families: vec![match style.font_family() {
                FontFamilyValue::System => MeasureFontFamily::System,
                FontFamilyValue::Named(name) => MeasureFontFamily::Named(name.clone()),
            }],
            font_size: style.font_size(),
            font_weight: style.font_weight().get(),
            font_style: match style.font_style() {
                FontStyleValue::Normal => MeasureFontStyle::Normal,
                FontStyleValue::Italic => MeasureFontStyle::Italic,
                FontStyleValue::Oblique => MeasureFontStyle::Oblique,
            },
            line_height: match style.line_height() {
                ComputedLineHeight::Normal => MeasureLineHeight::Normal,
                ComputedLineHeight::LogicalPixels(value) => {
                    MeasureLineHeight::LogicalPixels(value.get())
                }
            },
            letter_spacing: style.letter_spacing(),
            ..TextMeasureStyle::default()
        },
        locale: input.locale.clone(),
        direction: input.direction,
        wrap: input.wrap,
        max_lines: input.max_lines,
        overflow: input.overflow,
    };
    let measurement = MeasurementSpec {
        content_hash: content_hash(&input.text),
        style_hash: metric_style_hash(input, style),
        payload: MeasurementPayload::Text(payload.clone()),
        pending_policy: input.pending_policy,
    };
    LoweredPlainText {
        content: TextContent {
            payload,
            paint: TextPaint {
                foreground: lower_color(style.color()),
                decoration: whisker_protocol::TextDecoration {
                    lines: whisker_protocol::TextDecorationLines {
                        underline: matches!(
                            style.text_decoration().line(),
                            TextDecorationLineValue::Underline
                        ),
                        overline: false,
                        line_through: matches!(
                            style.text_decoration().line(),
                            TextDecorationLineValue::LineThrough
                        ),
                    },
                    color: lower_color(style.text_decoration().color()),
                    style: match style.text_decoration().style() {
                        TextDecorationStyleValue::Solid => {
                            whisker_protocol::TextDecorationStyle::Solid
                        }
                        TextDecorationStyleValue::Double => {
                            whisker_protocol::TextDecorationStyle::Double
                        }
                        TextDecorationStyleValue::Dotted => {
                            whisker_protocol::TextDecorationStyle::Dotted
                        }
                        TextDecorationStyleValue::Dashed => {
                            whisker_protocol::TextDecorationStyle::Dashed
                        }
                        TextDecorationStyleValue::Wavy => {
                            whisker_protocol::TextDecorationStyle::Wavy
                        }
                    },
                    thickness: whisker_protocol::TextDecorationThickness::Auto,
                },
                shadows: style
                    .text_shadow()
                    .into_iter()
                    .map(|shadow| TextShadow {
                        offset_x: shadow.offset_x(),
                        offset_y: shadow.offset_y(),
                        blur_radius: shadow.blur_radius(),
                        color: lower_color(shadow.color()),
                    })
                    .collect(),
            },
            prepared_content: None,
        },
        measurement,
    }
}

fn lower_color(color: &ColorValue) -> PaintColor {
    match color {
        ColorValue::Named(name) => PaintColor::Named(name.clone()),
        ColorValue::Rgba {
            red,
            green,
            blue,
            alpha,
        } => PaintColor::Srgba {
            red: *red,
            green: *green,
            blue: *blue,
            alpha: alpha.get(),
        },
        ColorValue::Hsla {
            hue_degrees,
            saturation,
            lightness,
            alpha,
        } => PaintColor::Hsla {
            hue_degrees: hue_degrees.get(),
            saturation: saturation.get(),
            lightness: lightness.get(),
            alpha: alpha.get(),
        },
    }
}

fn content_hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn metric_style_hash(input: &PlainTextInput, style: &InheritedStyle) -> u64 {
    let mut hasher = DefaultHasher::new();
    style.font_family().hash(&mut hasher);
    style.font_size().to_bits().hash(&mut hasher);
    style.font_weight().hash(&mut hasher);
    style.font_style().hash(&mut hasher);
    style.line_height().hash(&mut hasher);
    style.letter_spacing().to_bits().hash(&mut hasher);
    input.locale.hash(&mut hasher);
    input.direction.hash(&mut hasher);
    input.wrap.hash(&mut hasher);
    input.max_lines.hash(&mut hasher);
    input.overflow.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_style::{
        FontWeightValue, LengthPercentageValue, LengthUnit, LengthValue, LineHeightValue,
        SpecifiedStyle, StyleDeclaration, StyleEnvironment, StyleNumber, StyleProperty, StyleValue,
        TextDecorationValue, TextShadowValue, resolve_style,
    };

    fn resolved(declarations: Vec<StyleDeclaration>) -> whisker_style::ResolvedNodeStyle {
        let specified =
            declarations
                .into_iter()
                .fold(SpecifiedStyle::new(), |style, declaration| {
                    let (property, value) = declaration.into_parts();
                    style.push(property, value)
                });
        resolve_style(&specified, None, StyleEnvironment::default()).expect("valid text style")
    }

    #[test]
    fn defaults_lower_to_plain_text_measurement_and_presentation() {
        let style = resolved(Vec::new());
        let input = PlainTextInput::new("hello");
        let lowered = lower_plain_text(&input, style.computed().inherited_text());

        assert_eq!(lowered.content().payload.text, "hello");
        assert_eq!(
            lowered.content().payload.style.font_families,
            vec![MeasureFontFamily::System]
        );
        assert_eq!(lowered.content().payload.style.font_size, 14.0);
        assert_eq!(lowered.content().prepared_content, None);
        assert_eq!(
            lowered.measurement().pending_policy,
            PendingMeasurePolicy::Block
        );
        assert_eq!(
            lowered.measurement().payload,
            MeasurementPayload::Text(lowered.content().payload.clone())
        );

        let same = lower_plain_text(&input, style.computed().inherited_text());
        assert_eq!(lowered, same);
        let changed = lower_plain_text(
            &PlainTextInput::new("different"),
            style.computed().inherited_text(),
        );
        assert_ne!(
            lowered.measurement().content_hash,
            changed.measurement().content_hash
        );
    }

    #[test]
    fn resolved_metric_variants_and_options_reach_the_host_payload() {
        let style = resolved(vec![
            StyleDeclaration::new(
                StyleProperty::FontFamily,
                StyleValue::FontFamily(FontFamilyValue::Named("Inter".into())),
            ),
            StyleDeclaration::new(
                StyleProperty::FontSize,
                StyleValue::LengthPercentage(LengthPercentageValue::Length(
                    LengthValue::Dimension {
                        value: StyleNumber::new(18.0),
                        unit: LengthUnit::Px,
                    },
                )),
            ),
            StyleDeclaration::new(
                StyleProperty::FontWeight,
                StyleValue::FontWeight(FontWeightValue::BOLD),
            ),
            StyleDeclaration::new(
                StyleProperty::FontStyle,
                StyleValue::FontStyle(FontStyleValue::Italic),
            ),
            StyleDeclaration::new(
                StyleProperty::LineHeight,
                StyleValue::LineHeight(LineHeightValue::Number(StyleNumber::new(1.5))),
            ),
            StyleDeclaration::new(
                StyleProperty::LetterSpacing,
                StyleValue::Length(LengthValue::Dimension {
                    value: StyleNumber::new(1.0),
                    unit: LengthUnit::Px,
                }),
            ),
        ]);
        let input = PlainTextInput {
            text: "مرحبا".into(),
            locale: Some("ar".into()),
            direction: MeasureTextDirection::RightToLeft,
            wrap: MeasureTextWrap::NoWrap,
            max_lines: Some(1),
            overflow: MeasureTextOverflow::Ellipsis,
            pending_policy: PendingMeasurePolicy::RetainPrevious,
        };
        let lowered = lower_plain_text(&input, style.computed().inherited_text());
        let payload = &lowered.content().payload;

        assert_eq!(
            payload.style.font_families,
            vec![MeasureFontFamily::Named("Inter".into())]
        );
        assert_eq!(payload.style.font_size, 18.0);
        assert_eq!(payload.style.font_weight, 700);
        assert_eq!(payload.style.font_style, MeasureFontStyle::Italic);
        assert_eq!(
            payload.style.line_height,
            MeasureLineHeight::LogicalPixels(27.0)
        );
        assert_eq!(payload.style.letter_spacing, 1.0);
        assert_eq!(payload.locale.as_deref(), Some("ar"));
        assert_eq!(payload.direction, MeasureTextDirection::RightToLeft);
        assert_eq!(payload.wrap, MeasureTextWrap::NoWrap);
        assert_eq!(payload.max_lines, Some(1));
        assert_eq!(payload.overflow, MeasureTextOverflow::Ellipsis);
    }

    #[test]
    fn oblique_and_left_to_right_variants_are_lowered() {
        let style = resolved(vec![StyleDeclaration::new(
            StyleProperty::FontStyle,
            StyleValue::FontStyle(FontStyleValue::Oblique),
        )]);
        let mut input = PlainTextInput::new("variants");
        input.direction = MeasureTextDirection::LeftToRight;
        let lowered = lower_plain_text(&input, style.computed().inherited_text());

        assert_eq!(
            lowered.content().payload.style.font_style,
            MeasureFontStyle::Oblique
        );
        assert_eq!(
            lowered.content().payload.direction,
            MeasureTextDirection::LeftToRight
        );
    }

    #[test]
    fn named_and_hsl_text_paint_are_lowered_without_affecting_metrics() {
        let input = PlainTextInput::new("paint");
        let named = resolved(vec![StyleDeclaration::new(
            StyleProperty::Color,
            StyleValue::Color(ColorValue::Named("rebeccapurple".into())),
        )]);
        let named = lower_plain_text(&input, named.computed().inherited_text());
        assert_eq!(
            named.content().paint.foreground,
            PaintColor::Named("rebeccapurple".into())
        );

        let hsla = resolved(vec![StyleDeclaration::new(
            StyleProperty::Color,
            StyleValue::Color(ColorValue::Hsla {
                hue_degrees: StyleNumber::new(210.0),
                saturation: StyleNumber::new(80.0),
                lightness: StyleNumber::new(40.0),
                alpha: StyleNumber::new(0.5),
            }),
        )]);
        let hsla = lower_plain_text(&input, hsla.computed().inherited_text());
        assert_eq!(
            hsla.content().paint.foreground,
            PaintColor::Hsla {
                hue_degrees: 210.0,
                saturation: 80.0,
                lightness: 40.0,
                alpha: 0.5,
            }
        );
        assert_eq!(named.measurement(), hsla.measurement());
    }

    #[test]
    fn inherited_single_shadow_lowers_to_paint_without_changing_measurement() {
        let input = PlainTextInput::new("shadow");
        let plain = resolved(Vec::new());
        let shadowed = resolved(vec![StyleDeclaration::new(
            StyleProperty::TextShadow,
            StyleValue::TextShadow(TextShadowValue::Shadow {
                offset_x: LengthValue::Dimension {
                    value: StyleNumber::new(1.0),
                    unit: LengthUnit::Em,
                },
                offset_y: LengthValue::Dimension {
                    value: StyleNumber::new(2.0),
                    unit: LengthUnit::Px,
                },
                blur_radius: LengthValue::Dimension {
                    value: StyleNumber::new(3.0),
                    unit: LengthUnit::Px,
                },
                color: ColorValue::Named("red".into()),
            }),
        )]);
        let plain = lower_plain_text(&input, plain.computed().inherited_text());
        let shadowed = lower_plain_text(&input, shadowed.computed().inherited_text());
        assert_eq!(plain.measurement(), shadowed.measurement());
        assert_eq!(
            shadowed.content().paint.shadows,
            vec![TextShadow {
                offset_x: 14.0,
                offset_y: 2.0,
                blur_radius: 3.0,
                color: PaintColor::Named("red".into()),
            }]
        );
    }

    #[test]
    fn lynx_text_decoration_lowers_to_paint_without_changing_measurement() {
        let input = PlainTextInput::new("decoration");
        let plain = lower_plain_text(&input, resolved(Vec::new()).computed().inherited_text());
        let decorated = resolved(vec![StyleDeclaration::new(
            StyleProperty::TextDecoration,
            StyleValue::TextDecoration(TextDecorationValue {
                line: TextDecorationLineValue::LineThrough,
                style: TextDecorationStyleValue::Wavy,
                color: Some(ColorValue::Named("red".into())),
            }),
        )]);
        let decorated = lower_plain_text(&input, decorated.computed().inherited_text());
        assert_eq!(plain.measurement(), decorated.measurement());
        assert!(decorated.content().paint.decoration.lines.line_through);
        assert!(!decorated.content().paint.decoration.lines.underline);
        assert_eq!(
            decorated.content().paint.decoration.style,
            whisker_protocol::TextDecorationStyle::Wavy
        );
        assert_eq!(
            decorated.content().paint.decoration.color,
            PaintColor::Named("red".into())
        );
        for (style, expected) in [
            (
                TextDecorationStyleValue::Double,
                whisker_protocol::TextDecorationStyle::Double,
            ),
            (
                TextDecorationStyleValue::Dotted,
                whisker_protocol::TextDecorationStyle::Dotted,
            ),
            (
                TextDecorationStyleValue::Dashed,
                whisker_protocol::TextDecorationStyle::Dashed,
            ),
        ] {
            let resolved = resolved(vec![StyleDeclaration::new(
                StyleProperty::TextDecoration,
                StyleValue::TextDecoration(TextDecorationValue {
                    line: TextDecorationLineValue::Underline,
                    style,
                    color: None,
                }),
            )]);
            assert_eq!(
                lower_plain_text(&input, resolved.computed().inherited_text())
                    .content()
                    .paint
                    .decoration
                    .style,
                expected
            );
        }
    }
}
