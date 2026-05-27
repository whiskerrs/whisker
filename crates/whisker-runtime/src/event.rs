//! Typed event objects deserialized from the [`WhiskerValue`] body
//! Lynx hands an event handler.
//!
//! Mirrors Lynx's event hierarchy (see
//! <https://lynxjs.org/api/lynx-api/event/event.html>):
//!
//!   - [`Event`] — base shape every event carries (`type`,
//!     `timestamp`, `target`, `currentTarget`).
//!   - [`TouchEvent`] — `tap` / `longpress` / `touchstart` /
//!     `touchmove` / `touchend` / `touchcancel` / `click`. Adds the
//!     primary-touch [`Point`] `detail` plus `touches` /
//!     `changedTouches` arrays.
//!   - [`AnimationEvent`] — `animationstart` / `animationend` / … /
//!     `transitionend`. Adds the animation `detail`.
//!   - [`CustomEvent`] — component state-change events (`scroll`,
//!     input `change`, …). Carries an opaque [`WhiskerValue`]
//!     `detail`.
//!
//! A built-in builder's `on_<event>` method, or a
//! `#[whisker::module_component]` `on_<event>: TouchEvent` prop,
//! receives the event body as a [`WhiskerValue`] and recovers the
//! struct via [`WhiskerValue::deserialize_into`]. Every field is
//! `#[serde(default)]` so a body missing an optional key (or an
//! engine that names one slightly differently) degrades to a
//! zero-valued field rather than dropping the handler call.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, DeserializeOwned, Deserializer, MapAccess, Visitor};
use serde::Deserialize;

use crate::value::WhiskerValue;
use crate::view::{set_event_listener, Element};

/// Re-export so `event::BindType` sits next to `event::bind_typed` /
/// `event::bind_unit` — the propagation type these take. Canonical
/// definition lives in [`crate::view`].
pub use crate::view::BindType;

/// Register a **typed** event handler on `handle`.
///
/// The event body crosses the bridge as a [`WhiskerValue`]; this
/// deserializes it into `E` before calling `handler`. Used by the
/// built-in builders' `on_<event>` methods and by
/// `#[whisker::module_component]` for typed `on_<event>: E` props.
///
/// **The handler always fires when the event fires.** "The event
/// happened" is the primary signal; the typed payload is
/// supplementary. So if the body is absent (a bodyless event arrives
/// as [`WhiskerValue::Null`]) or its shape doesn't match `E`, the
/// handler is still called with `E::default()` — and the mismatch is
/// logged with the raw value so it stays diagnosable (the Case ②
/// philosophy: conversion mistakes are loggable, not invisible)
/// rather than silently swallowing the whole event.
pub fn bind_typed<E, F>(handle: Element, event_name: &'static str, bind_type: BindType, handler: F)
where
    E: DeserializeOwned + Default + 'static,
    F: Fn(E) + 'static,
{
    set_event_listener(
        handle,
        event_name,
        bind_type,
        Box::new(move |value: WhiskerValue| {
            let ev = value.deserialize_into::<E>().unwrap_or_else(|err| {
                eprintln!(
                    "[whisker] event `{event_name}`: payload did not deserialize into `{}`: \
                     {err} (raw: {value:?}); calling handler with default",
                    std::any::type_name::<E>(),
                );
                E::default()
            });
            handler(ev);
        }),
    );
}

/// Register an event handler that ignores the payload.
///
/// For `on_<event>: ()` props / call sites that only care that the
/// event fired. Wraps a `Fn()` into the value-carrying primitive.
pub fn bind_unit<F>(handle: Element, event_name: &str, bind_type: BindType, handler: F)
where
    F: Fn() + 'static,
{
    set_event_listener(
        handle,
        event_name,
        bind_type,
        Box::new(move |_value: WhiskerValue| handler()),
    );
}

/// The element an event targets / is listening on. Shared by
/// `target` (where the event originated) and `currentTarget` (the
/// element whose handler is firing).
#[derive(Debug, Clone, Default)]
pub struct Target {
    /// The element's `id` attribute (empty when unset).
    pub id: String,
    /// Lynx Engine's unique element identifier (its "sign").
    pub uid: i64,
    /// `data-*` attributes attached to the element, keyed without
    /// the `data-` prefix.
    pub dataset: BTreeMap<String, WhiskerValue>,
}

// The platform reporter hands us the *raw* event body, where `target`
// and `currentTarget` are plain integer signs (Lynx
// `LynxEvent.generateEventBody`: `body["target"] = targetSign`). The
// richer `{id, dataset, uid}` object is only synthesized downstream in
// the JS layer, which Whisker bypasses. So `Target` must deserialize
// from EITHER an integer (→ `uid`, with empty `id`/`dataset`) or a
// `{id, uid, dataset}` object — a hard "expected struct, got number"
// error here would otherwise fail the *whole* event struct and blank
// every field (including `detail`).
impl<'de> Deserialize<'de> for Target {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct TargetVisitor;
        impl<'de> Visitor<'de> for TargetVisitor {
            type Value = Target;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an element sign (integer) or a target object")
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Target, E> {
                Ok(Target {
                    uid: v,
                    ..Default::default()
                })
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Target, E> {
                Ok(Target {
                    uid: v as i64,
                    ..Default::default()
                })
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Target, E> {
                Ok(Target {
                    uid: v as i64,
                    ..Default::default()
                })
            }
            fn visit_unit<E: de::Error>(self) -> Result<Target, E> {
                Ok(Target::default())
            }
            fn visit_none<E: de::Error>(self) -> Result<Target, E> {
                Ok(Target::default())
            }
            fn visit_map<A>(self, map: A) -> Result<Target, A::Error>
            where
                A: MapAccess<'de>,
            {
                #[derive(Deserialize)]
                struct Obj {
                    #[serde(default)]
                    id: String,
                    #[serde(default)]
                    uid: i64,
                    #[serde(default)]
                    dataset: BTreeMap<String, WhiskerValue>,
                }
                let o = Obj::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(Target {
                    id: o.id,
                    uid: o.uid,
                    dataset: o.dataset,
                })
            }
        }
        deserializer.deserialize_any(TargetVisitor)
    }
}

/// A 2-D point in LynxView coordinates — the `detail` of a
/// [`TouchEvent`] (position of the first touch point).
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct Point {
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
}

/// A single active touch point inside a [`TouchEvent`].
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Touch {
    /// Stable id for the lifetime of one finger's touch sequence.
    #[serde(default)]
    pub identifier: i64,
    /// Position in the touched element's coordinate space.
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    /// Position in LynxView coordinates.
    #[serde(default)]
    pub page_x: f64,
    #[serde(default)]
    pub page_y: f64,
    /// Position in window coordinates.
    #[serde(default)]
    pub client_x: f64,
    #[serde(default)]
    pub client_y: f64,
}

/// Base event shape — fields present on every Lynx event.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Event {
    /// Event name (`"tap"`, `"touchstart"`, …).
    #[serde(rename = "type", default)]
    pub kind: String,
    /// Milliseconds since the event was generated.
    #[serde(default)]
    pub timestamp: f64,
    /// The element the event originated on.
    #[serde(default)]
    pub target: Target,
    /// The element whose listener is firing.
    #[serde(rename = "currentTarget", default)]
    pub current_target: Target,
}

/// Touch / tap / click event. The `detail` is the first touch
/// point's LynxView-coordinate position; `touches` /
/// `changed_touches` carry the full per-finger detail.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TouchEvent {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub timestamp: f64,
    #[serde(default)]
    pub target: Target,
    #[serde(default)]
    pub current_target: Target,
    /// Position of the first touch point (LynxView coordinates).
    #[serde(default)]
    pub detail: Point,
    /// All touch points currently on the surface.
    #[serde(default)]
    pub touches: Vec<Touch>,
    /// Touch points whose state changed in this event.
    #[serde(default)]
    pub changed_touches: Vec<Touch>,
}

/// Keyframe / transition animation lifecycle event.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AnimationEvent {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub timestamp: f64,
    #[serde(default)]
    pub target: Target,
    #[serde(rename = "currentTarget", default)]
    pub current_target: Target,
    /// `"keyframe-animation"` or `"transition-animation"`.
    #[serde(rename = "animation_type", default)]
    pub animation_type: String,
    /// `@keyframes` name or the transitioned CSS property.
    #[serde(rename = "animation_name", default)]
    pub animation_name: String,
    #[serde(rename = "new_animator", default)]
    pub new_animator: bool,
}

/// A component state-change event (`scroll`, input `change`, …).
/// The payload shape is component-specific, so `detail` stays an
/// opaque [`WhiskerValue`] the handler inspects itself.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CustomEvent {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub timestamp: f64,
    #[serde(default)]
    pub target: Target,
    #[serde(rename = "currentTarget", default)]
    pub current_target: Target,
    /// Component-supplied state. `WhiskerValue::Null` when absent.
    #[serde(default)]
    pub detail: WhiskerValue,
}

/// A 2-D size — `width` / `height` in px.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct Size {
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub height: f64,
}

// ---- scroll_view -----------------------------------------------------------

/// `<scroll_view>` scroll events — `scroll`, `scrolltoupper`,
/// `scrolltolower`, `scrollend`, `contentsizechanged`. The `detail`
/// carries the current scroll geometry. (CustomEvent → target-only, so
/// these have no catch/capture variants — see Lynx `CustomEvent`
/// defaults `Capture::kNo, Bubbles::kNo`.)
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ScrollEvent {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub timestamp: f64,
    #[serde(default)]
    pub target: Target,
    #[serde(rename = "currentTarget", default)]
    pub current_target: Target,
    #[serde(default)]
    pub detail: ScrollDetail,
}

/// Scroll geometry carried by a [`ScrollEvent`] (the event body's
/// `detail` dict — see Lynx `LynxScrollEventManager`).
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollDetail {
    /// Horizontal content offset (px).
    #[serde(default)]
    pub scroll_left: f64,
    /// Vertical content offset (px).
    #[serde(default)]
    pub scroll_top: f64,
    /// Total scrollable content width (px).
    #[serde(default)]
    pub scroll_width: f64,
    /// Total scrollable content height (px).
    #[serde(default)]
    pub scroll_height: f64,
    /// Horizontal delta since the previous scroll event (px).
    #[serde(default)]
    pub delta_x: f64,
    /// Vertical delta since the previous scroll event (px).
    #[serde(default)]
    pub delta_y: f64,
    /// Whether the user's finger is currently dragging the scroll view.
    #[serde(default)]
    pub is_dragging: bool,
}

// ---- image -----------------------------------------------------------------

/// `load` on `<image>` — the image request succeeded; `detail` gives
/// the intrinsic pixel size. (`error` / animated-image events surface
/// as [`CustomEvent`] — their detail is component-specific.)
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ImageLoadEvent {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub timestamp: f64,
    #[serde(default)]
    pub target: Target,
    #[serde(rename = "currentTarget", default)]
    pub current_target: Target,
    #[serde(default)]
    pub detail: ImageLoadDetail,
}

/// Intrinsic image size carried by an [`ImageLoadEvent`].
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct ImageLoadDetail {
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub height: f64,
}

// ---- text ------------------------------------------------------------------

/// `layout` on `<text>` — fired after text layout completes.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TextLayoutEvent {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub timestamp: f64,
    #[serde(default)]
    pub target: Target,
    #[serde(rename = "currentTarget", default)]
    pub current_target: Target,
    #[serde(default)]
    pub detail: TextLayoutDetail,
}

/// Layout info carried by a [`TextLayoutEvent`].
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextLayoutDetail {
    /// Number of laid-out lines.
    #[serde(default)]
    pub line_count: i64,
    /// Per-line ranges (and ellipsis info for truncated lines).
    #[serde(default)]
    pub lines: Vec<TextLineInfo>,
    /// Laid-out content size.
    #[serde(default)]
    pub size: Size,
}

/// One laid-out text line inside a [`TextLayoutDetail`].
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextLineInfo {
    /// Character index of the line's first glyph.
    #[serde(default)]
    pub start: i64,
    /// Character index just past the line's last glyph.
    #[serde(default)]
    pub end: i64,
    /// Number of characters replaced by the truncation ellipsis (0 if
    /// the line isn't truncated).
    #[serde(default)]
    pub ellipsis_count: i64,
}

/// `selectionchange` on `<text>` — the selected text range changed.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SelectionChangeEvent {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub timestamp: f64,
    #[serde(default)]
    pub target: Target,
    #[serde(rename = "currentTarget", default)]
    pub current_target: Target,
    #[serde(default)]
    pub detail: SelectionDetail,
}

/// Selection range carried by a [`SelectionChangeEvent`].
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SelectionDetail {
    /// Start character index, or -1 when there's no selection.
    #[serde(default)]
    pub start: i64,
    /// End character index, or -1 when there's no selection.
    #[serde(default)]
    pub end: i64,
    /// `"forward"` or `"backward"`.
    #[serde(default)]
    pub direction: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_event_from_value_tree() {
        // Shape mirrors Lynx's `generateEventBody` for a tap.
        let v = WhiskerValue::map([
            ("type", WhiskerValue::String("tap".into())),
            ("timestamp", WhiskerValue::Float(123.0)),
            (
                "detail",
                WhiskerValue::map([
                    ("x", WhiskerValue::Float(10.5)),
                    ("y", WhiskerValue::Float(20.0)),
                ]),
            ),
            (
                "target",
                WhiskerValue::map([
                    ("id", WhiskerValue::String("btn".into())),
                    ("uid", WhiskerValue::Int(7)),
                ]),
            ),
            (
                "touches",
                WhiskerValue::Array(vec![WhiskerValue::map([
                    ("identifier", WhiskerValue::Int(0)),
                    ("pageX", WhiskerValue::Float(10.5)),
                    ("pageY", WhiskerValue::Float(20.0)),
                ])]),
            ),
        ]);

        let e: TouchEvent = v.deserialize_into().expect("deserialize TouchEvent");
        assert_eq!(e.kind, "tap");
        assert_eq!(e.detail.x, 10.5);
        assert_eq!(e.target.id, "btn");
        assert_eq!(e.target.uid, 7);
        assert_eq!(e.touches.len(), 1);
        assert_eq!(e.touches[0].page_x, 10.5);
    }

    #[test]
    fn missing_fields_default_rather_than_fail() {
        // A body with only some keys (e.g. an event Lynx fills
        // partially) must still deserialize — every field defaults.
        let e: TouchEvent = WhiskerValue::map([("type", WhiskerValue::String("touchend".into()))])
            .deserialize_into()
            .expect("partial body deserializes");
        assert_eq!(e.kind, "touchend");
        assert!(e.touches.is_empty());
        assert_eq!(e.detail.x, 0.0);
    }

    #[test]
    fn custom_event_keeps_opaque_detail() {
        let v = WhiskerValue::map([(
            "detail",
            WhiskerValue::map([("scrollTop", WhiskerValue::Int(42))]),
        )]);
        let e: CustomEvent = v.deserialize_into().expect("deserialize CustomEvent");
        match e.detail {
            WhiskerValue::Map(m) => assert_eq!(m.get("scrollTop"), Some(&WhiskerValue::Int(42))),
            other => panic!("expected Map detail, got {other:?}"),
        }
    }

    #[test]
    fn scroll_event_detail_camel_case_mapping() {
        // Mirrors Lynx's LynxScrollEventManager detail dict.
        let v = WhiskerValue::map([
            ("type", WhiskerValue::String("scroll".into())),
            (
                "detail",
                WhiskerValue::map([
                    ("scrollLeft", WhiskerValue::Float(0.0)),
                    ("scrollTop", WhiskerValue::Float(120.0)),
                    ("scrollHeight", WhiskerValue::Float(2000.0)),
                    ("scrollWidth", WhiskerValue::Float(375.0)),
                    ("deltaY", WhiskerValue::Float(12.0)),
                    ("isDragging", WhiskerValue::Bool(true)),
                ]),
            ),
        ]);
        let e: ScrollEvent = v.deserialize_into().expect("deserialize ScrollEvent");
        assert_eq!(e.kind, "scroll");
        assert_eq!(e.detail.scroll_top, 120.0);
        assert_eq!(e.detail.scroll_height, 2000.0);
        assert_eq!(e.detail.delta_y, 12.0);
        assert!(e.detail.is_dragging);
        // Absent key degrades to default rather than failing.
        assert_eq!(e.detail.delta_x, 0.0);
    }

    #[test]
    fn text_layout_event_nested_lines_and_size() {
        let v = WhiskerValue::map([
            ("type", WhiskerValue::String("layout".into())),
            (
                "detail",
                WhiskerValue::map([
                    ("lineCount", WhiskerValue::Int(2)),
                    (
                        "size",
                        WhiskerValue::map([
                            ("width", WhiskerValue::Float(300.0)),
                            ("height", WhiskerValue::Float(40.0)),
                        ]),
                    ),
                    (
                        "lines",
                        WhiskerValue::Array(vec![
                            WhiskerValue::map([
                                ("start", WhiskerValue::Int(0)),
                                ("end", WhiskerValue::Int(10)),
                                ("ellipsisCount", WhiskerValue::Int(0)),
                            ]),
                            WhiskerValue::map([
                                ("start", WhiskerValue::Int(10)),
                                ("end", WhiskerValue::Int(18)),
                                ("ellipsisCount", WhiskerValue::Int(3)),
                            ]),
                        ]),
                    ),
                ]),
            ),
        ]);
        let e: TextLayoutEvent = v.deserialize_into().expect("deserialize TextLayoutEvent");
        assert_eq!(e.detail.line_count, 2);
        assert_eq!(e.detail.size.width, 300.0);
        assert_eq!(e.detail.lines.len(), 2);
        assert_eq!(e.detail.lines[1].end, 18);
        assert_eq!(e.detail.lines[1].ellipsis_count, 3);
    }

    #[test]
    fn integer_target_signs_dont_blank_the_event() {
        // The REAL reporter body: `target` / `currentTarget` are plain
        // integer signs (LynxEvent.generateEventBody), not objects.
        // Target's int-or-object deserialize must accept that so the
        // sibling `detail` still populates (a type mismatch here used
        // to fail the whole struct → all-zero payload).
        let v = WhiskerValue::map([
            ("type", WhiskerValue::String("scroll".into())),
            ("target", WhiskerValue::Int(33)),
            ("currentTarget", WhiskerValue::Int(33)),
            (
                "detail",
                WhiskerValue::map([
                    ("scrollLeft", WhiskerValue::Float(640.0)),
                    ("scrollWidth", WhiskerValue::Float(832.0)),
                    ("isDragging", WhiskerValue::Bool(true)),
                ]),
            ),
        ]);
        let e: ScrollEvent = v
            .deserialize_into()
            .expect("deserialize with integer target");
        assert_eq!(e.target.uid, 33); // sign mapped to uid
        assert_eq!(e.target.id, ""); // raw body carries no id
        assert_eq!(e.current_target.uid, 33);
        // The sibling detail must survive the integer target.
        assert_eq!(e.detail.scroll_left, 640.0);
        assert_eq!(e.detail.scroll_width, 832.0);
        assert!(e.detail.is_dragging);
    }

    #[test]
    fn touch_event_integer_target() {
        let v = WhiskerValue::map([
            ("type", WhiskerValue::String("tap".into())),
            ("target", WhiskerValue::Int(7)),
            (
                "detail",
                WhiskerValue::map([
                    ("x", WhiskerValue::Float(10.0)),
                    ("y", WhiskerValue::Float(20.0)),
                ]),
            ),
        ]);
        let e: TouchEvent = v
            .deserialize_into()
            .expect("deserialize TouchEvent int target");
        assert_eq!(e.target.uid, 7);
        assert_eq!(e.detail.x, 10.0);
    }
}
