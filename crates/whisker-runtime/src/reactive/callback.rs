//! [`Callback<In, Out>`] — a `Copy`, owner-bound wrapper around a
//! closure, for passing event handlers as component props.
//!
//! ## Why this type exists
//!
//! `#[component]` bodies are `FnMut` (whisker's hot-reload wrapper —
//! see [`Signal<T>`]'s docs). Moving a captured, non-`Copy` prop into a
//! nested `move` closure (e.g. `View(on_tap: move |_| on_tap())`)
//! violates that `FnMut` contract: the prop is moved out of the body on
//! the first call, so a second invocation can't move it again —
//! `error[E0507]`. The alternative is an `Rc<dyn Fn()>` prop plus a
//! manual `let cb = on_tap.clone();` before every handler closure.
//!
//! `Callback<In, Out>` avoids that: like [`Signal<T>`], it stores the
//! closure in an owner-bound [`StoredValue<T>`] arena slot and hands
//! back only a `Copy` handle, which moves into any number of `move`
//! closures for free. Mirrors Leptos's `Callback<In, Out>` — see
//! <https://docs.rs/leptos/latest/leptos/callback/index.html>.
//!
//! ```ignore
//! #[component]
//! fn tab_button(on_tap: Callback<()>) -> Element {
//!     render! {
//!         View(on_tap: move |_| on_tap.run(())) { … }
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
// `Clone`/`Copy` are hand-written for the same reason as `Signal<T>`:
// `#[derive]` would add spurious `In: Copy, Out: Copy` bounds, though
// neither is ever stored inline.
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
        // A Callback prop must move into several `move` closures
        // without `.clone()`.
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
