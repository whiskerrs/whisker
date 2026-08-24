use whisker_protocol::{BoxClip, OverflowClip};

use crate::{WebError, set_style};

pub(crate) fn apply(
    element: &web_sys::Element,
    clip: BoxClip,
    scroll_content: bool,
) -> Result<(), WebError> {
    set_style(
        element,
        "overflow-x",
        overflow(clip.horizontal, scroll_content),
    )?;
    set_style(
        element,
        "overflow-y",
        overflow(clip.vertical, scroll_content),
    )
}

fn overflow(value: OverflowClip, scroll_content: bool) -> &'static str {
    match (value, scroll_content) {
        (OverflowClip::Hidden, true) => "hidden",
        (OverflowClip::Visible, true) => "auto",
        (OverflowClip::Hidden, false) => "clip",
        (OverflowClip::Visible, false) => "visible",
    }
}
