use std::fmt::Write as _;

use whisker_protocol::{
    FontOpticalSizing, MeasureFontFamily, MeasureFontStyle, MeasureLineHeight,
    MeasureTextDirection, MeasureTextOverflow, MeasureTextWordBreak, MeasureTextWrap, TextContent,
    TextMeasurePayload,
};

use super::color::css_color;
use crate::{WebError, px, set_style};

pub(crate) fn apply(element: &web_sys::Element, content: &TextContent) -> Result<(), WebError> {
    apply_metrics_style(element, &content.payload)?;
    set_style(element, "color", &css_color(&content.paint.foreground))?;
    let decoration = &content.paint.decoration;
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
    set_style(
        element,
        "text-shadow",
        &text_shadows(&content.paint.shadows),
    )?;
    element.set_text_content(Some(&content.payload.text));
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
        "font-feature-settings",
        &settings_css(&text.style.features, |setting| {
            (setting.tag.get(), setting.value.to_string())
        }),
    )?;
    set_style(
        element,
        "font-variation-settings",
        &settings_css(&text.style.variations, |setting| {
            (setting.tag.get(), setting.value.to_string())
        }),
    )?;
    set_style(
        element,
        "font-optical-sizing",
        match text.style.optical_sizing {
            FontOpticalSizing::Auto => "auto",
            FontOpticalSizing::None => "none",
        },
    )?;
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
