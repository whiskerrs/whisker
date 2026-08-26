use whisker_protocol::{
    MeasureFontFamily, MeasureFontStyle, MeasureLineHeight, MeasureTextDirection,
    MeasureTextOverflow, MeasureTextWordBreak, MeasureTextWrap, TextContent, TextMeasurePayload,
};

use super::color::css_color;
use crate::{WebError, px, set_style};

pub(crate) fn apply(element: &web_sys::Element, content: &TextContent) -> Result<(), WebError> {
    apply_metrics_style(element, &content.payload)?;
    set_style(
        element,
        "text-align",
        match content.payload.alignment {
            whisker_protocol::MeasureTextAlignment::Start => "start",
            whisker_protocol::MeasureTextAlignment::End => "end",
            whisker_protocol::MeasureTextAlignment::Left => "left",
            whisker_protocol::MeasureTextAlignment::Right => "right",
            whisker_protocol::MeasureTextAlignment::Center => "center",
        },
    )?;
    set_style(element, "color", &css_color(&content.paint.foreground))?;
    let decoration = &content.paint.decoration;
    let line = if decoration.lines.underline {
        "underline"
    } else if decoration.lines.line_through {
        "line-through"
    } else {
        "none"
    };
    set_style(element, "text-decoration-line", line)?;
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
        "text-shadow",
        &content.paint.shadows.first().map_or_else(
            || "none".to_string(),
            |shadow| {
                format!(
                    "{} {} {} {}",
                    px(shadow.offset_x),
                    px(shadow.offset_y),
                    px(shadow.blur_radius),
                    css_color(&shadow.color),
                )
            },
        ),
    )?;
    element.set_text_content(Some(&content.payload.text));
    Ok(())
}

pub(crate) fn apply_metrics_style(
    element: &web_sys::Element,
    text: &TextMeasurePayload,
) -> Result<(), WebError> {
    let families = text
        .style
        .font_families
        .iter()
        .map(|family| match family {
            MeasureFontFamily::System => "system-ui".to_string(),
            MeasureFontFamily::Named(name) => format!("{name:?}"),
        })
        .collect::<Vec<_>>()
        .join(", ");
    set_style(element, "font-family", &families)?;
    set_style(element, "font-size", &px(text.style.font_size))?;
    set_style(element, "font-weight", &text.style.font_weight.to_string())?;
    set_style(
        element,
        "font-style",
        match text.style.font_style {
            MeasureFontStyle::Normal => "normal",
            MeasureFontStyle::Italic => "italic",
            MeasureFontStyle::Oblique => "oblique",
        },
    )?;
    set_style(
        element,
        "line-height",
        &match text.style.line_height {
            MeasureLineHeight::Normal => "normal".to_string(),
            MeasureLineHeight::LogicalPixels(value) => px(value),
        },
    )?;
    set_style(element, "letter-spacing", &px(text.style.letter_spacing))?;
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
    if let Some(max_lines) = text.max_lines {
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
    set_style(element, "overflow-wrap", "normal")
}
