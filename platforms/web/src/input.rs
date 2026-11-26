//! Browser pointer and wheel normalization and runtime dispatch.

use wasm_bindgen::JsCast;
use whisker::{InputDispatch, RuntimeInstance};
use whisker_protocol::{
    InputEvent, InputEventKind, InputPoint, NodeId, PointerId, PointerInput, PointerKind,
    WhiskerValue,
};

use crate::WebError;

/// Browser pointer phase accepted by the retained input router.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WebPointerPhase {
    Down,
    Move,
    Up,
    Cancel,
}

/// Browser pointer fields captured before protocol normalization.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WebPointerEvent {
    pub(crate) phase: WebPointerPhase,
    pub(crate) timestamp_ms: f64,
    pub(crate) pointer_id: u64,
    pub(crate) pointer_kind: PointerKind,
    pub(crate) client_position: InputPoint,
    pub(crate) buttons: u32,
    pub(crate) changed_button: i16,
}

/// Normalizes one root-relative browser pointer event and dispatches it through
/// the same typed runtime boundary used by native Hosts.
pub(crate) fn dispatch_pointer(
    runtime: &RuntimeInstance,
    root_origin: InputPoint,
    input: WebPointerEvent,
    presentation: &[whisker_protocol::HostPresentationUpdate],
) -> Result<InputEvent, WebError> {
    let pointer_id = PointerId::new(input.pointer_id)
        .ok_or_else(|| WebError("browser pointer id must be non-zero".into()))?;
    let event = InputEvent {
        presentation_revision: None,
        surface: runtime.surface().surface(),
        timestamp_ms: input.timestamp_ms,
        kind: match input.phase {
            WebPointerPhase::Down => InputEventKind::PointerDown,
            WebPointerPhase::Move => InputEventKind::PointerMove,
            WebPointerPhase::Up => InputEventKind::PointerUp,
            WebPointerPhase::Cancel => InputEventKind::PointerCancel,
        },
        pointer: Some(PointerInput {
            id: pointer_id,
            kind: input.pointer_kind,
            position: InputPoint {
                x: input.client_position.x - root_origin.x,
                y: input.client_position.y - root_origin.y,
            },
            buttons: input.buttons,
            changed_button: input.changed_button,
        }),
        target: None,
        detail: WhiskerValue::Null,
    };
    runtime
        .dispatch_input_with_presentation(&event, presentation)
        .map_err(|error| WebError(format!("dispatch Web pointer input: {error}")))?;
    Ok(event)
}

/// Converts the browser's signed pointer identifier into a stable, non-zero
/// protocol identifier. Positive browser IDs retain their value; zero and
/// defensive negative values occupy the otherwise unreachable high range.
pub(crate) fn stable_pointer_id(pointer_id: i32) -> u64 {
    if pointer_id > 0 {
        pointer_id as u64
    } else {
        u64::MAX - u64::from(pointer_id.unsigned_abs())
    }
}

pub(crate) fn pointer_kind(pointer_type: &str) -> PointerKind {
    match pointer_type {
        "mouse" => PointerKind::Mouse,
        "touch" => PointerKind::Touch,
        "pen" => PointerKind::Pen,
        _ => PointerKind::Unknown,
    }
}

/// Browser wheel fields, with the deltas already in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WebWheelEvent {
    pub(crate) timestamp_ms: f64,
    pub(crate) client_position: InputPoint,
    pub(crate) delta_x: f64,
    pub(crate) delta_y: f64,
    pub(crate) ctrl_key: bool,
    /// The innermost native scroll container under the pointer that can still scroll this way.
    pub(crate) scroller: Option<NodeId>,
}

/// Dispatches one root-relative browser wheel through the runtime, which reports whether a
/// catching listener took it.
pub(crate) fn dispatch_wheel(
    runtime: &RuntimeInstance,
    root_origin: InputPoint,
    input: WebWheelEvent,
    presentation: &[whisker_protocol::HostPresentationUpdate],
) -> Result<InputDispatch, WebError> {
    let mut detail = vec![
        ("deltaX", WhiskerValue::Float(input.delta_x)),
        ("deltaY", WhiskerValue::Float(input.delta_y)),
        ("ctrlKey", WhiskerValue::Bool(input.ctrl_key)),
    ];
    if let Some(scroller) = input.scroller {
        detail.push(("scroller", WhiskerValue::Int(scroller.get() as i64)));
    }
    let event = InputEvent {
        presentation_revision: None,
        surface: runtime.surface().surface(),
        timestamp_ms: input.timestamp_ms,
        kind: InputEventKind::Wheel,
        pointer: Some(PointerInput {
            id: PointerId::new(1).expect("non-zero"),
            kind: PointerKind::Mouse,
            position: InputPoint {
                x: input.client_position.x - root_origin.x,
                y: input.client_position.y - root_origin.y,
            },
            buttons: 0,
            changed_button: -1,
        }),
        target: None,
        detail: WhiskerValue::map(detail),
    };
    runtime
        .dispatch_input_with_presentation(&event, presentation)
        .map_err(|error| WebError(format!("dispatch Web wheel input: {error}")))
}

/// The innermost element from `target` up through the surface whose native scrolling can still
/// move by the delta.
pub(crate) fn wheel_scroller(
    target: Option<web_sys::Element>,
    delta_x: f64,
    delta_y: f64,
) -> Option<web_sys::Element> {
    let window = web_sys::window()?;
    let mut current = target;
    while let Some(element) = current {
        if let (Some(html), Ok(Some(style))) = (
            element.dyn_ref::<web_sys::HtmlElement>(),
            window.get_computed_style(&element),
        ) {
            let scrolls = |axis: &str| {
                style
                    .get_property_value(axis)
                    .is_ok_and(|value| value == "auto" || value == "scroll")
            };
            let room = |delta: f64, position: i32, client: i32, total: i32| {
                (delta > 0.0 && position + client < total - 1) || (delta < 0.0 && position > 0)
            };
            if (scrolls("overflow-y")
                && room(
                    delta_y,
                    html.scroll_top(),
                    html.client_height(),
                    html.scroll_height(),
                ))
                || (scrolls("overflow-x")
                    && room(
                        delta_x,
                        html.scroll_left(),
                        html.client_width(),
                        html.scroll_width(),
                    ))
            {
                return Some(element);
            }
        }
        current = element.parent_element();
    }
    None
}
