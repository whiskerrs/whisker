use whisker_protocol::VisualEffects;

use super::color::css_color;
use crate::{WebError, set_style};

pub(crate) fn supports(effects: &VisualEffects) -> bool {
    let mut remainder = effects.clone();
    remainder.box_shadows.clear();
    remainder == VisualEffects::default()
}

pub(crate) fn apply(element: &web_sys::Element, effects: &VisualEffects) -> Result<(), WebError> {
    if !supports(effects) {
        return Err(WebError(
            "DOM Host only implements box-shadow visual effects".into(),
        ));
    }
    let value = if effects.box_shadows.is_empty() {
        "none".into()
    } else {
        effects
            .box_shadows
            .iter()
            .map(|shadow| {
                format!(
                    "{} {}px {}px {}px {}px{}",
                    css_color(&shadow.color),
                    shadow.offset_x,
                    shadow.offset_y,
                    shadow.blur_radius,
                    shadow.spread_radius,
                    if shadow.inset { " inset" } else { "" },
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    set_style(element, "box-shadow", &value)
}
