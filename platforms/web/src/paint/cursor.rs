use std::fmt::Write as _;

use whisker_protocol::{Cursor, CursorKeyword, ResourceId};

use super::background_layers::escape_css_url;
use crate::{WebError, set_style};

pub(crate) fn apply(
    element: &web_sys::Element,
    cursor: &Cursor,
    resolve_resource: impl Fn(ResourceId) -> Option<String>,
) -> Result<(), WebError> {
    set_style(element, "cursor", &css(cursor, resolve_resource)?)
}

fn css(
    cursor: &Cursor,
    resolve_resource: impl Fn(ResourceId) -> Option<String>,
) -> Result<String, WebError> {
    let mut value = String::new();
    for candidate in &cursor.resources {
        let url = resolve_resource(candidate.resource).ok_or_else(|| {
            WebError(format!(
                "DOM Host cursor resource {} is not registered",
                candidate.resource.get()
            ))
        })?;
        if !value.is_empty() {
            value.push_str(", ");
        }
        write!(value, "url(\"{}\")", escape_css_url(&url)).expect("writing to String cannot fail");
        if let Some((x, y)) = candidate.hotspot {
            write!(value, " {x} {y}").expect("writing to String cannot fail");
        }
    }
    if !value.is_empty() {
        value.push_str(", ");
    }
    value.push_str(keyword(cursor.fallback));
    Ok(value)
}

pub(crate) const fn keyword(value: CursorKeyword) -> &'static str {
    match value {
        CursorKeyword::Auto => "auto",
        CursorKeyword::Default => "default",
        CursorKeyword::None => "none",
        CursorKeyword::ContextMenu => "context-menu",
        CursorKeyword::Help => "help",
        CursorKeyword::Pointer => "pointer",
        CursorKeyword::Progress => "progress",
        CursorKeyword::Wait => "wait",
        CursorKeyword::Cell => "cell",
        CursorKeyword::Crosshair => "crosshair",
        CursorKeyword::Text => "text",
        CursorKeyword::VerticalText => "vertical-text",
        CursorKeyword::Alias => "alias",
        CursorKeyword::Copy => "copy",
        CursorKeyword::Move => "move",
        CursorKeyword::NoDrop => "no-drop",
        CursorKeyword::NotAllowed => "not-allowed",
        CursorKeyword::Grab => "grab",
        CursorKeyword::Grabbing => "grabbing",
        CursorKeyword::ColResize => "col-resize",
        CursorKeyword::RowResize => "row-resize",
        CursorKeyword::NResize => "n-resize",
        CursorKeyword::EResize => "e-resize",
        CursorKeyword::SResize => "s-resize",
        CursorKeyword::WResize => "w-resize",
        CursorKeyword::NeResize => "ne-resize",
        CursorKeyword::NwResize => "nw-resize",
        CursorKeyword::SeResize => "se-resize",
        CursorKeyword::SwResize => "sw-resize",
        CursorKeyword::EwResize => "ew-resize",
        CursorKeyword::NsResize => "ns-resize",
        CursorKeyword::NeswResize => "nesw-resize",
        CursorKeyword::NwseResize => "nwse-resize",
        CursorKeyword::ZoomIn => "zoom-in",
        CursorKeyword::ZoomOut => "zoom-out",
    }
}

#[cfg(test)]
mod tests {
    use wasm_bindgen_test::wasm_bindgen_test;
    use whisker_protocol::{CursorResource, ResourceId};

    use super::*;

    #[wasm_bindgen_test]
    fn resource_candidates_include_hotspots_and_keyword_fallback() {
        let first = ResourceId::new(1).unwrap();
        let second = ResourceId::new(2).unwrap();
        let cursor = Cursor {
            resources: vec![
                CursorResource {
                    resource: first,
                    hotspot: Some((4, 8)),
                },
                CursorResource {
                    resource: second,
                    hotspot: None,
                },
            ],
            fallback: CursorKeyword::Pointer,
        };
        assert_eq!(
            css(&cursor, |resource| Some(format!(
                "cursor/{}.png",
                resource.get()
            )))
            .unwrap(),
            "url(\"cursor/1.png\") 4 8, url(\"cursor/2.png\"), pointer"
        );
    }
}
