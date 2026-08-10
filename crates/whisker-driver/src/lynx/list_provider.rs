//! Safe Rust wrapper around the bridge's native list item provider.
//!
//! Lynx's `<list>` element retrieves its items through a callback
//! contract (`componentAtIndex` / `enqueueComponent`) — see
//! `whiskerrs/lynx#9`. The framework normally registers lepus closures
//! for these; Whisker has no JS runtime, so we wire a pair of Rust
//! closures through a C trampoline instead. This module hides the
//! `Box<dyn FnMut>` ↔ `*mut c_void` round-trip and the `extern "C"`
//! trampoline plumbing so the consumer sees a typed Rust API.
//!
//! # Lifetime
//!
//! `install` hands ownership of the boxed closures to the bridge,
//! which holds them inside the C++ `ListElement` as a
//! `std::shared_ptr<void>` with a custom deleter. When the
//! `ListElement` is destroyed (or another provider replaces this
//! one), the deleter fires and Rust's `Box::from_raw(...)` reclaims
//! the closures.

use std::os::raw::{c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};

use whisker_driver_sys::{
    self as ffi, LynxListComponentAtIndexFn, LynxListEnqueueComponentFn, LynxUserDataFreeFn,
};
use whisker_runtime::view::Element;
use whisker_runtime::view::list_provider::NativeItemProvider;

use crate::lynx::renderer::BridgeRenderer;

// `NativeItemProvider` lives in `whisker-runtime` so view-layer code
// can build one without depending on the FFI layer; the trampolines
// and `Box<dyn FnMut>` ↔ raw pointer plumbing stay here.

/// Both crates must agree on what "no element produced" means; the
/// FFI value comes from the same `whisker-driver-sys` constant.
const _: () = assert!(
    whisker_runtime::view::list_provider::INVALID_ITEM_INDEX == ffi::LYNX_LIST_INVALID_INDEX
);

// The bridge passes our `Box<NativeItemProvider>` back as `*mut
// c_void` on every callback; the trampolines reconstruct a `&mut` and
// dispatch. Closure panics are caught — unwinding across the FFI
// boundary is UB — degrading to `ffi::LYNX_LIST_INVALID_INDEX` or a
// silent no-op, with a `tracing::error!` for diagnosis.

extern "C" fn trampoline_component_at_index(
    index: u32,
    operation_id: i64,
    reuse_notification: c_int,
    user_data: *mut c_void,
) -> i32 {
    if user_data.is_null() {
        return ffi::LYNX_LIST_INVALID_INDEX;
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
            ffi::LYNX_LIST_INVALID_INDEX
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
        &self,
        list_element: Element,
        provider: NativeItemProvider,
    ) -> bool {
        let Some(ptr) = self.lookup(list_element) else {
            // Drop the provider now, or the boxed closures leak.
            drop(provider);
            return false;
        };
        // Ownership passes to the bridge, which hands the box back to
        // `trampoline_free` when the element dies.
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

    /// An unwind across an `extern "C"` boundary is UB, so this pins
    /// the `catch_unwind` guards in both trampolines.
    #[test]
    fn trampoline_catches_panic_in_component_at_index() {
        let provider = Box::into_raw(Box::new(NativeItemProvider {
            component_at_index: Box::new(|_, _, _| panic!("boom")),
            enqueue_component: None,
        })) as *mut c_void;
        let sign = trampoline_component_at_index(0, 0, 0, provider);
        assert_eq!(sign, ffi::LYNX_LIST_INVALID_INDEX);
        trampoline_free(provider);
    }

    #[test]
    fn trampoline_catches_panic_in_enqueue() {
        let provider = Box::into_raw(Box::new(NativeItemProvider {
            component_at_index: Box::new(|_, _, _| 0),
            enqueue_component: Some(Box::new(|_| panic!("boom"))),
        })) as *mut c_void;
        trampoline_enqueue_component(42, provider);
        trampoline_free(provider);
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
        trampoline_free(provider);
    }
}
