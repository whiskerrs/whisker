use whisker_protocol::{
    InputEvent, InputEventKind, InputPoint, PointerId, PointerInput, PointerKind, SurfaceId,
};
use whisker_value::WhiskerValue;

/// Pointer phases emitted by a native Desktop window adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopPointerPhase {
    /// A pointer or mouse button became active.
    Down,
    /// An active pointer changed position.
    Move,
    /// A pointer or mouse button was released.
    Up,
    /// The operating system cancelled the active pointer stream.
    Cancel,
}

impl DesktopPointerPhase {
    const fn protocol(self) -> InputEventKind {
        match self {
            Self::Down => InputEventKind::PointerDown,
            Self::Move => InputEventKind::PointerMove,
            Self::Up => InputEventKind::PointerUp,
            Self::Cancel => InputEventKind::PointerCancel,
        }
    }
}

/// Mouse buttons normalized from a native Desktop window event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopMouseButton {
    /// Primary or left button.
    Primary,
    /// Auxiliary or middle button.
    Auxiliary,
    /// Secondary or right button.
    Secondary,
    /// Browser back button.
    Back,
    /// Browser forward button.
    Forward,
    /// A Host button without a portable CSS pointer mapping.
    Other,
}

/// Host-projected pointer metadata before conversion to the shared protocol.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DesktopPointerEvent {
    /// Monotonic Host timestamp in milliseconds.
    pub timestamp_ms: f64,
    /// Pointer lifecycle phase.
    pub phase: DesktopPointerPhase,
    /// Stable, non-zero identifier within this surface.
    pub pointer_id: u64,
    /// Host-classified pointer device kind.
    pub pointer_kind: PointerKind,
    /// Position in logical surface coordinates.
    pub position: InputPoint,
    /// CSS-compatible active-button bitset.
    pub buttons: u32,
    /// CSS-compatible changed button, or `-1` when not applicable.
    pub changed_button: i16,
}

impl DesktopMouseButton {
    const fn mask(self) -> u32 {
        match self {
            Self::Primary => 1,
            Self::Secondary => 2,
            Self::Auxiliary => 4,
            Self::Back => 8,
            Self::Forward => 16,
            Self::Other => 0,
        }
    }

    const fn changed(self) -> i16 {
        match self {
            Self::Primary => 0,
            Self::Auxiliary => 1,
            Self::Secondary => 2,
            Self::Back => 3,
            Self::Forward => 4,
            Self::Other => -1,
        }
    }
}

/// Stateful Desktop pointer normalizer shared by the three OS shells.
///
/// It retains the last logical mouse position and active mouse-button bitset;
/// touch identifiers are deterministically offset so they never collide with
/// the reserved mouse pointer identifier.
#[derive(Clone, Debug)]
pub struct DesktopPointerAdapter {
    surface: SurfaceId,
    mouse_position: Option<InputPoint>,
    mouse_buttons: u32,
}

impl DesktopPointerAdapter {
    /// Creates an adapter for one retained runtime surface.
    pub const fn new(surface: SurfaceId) -> Self {
        Self {
            surface,
            mouse_position: None,
            mouse_buttons: 0,
        }
    }

    /// Last logical cursor position observed by this surface.
    pub const fn mouse_position(&self) -> Option<InputPoint> {
        self.mouse_position
    }

    /// Normalizes a mouse cursor update at a logical window position.
    pub fn cursor_moved(&mut self, timestamp_ms: f64, position: [f32; 2]) -> InputEvent {
        let position = InputPoint {
            x: position[0],
            y: position[1],
        };
        self.mouse_position = Some(position);
        self.pointer_event(DesktopPointerEvent {
            timestamp_ms,
            phase: DesktopPointerPhase::Move,
            pointer_id: MOUSE_POINTER_ID,
            pointer_kind: PointerKind::Mouse,
            position,
            buttons: self.mouse_buttons,
            changed_button: -1,
        })
        .expect("the reserved mouse pointer identifier is valid")
    }

    /// Normalizes a native mouse-button transition at the last cursor position.
    pub fn mouse_button(
        &mut self,
        timestamp_ms: f64,
        button: DesktopMouseButton,
        pressed: bool,
    ) -> Option<InputEvent> {
        let position = self.mouse_position?;
        let mask = button.mask();
        if pressed {
            self.mouse_buttons |= mask;
        } else {
            self.mouse_buttons &= !mask;
        }
        self.pointer_event(DesktopPointerEvent {
            timestamp_ms,
            phase: if pressed {
                DesktopPointerPhase::Down
            } else {
                DesktopPointerPhase::Up
            },
            pointer_id: MOUSE_POINTER_ID,
            pointer_kind: PointerKind::Mouse,
            position,
            buttons: self.mouse_buttons,
            changed_button: button.changed(),
        })
    }

    /// Normalizes one winit-style touch update expressed in logical coordinates.
    pub fn touch(
        &self,
        timestamp_ms: f64,
        native_id: u64,
        phase: DesktopPointerPhase,
        position: [f32; 2],
    ) -> Option<InputEvent> {
        let pointer_id = native_id.checked_add(TOUCH_POINTER_ID_OFFSET)?;
        self.pointer_event(DesktopPointerEvent {
            timestamp_ms,
            phase,
            pointer_id,
            pointer_kind: PointerKind::Touch,
            position: InputPoint {
                x: position[0],
                y: position[1],
            },
            buttons: if matches!(phase, DesktopPointerPhase::Down | DesktopPointerPhase::Move) {
                1
            } else {
                0
            },
            changed_button: -1,
        })
    }

    /// Constructs the final protocol input used by production adapters and
    /// conformance fixtures after Host-specific event projection.
    pub fn pointer_event(&self, input: DesktopPointerEvent) -> Option<InputEvent> {
        let pointer_id = PointerId::new(input.pointer_id)?;
        let event = InputEvent {
            surface: self.surface,
            timestamp_ms: input.timestamp_ms,
            kind: input.phase.protocol(),
            pointer: Some(PointerInput {
                id: pointer_id,
                kind: input.pointer_kind,
                position: input.position,
                buttons: input.buttons,
                changed_button: input.changed_button,
            }),
            target: None,
            detail: WhiskerValue::Null,
        };
        event.validate().ok()?;
        Some(event)
    }
}

const MOUSE_POINTER_ID: u64 = 1;
const TOUCH_POINTER_ID_OFFSET: u64 = 2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_state_tracks_logical_position_buttons_and_changed_button() {
        let mut adapter = DesktopPointerAdapter::new(SurfaceId::new(1).unwrap());
        let moved = adapter.cursor_moved(1.0, [24.0, 16.0]);
        assert_eq!(moved.pointer.unwrap().buttons, 0);
        let down = adapter
            .mouse_button(2.0, DesktopMouseButton::Primary, true)
            .unwrap();
        let pointer = down.pointer.unwrap();
        assert_eq!(down.kind, InputEventKind::PointerDown);
        assert_eq!(pointer.position, InputPoint { x: 24.0, y: 16.0 });
        assert_eq!(pointer.buttons, 1);
        assert_eq!(pointer.changed_button, 0);
        let up = adapter
            .mouse_button(3.0, DesktopMouseButton::Primary, false)
            .unwrap();
        assert_eq!(up.pointer.unwrap().buttons, 0);
    }

    #[test]
    fn touch_ids_do_not_collide_with_the_reserved_mouse_id() {
        let adapter = DesktopPointerAdapter::new(SurfaceId::new(1).unwrap());
        let down = adapter
            .touch(1.0, 0, DesktopPointerPhase::Down, [3.0, 4.0])
            .unwrap();
        let pointer = down.pointer.unwrap();
        assert_eq!(pointer.id.get(), 2);
        assert_eq!(pointer.kind, PointerKind::Touch);
        assert_eq!(pointer.buttons, 1);
        assert_eq!(pointer.changed_button, -1);
    }
}
