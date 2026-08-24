use whisker_protocol::Transform;

use crate::{WebError, set_style};

pub(crate) fn apply(element: &web_sys::Element, transform: Transform) -> Result<(), WebError> {
    let value = transform
        .0
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    set_style(element, "transform", &format!("matrix3d({value})"))
}
