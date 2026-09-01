//! `#[whisker::module_element]` end-to-end tests.
//!
//! Verifies the proc-macro lowers a tag-name + prop list into:
//! - `Xxx::builder().<prop>(v).build()` shape
//! - a body that calls `view::create_element_by_name(tag)`
//! - structured `apply_style` plus per-prop `apply_attr` routing
//!
//! The in-memory `Recorder` captures every dispatched op into
//! `Op::*` so assertions can verify the underlying tag-name + per-
//! attribute set sequence.

use std::cell::RefCell;
use std::rc::Rc;

use whisker::flush;
use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, Owner};
use whisker::runtime::view::{DynRenderer, Element, install_renderer, uninstall_renderer};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    CreateByName {
        id: u32,
        tag_name: String,
    },
    Create {
        id: u32,
        tag: ElementTag,
    },
    SetAttr {
        id: u32,
        key: String,
        value: String,
    },
    SetSpecifiedStyle {
        id: u32,
        style: whisker_engine::whisker_style::SpecifiedStyle,
    },
    SetId {
        id: u32,
        value: String,
    },
    SetAccessibility {
        id: u32,
        label: Option<String>,
    },
    Append {
        parent: u32,
        child: u32,
    },
    Event {
        id: u32,
        name: String,
    },
}

#[derive(Default)]
struct Recorder {
    next: ::std::cell::Cell<u32>,
    log: Rc<RefCell<Vec<Op>>>,
}

impl Recorder {
    fn with_log() -> (Self, Rc<RefCell<Vec<Op>>>) {
        let r = Self::default();
        let log = r.log.clone();
        (r, log)
    }
}

impl DynRenderer for Recorder {
    fn create_element(&self, tag: ElementTag) -> Element {
        let id = self.next.get();
        self.next.set(id + 1);
        self.log.borrow_mut().push(Op::Create { id, tag });
        Element::from_raw(id)
    }
    fn create_element_by_name(&self, tag_name: &str) -> Element {
        let id = self.next.get();
        self.next.set(id + 1);
        self.log.borrow_mut().push(Op::CreateByName {
            id,
            tag_name: tag_name.into(),
        });
        Element::from_raw(id)
    }
    fn release_element(&self, _h: Element) {}
    fn set_attribute(&self, h: Element, k: &str, v: &str) {
        self.log.borrow_mut().push(Op::SetAttr {
            id: h.id(),
            key: k.into(),
            value: v.into(),
        });
    }
    fn set_specified_style(
        &self,
        h: Element,
        style: &whisker_engine::whisker_style::SpecifiedStyle,
    ) -> bool {
        self.log.borrow_mut().push(Op::SetSpecifiedStyle {
            id: h.id(),
            style: style.clone(),
        });
        true
    }
    fn set_element_id(&self, h: Element, value: String) {
        self.log.borrow_mut().push(Op::SetId { id: h.id(), value });
    }
    fn set_accessibility(&self, h: Element, value: Accessibility) {
        self.log.borrow_mut().push(Op::SetAccessibility {
            id: h.id(),
            label: value.label,
        });
    }
    fn append_child(&self, p: Element, c: Element) {
        self.log.borrow_mut().push(Op::Append {
            parent: p.id(),
            child: c.id(),
        });
    }
    fn remove_child(&self, _p: Element, _c: Element) {}
    fn set_event_listener(
        &self,
        h: Element,
        name: &str,
        _bind_type: whisker::runtime::view::BindType,
        _cb: Box<dyn Fn(whisker::WhiskerValue) + 'static>,
    ) {
        self.log.borrow_mut().push(Op::Event {
            id: h.id(),
            name: name.into(),
        });
    }
    fn set_root(&self, _p: Element) {}
    fn flush(&self) {}
}

fn with_recorder_and_owner<R>(f: impl FnOnce(Rc<RefCell<Vec<Op>>>) -> R) -> R {
    __reset_for_tests();
    let (rec, log) = Recorder::with_log();
    let prev = install_renderer(Box::new(rec));
    let owner = Owner::new(None);
    let out = owner.with(|| f(log));
    owner.dispose();
    uninstall_renderer(prev);
    out
}

// ---- Platform component declarations ------------------------------------------

#[whisker::module_element("x-zero-props")]
pub fn x_zero_props() {}

#[whisker::module_element("x-styled")]
pub fn x_styled(style: whisker::Style) {}

#[whisker::module_element("x-input")]
pub fn x_input(value: Signal<String>, placeholder: Signal<String>) {}

#[whisker::module_element("x-typed-checkbox")]
pub fn x_typed_checkbox(checked: Signal<bool>, count: Signal<i32>) {}

#[whisker::module_element("x-button")]
pub fn x_button(label: Signal<String>, on_press: ()) {}

#[whisker::module_element("x-input-payload")]
pub fn x_input_payload(value: Signal<String>, on_input: ::whisker::WhiskerValue) {}

#[whisker::module_element("x-typed-input")]
pub fn x_typed_input(on_change: ::whisker::event::TouchEvent) {}

#[whisker::module_element("x-container")]
pub fn x_container(style: whisker::Style, children: ::whisker::Children) {}

#[whisker::module_element(
    name = "whisker.test/GeneratedSchema",
    measurement = Custom,
    text_style = true,
    commands = [("focus", Null)],
)]
pub fn generated_schema(
    enabled: Signal<bool>,
    label: Signal<String>,
    style: whisker::Style,
    on_change: ::whisker::event::CustomEvent,
    children: ::whisker::Children,
) {
}

#[whisker::module_element(
    name = "whisker.test/NativeLabel",
    measurement = Text,
)]
pub fn native_label(children: ::whisker::TextChildren) {}

// ---- Tests -----------------------------------------------------------------

#[test]
fn named_form_generates_the_host_independent_schema_and_ids() {
    let provider = generated_schema_schema::element_provider();
    assert_eq!(provider.schema, generated_schema_schema::schema());
    assert_eq!(provider.schema.name, generated_schema_schema::NAME);
    assert_eq!(provider.schema.name, "whisker.test/GeneratedSchema");
    assert_eq!(provider.schema.child_policy, whisker::ChildPolicy::Elements);
    assert_eq!(
        provider.schema.measurement,
        whisker::ElementMeasurement::Custom
    );
    assert!(provider.schema.text_style);
    assert_eq!(provider.schema.commands.len(), 1);
    assert_eq!(provider.schema.commands[0].name, "focus");
    assert_eq!(
        provider.schema.commands[0].arguments,
        whisker::ElementValueKind::Null
    );
    assert_eq!(provider.schema.properties.len(), 2);
    assert_eq!(
        provider.schema.properties[0].property,
        generated_schema_schema::ENABLED_PROPERTY
    );
    assert_eq!(
        provider.schema.properties[0].value,
        whisker::ElementValueKind::Bool
    );
    assert_eq!(
        provider.schema.properties[1].property,
        generated_schema_schema::LABEL_PROPERTY
    );
    assert_eq!(
        provider.schema.properties[1].value,
        whisker::ElementValueKind::String
    );
    assert_eq!(provider.schema.events.len(), 1);
    assert_eq!(
        provider.schema.events[0].event,
        generated_schema_schema::CHANGE_EVENT
    );
    assert_eq!(provider.schema.events[0].detail, None);
    assert_eq!(provider.schema.validate(), Ok(()));
}

#[test]
fn text_children_generate_plain_text_policy_and_mount_raw_text() {
    assert_eq!(
        native_label_schema::schema().child_policy,
        whisker::ChildPolicy::PlainText
    );

    with_recorder_and_owner(|log| {
        let _handle = render! { NativeLabel { "hello" } };
        let operations = log.borrow();
        assert!(operations.iter().any(|operation| matches!(
            operation,
            Op::Create {
                tag: ElementTag::RawText,
                ..
            }
        )));
    });
}

#[test]
fn named_form_uses_the_same_name_for_runtime_lookup() {
    with_recorder_and_owner(|log| {
        let _handle = render! {
            GeneratedSchema(
                enabled: true,
                label: "generated",
                on_change: |_event| {},
            )
        };
        let names = log
            .borrow()
            .iter()
            .filter_map(|operation| match operation {
                Op::CreateByName { tag_name, .. } => Some(tag_name.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(names, vec![generated_schema_schema::NAME]);
    });
}

#[test]
fn module_element_builder_inherits_common_element_api() {
    with_recorder_and_owner(|log| {
        let _handle = render! {
            GeneratedSchema(
                enabled: true,
                label: "generated",
                style: css!(width: px(10)),
                id: "generated-id",
                accessibility: Accessibility::new().label("Generated element"),
                on_tap: |_event| {},
                on_change: |_event| {},
            )
        };
        let operations = log.borrow();
        assert!(operations.iter().any(|operation| matches!(
            operation,
            Op::SetId { value, .. } if value == "generated-id"
        )));
        assert!(operations.iter().any(|operation| matches!(
            operation,
            Op::SetAccessibility { label, .. }
                if label.as_deref() == Some("Generated element")
        )));
        assert!(operations.iter().any(|operation| matches!(
            operation,
            Op::Event { name, .. } if name == "tap"
        )));
        assert!(operations.iter().any(|operation| matches!(
            operation,
            Op::Event { name, .. } if name == "change"
        )));
    });

    let schema = generated_schema_schema::schema();
    assert_eq!(
        schema
            .properties
            .iter()
            .map(|property| property.name.as_str())
            .collect::<Vec<_>>(),
        vec!["enabled", "label"]
    );
    assert_eq!(
        schema
            .events
            .iter()
            .map(|event| event.name.as_str())
            .collect::<Vec<_>>(),
        vec!["change"]
    );
}

#[test]
fn module_element_builder_is_usable_without_render_macro() {
    with_recorder_and_owner(|log| {
        let handle = GeneratedSchema::builder()
            .enabled(true)
            .label("direct")
            .on_change(|_event| {})
            .build();
        assert_eq!(handle.id(), 0);
        let operations = log.borrow();
        assert!(operations.iter().any(|operation| matches!(
            operation,
            Op::SetAttr { key, value, .. } if key == "label" && value == "direct"
        )));
    });
}

#[test]
fn zero_props_creates_element_with_tag_name() {
    with_recorder_and_owner(|log| {
        let _h = render! {
            XZeroProps()
        };
        let creates: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::CreateByName { tag_name, .. } => Some(tag_name.clone()),
                _ => None,
            })
            .collect();
        // `concat!(env!("CARGO_PKG_NAME"), ":", tag)` resolves in this
        // integration-test crate to `whisker:x-zero-props`.
        assert_eq!(creates, vec!["whisker:x-zero-props".to_string()]);
    });
}

#[test]
fn zero_props_component_inherits_style_and_common_element_api() {
    with_recorder_and_owner(|log| {
        let _handle = render! {
            XZeroProps(
                style: css!(width: px(12)),
                id: "zero-props",
                on_tap: |_event| {},
            )
        };
        let operations = log.borrow();
        assert!(operations.iter().any(|operation| matches!(
            operation,
            Op::SetSpecifiedStyle { style, .. } if style.len() == 1
        )));
        assert!(operations.iter().any(|operation| matches!(
            operation,
            Op::SetId { value, .. } if value == "zero-props"
        )));
        assert!(operations.iter().any(|operation| matches!(
            operation,
            Op::Event { name, .. } if name == "tap"
        )));
    });
}

#[test]
fn style_prop_routes_through_structured_style() {
    with_recorder_and_owner(|log| {
        let _h = render! {
            XStyled(style: css!(background_color: Color::Named(NamedColor::Red), height: px(8)))
        };
        let styles: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetSpecifiedStyle { style, .. } => Some(style.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(styles.len(), 1);
        assert_eq!(styles[0].len(), 2);
    });
}

#[test]
fn dynamic_style_re_runs_on_signal_change() {
    with_recorder_and_owner(|log| {
        let (color, set_color) = signal(NamedColor::Red).split();
        let css = computed(move || Css::new().background_color(Color::Named(color.get())));
        let _h = render! {
            XStyled(style: css)
        };
        set_color.set(NamedColor::Blue);
        flush();
        let styles: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetSpecifiedStyle { style, .. } => Some(style.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(styles.len(), 2);
        assert_ne!(styles[0], styles[1]);
    });
}

#[test]
fn non_style_props_route_through_set_attribute_with_kebab_case() {
    // Regular SetAttribute calls. Snake-case prop names map to
    // kebab-case attribute names; both of these are single-word, so
    // kebab == snake here.
    with_recorder_and_owner(|log| {
        let _h = render! {
            XInput(value: "hello", placeholder: "type here")
        };
        let attrs: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } => Some((key.clone(), value.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            attrs,
            vec![
                ("value".to_string(), "hello".to_string()),
                ("placeholder".to_string(), "type here".to_string()),
            ]
        );
    });
}

#[test]
fn read_signal_prop_tracks_underlying_signal() {
    with_recorder_and_owner(|log| {
        let (value, set_value) = signal("alpha".to_string()).split();
        let _h = render! {
            XInput(value: value, placeholder: "static")
        };
        set_value.set("beta".into());
        flush();
        set_value.set("gamma".into());
        flush();
        let value_sets: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } if key == "value" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(value_sets, vec!["alpha", "beta", "gamma"]);
    });
}

#[test]
fn typed_signal_bool_and_integer_use_typed_attribute_helpers() {
    // The recorder's default typed setters forward to strings, retaining an
    // observable value while the Surface renderer receives Bool/I64 values.
    with_recorder_and_owner(|log| {
        let (checked, set_checked) = signal(false).split();
        let _h = render! {
            XTypedCheckbox(checked: checked, count: 42_i32)
        };
        set_checked.set(true);
        flush();
        let checked_sets: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } if key == "checked" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(checked_sets, vec!["false", "true"]);
        let count_set = log.borrow().iter().find_map(|op| match op {
            Op::SetAttr { key, value, .. } if key == "count" => Some(value.clone()),
            _ => None,
        });
        assert_eq!(count_set, Some("42".to_string()));
    });
}

#[test]
fn no_payload_event_handler_registers_listener() {
    // `on_press`, not `on_tap`: `tap` is a reserved Lynx gesture name
    // the macro rejects.
    with_recorder_and_owner(|log| {
        let _h = render! {
            XButton(label: "Click me", on_press: || {})
        };
        let events: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::Event { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(events, vec!["press".to_string()]);
    });
}

#[test]
fn raw_payload_event_handler_registers_listener() {
    with_recorder_and_owner(|log| {
        let _h = render! {
            XInputPayload(value: "", on_input: |_raw: ::whisker::WhiskerValue| {})
        };
        let events: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::Event { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(events, vec!["input".to_string()]);
    });
}

#[test]
fn typed_payload_event_handler_registers_listener() {
    with_recorder_and_owner(|log| {
        let _h = render! {
            XTypedInput(on_change: |_e: ::whisker::event::TouchEvent| {})
        };
        let events: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::Event { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(events, vec!["change".to_string()]);
    });
}

#[test]
fn children_prop_attaches_inner_view() {
    // `render!` lowers the `{ Inner() … }` block to a
    // `.children(Rc::new(move || { … }))` setter call.
    with_recorder_and_owner(|log| {
        let _h = render! {
            XContainer(style: css!(padding: px(10))) {
                Text(value: "child 1")
                Text(value: "child 2")
            }
        };
        let log_b = log.borrow();
        let container_id = log_b.iter().find_map(|op| match op {
            Op::CreateByName { id, tag_name } if tag_name == "whisker:x-container" => Some(*id),
            _ => None,
        });
        assert!(
            container_id.is_some(),
            "whisker:x-container element must be created"
        );
    });
}
