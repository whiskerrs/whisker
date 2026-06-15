//! Phase N — `ElementRef` end-to-end tests.
//!
//! Covers:
//! - `ElementRef::new()` allocates an unbound ref.
//! - Passing `ref:` on a `#[module_component]` call site binds the
//!   ref to the freshly-created element on mount.
//! - Disposing the surrounding owner clears the binding (the macro
//!   emits `on_cleanup(move || r.__unbind())`).
//! - `bound()` is reactive: an `effect(...)` observing it re-runs on
//!   mount and on unmount.
//! - `RefError::NotBound` is returned by `try_invoke` when unbound.
//! - `invoke_typed::<T>` round-trips primitives through `TryFrom<
//!   WhiskerValue>` (the bridge path is stubbed below — the typed
//!   conversion is exercised in isolation).
//!
//! No real C bridge: the in-memory `Recorder` swallows element ops,
//! and `invoke_element_method` returns an Error for unmounted refs,
//! so we focus on the binding-state machinery rather than dispatch.

use std::cell::RefCell;
use std::rc::Rc;

use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, effect, flush};
use whisker::runtime::view::{install_renderer, uninstall_renderer, DynRenderer, Element};
use whisker::Owner;

// ---- Minimal renderer ------------------------------------------------------

#[derive(Default)]
struct Recorder {
    next: ::std::cell::Cell<u32>,
    last_tag: Rc<RefCell<Option<String>>>,
}

impl Recorder {
    fn with_log() -> (Self, Rc<RefCell<Option<String>>>) {
        let r = Self::default();
        let log = r.last_tag.clone();
        (r, log)
    }
}

impl DynRenderer for Recorder {
    fn create_element(&self, _tag: ElementTag) -> Element {
        let id = self.next.get();
        self.next.set(id + 1);
        Element::from_raw(id)
    }
    fn create_element_by_name(&self, tag_name: &str) -> Element {
        let id = self.next.get();
        self.next.set(id + 1);
        *self.last_tag.borrow_mut() = Some(tag_name.to_string());
        Element::from_raw(id)
    }
    fn release_element(&self, _h: Element) {}
    fn set_attribute(&self, _h: Element, _k: &str, _v: &str) {}
    fn set_inline_styles(&self, _h: Element, _css: &str) {}
    fn append_child(&self, _p: Element, _c: Element) {}
    fn remove_child(&self, _p: Element, _c: Element) {}
    fn set_event_listener(
        &self,
        _h: Element,
        _n: &str,
        _bind_type: whisker::runtime::view::BindType,
        _cb: Box<dyn Fn(whisker::WhiskerValue) + 'static>,
    ) {
    }
    fn set_root(&self, _p: Element) {}
    fn flush(&self) {}
}

fn with_test_env<R>(f: impl FnOnce() -> R) -> R {
    __reset_for_tests();
    let (rec, _log) = Recorder::with_log();
    let prev = install_renderer(Box::new(rec));
    let owner = Owner::new(None);
    let out = owner.with(f);
    owner.dispose();
    uninstall_renderer(prev);
    out
}

// ---- Platform component declaration ---------------------------------------

#[whisker::module_component("x-ref-target")]
pub fn x_ref_target(value: Signal<String>) {}

// ---- Tests -----------------------------------------------------------------

#[test]
fn fresh_ref_is_unbound() {
    with_test_env(|| {
        let r = ElementRef::new();
        assert!(!r.is_bound());
        assert_eq!(r.element(), None);
    });
}

#[test]
fn passing_ref_at_call_site_binds_on_mount() {
    with_test_env(|| {
        let r = ElementRef::new();
        assert!(!r.is_bound());

        // Mount the element. The macro emits __bind on the returned
        // handle.
        let _h = render! {
            XRefTarget(ref: r, value: "hello")
        };

        assert!(r.is_bound(), "ref should be bound after mount");
        assert!(
            r.element().is_some(),
            "element() should return the bound Element handle"
        );
    });
}

#[test]
fn disposing_owner_unbinds_ref() {
    __reset_for_tests();
    let (rec, _log) = Recorder::with_log();
    let prev = install_renderer(Box::new(rec));

    // Allocate the ref in an outer owner so the bind/unbind cycle
    // doesn't take the ref's storage with it.
    let outer = Owner::new(None);
    let r = outer.with(ElementRef::new);

    // Mount inside an inner owner — the macro emits on_cleanup
    // against the inner owner.
    let inner = Owner::new(None);
    inner.with(|| {
        let _h = render! { XRefTarget(ref: r, value: "x") };
    });
    assert!(r.is_bound(), "ref should bind on mount");

    // Disposing the inner owner triggers the on_cleanup that
    // __unbinds the ref.
    inner.dispose();
    assert!(!r.is_bound(), "ref should unbind on inner-owner disposal");

    outer.dispose();
    uninstall_renderer(prev);
}

#[test]
fn bound_signal_is_reactive() {
    __reset_for_tests();
    let (rec, _log) = Recorder::with_log();
    let prev = install_renderer(Box::new(rec));

    let outer = Owner::new(None);
    let r = outer.with(ElementRef::new);

    let observed = Rc::new(RefCell::new(Vec::<bool>::new()));
    let observed_clone = observed.clone();

    // Effect must run in *some* owner; the outer one is fine.
    outer.with(|| {
        let bound = r.bound();
        effect(move || {
            observed_clone.borrow_mut().push(bound.get());
        });
    });
    flush();
    assert_eq!(*observed.borrow(), vec![false], "initial: unbound");

    let inner = Owner::new(None);
    inner.with(|| {
        let _h = render! { XRefTarget(ref: r, value: "x") };
    });
    flush();
    assert_eq!(
        *observed.borrow(),
        vec![false, true],
        "effect re-runs on bind"
    );

    inner.dispose();
    flush();
    assert_eq!(
        *observed.borrow(),
        vec![false, true, false],
        "effect re-runs on unbind"
    );

    outer.dispose();
    uninstall_renderer(prev);
}

#[test]
fn invoke_on_unbound_ref_returns_error_variant() {
    use whisker::platform_module::WhiskerValue;
    with_test_env(|| {
        let r = ElementRef::new();
        let v = r.invoke("play", WhiskerValue::Null);
        match v {
            WhiskerValue::Error(msg) => {
                assert!(
                    msg.contains("not bound"),
                    "error message should mention not-bound: got {msg:?}"
                );
            }
            other => panic!("expected Error variant, got {other:?}"),
        }
    });
}

#[test]
fn try_from_whisker_value_f64_rejects_string() {
    use whisker::platform_module::WhiskerValue;
    // `invoke_typed::<T>` now deserializes results via serde, but the
    // `TryFrom<WhiskerValue>` primitive conversions remain a public
    // surface — a String must not silently coerce to f64.
    let bad_payload = WhiskerValue::String("not-a-number".into());
    let result: Result<f64, _> = f64::try_from(bad_payload);
    let msg = result.expect_err("String can't convert to f64");
    assert!(
        msg.contains("expected Float"),
        "TryFrom<WhiskerValue> for f64 should reject String: {msg}"
    );
}

#[test]
fn try_from_whisker_value_primitives_roundtrip() {
    use whisker::platform_module::WhiskerValue;

    assert_eq!(<()>::try_from(WhiskerValue::Null), Ok(()));
    assert_eq!(bool::try_from(WhiskerValue::Bool(true)), Ok(true));
    assert_eq!(i64::try_from(WhiskerValue::Int(42)), Ok(42));
    assert_eq!(i32::try_from(WhiskerValue::Int(42)), Ok(42));
    assert!(i32::try_from(WhiskerValue::Int(i64::MAX)).is_err());
    assert_eq!(f64::try_from(WhiskerValue::Float(2.5)), Ok(2.5));
    // Int widens to f64
    assert_eq!(f64::try_from(WhiskerValue::Int(3)), Ok(3.0));
    assert_eq!(
        String::try_from(WhiskerValue::String("hi".into())),
        Ok("hi".to_string())
    );
    assert_eq!(
        Vec::<u8>::try_from(WhiskerValue::Bytes(vec![1, 2, 3])),
        Ok(vec![1, 2, 3])
    );

    // Type mismatch surfaces a String error.
    let err = bool::try_from(WhiskerValue::Int(1)).unwrap_err();
    assert!(err.contains("expected Bool"), "got {err:?}");
}

#[test]
fn element_ref_is_copy() {
    with_test_env(|| {
        let r = ElementRef::new();
        let r2 = r; // would be a use-of-moved-value error if not Copy
        assert_eq!(r.element(), r2.element());
    });
}
