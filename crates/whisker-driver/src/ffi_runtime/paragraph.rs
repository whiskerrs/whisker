use super::*;
use whisker_engine::whisker_protocol::{FontOpticalSizing, TextPaintRun};

#[derive(Default)]
pub(super) struct MobileParagraphs {
    arena: RawValueArena,
    // Published FFI pointers must survive subsequent pushes.
    #[allow(clippy::vec_box)]
    values: Vec<Box<WhiskerValueRaw>>,
}

impl MobileParagraphs {
    pub(super) fn push(
        &mut self,
        payload: &TextMeasurePayload,
        paints: &[TextPaintRun],
        geometry: Option<&whisker_engine::whisker_protocol::ParagraphMetrics>,
    ) -> *const WhiskerValueRaw {
        if payload.runs.is_empty() && payload.attachments.is_empty() {
            return std::ptr::null();
        }
        let value = paragraph_value(payload, paints, geometry);
        self.values.push(Box::new(self.arena.encode(&value)));
        self.values.last().expect("retained paragraph").as_ref()
    }
}

fn paragraph_value(
    payload: &TextMeasurePayload,
    paints: &[TextPaintRun],
    geometry: Option<&whisker_engine::whisker_protocol::ParagraphMetrics>,
) -> WhiskerValue {
    let mut utf16 = 0;
    let mut offsets = payload
        .text
        .char_indices()
        .map(|(byte, character)| {
            let entry = (byte as u32, utf16);
            utf16 += character.len_utf16() as u32;
            entry
        })
        .collect::<Vec<_>>();
    offsets.push((payload.text.len() as u32, utf16));
    let offset = |byte| {
        let index = offsets
            .binary_search_by_key(&byte, |entry| entry.0)
            .expect("validated UTF-8 range");
        WhiskerValue::Int(i64::from(offsets[index].1))
    };
    let runs = payload
        .runs
        .iter()
        .enumerate()
        .map(|(index, run)| {
            let style = &run.style;
            let (alignment, shift) = inline_alignment(run.alignment);
            let paint = paints.get(index).filter(|paint| paint.range == run.range);
            WhiskerValue::map([
                (
                    "action",
                    paint
                        .and_then(|paint| paint.action)
                        .map_or(WhiskerValue::Null, |span| {
                            WhiskerValue::Int(span.get() as i64)
                        }),
                ),
                ("start", offset(run.range.start)),
                ("end", offset(run.range.end)),
                ("alignment", WhiskerValue::Int(alignment)),
                ("shift", number(shift)),
                (
                    "span",
                    paint.map_or(WhiskerValue::Null, |paint| {
                        WhiskerValue::Int(paint.span.get() as i64)
                    }),
                ),
                (
                    "families",
                    WhiskerValue::Array(
                        style
                            .font_families
                            .iter()
                            .map(|family| {
                                WhiskerValue::String(match family {
                                    MeasureFontFamily::System => "system".to_owned(),
                                    MeasureFontFamily::Named(name) => name.clone(),
                                })
                            })
                            .collect(),
                    ),
                ),
                ("size", number(style.font_size)),
                ("weight", WhiskerValue::Int(i64::from(style.font_weight))),
                (
                    "italic",
                    WhiskerValue::Bool(style.font_style != MeasureFontStyle::Normal),
                ),
                ("spacing", number(style.letter_spacing)),
                (
                    "features",
                    WhiskerValue::Map(
                        style
                            .features
                            .iter()
                            .map(|feature| {
                                (
                                    String::from_utf8(feature.tag.get().to_vec())
                                        .expect("ASCII tag"),
                                    WhiskerValue::Int(i64::from(feature.value)),
                                )
                            })
                            .collect(),
                    ),
                ),
                (
                    "variations",
                    WhiskerValue::Map(
                        style
                            .variations
                            .iter()
                            .map(|variation| {
                                (
                                    String::from_utf8(variation.tag.get().to_vec())
                                        .expect("ASCII tag"),
                                    number(variation.value),
                                )
                            })
                            .collect(),
                    ),
                ),
                (
                    "optical",
                    WhiskerValue::Bool(style.optical_sizing == FontOpticalSizing::Auto),
                ),
                ("paint", paint.map_or(WhiskerValue::Null, paint_value)),
            ])
        })
        .collect();
    WhiskerValue::map([
        ("version", WhiskerValue::Int(1)),
        (
            "visibleEnd",
            geometry
                .and_then(|geometry| geometry.lines.last())
                .filter(|line| line.ellipsis_count > 0)
                .map_or(WhiskerValue::Null, |line| {
                    WhiskerValue::Int(i64::from(line.range.end - line.ellipsis_count))
                }),
        ),
        ("runs", WhiskerValue::Array(runs)),
        (
            "attachments",
            WhiskerValue::Array(
                payload
                    .attachments
                    .iter()
                    .map(|attachment| {
                        let (alignment, shift) = inline_alignment(attachment.alignment);
                        WhiskerValue::map([
                            ("node", WhiskerValue::Int(attachment.node.get() as i64)),
                            ("truncation", WhiskerValue::Bool(attachment.truncation)),
                            (
                                "label",
                                attachment
                                    .label
                                    .as_ref()
                                    .map_or(WhiskerValue::Null, |label| {
                                        WhiskerValue::String(label.clone())
                                    }),
                            ),
                            ("start", offset(attachment.range.start)),
                            ("end", offset(attachment.range.end)),
                            ("width", number(attachment.size.width)),
                            ("height", number(attachment.size.height)),
                            ("baseline", number(attachment.baseline)),
                            ("alignment", WhiskerValue::Int(alignment)),
                            ("shift", number(shift)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

fn number(value: f32) -> WhiskerValue {
    WhiskerValue::Float(f64::from(value))
}

fn paint_value(run: &TextPaintRun) -> WhiskerValue {
    let decoration = &run.paint.decoration;
    WhiskerValue::map([
        ("color", color_value(&run.paint.foreground)),
        (
            "background",
            run.background
                .as_ref()
                .map_or(WhiskerValue::Null, color_value),
        ),
        (
            "radii",
            WhiskerValue::Array(
                [
                    run.background_radii.top_left,
                    run.background_radii.top_right,
                    run.background_radii.bottom_right,
                    run.background_radii.bottom_left,
                ]
                .into_iter()
                .map(|radius| {
                    WhiskerValue::Array(
                        [
                            radius.horizontal.length,
                            radius.horizontal.fraction,
                            radius.vertical.length,
                            radius.vertical.fraction,
                        ]
                        .into_iter()
                        .map(number)
                        .collect(),
                    )
                })
                .collect(),
            ),
        ),
        ("underline", WhiskerValue::Bool(decoration.lines.underline)),
        ("strike", WhiskerValue::Bool(decoration.lines.line_through)),
        ("decorationColor", color_value(&decoration.color)),
        (
            "decorationStyle",
            WhiskerValue::Int(decoration.style as i64),
        ),
        (
            "shadows",
            WhiskerValue::Array(
                run.paint
                    .shadows
                    .iter()
                    .map(|shadow| {
                        WhiskerValue::map([
                            ("x", number(shadow.offset_x)),
                            ("y", number(shadow.offset_y)),
                            ("blur", number(shadow.blur_radius)),
                            ("color", color_value(&shadow.color)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

fn color_value(color: &PaintColor) -> WhiskerValue {
    let (red, green, blue, alpha) = match color {
        PaintColor::Named(name) => return WhiskerValue::String(name.clone()),
        PaintColor::Srgba {
            red,
            green,
            blue,
            alpha,
        } => (*red, *green, *blue, *alpha),
        PaintColor::Hsla {
            hue_degrees,
            saturation,
            lightness,
            alpha,
        } => {
            let (red, green, blue) =
                super::frame::hsl_to_rgb(*hue_degrees, *saturation / 100.0, *lightness / 100.0);
            (red, green, blue, *alpha)
        }
    };
    WhiskerValue::Array(vec![
        number(f32::from(red)),
        number(f32::from(green)),
        number(f32::from(blue)),
        number(alpha),
    ])
}

fn inline_alignment(alignment: whisker_engine::whisker_protocol::InlineAlignment) -> (i64, f32) {
    use whisker_engine::whisker_protocol::InlineAlignment::*;
    match alignment {
        Baseline => (0, 0.0),
        Top => (1, 0.0),
        Middle => (2, 0.0),
        Bottom => (3, 0.0),
        Offset(shift) => (4, shift),
    }
}
