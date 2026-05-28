//! Safe Rust wrapper around the bridge's native list item provider.
//!
//! Lynx's `<list>` element retrieves its items through a callback
//! contract (`componentAtIndex` / `enqueueComponent`) — see
//! `whiskerrs/lynx#9`. The framework normally registers lepus closures
//! for these; Whisker has no JS runtime, so we wire a pair of Rust
//! closures through a C trampoline instead. This module hides the
//! `Box<dyn FnMut>` ↔ `*mut c_void` round-trip and the
//! `extern "C"` trampoline plumbing so the consumer (the future
//! `ListMount`) only sees a typed Rust API.
//!
//! # Lifetime
//!
//! `install` hands ownership of the boxed closures to the bridge,
//! which holds them inside the C++ `ListElement` as a
//! `std::shared_ptr<void>` with a custom deleter. When the
//! `ListElement` is destroyed (or another provider replaces this
//! one), the deleter fires and Rust's `Box::from_raw(...)` reclaims
//! the closures.
//!
//! # Stub disclosure
//!
//! Until the Lynx fork release `v3.7.0-whisker.9` ships, the bridge
//! C side of this is a no-op that frees the boxed closures
//! immediately — so installing a provider today is observable in
//! Rust (closures are dropped) but has no effect on the list. The
//! Rust contract here is final; the body of
//! `whisker_bridge_list_set_native_item_provider` is what changes
//! after the version bump.

use std::os::raw::{c_int, c_void};
use std::panic::{catch_unwind, AssertUnwindSafe};

use whisker_driver_sys::{
    self as ffi, LynxListComponentAtIndexFn, LynxListEnqueueComponentFn, LynxUserDataFreeFn,
};
use whisker_runtime::view::Element;

use crate::lynx::renderer::BridgeRenderer;

/// A native item provider for a Whisker-driven `<list>`. Holds the
/// two callbacks Lynx's list machinery will invoke on demand:
///
/// - `component_at_index(index, op_id, reuse)` — return the
///   `Element::sign` of the FiberElement to use for `index`, or
///   [`INVALID_INDEX`] on failure.
/// - `enqueue_component(sign)` — called when the item at `sign`
///   leaves the viewport so the provider can pool / drop it.
///
/// The closures are `FnMut` and `'static` because they live for as
/// long as the list element, mutating their own pool state across
/// calls.
pub struct NativeItemProvider {
    pub component_at_index: Box<dyn FnMut(u32, i64, bool) -> i32 + 'static>,
    pub enqueue_component: Option<Box<dyn FnMut(i32) + 'static>>,
}

/// Mirror of `LYNX_LIST_INVALID_INDEX` — returned by
/// `component_at_index` to signal "no element produced". The list will
/// skip the slot.
pub const INVALID_INDEX: i32 = ffi::LYNX_LIST_INVALID_INDEX;

// ---- Trampoline ---------------------------------------------------------
//
// The bridge passes our `Box<NativeItemProvider>` back as `*mut
// c_void` on every callback. The trampolines reconstruct a `&mut`
// reference and dispatch to the appropriate closure. Panics inside
// the closures are caught so they don't unwind across the FFI
// boundary (which is UB) — they become `INVALID_INDEX` returns or
// silent no-ops, with a `tracing::error!` for diagnosis.

extern "C" fn trampoline_component_at_index(
    index: u32,
    operation_id: i64,
    reuse_notification: c_int,
    user_data: *mut c_void,
) -> i32 {
    if user_data.is_null() {
        return INVALID_INDEX;
    }
    // SAFETY: `user_data` is the cookie we handed to the bridge in
    // `install`; the bridge guarantees exclusive access during the
    // callback (the list calls componentAtIndex serially on the
    // pipeline thread).
    let provider = unsafe { &mut *(user_data as *mut NativeItemProvider) };
    let result = catch_unwind(AssertUnwindSafe(|| {
        (provider.component_at_index)(index, operation_id, reuse_notification != 0)
    }));
    match result {
        Ok(sign) => sign,
        Err(_) => {
            eprintln!("whisker: native list provider panicked in component_at_index");
            INVALID_INDEX
        }
    }
}

extern "C" fn trampoline_enqueue_component(sign: i32, user_data: *mut c_void) {
    if user_data.is_null() {
        return;
    }
    let provider = unsafe { &mut *(user_data as *mut NativeItemProvider) };
    let Some(cb) = provider.enqueue_component.as_mut() else {
        return;
    };
    let _ = catch_unwind(AssertUnwindSafe(|| (cb)(sign)));
}

extern "C" fn trampoline_free(user_data: *mut c_void) {
    if user_data.is_null() {
        return;
    }
    // SAFETY: the cookie is exactly the `Box<NativeItemProvider>`
    // raw pointer we handed off in `install`. The bridge invokes this
    // exactly once per install, when the list element is destroyed
    // OR another provider replaces this one — so reclaiming the box
    // here is correct.
    unsafe {
        drop(Box::from_raw(user_data as *mut NativeItemProvider));
    }
}

// ---- install ------------------------------------------------------------

impl BridgeRenderer {
    /// Hand `provider` to the bridge so it drives the C++ `<list>`'s
    /// item lifecycle. Replaces any previously installed provider on
    /// `list_element` (the bridge frees the previous cookie). The
    /// closures inside `provider` survive until the list element is
    /// destroyed.
    ///
    /// Returns `false` if the renderer has no live native handle for
    /// the element (e.g. it was already released) — in that case the
    /// provider is dropped immediately.
    pub(crate) fn install_list_native_item_provider(
        &mut self,
        list_element: Element,
        provider: NativeItemProvider,
    ) -> bool {
        let Some(ptr) = self.lookup(list_element) else {
            // No live handle — drop the provider immediately so we
            // don't leak the boxed closures.
            drop(provider);
            return false;
        };
        // Forfeit ownership of the box into the bridge. The bridge
        // hands it back to `trampoline_free` when the element dies.
        let raw = Box::into_raw(Box::new(provider)) as *mut c_void;
        unsafe {
            ffi::whisker_bridge_list_set_native_item_provider(
                ptr.as_ptr(),
                trampoline_component_at_index as LynxListComponentAtIndexFn,
                trampoline_enqueue_component as LynxListEnqueueComponentFn,
                raw,
                trampoline_free as LynxUserDataFreeFn,
            );
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The trampoline must not unwind on closure panic — verifies the
    /// `catch_unwind` guards in `trampoline_component_at_index` /
    /// `trampoline_enqueue_component`. Important: an unwind across
    /// an `extern "C"` boundary is UB; this test pins the contract.
    #[test]
    fn trampoline_catches_panic_in_component_at_index() {
        let provider = Box::into_raw(Box::new(NativeItemProvider {
            component_at_index: Box::new(|_, _, _| panic!("boom")),
            enqueue_component: None,
        })) as *mut c_void;
        let sign = trampoline_component_at_index(0, 0, 0, provider);
        assert_eq!(sign, INVALID_INDEX);
        unsafe { trampoline_free(provider) };
    }

    #[test]
    fn trampoline_catches_panic_in_enqueue() {
        let provider = Box::into_raw(Box::new(NativeItemProvider {
            component_at_index: Box::new(|_, _, _| 0),
            enqueue_component: Some(Box::new(|_| panic!("boom"))),
        })) as *mut c_void;
        // Should not unwind / abort.
        trampoline_enqueue_component(42, provider);
        unsafe { trampoline_free(provider) };
    }

    #[test]
    fn trampoline_propagates_args_and_return() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let calls: Rc<RefCell<Vec<(u32, i64, bool)>>> = Rc::new(RefCell::new(Vec::new()));
        let calls_in = calls.clone();
        let provider = Box::into_raw(Box::new(NativeItemProvider {
            component_at_index: Box::new(move |i, op, reuse| {
                calls_in.borrow_mut().push((i, op, reuse));
                7 + i as i32
            }),
            enqueue_component: None,
        })) as *mut c_void;

        assert_eq!(trampoline_component_at_index(3, 100, 1, provider), 10);
        assert_eq!(trampoline_component_at_index(5, 200, 0, provider), 12);

        let calls = calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], (3, 100, true));
        assert_eq!(calls[1], (5, 200, false));
        drop(calls);
        unsafe { trampoline_free(provider) };
    }
}
