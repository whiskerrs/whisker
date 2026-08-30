use super::*;

pub(super) fn event_class_mask(name: &str) -> u64 {
    match name {
        "touchstart" | "touchmove" | "touchend" | "touchcancel" | "pointerdown" | "pointermove"
        | "pointerup" | "pointercancel" => EVENT_POINTER,
        "tap" | "click" | "longpress" => EVENT_ACTIVATION,
        _ => EVENT_NAMED,
    }
}

pub(super) fn event_mask(kind: &BoundElementKind, name: &str) -> u64 {
    kind.registration()
        .and_then(|registration| registration.event_named(name))
        .and_then(|event| event.mask())
        .unwrap_or_else(|| event_class_mask(name))
}

pub(super) fn input_body(event: &InputEvent, target: WhiskerValue) -> WhiskerValue {
    let pointer_kind = event.pointer.map(|pointer| match pointer.kind {
        whisker_engine::whisker_protocol::PointerKind::Mouse => "mouse",
        whisker_engine::whisker_protocol::PointerKind::Touch => "touch",
        whisker_engine::whisker_protocol::PointerKind::Pen => "pen",
        whisker_engine::whisker_protocol::PointerKind::Unknown => "unknown",
    });
    let detail = if let Some(pointer) = event.pointer {
        WhiskerValue::map([
            ("x", WhiskerValue::Float(f64::from(pointer.position.x))),
            ("y", WhiskerValue::Float(f64::from(pointer.position.y))),
        ])
    } else {
        event.detail.clone()
    };
    let mut entries = vec![
        (
            "type",
            WhiskerValue::String(
                event
                    .kind
                    .name(event.pointer.map(|pointer| pointer.kind))
                    .to_owned(),
            ),
        ),
        ("timestamp", WhiskerValue::Float(event.timestamp_ms)),
        ("target", target.clone()),
        ("currentTarget", target),
        ("detail", detail),
    ];
    if let Some(pointer) = event.pointer {
        let touch = WhiskerValue::map([
            ("identifier", WhiskerValue::Int(pointer.id.get() as i64)),
            ("x", WhiskerValue::Float(f64::from(pointer.position.x))),
            ("y", WhiskerValue::Float(f64::from(pointer.position.y))),
            ("pageX", WhiskerValue::Float(f64::from(pointer.position.x))),
            ("pageY", WhiskerValue::Float(f64::from(pointer.position.y))),
            (
                "clientX",
                WhiskerValue::Float(f64::from(pointer.position.x)),
            ),
            (
                "clientY",
                WhiskerValue::Float(f64::from(pointer.position.y)),
            ),
        ]);
        let active_touches = if matches!(
            event.kind,
            whisker_engine::whisker_protocol::InputEventKind::PointerUp
                | whisker_engine::whisker_protocol::InputEventKind::PointerCancel
        ) {
            Vec::new()
        } else {
            vec![touch.clone()]
        };
        entries.extend([
            ("pointerId", WhiskerValue::Int(pointer.id.get() as i64)),
            (
                "pointerType",
                WhiskerValue::String(pointer_kind.unwrap_or("unknown").to_owned()),
            ),
            ("buttons", WhiskerValue::Int(i64::from(pointer.buttons))),
            (
                "button",
                WhiskerValue::Int(i64::from(pointer.changed_button)),
            ),
            ("touches", WhiskerValue::Array(active_touches)),
            ("changedTouches", WhiskerValue::Array(vec![touch])),
        ]);
    }
    WhiskerValue::map(entries)
}

pub(super) fn motion_event_body(
    event: &PendingMotionEvent,
    timestamp_ms: f64,
    target: WhiskerValue,
) -> WhiskerValue {
    WhiskerValue::map([
        ("type", WhiskerValue::String(event.kind.to_owned())),
        ("timestamp", WhiskerValue::Float(timestamp_ms)),
        ("target", target.clone()),
        ("currentTarget", target),
        (
            "animation_type",
            WhiskerValue::String(event.animation_type.to_owned()),
        ),
        ("animation_name", WhiskerValue::String(event.name.clone())),
        ("new_animator", WhiskerValue::Bool(true)),
    ])
}

pub(super) fn with_current_target(body: &WhiskerValue, target: WhiskerValue) -> WhiskerValue {
    let mut body = body.clone();
    if let WhiskerValue::Map(entries) = &mut body {
        entries.insert("currentTarget".to_owned(), target);
    }
    body
}

pub(super) fn retained_value(value: &WhiskerValue) -> Option<WhiskerValue> {
    Some(match value {
        WhiskerValue::Null => WhiskerValue::Null,
        WhiskerValue::Bool(value) => WhiskerValue::Bool(*value),
        WhiskerValue::Int(value) => WhiskerValue::Int(*value),
        WhiskerValue::Float(value) => WhiskerValue::Float(*value),
        WhiskerValue::String(value) => WhiskerValue::String(value.clone()),
        WhiskerValue::Bytes(value) => WhiskerValue::Bytes(value.clone()),
        WhiskerValue::Array(values) => WhiskerValue::Array(
            values
                .iter()
                .map(retained_value)
                .collect::<Option<Vec<_>>>()?,
        ),
        WhiskerValue::Map(values) => WhiskerValue::Map(
            values
                .iter()
                .map(|(name, value)| Some((name.clone(), retained_value(value)?)))
                .collect::<Option<std::collections::BTreeMap<_, _>>>()?,
        ),
        WhiskerValue::Error(_) => return None,
    })
}

pub(super) fn command_arguments(
    value: &WhiskerValue,
    expected: ElementValueKind,
) -> Option<WhiskerValue> {
    if expected == ElementValueKind::Null
        && matches!(
            value,
            WhiskerValue::Map(values)
                if matches!(values.get("args"), Some(WhiskerValue::Array(args)) if args.is_empty())
        )
    {
        return Some(WhiskerValue::Null);
    }
    let value = retained_value(value)?;
    expected.accepts(&value).then_some(value)
}
