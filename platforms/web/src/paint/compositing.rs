use whisker_protocol::Visibility;

use crate::{WebError, set_style};

pub(crate) fn apply_opacity(element: &web_sys::Element, opacity: f32) -> Result<(), WebError> {
    set_style(element, "opacity", &opacity.to_string())
}

pub(crate) fn apply_visibility(
    element: &web_sys::Element,
    visibility: Visibility,
) -> Result<(), WebError> {
    set_style(
        element,
        "visibility",
        if visibility == Visibility::Visible {
            "visible"
        } else {
            "hidden"
        },
    )
}

pub(crate) fn apply_z_order(element: &web_sys::Element, z_order: i32) -> Result<(), WebError> {
    set_style(element, "z-index", &z_order.to_string())
}
