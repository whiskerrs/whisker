use std::fmt::Write as _;

use whisker_protocol::{
    FontOpticalSizing, MeasureFontFamily, MeasureFontStyle, MeasureLineHeight,
    MeasureTextDirection, MeasureTextOverflow, MeasureTextWordBreak, MeasureTextWrap, TextContent,
    TextMeasurePayload,
};

use super::color::css_color;
use crate::{WebError, px, set_style};

#[cfg(test)]
pub(crate) fn apply(element: &web_sys::Element, content: &TextContent) -> Result<(), WebError> {
    apply_prepared(element, content, None)
}

pub(crate) fn apply_prepared(
    element: &web_sys::Element,
    content: &TextContent,
    prepared: Option<&web_sys::Element>,
) -> Result<(), WebError> {
    let selection = crate::text_element::before_update(element, &content.payload.text)?;
    element
        .set_attribute(
            "data-whisker-text-revision",
            &content
                .prepared_content
                .map_or(0, |id| id.get())
                .to_string(),
        )
        .map_err(|error| crate::js_error("set paragraph revision", error))?;
    if !content.payload.runs.is_empty() || !content.payload.attachments.is_empty() {
        element
            .set_attribute("data-whisker-text-source", &content.payload.text)
            .map_err(|error| crate::js_error("set paragraph source", error))?;
    } else {
        element
            .remove_attribute("data-whisker-text-source")
            .map_err(|error| crate::js_error("clear paragraph source", error))?;
    }
    apply_metrics_style(element, &content.payload)?;
    set_style(element, "color", &css_color(&content.paint.foreground))?;
    apply_decoration(element, &content.paint.decoration)?;
    set_style(
        element,
        "text-shadow",
        &text_shadows(&content.paint.shadows),
    )?;
    if let Some(prepared) = prepared {
        if prepared.parent_element().as_ref() != Some(element) {
            element.set_text_content(None);
            element
                .append_child(prepared)
                .map_err(|error| crate::js_error("present prepared paragraph", error))?;
        }
        let children = prepared.children();
        for index in 0..children.length() {
            let Some(wrapper) = children.item(index) else {
                continue;
            };
            let Some(start) = wrapper
                .get_attribute("data-start")
                .and_then(|value| value.parse::<u32>().ok())
            else {
                continue;
            };
            let Some(span) = wrapper.first_element_child() else {
                continue;
            };
            if let Some(paint) = content
                .runs
                .iter()
                .find(|paint| paint.range.start <= start && start < paint.range.end)
            {
                apply_run_paint(&span, paint)?;
            }
        }
    } else {
        apply_content(element, &content.payload, &content.runs)?;
    }
    if let Some(geometry) = &content.paragraph {
        if let Some(line) = geometry.lines.last().filter(|line| line.ellipsis_count > 0) {
            let end = whisker_protocol::TextRange {
                start: 0,
                end: line.range.end - line.ellipsis_count,
            }
            .to_utf8(&content.payload.text)
            .ok_or_else(|| WebError("invalid displayed text range".into()))?
            .end;
            crate::measure::paragraph::ParagraphDom::read(element, &content.payload)?.show_prefix(
                end,
                content.payload.overflow == MeasureTextOverflow::Ellipsis,
            )?;
        }
    }
    crate::text_element::after_update(element, selection)?;
    Ok(())
}

pub(crate) fn apply_content(
    element: &web_sys::Element,
    payload: &TextMeasurePayload,
    paints: &[whisker_protocol::TextPaintRun],
) -> Result<(), WebError> {
    if payload.runs.is_empty() && payload.attachments.is_empty() {
        element.set_text_content(Some(&payload.text));
        return Ok(());
    }
    let document = element
        .owner_document()
        .ok_or_else(|| WebError("text has no document".into()))?;
    element.set_text_content(None);
    let mut boundaries = vec![0, payload.text.len() as u32];
    for run in &payload.runs {
        boundaries.extend([run.range.start, run.range.end]);
    }
    for attachment in &payload.attachments {
        boundaries.extend([attachment.range.start, attachment.range.end]);
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    let reference = document
        .create_element("span")
        .map_err(|error| crate::js_error("create paragraph reference", error))?;
    reference
        .set_attribute("data-whisker-paragraph", "")
        .map_err(|error| crate::js_error("identify paragraph reference", error))?;
    element
        .append_child(&reference)
        .map_err(|error| crate::js_error("append paragraph reference", error))?;
    for range in boundaries.windows(2) {
        let wrapper = document
            .create_element("span")
            .map_err(|error| crate::js_error("create run reference", error))?;
        wrapper
            .set_attribute("data-start", &range[0].to_string())
            .map_err(|error| crate::js_error("set source offset", error))?;
        wrapper
            .set_attribute("data-end", &range[1].to_string())
            .map_err(|error| crate::js_error("set source offset", error))?;
        let span = document
            .create_element("span")
            .map_err(|error| crate::js_error("create text span", error))?;
        if let Some((index, run)) = payload
            .runs
            .iter()
            .enumerate()
            .find(|(_, run)| run.range.start <= range[0] && range[1] <= run.range.end)
        {
            apply_font_style(&span, &run.style)?;
            let alignment = match run.alignment {
                whisker_protocol::InlineAlignment::Baseline => "baseline".into(),
                whisker_protocol::InlineAlignment::Top => "top".into(),
                whisker_protocol::InlineAlignment::Middle => "middle".into(),
                whisker_protocol::InlineAlignment::Bottom => "bottom".into(),
                whisker_protocol::InlineAlignment::Offset(shift) => px(shift),
            };
            set_style(&span, "vertical-align", &alignment)?;
            if let Some(paint) = paints.get(index).filter(|paint| paint.range == run.range) {
                apply_run_paint(&span, paint)?;
            }
        }
        if let Some(attachment) = payload
            .attachments
            .iter()
            .find(|attachment| attachment.range.start == range[0])
        {
            set_style(&span, "display", "inline-block")?;
            set_style(&span, "width", &px(attachment.size.width))?;
            set_style(&span, "height", &px(attachment.size.height))?;
            set_style(&span, "overflow", "hidden")?;
            let alignment = match attachment.alignment {
                whisker_protocol::InlineAlignment::Baseline => {
                    px(attachment.baseline - attachment.size.height)
                }
                whisker_protocol::InlineAlignment::Top => "top".into(),
                whisker_protocol::InlineAlignment::Middle => "middle".into(),
                whisker_protocol::InlineAlignment::Bottom => "bottom".into(),
                whisker_protocol::InlineAlignment::Offset(shift) => {
                    px(attachment.baseline - attachment.size.height + shift)
                }
            };
            set_style(&span, "vertical-align", &alignment)?;
            if let Some(label) = &attachment.label {
                span.set_attribute("data-whisker-copy-label", label)
                    .map_err(|error| crate::js_error("set inline copy label", error))?;
            }
            span.set_attribute(
                "data-whisker-inline-node",
                &attachment.node.get().to_string(),
            )
            .map_err(|error| crate::js_error("set inline identity", error))?;
        } else {
            span.set_text_content(payload.text.get(range[0] as usize..range[1] as usize));
        }
        wrapper
            .append_child(&span)
            .map_err(|error| crate::js_error("append styled span", error))?;
        reference
            .append_child(&wrapper)
            .map_err(|error| crate::js_error("append text span", error))?;
    }
    Ok(())
}

fn apply_run_paint(
    span: &web_sys::Element,
    paint: &whisker_protocol::TextPaintRun,
) -> Result<(), WebError> {
    for (name, value) in [
        (
            "data-whisker-text-action",
            paint.action.map(|span| span.get().to_string()),
        ),
        ("role", paint.action.map(|_| "button".into())),
        ("tabindex", paint.action.map(|_| "0".into())),
    ] {
        if let Some(value) = value {
            span.set_attribute(name, &value)
        } else {
            span.remove_attribute(name)
        }
        .map_err(|error| crate::js_error("set inline accessibility action", error))?;
    }

    set_style(span, "color", &css_color(&paint.paint.foreground))?;
    apply_decoration(span, &paint.paint.decoration)?;
    set_style(span, "text-shadow", &text_shadows(&paint.paint.shadows))?;
    set_style(
        span,
        "background-color",
        &paint
            .background
            .as_ref()
            .map(css_color)
            .unwrap_or_else(|| "transparent".into()),
    )?;
    for (property, radius) in [
        ("border-top-left-radius", paint.background_radii.top_left),
        ("border-top-right-radius", paint.background_radii.top_right),
        (
            "border-bottom-right-radius",
            paint.background_radii.bottom_right,
        ),
        (
            "border-bottom-left-radius",
            paint.background_radii.bottom_left,
        ),
    ] {
        set_style(
            span,
            property,
            &format!(
                "calc({} + {}%) calc({} + {}%)",
                px(radius.horizontal.length),
                radius.horizontal.fraction * 100.0,
                px(radius.vertical.length),
                radius.vertical.fraction * 100.0
            ),
        )?;
    }
    set_style(span, "box-decoration-break", "clone")?;
    set_style(span, "-webkit-box-decoration-break", "clone")?;
    span.set_attribute("data-whisker-text-span", &paint.span.get().to_string())
        .map_err(|error| crate::js_error("set text span identity", error))?;
    Ok(())
}

fn apply_decoration(
    element: &web_sys::Element,
    decoration: &whisker_protocol::TextDecoration,
) -> Result<(), WebError> {
    set_style(
        element,
        "text-decoration-line",
        &decoration_lines(decoration.lines),
    )?;
    set_style(
        element,
        "text-decoration-style",
        match decoration.style {
            whisker_protocol::TextDecorationStyle::Solid => "solid",
            whisker_protocol::TextDecorationStyle::Double => "double",
            whisker_protocol::TextDecorationStyle::Dotted => "dotted",
            whisker_protocol::TextDecorationStyle::Dashed => "dashed",
            whisker_protocol::TextDecorationStyle::Wavy => "wavy",
        },
    )?;
    set_style(
        element,
        "text-decoration-color",
        &css_color(&decoration.color),
    )?;
    set_style(
        element,
        "text-decoration-thickness",
        &match decoration.thickness {
            whisker_protocol::TextDecorationThickness::Auto => "auto".to_owned(),
            whisker_protocol::TextDecorationThickness::FromFont => "from-font".to_owned(),
            whisker_protocol::TextDecorationThickness::Length(value) => px(value),
        },
    )?;
    Ok(())
}

fn decoration_lines(lines: whisker_protocol::TextDecorationLines) -> String {
    let mut value = String::new();
    for (enabled, keyword) in [
        (lines.underline, "underline"),
        (lines.overline, "overline"),
        (lines.line_through, "line-through"),
    ] {
        if enabled {
            if !value.is_empty() {
                value.push(' ');
            }
            value.push_str(keyword);
        }
    }
    if value.is_empty() {
        value.push_str("none");
    }
    value
}

fn text_shadows(shadows: &[whisker_protocol::TextShadow]) -> String {
    if shadows.is_empty() {
        return "none".to_owned();
    }
    let mut value = String::new();
    for shadow in shadows {
        if !value.is_empty() {
            value.push_str(", ");
        }
        write!(
            value,
            "{} {} {} {}",
            px(shadow.offset_x),
            px(shadow.offset_y),
            px(shadow.blur_radius),
            css_color(&shadow.color),
        )
        .expect("writing to String cannot fail");
    }
    value
}

pub(crate) fn apply_metrics_style(
    element: &web_sys::Element,
    text: &TextMeasurePayload,
) -> Result<(), WebError> {
    apply_font_style(element, &text.style)?;
    set_style(
        element,
        "text-indent",
        &format!(
            "calc({} + {}%)",
            px(text.indent.logical_pixels),
            text.indent.percentage,
        ),
    )?;
    set_style(
        element,
        "white-space",
        if text.wrap == MeasureTextWrap::NoWrap {
            "nowrap"
        } else if text.wrap == MeasureTextWrap::PreserveWhitespace {
            "pre-wrap"
        } else {
            "normal"
        },
    )?;
    set_style(
        element,
        "word-break",
        match text.word_break {
            MeasureTextWordBreak::Normal => "normal",
            MeasureTextWordBreak::BreakAll => "break-all",
            MeasureTextWordBreak::KeepAll => "keep-all",
        },
    )?;
    set_style(
        element,
        "text-overflow",
        match text.overflow {
            MeasureTextOverflow::Clip => "clip",
            MeasureTextOverflow::Ellipsis => "ellipsis",
        },
    )?;
    set_style(element, "overflow", "hidden")?;
    if let Some(max_lines) = text
        .max_lines
        .filter(|_| text.runs.is_empty() && text.attachments.is_empty())
    {
        set_style(element, "display", "-webkit-box")?;
        set_style(element, "-webkit-box-orient", "vertical")?;
        set_style(element, "-webkit-line-clamp", &max_lines.to_string())?;
    } else {
        set_style(element, "display", "block")?;
        set_style(element, "-webkit-box-orient", "initial")?;
        set_style(element, "-webkit-line-clamp", "initial")?;
    }
    set_style(
        element,
        "direction",
        match text.direction {
            MeasureTextDirection::Auto => "initial",
            MeasureTextDirection::LeftToRight => "ltr",
            MeasureTextDirection::RightToLeft => "rtl",
        },
    )?;
    set_style(
        element,
        "text-align",
        match text.alignment {
            whisker_protocol::MeasureTextAlignment::Start => "start",
            whisker_protocol::MeasureTextAlignment::End => "end",
            whisker_protocol::MeasureTextAlignment::Left => "left",
            whisker_protocol::MeasureTextAlignment::Right => "right",
            whisker_protocol::MeasureTextAlignment::Center => "center",
        },
    )?;
    set_style(element, "overflow-wrap", "normal")
}

fn apply_font_style(
    element: &web_sys::Element,
    style: &whisker_protocol::TextMeasureStyle,
) -> Result<(), WebError> {
    let families = style
        .font_families
        .iter()
        .map(|family| match family {
            MeasureFontFamily::System => "system-ui".to_string(),
            MeasureFontFamily::Named(name) => format!("{name:?}"),
        })
        .collect::<Vec<_>>()
        .join(", ");
    set_style(element, "font-family", &families)?;
    set_style(element, "font-size", &px(style.font_size))?;
    set_style(element, "font-weight", &style.font_weight.to_string())?;
    set_style(
        element,
        "font-style",
        match style.font_style {
            MeasureFontStyle::Normal => "normal",
            MeasureFontStyle::Italic => "italic",
            MeasureFontStyle::Oblique => "oblique",
        },
    )?;
    set_style(
        element,
        "line-height",
        &match style.line_height {
            MeasureLineHeight::Normal => "normal".to_string(),
            MeasureLineHeight::LogicalPixels(value) => px(value),
        },
    )?;
    set_style(element, "letter-spacing", &px(style.letter_spacing))?;
    set_style(
        element,
        "font-feature-settings",
        &settings_css(&style.features, |setting| {
            (setting.tag.get(), setting.value.to_string())
        }),
    )?;
    set_style(
        element,
        "font-variation-settings",
        &settings_css(&style.variations, |setting| {
            (setting.tag.get(), setting.value.to_string())
        }),
    )?;
    set_style(
        element,
        "font-optical-sizing",
        match style.optical_sizing {
            FontOpticalSizing::Auto => "auto",
            FontOpticalSizing::None => "none",
        },
    )?;
    Ok(())
}

fn settings_css<T>(values: &[T], map: impl Fn(&T) -> ([u8; 4], String)) -> String {
    if values.is_empty() {
        return "normal".to_string();
    }
    values
        .iter()
        .map(|value| {
            let (tag, value) = map(value);
            format!(
                "'{}' {value}",
                String::from_utf8(tag.to_vec()).expect("protocol validates OpenType tags")
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use wasm_bindgen_test::wasm_bindgen_test;
    use whisker_protocol::{PaintColor, TextDecorationLines, TextShadow};

    use super::*;

    #[wasm_bindgen_test]
    fn combines_decoration_lines_in_css_order() {
        assert_eq!(
            decoration_lines(TextDecorationLines {
                underline: true,
                overline: true,
                line_through: true,
            }),
            "underline overline line-through"
        );
        assert_eq!(decoration_lines(TextDecorationLines::default()), "none");
    }

    #[wasm_bindgen_test]
    fn serializes_every_text_shadow_in_paint_order() {
        let shadows = [
            TextShadow {
                offset_x: 1.0,
                offset_y: 2.0,
                blur_radius: 3.0,
                color: PaintColor::Srgba {
                    red: 255,
                    green: 0,
                    blue: 0,
                    alpha: 1.0,
                },
            },
            TextShadow {
                offset_x: -1.0,
                offset_y: 0.0,
                blur_radius: 4.0,
                color: PaintColor::Srgba {
                    red: 0,
                    green: 0,
                    blue: 255,
                    alpha: 0.5,
                },
            },
        ];
        assert_eq!(
            text_shadows(&shadows),
            "1px 2px 3px rgba(255, 0, 0, 1), -1px 0px 4px rgba(0, 0, 255, 0.5)"
        );
    }
}
