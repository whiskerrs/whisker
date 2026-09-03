use whisker_protocol::{BoxClip, OverflowClip};

use crate::{WebError, set_style};

pub(crate) fn apply(
    element: &web_sys::Element,
    clip: BoxClip,
    scroll_content: bool,
) -> Result<(), WebError> {
    if scroll_content {
        return Ok(());
    }
    let [horizontal, vertical] = overflow(clip);
    set_style(element, "overflow-x", horizontal)?;
    set_style(element, "overflow-y", vertical)
}

fn overflow(clip: BoxClip) -> [&'static str; 2] {
    [overflow_axis(clip.horizontal), overflow_axis(clip.vertical)]
}

fn overflow_axis(value: OverflowClip) -> &'static str {
    match value {
        OverflowClip::Hidden => "clip",
        OverflowClip::Visible => "visible",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_elements_follow_the_protocol_clip() {
        let mixed = BoxClip {
            horizontal: OverflowClip::Hidden,
            vertical: OverflowClip::Visible,
        };
        assert_eq!(overflow(mixed), ["clip", "visible"]);
    }
}
