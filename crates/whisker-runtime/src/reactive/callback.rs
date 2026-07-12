//! [`Callback<In, Out>`] — a `Copy`, owner-bound wrapper around a
//! closure, for passing event handlers as component props.
//!
//! ## Why this type exists
//!
//! `#[component]` bodies are `FnMut` (whisker's hot-reload wrapper —
//! see [`Signal<T>`]'s docs, whisker issue #8). Moving a captured,
//! non-`Copy` prop into a nested `move` closure (e.g.
//! `view(on_tap: move |_| on_tap())`) violates that `FnMut` contract:
//! the prop is moved out of the body on the first call, so a second
//! invocation of the body (hot-reload, or any future re-render) can't
//! move it again — `error[E0507]`. Until now the workaround has been
//! an `Rc<dyn Fn()>` prop plus a manual `let cb = on_tap.clone();`
//! before each handler closure that needs it.
//!
//! `Callback<In, Out>` removes the workaround: like [`Signal<T>`], it
//! stores the actual closure in an owner-bound [`StoredValue<T>`]
//! arena slot and hands back only a `Copy` handle (a `NodeId`). A
//! `Copy` value never needs `.clone()` — it can be moved into any
//! number of `move` closures for free. Mirrors Leptos's
//! `Callback<In, Out>`, which solves the identical problem the same
//! way (an arena-backed, `Copy` handle) — see
//! <https://docs.rs/leptos/latest/leptos/callback/index.html>.
//!
//! ```ignore
//! #[component]
//! fn tab_button(on_tap: Callback<()>) -> Element {
//!     render! {
//!         view(on_tap: move |_| on_tap.run(())) { … }
//!         //           ^^^^^^^ moved into the closure directly — no `.clone()`
//!     }
//! }
//!
//! // call site — a plain closure converts via `impl Into<Callback<In, Out>>`,
//! // which #[component]'s generated builder setter accepts (non-generic
//! // fields get `setter(into)`):
//! TabButton(on_tap: move |_| { nav.select("/"); })
//! ```
//!
//! [`Signal<T>`]: super::prop::Signal

use super::stored::StoredValue;

/// A `Copy`, owner-bound callback: `In -> Out`. See the module docs for
/// why this exists instead of `Rc<dyn Fn(In) -> Out>`.
///
/// `In` defaults to `()` for the common "fire on tap, no payload"
/// case — construct with a zero-arg closure (`move || …`) and call
/// with [`Callback::call`] rather than `.run(())`.
//
// NOTE: `Clone`/`Copy` are hand-written rather than `#[derive]`d, same
// reasoning as `Signal<T>` — a derived impl would add spurious
// `In: Clone + Copy, Out: Clone + Copy` bounds, but neither type
// parameter is ever stored inline (they only appear in the boxed
// closure's signature), so `Callback<In, Out>` must be `Copy`
// unconditionally.
pub struct Callback<In: 'static = (), Out: 'static = ()> {
    inner: StoredValue<Box<dyn Fn(In) -> Out>>,
}

impl<In: 'static, Out: 'static> Clone for Callback<In, Out> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<In: 'static, Out: 'static> Copy for Callback<In, Out> {}

impl<In: 'static, Out: 'static> Callback<In, Out> {
    /// Wrap `f` in an owner-bound arena slot, returning a `Copy`
    /// handle to it. Prefer converting a plain closure via `.into()`
    /// at the prop call site — `Callback::new` is for constructing one
    /// to store in a local binding.
    pub fn new(f: impl Fn(In) -> Out + 'static) -> Self {
        Self {
            inner: StoredValue::new(Box::new(f) as Box<dyn Fn(In) -> Out>),
        }
    }

    /// Invoke the wrapped closure with `input`.
    pub fn run(&self, input: In) -> Out {
        self.inner.with(|f| f(input))
    }
}

impl<Out: 'static> Callback<(), Out> {
    /// Convenience for the default `In = ()` case: `.call()` instead
    /// of `.run(())`.
    pub fn call(&self) -> Out {
        self.run(())
    }
}

impl<F, In, Out> From<F> for Callback<In, Out>
where
    F: Fn(In) -> Out + 'static,
    In: 'static,
    Out: 'static,
{
    fn from(f: F) -> Self {
        Callback::new(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::__reset_for_tests;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn callback_is_copy() {
        // The whole point: a Callback prop can be moved into several
        // `move` closures without `.clone()`. Wouldn't compile if
        // `Callback` weren't `Copy`.
        __reset_for_tests();
        fn assert_copy<C: Copy>(_: &C) {}

        let cb: Callback<(), i32> = Callback::new(|_| 1);
        assert_copy(&cb);

        let first = move || cb.run(());
        let second = move || cb.run(());
        assert_eq!(first(), 1);
        assert_eq!(second(), 1);
    }

    #[test]
    fn run_forwards_input_and_returns_output() {
        __reset_for_tests();
        let cb: Callback<i32, i32> = Callback::new(|x| x * 2);
        assert_eq!(cb.run(21), 42);
    }

    #[test]
    fn call_is_run_with_unit_input() {
        __reset_for_tests();
        let log = Rc::new(Cell::new(0));
        let log2 = log.clone();
        let cb: Callback<()> = Callback::new(move |()| {
            log2.set(log2.get() + 1);
        });
        cb.call();
        cb.call();
        assert_eq!(log.get(), 2);
    }

    #[test]
    fn closure_converts_via_into() {
        __reset_for_tests();
        fn takes_callback(cb: impl Into<Callback<i32, i32>>) -> Callback<i32, i32> {
            cb.into()
        }
        let cb = takes_callback(|x| x + 1);
        assert_eq!(cb.run(9), 10);
    }

    #[test]
    fn each_clone_shares_the_same_underlying_closure() {
        __reset_for_tests();
        let log = Rc::new(Cell::new(0));
        let log2 = log.clone();
        let cb: Callback<()> = Callback::new(move |()| log2.set(log2.get() + 1));
        let a = cb;
        let b = cb;
        a.call();
        b.call();
        assert_eq!(log.get(), 2);
    }
}
