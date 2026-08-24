use whisker_protocol::{
    MeasureFontFamily, MeasureFontStyle, MeasureLineHeight, MeasureTextDirection, MeasureTextWrap,
    TextContent, TextMeasurePayload,
};

use super::color::css_color;
use crate::{WebError, px, set_style};

pub(crate) fn apply(element: &web_sys::Element, content: &TextContent) -> Result<(), WebError> {
    apply_metrics_style(element, &content.payload)?;
    set_style(element, "color", &css_color(&content.paint.foreground))?;
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
        "white-space",
        if text.wrap == MeasureTextWrap::NoWrap {
            "nowrap"
        } else {
            "normal"
        },
    )?;
    set_style(
        element,
        "direction",
        match text.direction {
            MeasureTextDirection::Auto => "initial",
            MeasureTextDirection::LeftToRight => "ltr",
            MeasureTextDirection::RightToLeft => "rtl",
        },
    )?;
    set_style(element, "overflow-wrap", "anywhere")
}
