//! Panic capture and error-boundary routing.
//!
//! A panic in user code is caught where the runtime calls that code
//! ([`catch`]), routed to the nearest owner with an error boundary, and
//! delivered at the next safe point ([`deliver`]) rather than during the
//! unwind. A panic with no boundary above it is re-raised unchanged.

use std::any::Any;
use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, Location};
use std::rc::Rc;
use std::sync::Once;

use super::runtime::Owner;
use super::{try_with_runtime, with_runtime};

/// Prefix of a runtime-invariant panic message; such panics are never
/// caught, because recovering would hide corrupted runtime state.
pub(crate) const INVARIANT_PREFIX: &str = "whisker-runtime invariant violated: ";

/// Panics with a runtime-invariant violation that no boundary catches.
macro_rules! invariant {
    ($cond:expr, $msg:literal) => {
        if !$cond {
            ::std::panic!("{}{}", $crate::reactive::error::INVARIANT_PREFIX, $msg);
        }
    };
}
pub(crate) use invariant;

/// Where in the runtime the failing code was running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorPhase {
    /// A component body or a boundary's children.
    Render,
    /// An `effect` run.
    Effect,
    /// A `computed` recomputation.
    Computed,
    /// An event handler, `on_mount`, task poll or module callback.
    Callback,
    /// An `on_cleanup` callback.
    Cleanup,
}

/// Source position of a panic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorLocation {
    pub file: String,
    pub line: u32,
    pub column: u32,
}

/// A caught panic, as an error boundary receives it.
#[derive(Debug, Clone)]
pub struct ErrorInfo {
    pub message: String,
    pub location: Option<ErrorLocation>,
    pub phase: ErrorPhase,
    /// Captured only in debug builds, and only when `RUST_BACKTRACE` asks for it.
    pub backtrace: Option<String>,
}

impl std::fmt::Display for ErrorInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.location {
            Some(at) => write!(
                f,
                "{} ({}:{}:{})",
                self.message, at.file, at.line, at.column
            ),
            None => f.write_str(&self.message),
        }
    }
}

pub(crate) type BoundaryHandler = Rc<dyn Fn(ErrorInfo)>;

pub(crate) struct Failure {
    boundary: Owner,
    handler: BoundaryHandler,
    info: ErrorInfo,
}

struct LastPanic {
    message: String,
    location: Option<ErrorLocation>,
    backtrace: Option<String>,
}

thread_local! {
    static LAST_PANIC: RefCell<Option<LastPanic>> = const { RefCell::new(None) };
    static CATCH_SCOPES: RefCell<Vec<Option<Owner>>> = const { RefCell::new(Vec::new()) };
}

/// Makes the current owner an error boundary: a panic under it calls
/// `handler` at the next safe point, for as long as the owner lives.
pub fn set_error_boundary(handler: impl Fn(ErrorInfo) + 'static) {
    with_runtime(|rt| {
        let owner = rt.current_owner();
        invariant!(
            owner.is_some(),
            "set_error_boundary called outside any owner"
        );
        if let Some(scope) = owner.and_then(|owner| rt.owners.get_mut(owner)) {
            scope.error_boundary = Some(Rc::new(handler));
        }
    });
}

/// Registers `reset` on the current owner; [`reset_for_hot_patch`] runs it
/// while the owner lives, so a boundary showing its fallback retries
/// the patched code.
pub fn reset_on_hot_patch(reset: impl Fn() + 'static) {
    with_runtime(|rt| {
        if let Some(scope) = rt
            .current_owner()
            .and_then(|owner| rt.owners.get_mut(owner))
        {
            scope.hot_patch_reset = Some(Rc::new(reset));
        }
    });
}

/// Runs every live [`reset_on_hot_patch`] callback.
pub fn reset_for_hot_patch() {
    let resets: Vec<Rc<dyn Fn()>> = with_runtime(|rt| {
        rt.owners
            .values()
            .filter(|scope| !scope.closing)
            .filter_map(|scope| scope.hot_patch_reset.clone())
            .collect()
    });
    for reset in resets {
        reset();
    }
}

/// Runs `f`, routing a panic to the boundary nearest `owner`.
///
/// Returns `None` when `f` panicked and the panic was routed (or its
/// boundary is already gone). Re-raises the panic when no boundary is
/// above `owner`, and always for runtime-invariant violations.
pub fn catch<R>(owner: Option<Owner>, phase: ErrorPhase, f: impl FnOnce() -> R) -> Option<R> {
    install_hook();
    CATCH_SCOPES.with_borrow_mut(|scopes| scopes.push(owner));
    struct Pop;
    impl Drop for Pop {
        fn drop(&mut self) {
            CATCH_SCOPES.with_borrow_mut(|scopes| scopes.pop());
        }
    }
    let result = {
        let _pop = Pop;
        std::panic::catch_unwind(AssertUnwindSafe(f))
    };
    let payload = match result {
        Ok(value) => return Some(value),
        Err(payload) => payload,
    };
    if is_invariant(payload.as_ref()) {
        std::panic::resume_unwind(payload);
    }
    match with_runtime(|rt| route(rt, owner)) {
        Route::Boundary(boundary, handler) => {
            let info = take_info(payload.as_ref(), phase);
            with_runtime(|rt| {
                rt.failures.push(Failure {
                    boundary,
                    handler,
                    info,
                })
            });
            crate::runtime_wake::wake_runtime();
            None
        }
        Route::Moot => {
            LAST_PANIC.with_borrow_mut(Option::take);
            None
        }
        Route::Unhandled => std::panic::resume_unwind(payload),
    }
}

/// Whether the panic now unwinding will stop at an error boundary instead
/// of ending the process. Meant for panic hooks, which run before any
/// [`catch`]: a crash reporter records a caught panic as non-fatal.
pub fn panic_will_be_caught(payload: &(dyn Any + Send)) -> bool {
    if is_invariant(payload) {
        return false;
    }
    let scopes = CATCH_SCOPES.with_borrow(Clone::clone);
    try_with_runtime(|rt| {
        scopes
            .iter()
            .rev()
            .any(|owner| !matches!(route(rt, *owner), Route::Unhandled))
    })
    .unwrap_or(false)
}

/// Calls the boundary handler of every queued failure. Returns whether
/// any handler ran, so the caller knows to flush again.
pub(crate) fn deliver() -> bool {
    let failures = with_runtime(|rt| std::mem::take(&mut rt.failures));
    let mut delivered = false;
    for failure in failures {
        let alive = with_runtime(|rt| {
            rt.owners
                .get(failure.boundary)
                .is_some_and(|scope| !scope.closing)
        });
        if alive {
            (failure.handler)(failure.info);
            delivered = true;
        }
    }
    delivered
}

enum Route {
    Boundary(Owner, BoundaryHandler),
    /// The failing region was already disposed, so there is nothing to replace.
    Moot,
    Unhandled,
}

fn route(rt: &super::runtime::ReactiveRuntime, owner: Option<Owner>) -> Route {
    let Some(mut at) = owner else {
        return Route::Unhandled;
    };
    if rt.owners.get(at).is_none() {
        return Route::Moot;
    }
    loop {
        let Some(scope) = rt.owners.get(at) else {
            return Route::Unhandled;
        };
        if let Some(handler) = &scope.error_boundary {
            return if scope.closing {
                Route::Moot
            } else {
                Route::Boundary(at, handler.clone())
            };
        }
        match scope.parent {
            Some(parent) => at = parent,
            None => return Route::Unhandled,
        }
    }
}

fn is_invariant(payload: &(dyn Any + Send)) -> bool {
    payload_message(payload).is_some_and(|message| message.starts_with(INVARIANT_PREFIX))
}

fn payload_message(payload: &(dyn Any + Send)) -> Option<&str> {
    payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&'static str>().copied())
}

fn take_info(payload: &(dyn Any + Send), phase: ErrorPhase) -> ErrorInfo {
    let last = LAST_PANIC.with_borrow_mut(Option::take);
    let message = payload_message(payload)
        .map(str::to_owned)
        .or_else(|| last.as_ref().map(|last| last.message.clone()))
        .unwrap_or_else(|| "a panic".to_owned());
    let (location, backtrace) = last
        .map(|last| (last.location, last.backtrace))
        .unwrap_or_default();
    ErrorInfo {
        message,
        location,
        phase,
        backtrace,
    }
}

fn install_hook() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            record(info.payload(), info.location());
            previous(info);
        }));
    });
}

fn record(payload: &(dyn Any + Send), location: Option<&Location<'_>>) {
    let backtrace = if cfg!(debug_assertions) {
        let backtrace = std::backtrace::Backtrace::capture();
        (backtrace.status() == std::backtrace::BacktraceStatus::Captured)
            .then(|| backtrace.to_string())
    } else {
        None
    };
    let last = LastPanic {
        message: payload_message(payload).unwrap_or("a panic").to_owned(),
        location: location.map(|at| ErrorLocation {
            file: at.file().to_owned(),
            line: at.line(),
            column: at.column(),
        }),
        backtrace,
    };
    let _ = LAST_PANIC.try_with(|slot| {
        if let Ok(mut slot) = slot.try_borrow_mut() {
            *slot = Some(last);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::{__reset_for_tests, computed, flush, signal};

    fn boundary() -> (Owner, Rc<RefCell<Vec<ErrorInfo>>>) {
        __reset_for_tests();
        let caught = Rc::new(RefCell::new(Vec::new()));
        let owner = Owner::new(None);
        let sink = caught.clone();
        owner.with(|| set_error_boundary(move |info| sink.borrow_mut().push(info)));
        (owner, caught)
    }

    #[test]
    fn caught_panic_is_delivered_with_its_location_and_phase() {
        let (owner, caught) = boundary();
        let child = Owner::new(Some(owner));
        assert!(catch(Some(child), ErrorPhase::Effect, || panic!("boom")).is_none());
        assert!(caught.borrow().is_empty());

        flush();

        let caught = caught.borrow();
        assert_eq!(caught[0].message, "boom");
        assert_eq!(caught[0].phase, ErrorPhase::Effect);
        assert!(
            caught[0]
                .location
                .as_ref()
                .unwrap()
                .file
                .ends_with("error.rs")
        );
    }

    #[test]
    fn invariant_violation_is_never_caught() {
        let (owner, caught) = boundary();
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            catch(Some(owner), ErrorPhase::Effect, || {
                invariant!(false, "broken");
            })
        }));
        assert!(result.is_err());
        flush();
        assert!(caught.borrow().is_empty());
    }

    #[test]
    fn delivery_is_dropped_when_the_boundary_is_gone() {
        let (owner, caught) = boundary();
        catch(Some(owner), ErrorPhase::Effect, || panic!("boom"));
        owner.dispose();
        flush();
        assert!(caught.borrow().is_empty());
    }

    #[test]
    fn panic_will_be_caught_follows_the_boundary() {
        let (owner, _caught) = boundary();
        let payload: Box<dyn Any + Send> = Box::new("probe");
        let invariant: Box<dyn Any + Send> = Box::new(format!("{INVARIANT_PREFIX}probe"));

        let inside = catch(Some(owner), ErrorPhase::Effect, || {
            (
                panic_will_be_caught(payload.as_ref()),
                panic_will_be_caught(invariant.as_ref()),
            )
        });
        let unowned = catch(None, ErrorPhase::Effect, || {
            panic_will_be_caught(payload.as_ref())
        });

        assert_eq!(inside, Some((true, false)));
        assert_eq!(unowned, Some(false));
        assert!(!panic_will_be_caught(payload.as_ref()));
    }

    #[test]
    fn callback_panic_cancels_the_callback() {
        let (owner, caught) = boundary();
        let callback = owner.with(|| crate::lifetime::Scoped::new(|| panic!("handler")));
        assert_eq!(callback.with(|f| f()), None);
        assert!(!callback.is_active());
        flush();
        assert_eq!(caught.borrow()[0].phase, ErrorPhase::Callback);
    }

    #[test]
    fn computed_recomputes_after_a_panicking_run() {
        let (owner, caught) = boundary();
        let (source, set_source) = signal(1).split();
        let doubled = owner.with(|| {
            computed(move || {
                let value = source.get();
                assert!(value != 2, "two");
                value * 2
            })
        });
        set_source.set(2);
        flush();
        set_source.set(3);
        flush();
        assert_eq!(doubled.get(), 6);
        assert_eq!(caught.borrow().len(), 1);
    }
}
