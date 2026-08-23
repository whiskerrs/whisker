//! Instance-aware background-to-UI dispatch.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::RuntimeWakeHandle;

type Dispatch = Box<dyn FnOnce() + Send + 'static>;

struct DispatchState {
    open: bool,
    queue: VecDeque<Dispatch>,
}

/// Cloneable handle that posts work into one runtime instance.
///
/// Capture this handle while application code is running, move it to a worker
/// or Tokio task, and call [`Self::post`]. The closure runs the next time the
/// Host enters that runtime on its UI thread.
#[derive(Clone)]
pub struct RuntimeDispatcher {
    state: Arc<Mutex<DispatchState>>,
    wake: RuntimeWakeHandle,
}

impl RuntimeDispatcher {
    pub(crate) fn new(wake: RuntimeWakeHandle) -> Self {
        Self {
            state: Arc::new(Mutex::new(DispatchState {
                open: true,
                queue: VecDeque::new(),
            })),
            wake,
        }
    }

    /// Posts one closure and wakes the owning Host event loop.
    ///
    /// Returns `false` and drops the closure if the runtime has already been
    /// unmounted. This makes handles retained by long-lived workers safe to
    /// call after Host teardown.
    pub fn post(&self, callback: impl FnOnce() + Send + 'static) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !state.open {
            return false;
        }
        state.queue.push_back(Box::new(callback));
        drop(state);
        self.wake.wake();
        true
    }

    fn pop(&self) -> Option<Dispatch> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .queue
            .pop_front()
    }

    pub(crate) fn close(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.open = false;
        state.queue.clear();
    }
}

thread_local! {
    static ACTIVE_DISPATCHER: RefCell<Option<RuntimeDispatcher>> = const { RefCell::new(None) };
}

/// Returns the dispatcher for the currently entered runtime instance.
pub fn runtime_dispatcher() -> Option<RuntimeDispatcher> {
    ACTIVE_DISPATCHER.with_borrow(Clone::clone)
}

pub(crate) fn swap_active(dispatcher: &mut Option<RuntimeDispatcher>) {
    ACTIVE_DISPATCHER.with_borrow_mut(|active| std::mem::swap(active, dispatcher));
}

/// Drains posted closures for the active instance.
///
/// Host integrations call this only after installing the instance's renderer.
#[doc(hidden)]
pub fn drain_runtime_dispatches() {
    const DISPATCH_CAP: usize = 4096;
    let dispatcher = runtime_dispatcher();
    let Some(dispatcher) = dispatcher else {
        return;
    };
    for _ in 0..DISPATCH_CAP {
        let Some(callback) = dispatcher.pop() else {
            return;
        };
        callback();
    }
    crate::runtime_wake::wake_runtime();
    eprintln!(
        "whisker-runtime: UI dispatch exceeded {DISPATCH_CAP} callbacks; deferring the remainder"
    );
}
