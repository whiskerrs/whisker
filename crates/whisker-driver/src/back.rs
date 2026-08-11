//! Process-global back-interception registry — Whisker's analogue of
//! React Native's `BackHandler`.
//!
//! [`on_back`] registers a handler for the platform back action
//! (Android's hardware button / back gesture). While a registration is
//! *active* — its [`BackGuard`] alive and its owning scope not paused —
//! the back action is delivered to the most recent active handler
//! instead of popping the navigation stack, and the platform's
//! interactive back preview is suppressed for that screen. With no
//! active handler and nothing left to pop, the platform default runs
//! (the app exits or moves to the background).
//!
//! Registration means consumption: there is no boolean return to
//! decline an event, because Android's predictive back requires the
//! interception decision *before* the gesture starts. Scope the guard
//! to the screen instead — a registration made inside a component is
//! tied to that component's scope, so a screen buried in the back
//! stack (paused) stops intercepting until it is on top again.
//!
//! Delivery is wired by the platform gesture component
//! (`whisker-router`'s `AndroidPredictiveBack`); without a router
//! mounted the registry is inert and the platform default applies.
//! Main-thread only, like [`crate::focus`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use whisker_runtime::reactive::{Owner, RwSignal};

struct Entry {
    id: u64,
    owner: Option<Owner>,
    handler: Rc<dyn Fn()>,
}

impl Entry {
    fn is_active(&self) -> bool {
        self.owner.is_none_or(|o| !o.is_paused())
    }
}

thread_local! {
    static HANDLERS: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
    static NEXT_ID: Cell<u64> = const { Cell::new(0) };
    static EXIT_IMPL: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };
    static REVISION: Cell<Option<RwSignal<u64>>> = const { Cell::new(None) };
}

/// Registry-change signal, minted lazily under a detached root so its
/// lifetime is the process, not whichever scope touches it first.
fn revision_signal() -> RwSignal<u64> {
    REVISION.with(|slot| {
        if let Some(sig) = slot.get() {
            return sig;
        }
        let sig = Owner::detached_root().with(|| RwSignal::new(0_u64));
        slot.set(Some(sig));
        sig
    })
}

fn bump_revision() {
    let sig = revision_signal();
    sig.set(sig.get_untracked() + 1);
}

/// Keeps its [`on_back`] registration alive. Dropping the guard
/// unregisters the handler; hold it in the registering component's
/// state so unmount unregisters automatically.
#[must_use = "the handler is unregistered when the guard drops"]
pub struct BackGuard {
    id: u64,
}

impl Drop for BackGuard {
    fn drop(&mut self) {
        HANDLERS.with(|h| h.borrow_mut().retain(|e| e.id != self.id));
        bump_revision();
    }
}

/// Intercept the platform back action while the returned guard lives.
///
/// The handler fires instead of a stack pop or an app exit whenever
/// back is triggered with this registration active. Registrations
/// stack: the most recent active one wins, so a screen pushed on top
/// of a guarded screen can install its own. A registration made
/// inside a component deactivates while that component's scope is
/// paused (e.g. buried in the router's back stack).
///
/// ```ignore
/// // Root screen: confirm before leaving the app.
/// let show_exit_dialog = RwSignal::new(false);
/// let _guard = back::on_back(move || show_exit_dialog.set(true));
/// // The dialog's "exit" button then calls `back::exit_app()`.
/// ```
pub fn on_back(handler: impl Fn() + 'static) -> BackGuard {
    let id = NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    HANDLERS.with(|h| {
        h.borrow_mut().push(Entry {
            id,
            owner: Owner::current(),
            handler: Rc::new(handler),
        })
    });
    bump_revision();
    BackGuard { id }
}

/// Trigger the platform's default back-out-of-the-app behavior
/// (Android: finish or move the task to the background, matching what
/// an unhandled back press would have done). Call from an [`on_back`]
/// handler once the user confirms leaving. No-op where no platform
/// exit is wired (iOS, or no router mounted).
pub fn exit_app() {
    let f = EXIT_IMPL.with(|e| e.borrow().clone());
    if let Some(f) = f {
        f();
    }
}

/// Framework wiring: install the platform exit implementation
/// [`exit_app`] calls. Registered by the platform gesture component.
pub fn set_exit_impl(f: impl Fn() + 'static) {
    EXIT_IMPL.with(|e| *e.borrow_mut() = Some(Rc::new(f)));
}

/// Framework wiring: whether an active handler exists right now.
/// Reads the registry's revision signal, so calling inside an
/// `effect` re-runs it on register / unregister.
pub fn has_active_handler() -> bool {
    let _ = revision_signal().get();
    HANDLERS.with(|h| h.borrow().iter().any(Entry::is_active))
}

/// Framework wiring: deliver one back action to the most recent
/// active handler. Returns `false` when no handler consumed it and
/// the caller should fall back to its own behavior (pop / default).
pub fn dispatch() -> bool {
    let handler = HANDLERS.with(|h| {
        h.borrow()
            .iter()
            .rev()
            .find(|e| e.is_active())
            .map(|e| e.handler.clone())
    });
    match handler {
        Some(f) => {
            f();
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset() {
        HANDLERS.with(|h| h.borrow_mut().clear());
        EXIT_IMPL.with(|e| *e.borrow_mut() = None);
    }

    #[test]
    fn dispatch_runs_the_most_recent_handler_and_reports_consumption() {
        reset();
        let hits: Rc<RefCell<Vec<&str>>> = Rc::new(RefCell::new(Vec::new()));
        let (h1, h2) = (hits.clone(), hits.clone());
        let _a = on_back(move || h1.borrow_mut().push("a"));
        let _b = on_back(move || h2.borrow_mut().push("b"));
        assert!(dispatch());
        assert_eq!(*hits.borrow(), vec!["b"]);
    }

    #[test]
    fn dropping_the_guard_unregisters() {
        reset();
        let hits = Rc::new(Cell::new(0));
        let h = hits.clone();
        let guard = on_back(move || h.set(h.get() + 1));
        assert!(has_active_handler());
        drop(guard);
        assert!(!has_active_handler());
        assert!(!dispatch());
        assert_eq!(hits.get(), 0);
    }

    #[test]
    fn paused_owner_deactivates_its_handler() {
        reset();
        let owner = Owner::new(None);
        let hits = Rc::new(Cell::new(0));
        let h = hits.clone();
        let _guard = owner.with(|| on_back(move || h.set(h.get() + 1)));
        assert!(has_active_handler());
        owner.pause();
        assert!(!has_active_handler());
        assert!(!dispatch());
        owner.resume();
        assert!(dispatch());
        assert_eq!(hits.get(), 1);
    }

    #[test]
    fn paused_top_falls_through_to_the_next_active_handler() {
        reset();
        let hits: Rc<RefCell<Vec<&str>>> = Rc::new(RefCell::new(Vec::new()));
        let (h1, h2) = (hits.clone(), hits.clone());
        let _under = on_back(move || h1.borrow_mut().push("under"));
        let top_owner = Owner::new(None);
        let _top = top_owner.with(|| on_back(move || h2.borrow_mut().push("top")));
        top_owner.pause();
        assert!(dispatch());
        assert_eq!(*hits.borrow(), vec!["under"]);
    }

    #[test]
    fn exit_app_routes_through_the_installed_impl() {
        reset();
        let hits = Rc::new(Cell::new(0));
        let h = hits.clone();
        exit_app();
        set_exit_impl(move || h.set(h.get() + 1));
        exit_app();
        assert_eq!(hits.get(), 1);
    }
}
