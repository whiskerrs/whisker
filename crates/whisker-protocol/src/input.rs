//! Host-normalized input delivered to the Rust event router.

use crate::{NodeId, PointerId, ProtocolValue, SurfaceId};

/// Logical point in surface coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputPoint {
    /// Horizontal logical-pixel coordinate.
    pub x: f32,
    /// Vertical logical-pixel coordinate.
    pub y: f32,
}

impl InputPoint {
    /// Returns whether both coordinates are finite.
    pub fn is_valid(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

/// Physical source of one pointer stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PointerKind {
    /// Mouse or trackpad cursor.
    Mouse,
    /// Direct touch contact.
    Touch,
    /// Stylus or pen contact.
    Pen,
    /// Host source not represented by this protocol version.
    Unknown,
}

/// Semantic event name routed through Rust capture and bubble phases.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum InputEventKind {
    /// Pointer became active.
    PointerDown,
    /// Active pointer moved.
    PointerMove,
    /// Pointer ended normally.
    PointerUp,
    /// Pointer stream was cancelled.
    PointerCancel,
    /// Host-recognized activation compatible with `click`.
    Click,
    /// Host-recognized tap gesture.
    Tap,
    /// Host-recognized long-press gesture.
    LongPress,
    /// Element-provider event negotiated by name.
    Named(String),
}

impl InputEventKind {
    /// Returns the stable authoring name used by `on_<event>` listeners.
    pub fn name(&self, pointer: Option<PointerKind>) -> &str {
        match self {
            Self::PointerDown if pointer == Some(PointerKind::Touch) => "touchstart",
            Self::PointerMove if pointer == Some(PointerKind::Touch) => "touchmove",
            Self::PointerUp if pointer == Some(PointerKind::Touch) => "touchend",
            Self::PointerCancel if pointer == Some(PointerKind::Touch) => "touchcancel",
            Self::PointerDown => "pointerdown",
            Self::PointerMove => "pointermove",
            Self::PointerUp => "pointerup",
            Self::PointerCancel => "pointercancel",
            Self::Click => "click",
            Self::Tap => "tap",
            Self::LongPress => "longpress",
            Self::Named(name) => name,
        }
    }
}

/// Optional pointer data attached to an input event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerInput {
    /// Stable identifier for one active pointer stream.
    pub id: PointerId,
    /// Physical pointer source.
    pub kind: PointerKind,
    /// Current position in surface coordinates.
    pub position: InputPoint,
    /// Bitset of currently pressed Host buttons.
    pub buttons: u32,
    /// Button whose state changed, or `-1` when not applicable.
    pub changed_button: i16,
}

/// One input or provider event entering a retained surface.
#[derive(Clone, Debug, PartialEq)]
pub struct InputEvent {
    /// Destination surface.
    pub surface: SurfaceId,
    /// Monotonic Host timestamp in milliseconds.
    pub timestamp_ms: f64,
    /// Semantic event kind.
    pub kind: InputEventKind,
    /// Pointer data when this event belongs to a pointer stream.
    pub pointer: Option<PointerInput>,
    /// Explicit target for native controls and provider events. Pointer events
    /// normally leave this empty so Rust performs retained-scene hit testing.
    pub target: Option<NodeId>,
    /// Provider-specific typed detail.
    pub detail: ProtocolValue,
}

/// Invalid Host input rejected before listener lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEventError {
    /// Timestamp was NaN or infinite.
    InvalidTimestamp,
    /// Pointer coordinates were NaN or infinite.
    InvalidPosition,
}

impl InputEvent {
    /// Validates finite timing and geometry at the Host boundary.
    pub fn validate(&self) -> Result<(), InputEventError> {
        if !self.timestamp_ms.is_finite() {
            return Err(InputEventError::InvalidTimestamp);
        }
        if self
            .pointer
            .is_some_and(|pointer| !pointer.position.is_valid())
        {
            return Err(InputEventError::InvalidPosition);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_names_cover_pointer_sources_gestures_and_providers() {
        for (kind, touch_name, pointer_name) in [
            (InputEventKind::PointerDown, "touchstart", "pointerdown"),
            (InputEventKind::PointerMove, "touchmove", "pointermove"),
            (InputEventKind::PointerUp, "touchend", "pointerup"),
            (
                InputEventKind::PointerCancel,
                "touchcancel",
                "pointercancel",
            ),
        ] {
            assert_eq!(kind.name(Some(PointerKind::Touch)), touch_name);
            assert_eq!(kind.name(Some(PointerKind::Mouse)), pointer_name);
        }
        assert_eq!(InputEventKind::Click.name(None), "click");
        assert_eq!(InputEventKind::Tap.name(None), "tap");
        assert_eq!(InputEventKind::LongPress.name(None), "longpress");
        assert_eq!(InputEventKind::Named("ready".into()).name(None), "ready");
    }

    #[test]
    fn validates_every_timestamp_and_pointer_geometry_path() {
        let mut event = InputEvent {
            surface: SurfaceId::new(1).unwrap(),
            timestamp_ms: 1.0,
            kind: InputEventKind::Click,
            pointer: None,
            target: None,
            detail: ProtocolValue::Null,
        };
        assert_eq!(event.validate(), Ok(()));

        event.timestamp_ms = f64::NAN;
        assert_eq!(event.validate(), Err(InputEventError::InvalidTimestamp));
        event.timestamp_ms = 1.0;
        event.pointer = Some(PointerInput {
            id: PointerId::new(1).unwrap(),
            kind: PointerKind::Pen,
            position: InputPoint { x: 2.0, y: 3.0 },
            buttons: 0,
            changed_button: -1,
        });
        assert_eq!(event.validate(), Ok(()));

        event.pointer.as_mut().unwrap().position.x = f32::NAN;
        assert_eq!(event.validate(), Err(InputEventError::InvalidPosition));
        event.pointer.as_mut().unwrap().position.x = 2.0;
        event.pointer.as_mut().unwrap().position.y = f32::INFINITY;
        assert_eq!(event.validate(), Err(InputEventError::InvalidPosition));
    }
}
