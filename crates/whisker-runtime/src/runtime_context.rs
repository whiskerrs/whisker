//! Instance-scoped state entered on a Host-owned UI thread.

use std::cell::{Cell, RefCell};

use crate::anim_hook::AnimationState;
use crate::dispatch::RuntimeDispatcher;
use crate::reactive::runtime::ReactiveRuntime;
use crate::runtime_wake::RuntimeWakeHandle;
use crate::tasks::TaskState;
use crate::view::renderer::ViewRuntimeState;
use crate::{anim_hook, dispatch, reactive, runtime_wake, tasks, view};

struct ContextState {
    reactive: ReactiveRuntime,
    view: ViewRuntimeState,
    tasks: TaskState,
    animation: AnimationState,
    pending_mount: Option<(crate::reactive::MountId, crate::view::Element)>,
    wake: Option<RuntimeWakeHandle>,
    dispatcher: Option<RuntimeDispatcher>,
}

impl ContextState {
    fn new(wake: RuntimeWakeHandle) -> Self {
        let dispatcher = RuntimeDispatcher::new(wake.clone());
        Self {
            reactive: ReactiveRuntime::new(),
            view: ViewRuntimeState::new(),
            tasks: TaskState::new(),
            animation: AnimationState::new(),
            pending_mount: None,
            wake: Some(wake),
            dispatcher: Some(dispatcher),
        }
    }

    fn swap_active(&mut self) {
        reactive::swap_runtime(&mut self.reactive);
        view::renderer::swap_runtime_state(&mut self.view);
        reactive::component::swap_pending_mount(&mut self.pending_mount);
        tasks::swap_state(&mut self.tasks);
        anim_hook::swap_state(&mut self.animation);
        runtime_wake::swap_active_wake(&mut self.wake);
        dispatch::swap_active(&mut self.dispatcher);
    }
}

/// Isolated single-threaded state for one mounted Whisker runtime.
///
/// A context owns reactive nodes, view bookkeeping, local async tasks, and
/// animation state while the Host is not executing it. [`Self::enter`] moves
/// that state into the current UI thread for one short event or frame and
/// restores the previous context even if application code panics.
pub struct RuntimeContext {
    state: RefCell<ContextState>,
    entered: Cell<bool>,
    closed: Cell<bool>,
}

impl RuntimeContext {
    /// Creates an isolated context connected to one any-thread Host wake-up.
    pub fn new(wake: RuntimeWakeHandle) -> Self {
        Self {
            state: RefCell::new(ContextState::new(wake)),
            entered: Cell::new(false),
            closed: Cell::new(false),
        }
    }

    /// Runs one non-reentrant unit of UI work in this context.
    pub fn enter<R>(&self, f: impl FnOnce() -> R) -> R {
        assert!(
            !self.closed.get(),
            "Whisker RuntimeContext cannot be entered after shutdown"
        );
        assert!(
            !self.entered.replace(true),
            "Whisker RuntimeContext cannot be entered re-entrantly"
        );
        self.state.borrow_mut().swap_active();

        struct Restore<'a>(&'a RuntimeContext);
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                self.0.state.borrow_mut().swap_active();
                self.0.entered.set(false);
            }
        }

        let _restore = Restore(self);
        f()
    }

    /// Returns whether this context is already executing on the current UI lane.
    pub fn is_entered(&self) -> bool {
        self.entered.get()
    }

    /// Drops all instance-owned work and closes externally retained dispatchers.
    ///
    /// The Host calls this after disposing the application owner during
    /// permanent unmount. The context cannot be entered again afterwards.
    pub fn shutdown(&self) {
        assert!(
            !self.entered.get(),
            "Whisker RuntimeContext cannot shut down while entered"
        );
        if self.closed.replace(true) {
            return;
        }
        let mut state = self.state.borrow_mut();
        if let Some(dispatcher) = &state.dispatcher {
            dispatcher.close();
        }
        state.reactive = ReactiveRuntime::new();
        state.view = ViewRuntimeState::new();
        state.tasks = TaskState::new();
        state.animation = AnimationState::new();
        state.pending_mount = None;
    }
}

impl Drop for RuntimeContext {
    fn drop(&mut self) {
        if let Some(dispatcher) = &self.state.get_mut().dispatcher {
            dispatcher.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::reactive::{RwSignal, effect};
    use crate::tasks::{run_until_stalled, spawn_local};

    fn context() -> RuntimeContext {
        RuntimeContext::new(RuntimeWakeHandle::new(|| {}))
    }

    #[test]
    fn reactive_arenas_are_isolated_on_one_thread() {
        let first = context();
        let second = context();

        let first_signal = first.enter(|| RwSignal::new(10));
        let second_signal = second.enter(|| RwSignal::new(20));

        assert_eq!(first.enter(|| first_signal.get()), 10);
        assert_eq!(second.enter(|| second_signal.get()), 20);

        first.enter(|| first_signal.set(11));
        assert_eq!(first.enter(|| first_signal.get()), 11);
        assert_eq!(second.enter(|| second_signal.get()), 20);
    }

    #[test]
    fn local_task_pools_are_isolated_on_one_thread() {
        let first = context();
        let second = context();
        let first_runs = Rc::new(Cell::new(0));
        let second_runs = Rc::new(Cell::new(0));

        first.enter(|| {
            let runs = Rc::clone(&first_runs);
            spawn_local(async move { runs.set(runs.get() + 1) });
        });
        second.enter(|| {
            let runs = Rc::clone(&second_runs);
            spawn_local(async move { runs.set(runs.get() + 1) });
        });

        first.enter(run_until_stalled);
        assert_eq!(first_runs.get(), 1);
        assert_eq!(second_runs.get(), 0);
        second.enter(run_until_stalled);
        assert_eq!(second_runs.get(), 1);
    }

    #[test]
    fn scheduled_effect_wakes_only_its_context() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let wakes = Arc::new(AtomicUsize::new(0));
        let wake_count = Arc::clone(&wakes);
        let context = RuntimeContext::new(RuntimeWakeHandle::new(move || {
            wake_count.fetch_add(1, Ordering::SeqCst);
        }));

        let signal = context.enter(|| {
            let signal = RwSignal::new(0);
            effect(move || {
                let _ = signal.get();
            });
            signal
        });
        let before = wakes.load(Ordering::SeqCst);
        context.enter(|| signal.set(1));

        assert_eq!(wakes.load(Ordering::SeqCst), before + 1);
    }

    #[test]
    fn dispatcher_marshals_worker_closure_into_owning_context() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let wakes = Arc::new(AtomicUsize::new(0));
        let wake_count = Arc::clone(&wakes);
        let context = RuntimeContext::new(RuntimeWakeHandle::new(move || {
            wake_count.fetch_add(1, Ordering::SeqCst);
        }));
        let (signal, dispatcher) = context.enter(|| {
            (
                RwSignal::new(0),
                crate::runtime_dispatcher().expect("active dispatcher"),
            )
        });

        std::thread::spawn(move || dispatcher.post(move || signal.set(7)))
            .join()
            .unwrap();
        assert_eq!(wakes.load(Ordering::SeqCst), 1);
        assert_eq!(
            context.enter(|| {
                crate::dispatch::drain_runtime_dispatches();
                signal.get()
            }),
            7
        );
    }

    #[test]
    fn shutdown_rejects_work_from_retained_dispatchers() {
        let context = context();
        let dispatcher = context.enter(|| crate::runtime_dispatcher().unwrap());

        context.shutdown();

        assert!(!dispatcher.post(|| panic!("closed callback must be dropped")));
    }
}
