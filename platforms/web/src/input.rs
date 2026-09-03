//! Browser pointer normalization and runtime dispatch.

use whisker::RuntimeInstance;
use whisker_protocol::{
    InputEvent, InputEventKind, InputPoint, PointerId, PointerInput, PointerKind, WhiskerValue,
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
