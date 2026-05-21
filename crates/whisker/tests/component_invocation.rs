//! Integration tests for the unified component invocation syntax
//! (issue #18). Verifies that `render! { my_component { … } }` lowers
//! to `my_component(MyComponentProps::builder().…build())` and that
//! the auto-generated `Props` struct exposes all the expected setter
//! behaviours: `Into` coercion, `Option<T>` strip, `Children` default,
//! and generics.
//!
//! For `String`-shaped props we side-channel the received value into
//! a thread-local `Vec<String>` rather than trying to interpolate the
//! prop inside `render!` — interpolation routes through an `Fn +
//! 'static` effect closure that has to take ownership of any
//! non-`Copy` capture, which conflicts with the `#[component]`-wrapped
//! `FnMut` outer closure. (Real apps that need to interpolate a
//! `String` prop typically `clone()` it into a local, then move that
//! into the effect.)

use std::cell::RefCell;
use std::rc::Rc;
use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, __reset_pending_mount_for_tests, create_owner};
use whisker::runtime::view::{
    install_renderer, uninstall_renderer, DynRenderer, ElementHandle, View,
};
use whisker::with_owner;

// ----- Recording renderer ----------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    Create { id: u32, tag: ElementTag },
    SetAttr { id: u32, key: String, value: String },
    SetStyles { id: u32, css: String },
    Append { parent: u32, child: u32 },
}

#[derive(Default)]
struct Recorder {
    next: u32,
    log: Rc<RefCell<Vec<Op>>>,
}

impl DynRenderer for Recorder {
    fn create_element(&mut self, tag: ElementTag) -> ElementHandle {
        let id = self.next;
        self.next += 1;
        self.log.borrow_mut().push(Op::Create { id, tag });
        ElementHandle::from_raw(id)
    }
    fn release_element(&mut self, _h: ElementHandle) {}
    fn set_attribute(&mut self, h: ElementHandle, k: &str, v: &str) {
        self.log.borrow_mut().push(Op::SetAttr {
            id: h.id(),
            key: k.into(),
            value: v.into(),
        });
    }
    fn set_inline_styles(&mut self, h: ElementHandle, css: &str) {
        self.log.borrow_mut().push(Op::SetStyles {
            id: h.id(),
            css: css.into(),
        });
    }
    fn append_child(&mut self, p: ElementHandle, c: ElementHandle) {
        self.log.borrow_mut().push(Op::Append {
            parent: p.id(),
            child: c.id(),
        });
    }
    fn remove_child(&mut self, _p: ElementHandle, _c: ElementHandle) {}
    fn set_event_listener(&mut self, _h: ElementHandle, _name: &str, _cb: Box<dyn Fn() + 'static>) {}
    fn set_root(&mut self, _p: ElementHandle) {}
    fn flush(&mut self) {}
}

fn fresh() {
    __reset_for_tests();
    __reset_pending_mount_for_tests();
    PROP_CAPTURES.with(|c| c.borrow_mut().clear());
}

fn with_test_env<R>(f: impl FnOnce(Rc<RefCell<Vec<Op>>>) -> R) -> R {
    fresh();
    let rec = Recorder::default();
    let log = rec.log.clone();
    let prev = install_renderer(Box::new(rec));
    let owner = create_owner(None);
    let result = with_owner(owner, || f(log));
    uninstall_renderer(prev);
    result
}

// Side-channel for prop captures. Component bodies push the
// stringified values of the props they received here; tests read it
// back to assert what made it through the builder.
thread_local! {
    static PROP_CAPTURES: RefCell<Vec<String>> = RefCell::new(Vec::new());
}

fn captures() -> Vec<String> {
    PROP_CAPTURES.with(|c| c.borrow().clone())
}

fn push_capture(s: impl Into<String>) {
    PROP_CAPTURES.with(|c| c.borrow_mut().push(s.into()));
}

// ----- Test components -------------------------------------------------------

#[component]
fn no_props_component() -> ElementHandle {
    push_capture("no_props_component:invoked");
    render! { view() }
}

#[component]
fn one_string_prop(label: String) -> ElementHandle {
    push_capture(format!("one_string_prop:label={label}"));
    render! { view() }
}

#[component]
fn two_props(title: String, count: i32) -> ElementHandle {
    push_capture(format!("two_props:title={title},count={count}"));
    render! { view() }
}

#[component]
fn option_prop(alt: Option<String>) -> ElementHandle {
    // `.as_deref()` borrows the inner str so the FnMut closure
    // surrounding this body can be invoked more than once (per-
    // component remount). Calling `.unwrap_or_else(...)` directly
    // moves `alt` out of the closure.
    let v = alt
        .as_deref()
        .map(str::to_owned)
        .unwrap_or_else(|| "default".to_string());
    push_capture(format!("option_prop:alt={v}"));
    render! { view() }
}

#[component]
fn with_default_prop(#[prop(default = 5)] count: i32) -> ElementHandle {
    push_capture(format!("with_default_prop:count={count}"));
    render! { view() }
}

#[component]
fn with_children(label: String, children: Children) -> ElementHandle {
    push_capture(format!("with_children:label={label}"));
    // Materialise the children imperatively. We can't write the
    // ergonomic `view { {children()} }` here because `render!`'s
    // `{expr}` interpolation wraps the expression in a `Fn + 'static`
    // effect closure that moves `children` (`Rc<dyn Fn() -> View>`,
    // not `Copy`) out of `with_children`'s FnMut outer closure. Real
    // apps that need static-children mounting use the same shape;
    // dynamic-children patterns go through a signal + reactive
    // wrapper. (Tracked: tightening this ergonomics gap is a
    // follow-up to issue #18.)
    let h = whisker::runtime::view::create_element(ElementTag::View);
    let view = children();
    view.attach_to(h);
    h
}

#[component]
fn generic_label<T: std::fmt::Display + Clone + 'static>(value: T) -> ElementHandle {
    push_capture(format!("generic_label:value={value}"));
    render! { view() }
}

// ----- Tests -----------------------------------------------------------------

#[test]
fn component_with_no_props_invokable_via_braces() {
    with_test_env(|log| {
        let _h = render! { no_props_component() };

        // Side-channel: body ran.
        assert_eq!(captures(), vec!["no_props_component:invoked"]);

        // Sanity: at least one view element was created by the body.
        let view_creates = log
            .borrow()
            .iter()
            .filter(|op| matches!(op, Op::Create { tag: ElementTag::View, .. }))
            .count();
        assert!(view_creates >= 1);
    });
}

#[test]
fn component_with_string_prop_accepts_str_literal_via_into_coercion() {
    with_test_env(|_log| {
        // `label: "hello"` — typed-builder `setter(into)` should
        // convert the `&'static str` to `String`.
        let _h = render! { one_string_prop(label: "hello") };
        assert_eq!(captures(), vec!["one_string_prop:label=hello"]);
    });
}

#[test]
fn component_with_string_prop_accepts_owned_string() {
    with_test_env(|_log| {
        let owned = String::from("owned");
        let _h = render! { one_string_prop(label: owned) };
        assert_eq!(captures(), vec!["one_string_prop:label=owned"]);
    });
}

#[test]
fn component_with_two_props_uses_named_setters() {
    with_test_env(|_log| {
        let _h = render! {
            two_props(
                title: "Greeting",
                count: 42_i32,
            )
        };
        assert_eq!(captures(), vec!["two_props:title=Greeting,count=42"]);
    });
}

#[test]
fn option_prop_can_be_omitted() {
    with_test_env(|_log| {
        // No `alt:` kwarg — typed-builder's `default` kicks in → None.
        let _h = render! { option_prop() };
        assert_eq!(captures(), vec!["option_prop:alt=default"]);
    });
}

#[test]
fn option_prop_accepts_inner_via_strip_option_into() {
    with_test_env(|_log| {
        // `strip_option + into` lets the user pass `&str` directly
        // (no `Some(...)` wrapping needed).
        let _h = render! { option_prop(alt: "custom") };
        assert_eq!(captures(), vec!["option_prop:alt=custom"]);
    });
}

#[test]
fn prop_default_attribute_supplies_value_when_omitted() {
    with_test_env(|_log| {
        let _h = render! { with_default_prop() };
        assert_eq!(captures(), vec!["with_default_prop:count=5"]);
    });
}

#[test]
fn prop_default_attribute_overridable_at_call_site() {
    with_test_env(|_log| {
        let _h = render! { with_default_prop(count: 99) };
        assert_eq!(captures(), vec!["with_default_prop:count=99"]);
    });
}

#[test]
fn children_prop_receives_wrapped_closure() {
    with_test_env(|log| {
        // Nested children should be routed into a `.children(...)`
        // closure that the component invokes inside its body. The
        // body emits one outer `view`; the children closure emits
        // two `text` elements inside it.
        let _h = render! {
            with_children(label: "wrapper") {
                text(value: "child-1")
                text(value: "child-2")
            }
        };

        let captured = captures();
        assert_eq!(captured.len(), 1, "with_children should be invoked once");
        assert_eq!(captured[0], "with_children:label=wrapper");

        // Both children should have rendered their text.
        let texts: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } if key == "text" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|t| t == "child-1"), "got texts: {texts:?}");
        assert!(texts.iter().any(|t| t == "child-2"), "got texts: {texts:?}");
    });
}

#[test]
fn children_prop_defaults_to_empty_view_when_omitted() {
    with_test_env(|log| {
        let _h = render! {
            with_children(label: "only label")
        };

        assert_eq!(captures(), vec!["with_children:label=only label"]);

        // No raw_text elements (empty children closure → View::Empty).
        let raw_text_creates = log
            .borrow()
            .iter()
            .filter(|op| matches!(op, Op::Create { tag: ElementTag::RawText, .. }))
            .count();
        assert_eq!(raw_text_creates, 0, "no raw_text expected when children omitted");
    });
}

#[test]
fn generic_component_with_i32_arg() {
    with_test_env(|_log| {
        let _h = render! { generic_label(value: 7_i32) };
        assert_eq!(captures(), vec!["generic_label:value=7"]);
    });
}

#[test]
fn generic_component_with_string_arg() {
    with_test_env(|_log| {
        let _h = render! {
            generic_label(value: String::from("stringly typed"))
        };
        assert_eq!(captures(), vec!["generic_label:value=stringly typed"]);
    });
}

#[test]
fn nested_component_invocations() {
    with_test_env(|_log| {
        // Component inside a component, both via the new brace syntax.
        // Outer's children closure invokes the inner component.
        let _h = render! {
            with_children(label: "outer") {
                one_string_prop(label: "inner")
            }
        };

        let captured = captures();
        // Outer body runs first (sees its label), then the children
        // closure runs as part of the outer body's `view { {body} }`,
        // invoking one_string_prop.
        assert!(
            captured.iter().any(|s| s == "with_children:label=outer"),
            "outer captured; got: {captured:?}",
        );
        assert!(
            captured.iter().any(|s| s == "one_string_prop:label=inner"),
            "inner captured; got: {captured:?}",
        );
    });
}

#[test]
fn props_struct_is_constructable_directly() {
    // Smoke test: the auto-generated builder is reachable from user
    // code as `XxxProps::builder()`. Not the recommended path (users
    // go through `render!`), but it's the typed-builder API surface
    // and shouldn't break by accident.
    fresh();
    let owner = create_owner(None);
    let rec = Recorder::default();
    let prev = install_renderer(Box::new(rec));
    with_owner(owner, || {
        let _h = one_string_prop(
            OneStringPropProps::builder()
                .label("direct construction")
                .build(),
        );
    });
    uninstall_renderer(prev);

    assert_eq!(captures(), vec!["one_string_prop:label=direct construction"]);
}

#[test]
fn view_module_exposes_children_alias() {
    // The `Children` type alias must be reachable for users to
    // declare component props of that type.
    fn _accepts(_: whisker::Children) {}
    let c: whisker::Children = ::std::rc::Rc::new(|| View::Empty);
    _accepts(c);
}
