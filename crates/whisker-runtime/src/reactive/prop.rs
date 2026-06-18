//! [`Signal<T>`] — the unified prop-value type used by built-in tags,
//! `#[component]`, and `#[whisker::module_component]` builders.
//!
//! ## Why this type exists
//!
//! Whisker's three "component" surfaces — built-in tags (`view`,
//! `text`, …), user `#[component]`s, and `#[whisker::module_component]`
//! — share a single calling convention for props:
//!
//! ```ignore
//! Component(prop: value)              // static — set once
//! Component(prop: signal)             // dynamic — tracked, reactively updated
//! Component(prop: computed(…))        // dynamic — memo-style derivation
//! ```
//!
//! `Signal<T>` encodes this in two variants:
//!
//! - [`Signal::Stored`] — a plain value the builder sets once and
//!   forgets about, held in an owner-bound [`StoredValue<T>`] arena
//!   slot so the whole enum stays `Copy`.
//! - [`Signal::Dynamic`] — a [`ReadSignal<T>`] handle. The builder
//!   wraps its read in an `effect`, so the underlying signal becomes
//!   a dependency and changes propagate to the element automatically.
//!
//! Builder methods accept `impl Into<Signal<T>>`, so the call-site
//! conversion is implicit: passing a `T`, a [`ReadSignal<T>`], a
//! [`RwSignal<T>`], or a [`Memo<T>`]-like `ReadSignal<T>` from
//! [`computed`] all "just work".
//!
//! ## Reactivity flow
//!
//! ```ignore
//! // user writes:
//! text(value: my_signal)
//!
//! // render! macro emits (no auto move-closure wrapping):
//! __tags::__text_ctor().value(my_signal).__h()
//!
//! // .value() does:
//! fn value(self, v: impl Into<Signal<String>>) -> Self {
//!     match v.into() {
//!         Signal::Stored(s) => set_attribute(h, "value", &s.get()),
//!         Signal::Dynamic(sig) => {
//!             effect(move || set_attribute(h, "value", &sig.get()));
//!             //                                          ^^^^^^^
//!             //                                          inside effect:
//!             //                                          sig.get() registers
//!             //                                          this effect as a
//!             //                                          subscriber of sig.
//!         }
//!     }
//!     self
//! }
//! ```
//!
//! Passing `my_signal.get()` instead — pre-reading the signal at the
//! call site — produces a `Signal::Stored`: the read happens once
//! before [`effect`] is even on the observer stack, so no
//! subscription is registered, and the prop becomes a one-shot
//! snapshot. This is the user-facing "static vs dynamic" distinction.
//!
//! ## Why not a closure variant?
//!
//! Earlier design passes considered a `Closure(Box<dyn Fn() -> T>)`
//! variant so callers could write `text(value: || format!(…))` and
//! get reactivity without naming an intermediate. Dropped: the
//! "closure ⇒ dynamic" rule is hard to internalise for newcomers,
//! and the explicit alternative (`computed(move || …)`) names the
//! derivation and gives it memoisation for free.
//!
//! [`computed`]: super::computed
//! [`effect`]: super::effect
//! [`Memo<T>`]: super::computed

use super::signal::{ReadSignal, RwSignal};
use super::stored::StoredValue;

/// Prop value: either a static `T` or a reactive [`ReadSignal<T>`].
///
/// Built-in tag builders / `#[component]` generated builders /
/// `#[whisker::module_component]` generated builders all accept
/// `impl Into<Signal<T>>`. The variant determines whether the
/// builder sets the attribute once ([`Stored`]) or wraps the read
/// in an `effect` ([`Dynamic`]).
///
/// [`Stored`]: Signal::Stored
/// [`Dynamic`]: Signal::Dynamic
///
/// `Copy` (and `Clone`) regardless of whether `T: Clone` — the
/// `Stored` arm holds an owner-bound [`StoredValue<T>`] (itself a
/// `Copy` arena handle) rather than an inline `T`, and the `Dynamic`
/// arm is a `Copy` [`ReadSignal`] handle (internally a [`NodeId`]).
/// This is what lets `#[component]` bodies (`FnMut`) move a
/// `Signal<T>` prop into several nested `move` closures without
/// `.clone()` — see whisker issue #8.
///
/// [`NodeId`]: super::NodeId
//
// NOTE: `Clone`/`Copy` are implemented by hand below rather than
// `#[derive]`d. A derived impl would add a spurious `T: Copy` bound,
// but both arms (`StoredValue<T>` / `ReadSignal<T>`) are `Copy` for
// *any* `T: 'static` — they're arena handles, not inline values — so
// `Signal<T>` must be `Copy` unconditionally (e.g. `Signal<String>`).
pub enum Signal<T: 'static> {
    /// Plain value, held in an owner-bound [`StoredValue<T>`] arena
    /// slot. The builder method that consumes this calls
    /// `set_attribute` / `set_inline_styles` / etc. exactly once
    /// with the value. No reactive subscription is set up; reading
    /// the `StoredValue` does not tick the dependency graph.
    Stored(StoredValue<T>),
    /// Reactive handle. The builder wraps its read in
    /// [`super::effect`] — each read inside that effect registers
    /// the underlying signal as a dependency, so subsequent
    /// `.set` / `.update` calls trigger an attribute re-write.
    ///
    /// Constructed via the [`From`] impls below — users typically
    /// pass `ReadSignal<T>`, `RwSignal<T>`, or the
    /// `ReadSignal<T>` returned by [`super::computed`].
    Dynamic(ReadSignal<T>),
}

// Hand-written so the bound is `T: 'static`, not `T: Copy` — see the
// note on the enum. Both variants wrap `Copy` arena handles.
impl<T: 'static> Clone for Signal<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: 'static> Copy for Signal<T> {}

impl<T: 'static + Clone> Signal<T> {
    /// Read the current value.
    ///
    /// - For [`Signal::Stored`]: returns a clone of the held value.
    ///   No reactivity is involved.
    /// - For [`Signal::Dynamic`]: forwards to [`ReadSignal::get`],
    ///   which **registers the underlying signal as a dependency**
    ///   of whatever effect / computed is currently on the observer
    ///   stack. Outside any tracking scope this is just a value
    ///   read.
    ///
    /// User-facing `#[component]` / `#[whisker::module_component]`
    /// bodies use this to read a `Signal<T>` prop:
    ///
    /// ```ignore
    /// #[component]
    /// fn dynamic_tile(color: Signal<String>) -> Element {
    ///     let style = computed(move || format!("color: {};", color.get()));
    ///     //                                                  ^^^^^^^^^^^^
    ///     //                                                  registers sig
    ///     //                                                  with the
    ///     //                                                  enclosing
    ///     //                                                  computed.
    ///     render! { view(style: style) { … } }
    /// }
    /// ```
    pub fn get(&self) -> T {
        match self {
            Signal::Stored(v) => v.get(),
            Signal::Dynamic(sig) => sig.get(),
        }
    }
}

// From impls — the conversions builder methods rely on.
//
// `impl<T> From<T> for Signal<T>` is the catch-all "plain value
// becomes Static" path; the others handle reactive handles. Coherence
// holds because the source types are concrete (`ReadSignal<T>`,
// `RwSignal<T>`) — they match a specific generic instantiation, not
// any `T`.

impl<T: 'static> From<T> for Signal<T> {
    fn from(v: T) -> Self {
        // NOTE: constructing a *static* `Signal` now allocates an
        // owner-bound arena slot ([`StoredValue::new`]) rather than
        // storing `v` inline — this is the cost of making `Signal<T>`
        // `Copy`. Mirrors `StoredValue`'s detached-owner fallback: if
        // there's no current reactive owner it logs a warning and
        // parks the value in a detached scope (no panic). So a static
        // `Signal` built entirely outside a reactive context still
        // works, it just isn't tied to a disposable scope.
        Signal::Stored(StoredValue::new(v))
    }
}

// `Signal<T: Default>::default() -> Signal::Stored(T::default())`.
// Used by `#[whisker::module_component]`'s builder: a prop the caller
// omits falls back to `unwrap_or_default()`, which produces a
// reasonable "attribute not set" value (`""` for `Signal<String>`,
// `false` for `Signal<bool>`, etc.). Phase 7-Φ.H.2 follow-up.
impl<T: 'static + Default> Default for Signal<T> {
    fn default() -> Self {
        Signal::Stored(StoredValue::new(T::default()))
    }
}

impl<T: 'static + Clone> From<ReadSignal<T>> for Signal<T> {
    fn from(s: ReadSignal<T>) -> Self {
        Signal::Dynamic(s)
    }
}

impl<T: 'static + Clone> From<RwSignal<T>> for Signal<T> {
    fn from(s: RwSignal<T>) -> Self {
        // RwSignal and ReadSignal share an arena `NodeId`; project to
        // the read-only handle for storage.
        Signal::Dynamic(s.read_only())
    }
}

// Convenience: `&str` literal → `Signal<String>::Static`. Without
// this specific impl users would have to write `.style("foo".to_string())`
// because `&str` doesn't directly impl `Into<Signal<String>>` (only
// `Into<Signal<&str>>` via the blanket `From<T> for Signal<T>`).
impl From<&str> for Signal<String> {
    fn from(s: &str) -> Self {
        Signal::Stored(StoredValue::new(s.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::{__reset_for_tests, computed, effect, flush, signal, RwSignal};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn static_variant_returns_held_value() {
        __reset_for_tests();
        let s: Signal<&'static str> = "hello".into();
        assert!(matches!(s, Signal::Stored(_)));
        assert_eq!(s.get(), "hello");
    }

    #[test]
    fn signal_is_copy() {
        // The whole point of issue #8: a non-Copy-`T` `Signal<T>` prop
        // can be moved into multiple `move` closures without `.clone()`.
        // This wouldn't even compile if `Signal<T>` weren't `Copy`.
        __reset_for_tests();

        fn assert_copy<C: Copy>(_: &C) {}

        let s: Signal<String> = "abc".into();
        assert_copy(&s);

        // Move the *same* binding into two independent `move` closures.
        let first = move || s.get();
        let second = move || s.get();
        assert_eq!(first(), "abc");
        assert_eq!(second(), "abc");

        // A dynamic Signal is Copy too.
        let (count, _set) = signal(5_i32).split();
        let d: Signal<i32> = count.into();
        assert_copy(&d);
        let a = move || d.get();
        let b = move || d.get();
        assert_eq!(a(), 5);
        assert_eq!(b(), 5);
    }

    #[test]
    fn read_signal_variant_returns_current_value() {
        __reset_for_tests();
        let (count, set_count) = signal(0_i32).split();
        let s: Signal<i32> = count.into();
        assert!(matches!(s, Signal::Dynamic(_)));
        assert_eq!(s.get(), 0);
        set_count.set(7);
        flush();
        assert_eq!(s.get(), 7);
    }

    #[test]
    fn rw_signal_converts_to_dynamic_variant() {
        __reset_for_tests();
        let rw = RwSignal::new(42_i32);
        let s: Signal<i32> = rw.into();
        assert!(matches!(s, Signal::Dynamic(_)));
        assert_eq!(s.get(), 42);
    }

    #[test]
    fn dynamic_signal_get_inside_effect_registers_dep() {
        // The whole reason this type exists — make sure a
        // `Signal::Dynamic(...).get()` call inside an effect
        // produces a subscription that fires on .set.
        __reset_for_tests();
        let (count, set_count) = signal(0_i32).split();
        let s: Signal<i32> = count.into();
        let log: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
        let log_clone = log.clone();
        effect(move || {
            log_clone.borrow_mut().push(s.get());
        });
        // initial run
        assert_eq!(&*log.borrow(), &[0]);
        // update → effect re-runs
        set_count.set(1);
        flush();
        set_count.set(2);
        flush();
        assert_eq!(&*log.borrow(), &[0, 1, 2]);
    }

    #[test]
    fn static_signal_get_inside_effect_does_not_subscribe() {
        // Symmetric check: a Static signal never registers a
        // subscription, so changes to "the value it was made from"
        // (none, really, since it's just a value) can't affect the
        // effect.
        __reset_for_tests();
        let s: Signal<i32> = 100.into();
        let log: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
        let log_clone = log.clone();
        effect(move || {
            log_clone.borrow_mut().push(s.get());
        });
        assert_eq!(&*log.borrow(), &[100]);
    }

    #[test]
    fn computed_return_value_converts_into_dynamic_signal() {
        // `computed` returns ReadSignal<T>, which `From<ReadSignal<T>>
        // for Signal<T>` picks up. End-to-end: derivations flow as
        // dynamic props.
        __reset_for_tests();
        let (count, set_count) = signal(0_i32).split();
        let doubled = computed(move || count.get() * 2);
        let s: Signal<i32> = doubled.into();
        assert!(matches!(s, Signal::Dynamic(_)));
        assert_eq!(s.get(), 0);
        set_count.set(5);
        flush();
        assert_eq!(s.get(), 10);
    }
}
