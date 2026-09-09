use super::*;
use whisker_protocol::SurfaceId;

fn pointer(kind: InputEventKind) -> InputEvent {
    InputEvent {
        presentation_revision: Some(7),
        surface: SurfaceId::new(1).unwrap(),
        timestamp_ms: 10.0,
        kind,
        target: Some(NodeId::new(1).unwrap()),
        detail: WhiskerValue::Null,
        pointer: Some(PointerInput {
            id: PointerId::new(1).unwrap(),
            kind: PointerKind::Touch,
            position: InputPoint { x: 5.0, y: 5.0 },
            buttons: 1,
            changed_button: 0,
        }),
    }
}

#[test]
fn native_selection_cancels_taps_even_inside_the_movement_slop() {
    let down = pointer(InputEventKind::PointerDown);
    let mut recognizer = ActivationRecognizer::default();
    recognizer.observe(&down, down.target, false);
    assert!(!recognizer.has_pending_longpress());
    let selected = InputEvent {
        kind: InputEventKind::Named("selectionchange".into()),
        pointer: None,
        detail: WhiskerValue::map([
            ("start", WhiskerValue::Int(0)),
            ("end", WhiskerValue::Int(2)),
        ]),
        ..down.clone()
    };
    recognizer.observe(&selected, down.target, false);
    assert!(
        recognizer
            .observe(&pointer(InputEventKind::PointerUp), down.target, false)
            .is_none()
    );
}

#[test]
fn synthesized_tap_keeps_the_pointer_down_presentation() {
    let down = pointer(InputEventKind::PointerDown);
    let mut recognizer = ActivationRecognizer::default();
    recognizer.observe(&down, down.target, false);
    let up = InputEvent {
        presentation_revision: Some(8),
        ..pointer(InputEventKind::PointerUp)
    };
    let activation = recognizer.observe(&up, up.target, false).unwrap();
    assert_eq!(activation.tap.presentation_revision, Some(7));
}
