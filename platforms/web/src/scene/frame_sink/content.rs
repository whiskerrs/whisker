use super::*;

pub(super) fn apply_accessibility(
    element: &web_sys::Element,
    accessibility: &whisker_protocol::Accessibility,
) -> Result<(), WebError> {
    set_optional_attribute(element, "aria-label", accessibility.label.as_deref())?;
    set_optional_attribute(element, "aria-description", accessibility.hint.as_deref())?;
    set_optional_attribute(
        element,
        "data-whisker-accessibility-id",
        accessibility.identifier.as_deref(),
    )?;
    let role = accessibility.role.map(|role| match role {
        whisker_protocol::AccessibilityRole::Group => "group",
        whisker_protocol::AccessibilityRole::Text => "paragraph",
        whisker_protocol::AccessibilityRole::Button => "button",
        whisker_protocol::AccessibilityRole::Link => "link",
        whisker_protocol::AccessibilityRole::Image => "img",
        whisker_protocol::AccessibilityRole::Header => "heading",
        whisker_protocol::AccessibilityRole::Checkbox => "checkbox",
        whisker_protocol::AccessibilityRole::Radio => "radio",
        whisker_protocol::AccessibilityRole::Switch => "switch",
        whisker_protocol::AccessibilityRole::Adjustable => "slider",
        whisker_protocol::AccessibilityRole::SearchBox => "searchbox",
        whisker_protocol::AccessibilityRole::Tab => "tab",
        _ => "group",
    });
    set_optional_attribute(element, "role", role)?;
    set_bool_attribute(element, "aria-hidden", accessibility.hidden)?;
    set_bool_attribute(element, "aria-modal", accessibility.modal)?;
    set_optional_bool_attribute(element, "aria-disabled", accessibility.state.disabled)?;
    set_optional_bool_attribute(element, "aria-selected", accessibility.state.selected)?;
    set_optional_attribute(
        element,
        "aria-checked",
        accessibility.state.checked.map(|checked| checked.as_str()),
    )?;
    set_optional_attribute(
        element,
        "aria-expanded",
        accessibility
            .state
            .expanded
            .map(|value| if value { "true" } else { "false" }),
    )?;
    Ok(())
}

pub(super) fn set_optional_attribute(
    element: &web_sys::Element,
    name: &str,
    value: Option<&str>,
) -> Result<(), WebError> {
    if let Some(value) = value {
        element
            .set_attribute(name, value)
            .map_err(|error| js_error("set accessibility attribute", error))
    } else {
        element
            .remove_attribute(name)
            .map_err(|error| js_error("clear accessibility attribute", error))
    }
}

pub(super) fn set_bool_attribute(
    element: &web_sys::Element,
    name: &str,
    value: bool,
) -> Result<(), WebError> {
    if value {
        set_optional_attribute(element, name, Some("true"))
    } else {
        set_optional_attribute(element, name, None)
    }
}

pub(super) fn set_optional_bool_attribute(
    element: &web_sys::Element,
    name: &str,
    value: Option<bool>,
) -> Result<(), WebError> {
    set_optional_attribute(
        element,
        name,
        value.map(|value| if value { "true" } else { "false" }),
    )
}

pub(super) fn reset_pooled_element(element: &web_sys::Element) -> Result<(), WebError> {
    element.set_text_content(None);
    let attributes = element.get_attribute_names();
    for index in 0..attributes.length() {
        if let Some(name) = attributes.get(index).as_string() {
            element
                .remove_attribute(&name)
                .map_err(|error| js_error("reset pooled DOM attribute", error))?;
        }
    }
    set_style(element, "position", "absolute")?;
    set_style(element, "box-sizing", "border-box")
}

pub(super) fn sync_scroll_snap_child(
    parent: &web_sys::Element,
    child: &web_sys::Element,
) -> Result<(), WebError> {
    let Some(alignment) = parent.get_attribute("data-whisker-snap-align") else {
        return Ok(());
    };
    set_style(child, "scroll-snap-align", &alignment)?;
    let stop = parent
        .get_attribute("data-whisker-snap-stop")
        .unwrap_or_else(|| "normal".to_owned());
    set_style(child, "scroll-snap-stop", &stop)
}

pub(super) fn settle_scroll_snap(element: &web_sys::Element) {
    let Some(factor) = element
        .get_attribute("data-whisker-snap-factor")
        .and_then(|value| value.parse::<f64>().ok())
    else {
        return;
    };
    let offset = element
        .get_attribute("data-whisker-snap-offset")
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0);
    let horizontal = element
        .get_attribute("data-whisker-scroll-orientation")
        .as_deref()
        == Some("horizontal");
    let Some(scroller) = element.dyn_ref::<web_sys::HtmlElement>() else {
        return;
    };
    let viewport = if horizontal {
        scroller.client_width()
    } else {
        scroller.client_height()
    } as f64;
    let maximum = if horizontal {
        scroller.scroll_width() - scroller.client_width()
    } else {
        scroller.scroll_height() - scroller.client_height()
    }
    .max(0) as f64;
    let current = if horizontal {
        scroller.scroll_left()
    } else {
        scroller.scroll_top()
    } as f64;
    let factor = factor.clamp(0.0, 1.0);
    let mut target = None::<f64>;
    let children = element.children();
    for index in 0..children.length() {
        let Some(child) = children.item(index) else {
            continue;
        };
        let Some(child) = child.dyn_ref::<web_sys::HtmlElement>() else {
            continue;
        };
        let start = if horizontal {
            child.offset_left()
        } else {
            child.offset_top()
        } as f64;
        let extent = if horizontal {
            child.offset_width()
        } else {
            child.offset_height()
        } as f64;
        let candidate = (start + extent * factor - viewport * factor + offset).clamp(0.0, maximum);
        if target.is_none_or(|target| (candidate - current).abs() < (target - current).abs()) {
            target = Some(candidate);
        }
    }
    let Some(target) = target.map(f64::round) else {
        return;
    };
    if (target - current).abs() < 0.5 {
        return;
    }
    if horizontal {
        scroller.set_scroll_left(target as i32);
    } else {
        scroller.set_scroll_top(target as i32);
    }
}

pub(super) fn position_text(
    element: &web_sys::Element,
    rect: whisker_protocol::LayoutRect,
) -> Result<(), WebError> {
    set_style(element, "left", &px(rect.x))?;
    set_style(element, "top", &px(rect.y))?;
    set_style(element, "width", &px(rect.width))?;
    set_style(element, "height", &px(rect.height))?;
    set_style(element, "overflow", "hidden")
}
