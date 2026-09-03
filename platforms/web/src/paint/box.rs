use whisker_protocol::{BorderLineStyle, BoxPaint, PaintCornerRadius, PaintLengthPercentage};

use super::color::css_color;
use crate::{WebError, set_style};

pub(crate) fn apply(
    element: &web_sys::Element,
    paint: &BoxPaint,
    border_widths: [f32; 4],
) -> Result<(), WebError> {
    set_style(
        element,
        "background-color",
        &css_color(&paint.background_color),
    )?;
    apply_border_widths(element, border_widths)?;
    let colors = &paint.border_colors;
    set_style(element, "border-top-color", &css_color(&colors.top))?;
    set_style(element, "border-right-color", &css_color(&colors.right))?;
    set_style(element, "border-bottom-color", &css_color(&colors.bottom))?;
    set_style(element, "border-left-color", &css_color(&colors.left))?;
    let styles = &paint.border_styles;
    set_style(element, "border-top-style", border_style(styles.top))?;
    set_style(element, "border-right-style", border_style(styles.right))?;
    set_style(element, "border-bottom-style", border_style(styles.bottom))?;
    set_style(element, "border-left-style", border_style(styles.left))?;
    let radii = &paint.border_radii;
    set_style(
        element,
        "border-top-left-radius",
        &corner_radius(radii.top_left),
    )?;
    set_style(
        element,
        "border-top-right-radius",
        &corner_radius(radii.top_right),
    )?;
    set_style(
        element,
        "border-bottom-right-radius",
        &corner_radius(radii.bottom_right),
    )?;
    set_style(
        element,
        "border-bottom-left-radius",
        &corner_radius(radii.bottom_left),
    )?;
    Ok(())
}

pub(crate) fn apply_border_widths(
    element: &web_sys::Element,
    widths: [f32; 4],
) -> Result<(), WebError> {
    set_style(element, "border-top-width", &format!("{}px", widths[0]))?;
    set_style(element, "border-right-width", &format!("{}px", widths[1]))?;
    set_style(element, "border-bottom-width", &format!("{}px", widths[2]))?;
    set_style(element, "border-left-width", &format!("{}px", widths[3]))
}

fn length(value: PaintLengthPercentage) -> String {
    if value.fraction == 0.0 {
        format!("{}px", value.length)
    } else {
        format!("calc({}px + {}%)", value.length, value.fraction * 100.0)
    }
}

fn corner_radius(value: PaintCornerRadius) -> String {
    format!("{} {}", length(value.horizontal), length(value.vertical))
}

fn border_style(value: BorderLineStyle) -> &'static str {
    match value {
        BorderLineStyle::None => "none",
        BorderLineStyle::Hidden => "hidden",
        BorderLineStyle::Solid => "solid",
        BorderLineStyle::Dashed => "dashed",
        BorderLineStyle::Dotted => "dotted",
        BorderLineStyle::Double => "double",
        BorderLineStyle::Groove => "groove",
        BorderLineStyle::Ridge => "ridge",
        BorderLineStyle::Inset => "inset",
        BorderLineStyle::Outset => "outset",
    }
}
