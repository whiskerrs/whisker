use whisker_protocol::{BoxClip, OverflowClip};

use crate::{WebError, set_style};

pub(crate) fn apply(
    element: &web_sys::Element,
    clip: BoxClip,
    scroll_content: bool,
) -> Result<(), WebError> {
    let [horizontal, vertical] = overflow(clip, scroll_content);
    set_style(element, "overflow-x", horizontal)?;
    set_style(element, "overflow-y", vertical)
}

fn overflow(clip: BoxClip, scroll_content: bool) -> [&'static str; 2] {
    if scroll_content {
        // A built-in ScrollView is a vertical native scroll container. Its
        // base style clips overflowing content, but that clip must not replace
        // the Host's scrolling mechanism with CSS `overflow: hidden`.
        return ["hidden", "auto"];
    }
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
    fn scroll_container_keeps_vertical_native_scrolling_enabled() {
        let hidden = BoxClip {
            horizontal: OverflowClip::Hidden,
            vertical: OverflowClip::Hidden,
        };
        assert_eq!(overflow(hidden, true), ["hidden", "auto"]);
    }

    #[test]
    fn ordinary_elements_follow_the_protocol_clip() {
        let mixed = BoxClip {
            horizontal: OverflowClip::Hidden,
            vertical: OverflowClip::Visible,
        };
        assert_eq!(overflow(mixed, false), ["clip", "visible"]);
    }
}
