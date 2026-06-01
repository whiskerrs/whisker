//! [`on_back`] — LIFO back-handler chain.
//!
//! Components register a closure that returns `true` if it
//! consumed the back event, `false` to forward it down the chain.
//! Walking from most-recently-registered to oldest matches both
//! React Native's `BackHandler` and Android's
//! `OnBackPressedDispatcher` priority semantics.

use std::cell::RefCell;
use std::rc::Rc;

thread_local! {
    /// LIFO of registered back handlers. Each entry is wrapped in
    /// an `Rc` so guards can identify their own slot for removal
    /// without an O(n) sweep over the closures themselves.
    static HANDLERS: RefCell<Vec<HandlerSlot>> = const { RefCell::new(Vec::new()) };
}

struct HandlerSlot {
    id: u64,
    cb: Rc<dyn Fn() -> bool>,
}

thread_local! {
    static NEXT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// RAII guard returned by [`on_back`]. Dropping it removes the
/// handler from the chain — typically called automatically when
/// the owning component unmounts (via `on_cleanup`).
pub struct BackHandlerGuard {
    id: u64,
    /// Set to `false` by [`Self::forget`] to detach without
    /// removing the handler from the chain.
    active: bool,
}

impl BackHandlerGuard {
    /// Disarm the guard so dropping it does NOT remove the handler.
    /// Useful when the handler should outlive the local binding.
    pub fn forget(mut self) {
        self.active = false;
    }
}

impl Drop for BackHandlerGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let id = self.id;
        HANDLERS.with(|h| {
            let mut h = h.borrow_mut();
            if let Some(pos) = h.iter().position(|s| s.id == id) {
                h.remove(pos);
            }
        });
    }
}

/// Register a back handler. Returns a [`BackHandlerGuard`] that
/// removes the handler on drop.
pub fn on_back<F>(handler: F) -> BackHandlerGuard
where
    F: Fn() -> bool + 'static,
{
    let id = NEXT_ID.with(|c| {
        let v = c.get();
        c.set(v.wrapping_add(1));
        v
    });
    HANDLERS.with(|h| {
        h.borrow_mut().push(HandlerSlot {
            id,
            cb: Rc::new(handler),
        });
    });
    BackHandlerGuard { id, active: true }
}

/// Dispatch a back event through the chain.
///
/// Walks from most-recently-registered to oldest, stopping at the
/// first handler that returns `true`. Returns the same `bool`: the
/// caller (host platform glue) interprets `false` as "let the OS
/// handle it" (finish the activity / dismiss the view controller).
pub fn dispatch_back() -> bool {
    // Snapshot the chain so handlers can register/unregister
    // during dispatch without confusing the iteration.
    let snapshot: Vec<Rc<dyn Fn() -> bool>> =
        HANDLERS.with(|h| h.borrow().iter().rev().map(|s| Rc::clone(&s.cb)).collect());
    for cb in snapshot {
        if cb() {
            return true;
        }
    }
    false
}

/// Test helper: wipe the back-handler chain. Useful between unit
/// tests since the storage is thread-local.
#[doc(hidden)]
pub fn __reset_for_tests() {
    HANDLERS.with(|h| h.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn no_handlers_returns_false() {
        __reset_for_tests();
        assert!(!dispatch_back());
    }

    #[test]
    fn single_handler_consumes() {
        __reset_for_tests();
        let _g = on_back(|| true);
        assert!(dispatch_back());
    }

    #[test]
    fn lifo_walk_stops_at_first_true() {
        __reset_for_tests();
        let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
        let l1 = Rc::clone(&log);
        let _g1 = on_back(move || {
            l1.borrow_mut().push("oldest");
            true
        });
        let l2 = Rc::clone(&log);
        let _g2 = on_back(move || {
            l2.borrow_mut().push("newest");
            true
        });
        assert!(dispatch_back());
        // newest fires first, returns true, oldest never runs
        assert_eq!(log.borrow().as_slice(), ["newest"]);
    }

    #[test]
    fn false_handler_yields_to_next() {
        __reset_for_tests();
        let count = Rc::new(Cell::new(0));
        let c1 = Rc::clone(&count);
        let _g1 = on_back(move || {
            c1.set(c1.get() + 1);
            true
        });
        let c2 = Rc::clone(&count);
        let _g2 = on_back(move || {
            c2.set(c2.get() + 10);
            false
        });
        assert!(dispatch_back());
        // newest ran (+10, returned false), then oldest ran (+1, returned true)
        assert_eq!(count.get(), 11);
    }

    #[test]
    fn guard_drop_removes_handler() {
        __reset_for_tests();
        {
            let _g = on_back(|| true);
            assert!(dispatch_back());
        }
        assert!(!dispatch_back());
    }

    #[test]
    fn forget_detaches_drop_no_op() {
        __reset_for_tests();
        let g = on_back(|| true);
        g.forget();
        assert!(dispatch_back());
        __reset_for_tests(); // cleanup since we leaked
    }
}
