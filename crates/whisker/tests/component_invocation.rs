//! Integration tests for the unified component invocation syntax:
//! `render! { MyComponent(…) }` lowers to
//! `MyComponent(MyComponentProps::builder().…build())`, and the
//! auto-generated `Props` struct exposes the expected setter
//! behaviours — `Into` coercion, `Option<T>` strip, `Children` default,
//! and generics.
//!
//! `String`-shaped props are side-channelled into a thread-local
//! `Vec<String>` rather than interpolated inside `render!`:
//! interpolation routes through an `Fn + 'static` effect closure that
//! must own any non-`Copy` capture, which conflicts with the
//! `#[component]`-wrapped `FnMut` outer closure. (Real apps `clone()`
//! such a prop into a local first.)

use std::cell::RefCell;
use std::rc::Rc;
use whisker::Owner;
use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, __reset_pending_mount_for_tests};
use whisker::runtime::view::{DynRenderer, Element, View, install_renderer, uninstall_renderer};

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
    next: ::std::cell::Cell<u32>,
    log: Rc<RefCell<Vec<Op>>>,
}

impl DynRenderer for Recorder {
    fn create_element(&self, tag: ElementTag) -> Element {
        let id = self.next.get();
        self.next.set(id + 1);
        self.log.borrow_mut().push(Op::Create { id, tag });
        Element::from_raw(id)
    }
    fn create_element_by_name(&self, _tag_name: &str) -> Element {
        let id = self.next.get();
        self.next.set(id + 1);
        self.log.borrow_mut().push(Op::Create {
            id,
            tag: ElementTag::View,
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
    fn set_inline_styles(&self, h: Element, css: &str) {
        self.log.borrow_mut().push(Op::SetStyles {
            id: h.id(),
            css: css.into(),
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
        _h: Element,
        _name: &str,
        _bind_type: whisker::runtime::view::BindType,
        _cb: Box<dyn Fn(whisker::WhiskerValue) + 'static>,
    ) {
    }
    fn set_root(&self, _p: Element) {}
    fn flush(&self) {}
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
    let owner = Owner::new(None);
    let result = owner.with(|| f(log));
    uninstall_renderer(prev);
    result
}

thread_local! {
    static PROP_CAPTURES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn captures() -> Vec<String> {
    PROP_CAPTURES.with(|c| c.borrow().clone())
}

pub(crate) fn push_capture(s: impl Into<String>) {
    PROP_CAPTURES.with(|c| c.borrow_mut().push(s.into()));
}

// ----- Test components -------------------------------------------------------

#[component]
fn no_props_component() -> Element {
    push_capture("no_props_component:invoked");
    render! { view() }
}

#[component]
fn one_string_prop(label: String) -> Element {
    push_capture(format!("one_string_prop:label={label}"));
    render! { view() }
}

#[component]
fn two_props(title: String, count: i32) -> Element {
    push_capture(format!("two_props:title={title},count={count}"));
    render! { view() }
}

#[component]
fn option_prop(alt: Option<String>) -> Element {
    // `.as_deref()` borrows the inner str so the surrounding FnMut can
    // be invoked more than once; `.unwrap_or_else(...)` directly would
    // move `alt` out of the closure.
    let v = alt
        .as_deref()
        .map(str::to_owned)
        .unwrap_or_else(|| "default".to_string());
    push_capture(format!("option_prop:alt={v}"));
    render! { view() }
}

#[component]
fn with_default_prop(#[prop(default = 5)] count: i32) -> Element {
    push_capture(format!("with_default_prop:count={count}"));
    render! { view() }
}

#[component]
fn with_children(label: String, children: Children) -> Element {
    push_capture(format!("with_children:label={label}"));
    // `children()` lowers to `view::mount_children(&children)`, which
    // borrows the Rc so the surrounding FnMut can re-run.
    render! {
        view() {
            children()
        }
    }
}

#[component]
fn generic_label<T: std::fmt::Display + Clone + 'static>(value: T) -> Element {
    push_capture(format!("generic_label:value={value}"));
    render! { view() }
}

// ----- Tests -----------------------------------------------------------------

#[test]
fn component_with_no_props_invokable_via_braces() {
    with_test_env(|log| {
        let _h = render! { NoPropsComponent() };

        assert_eq!(captures(), vec!["no_props_component:invoked"]);

        let view_creates = log
            .borrow()
            .iter()
            .filter(|op| {
                matches!(
                    op,
                    Op::Create {
                        tag: ElementTag::View,
                        ..
                    }
                )
            })
            .count();
        assert!(view_creates >= 1);
    });
}

#[test]
fn component_with_string_prop_accepts_str_literal_via_into_coercion() {
    with_test_env(|_log| {
        let _h = render! { OneStringProp(label: "hello") };
        assert_eq!(captures(), vec!["one_string_prop:label=hello"]);
    });
}

#[test]
fn component_with_string_prop_accepts_owned_string() {
    with_test_env(|_log| {
        let owned = String::from("owned");
        let _h = render! { OneStringProp(label: owned) };
        assert_eq!(captures(), vec!["one_string_prop:label=owned"]);
    });
}

#[test]
fn component_with_two_props_uses_named_setters() {
    with_test_env(|_log| {
        let _h = render! {
            TwoProps(
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
        // No `alt:` kwarg — the builder's default kicks in.
        let _h = render! { OptionProp() };
        assert_eq!(captures(), vec!["option_prop:alt=default"]);
    });
}

#[test]
fn option_prop_accepts_inner_via_strip_option_into() {
    with_test_env(|_log| {
        let _h = render! { OptionProp(alt: "custom") };
        assert_eq!(captures(), vec!["option_prop:alt=custom"]);
    });
}

#[test]
fn prop_default_attribute_supplies_value_when_omitted() {
    with_test_env(|_log| {
        let _h = render! { WithDefaultProp() };
        assert_eq!(captures(), vec!["with_default_prop:count=5"]);
    });
}

#[test]
fn prop_default_attribute_overridable_at_call_site() {
    with_test_env(|_log| {
        let _h = render! { WithDefaultProp(count: 99) };
        assert_eq!(captures(), vec!["with_default_prop:count=99"]);
    });
}

#[test]
fn children_prop_receives_wrapped_closure() {
    with_test_env(|log| {
        let _h = render! {
            WithChildren(label: "wrapper") {
                text(value: "child-1")
                text(value: "child-2")
            }
        };

        let captured = captures();
        assert_eq!(captured.len(), 1, "with_children should be invoked once");
        assert_eq!(captured[0], "with_children:label=wrapper");

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
            WithChildren(label: "only label")
        };

        assert_eq!(captures(), vec!["with_children:label=only label"]);

        let raw_text_creates = log
            .borrow()
            .iter()
            .filter(|op| {
                matches!(
                    op,
                    Op::Create {
                        tag: ElementTag::RawText,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(
            raw_text_creates, 0,
            "no raw_text expected when children omitted"
        );
    });
}

#[test]
fn generic_component_with_i32_arg() {
    with_test_env(|_log| {
        let _h = render! { GenericLabel(value: 7_i32) };
        assert_eq!(captures(), vec!["generic_label:value=7"]);
    });
}

#[test]
fn generic_component_with_string_arg() {
    with_test_env(|_log| {
        let _h = render! {
            GenericLabel(value: String::from("stringly typed"))
        };
        assert_eq!(captures(), vec!["generic_label:value=stringly typed"]);
    });
}

#[test]
fn nested_component_invocations() {
    with_test_env(|_log| {
        let _h = render! {
            WithChildren(label: "outer") {
                OneStringProp(label: "inner")
            }
        };

        let captured = captures();
        // The outer body runs first, then its children closure invokes
        // the inner component.
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
#[should_panic(expected = "required field `label` was not set")]
fn build_panics_when_required_field_missing() {
    // The builder checks required fields at runtime, so the panic
    // message must name the field for the user to find the call site.
    fresh();
    let _ = OneStringPropProps::builder().build();
}

#[test]
fn build_default_field_uses_user_supplied_default() {
    // Covers the build_assignment path for
    // `PropKind::Default { is_generic: false }`.
    fresh();
    let props = WithDefaultPropProps::builder().build();
    assert_eq!(props.count, 5);
}

#[test]
fn build_default_field_override_replaces_default() {
    fresh();
    let props = WithDefaultPropProps::builder().count(99).build();
    assert_eq!(props.count, 99);
}

#[test]
fn build_option_field_defaults_to_none() {
    fresh();
    let props = OptionPropProps::builder().build();
    assert_eq!(props.alt, None);
}

#[test]
fn build_option_field_strips_outer_option_in_setter() {
    // Setter takes `impl Into<String>` and wraps to `Some(_)` at build
    // — the strip-option ergonomics.
    fresh();
    let props = OptionPropProps::builder().alt("hi").build();
    assert_eq!(props.alt.as_deref(), Some("hi"));
}

#[test]
fn build_children_defaults_to_empty_view_closure() {
    fresh();
    let props = WithChildrenProps::builder().label("x").build();
    let v = (props.children)();
    assert!(matches!(v, whisker::runtime::view::View::Empty));
}

#[test]
fn build_into_setter_accepts_str_literal_for_string_field() {
    fresh();
    let props = OneStringPropProps::builder().label("from str").build();
    assert_eq!(props.label, "from str");
}

#[test]
fn build_into_setter_accepts_owned_string() {
    fresh();
    let owned: String = "from owned".into();
    let props = OneStringPropProps::builder().label(owned).build();
    assert_eq!(props.label, "from owned");
}

#[test]
fn build_generic_setter_accepts_concrete_type() {
    // A generic prop's setter takes `T` directly: the call site picks
    // `T` at the chain head.
    fresh();
    let props = GenericLabelProps::builder().value(7_i32).build();
    assert_eq!(props.value, 7);
}

#[test]
fn props_struct_is_constructable_directly() {
    // The builder must stay reachable as `XxxProps::builder()` even
    // though `render!` is the recommended path.
    fresh();
    let owner = Owner::new(None);
    let rec = Recorder::default();
    let prev = install_renderer(Box::new(rec));
    owner.with(|| {
        // The snake_case fn is private inside its inner module, so a
        // direct call must go through the PascalCase alias.
        let _h = OneStringProp(
            OneStringPropProps::builder()
                .label("direct construction")
                .build(),
        );
    });
    uninstall_renderer(prev);

    assert_eq!(
        captures(),
        vec!["one_string_prop:label=direct construction"]
    );
}

#[test]
fn view_module_exposes_children_alias() {
    fn _accepts(_: whisker::Children) {}
    let c: whisker::Children = ::std::rc::Rc::new(|| View::Empty);
    _accepts(c);
}

// A component whose module exports ONLY its PascalCase alias. The
// macro emits a same-named type alias in the type namespace alongside
// the value-namespace fn, so `render!` can lower `Card(...)` to
// `Card::builder()` without a separate `CardProps` import.
mod alias_only {
    use whisker::prelude::*;
    use whisker::runtime::view::Element;

    use crate::push_capture;

    #[component]
    pub fn card(title: String) -> Element {
        push_capture(format!("card:title={title}"));
        render! { view() }
    }
}

#[test]
fn component_usable_in_render_with_only_pascal_alias_imported() {
    // Imported by PascalCase name only — NOT `alias_only::CardProps`.
    // Without the type-namespace alias this file would not compile.
    use alias_only::Card;

    with_test_env(|_log| {
        let _h = render! { Card(title: "boxed") };
        assert_eq!(captures(), vec!["card:title=boxed"]);
    });
}
