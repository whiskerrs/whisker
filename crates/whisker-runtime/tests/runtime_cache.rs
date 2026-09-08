use std::cell::{Cell, RefCell};
use std::rc::Rc;
use whisker_runtime::reactive::{Owner, RwSignal, effect, flush, on_cleanup};
use whisker_runtime::{RuntimeContext, RuntimeWakeHandle, runtime_local};

fn context() -> RuntimeContext {
    RuntimeContext::new(RuntimeWakeHandle::new(|| {}))
}

#[test]
fn declarations_are_independent_and_non_clone_values_initialize_once_per_runtime() {
    struct State(Cell<usize>);
    runtime_local! {
        static FIRST: State = State(Cell::new(1));
        static SECOND: State = State(Cell::new(2));
    }
    let first = context();
    let second = context();
    first.enter(|| FIRST.with(|state| state.0.set(42)));
    second.enter(|| {
        assert_eq!(FIRST.with(|state| state.0.get()), 1);
        assert_eq!(SECOND.with(|state| state.0.get()), 2);
    });
    first.enter(|| assert_eq!(FIRST.with(|state| state.0.get()), 42));
    drop(first);
    context().enter(|| assert_eq!(FIRST.with(|state| state.0.get()), 1));
}

#[test]
fn nested_initializers_are_untracked_and_values_outlive_the_first_component() {
    runtime_local! {
        static SOURCE: RwSignal<usize> = RwSignal::new(1);
        static SNAPSHOT: usize = SOURCE.with(|signal| signal.get());
    }
    let runtime = context();
    runtime.enter(|| {
        let owner = Owner::new(None);
        let runs = Rc::new(Cell::new(0));
        owner.with(|| {
            let runs = runs.clone();
            effect(move || {
                SNAPSHOT.with(|_| runs.set(runs.get() + 1));
            });
        });
        SOURCE.with(|signal| signal.set(2));
        flush();
        assert_eq!(runs.get(), 1);
        owner.dispose();
        assert_eq!(SOURCE.with(|signal| signal.get()), 2);
        assert_eq!(SNAPSHOT.with(|value| *value), 1);
    });
}

#[test]
fn cache_values_can_be_borrowed_recursively_after_initialization() {
    runtime_local! { static VALUE: usize = 3; }
    context().enter(|| assert_eq!(VALUE.with(|a| VALUE.with(|b| a + b)), 6));
}

#[test]
fn recursive_initialization_panics_and_rolls_back_for_a_retry() {
    runtime_local! {
        static RECURSE: Cell<bool> = Cell::new(true);
        static VALUE: usize = if RECURSE.with(|flag| flag.replace(false)) { VALUE.with(|value| *value) } else { 7 };
    }
    context().enter(|| {
        assert!(std::panic::catch_unwind(|| VALUE.with(|_| ())).is_err());
        assert_eq!(VALUE.with(|value| *value), 7);
    });
}

#[test]
fn a_panicking_initializer_cleans_up_subscriptions_and_tasks_before_retrying() {
    use whisker_runtime::module::{ModuleHost, PlatformModule, with_module_host};
    runtime_local! {
        static ATTEMPTS: Cell<usize> = Cell::new(0);
        static VALUE: usize = {
            let sub = PlatformModule::named("test").on_event("change", |_| {});
            on_cleanup(move || drop(sub));
            whisker_runtime::tasks::spawn_local(std::future::pending());
            if ATTEMPTS.with(|attempts| { let n=attempts.get(); attempts.set(n+1); n }) == 0 { panic!("initialization failed"); }
            42
        };
    }
    let observations = Rc::new(RefCell::new(Vec::new()));
    let log = observations.clone();
    let host = ModuleHost::new(
        |_, _, _, _, _| false,
        move |_, _, active| log.borrow_mut().push(active),
    );
    let runtime = context();
    runtime.enter(|| {
        with_module_host(&host, || {
            assert!(std::panic::catch_unwind(|| VALUE.with(|_| ())).is_err());
            assert_eq!(*observations.borrow(), vec![true, false]);
            assert_eq!(VALUE.with(|value| *value), 42);
        })
    });
    runtime.shutdown();
    assert_eq!(*observations.borrow(), vec![true, false, true, false]);
}

#[test]
fn shutdown_drops_values_in_reverse_initialization_order_in_the_original_runtime() {
    struct Value {
        label: &'static str,
        signal: RwSignal<usize>,
        log: Rc<RefCell<Vec<(&'static str, usize)>>>,
    }
    impl Drop for Value {
        fn drop(&mut self) {
            self.log.borrow_mut().push((self.label, self.signal.get()));
        }
    }
    runtime_local! {
        static LOG: Rc<RefCell<Vec<(&'static str,usize)>>> = Rc::new(RefCell::new(Vec::new()));
        static FIRST: Value = Value { label: "first", signal: RwSignal::new(1), log: LOG.with(Clone::clone) };
        static SECOND: Value = Value { label: "second", signal: RwSignal::new(2), log: LOG.with(Clone::clone) };
    }
    for explicit in [true, false] {
        let first = context();
        let second = context();
        let log = first.enter(|| {
            FIRST.with(|_| ());
            SECOND.with(|_| ());
            LOG.with(Clone::clone)
        });
        second.enter(|| {
            let _signal = RwSignal::new(999);
            if explicit {
                first.shutdown();
                first.shutdown();
            }
            drop(first);
        });
        assert_eq!(*log.borrow(), vec![("second", 2), ("first", 1)]);
    }
}

#[test]
fn cache_rejects_use_outside_a_runtime_and_on_another_thread() {
    runtime_local! { static VALUE: usize = 1; }
    assert!(std::panic::catch_unwind(|| VALUE.with(|_| ())).is_err());
    context().enter(|| {
        assert_eq!(VALUE.with(|value| *value), 1);
        assert!(
            std::thread::spawn(|| std::panic::catch_unwind(|| VALUE.with(|_| ())).is_err())
                .join()
                .unwrap()
        );
    });
}

#[test]
fn shutdown_rejects_new_cache_initialization_but_remains_idempotent() {
    runtime_local! { static UNUSED: usize = 1; }
    let runtime = context();
    let rejected = Rc::new(Cell::new(false));
    runtime.enter(|| {
        let owner = Owner::new(None);
        let rejected = rejected.clone();
        owner.with(|| {
            on_cleanup(move || {
                rejected.set(std::panic::catch_unwind(|| UNUSED.with(|_| ())).is_err());
            })
        });
    });
    runtime.shutdown();
    assert!(rejected.get());
    runtime.shutdown();
}

#[test]
fn detached_owner_drop_is_routed_to_its_original_runtime() {
    use whisker_runtime::lifetime::OwnedOwner;
    let first = context();
    let second = context();
    let (owner, value) = first.enter(|| {
        let owner = OwnedOwner::new();
        let value = owner.with(|| RwSignal::new(1));
        (owner, value)
    });
    second.enter(|| {
        let other = RwSignal::new(2);
        drop(owner);
        assert_eq!(other.get(), 2);
    });
    first.enter(|| {
        whisker_runtime::drain_runtime_dispatches();
        assert_eq!(value.try_get(), None);
    });
}

#[test]
fn component_cleanup_can_read_runtime_cached_signals_during_shutdown() {
    runtime_local! { static VALUE: RwSignal<usize> = RwSignal::new(42); }
    let runtime = context();
    let seen = Rc::new(Cell::new(0));
    runtime.enter(|| {
        let owner = Owner::new(None);
        let seen = seen.clone();
        owner.with(|| {
            let value = VALUE.with(|value| *value);
            on_cleanup(move || seen.set(value.get()));
        });
    });
    runtime.shutdown();
    assert_eq!(seen.get(), 42);
}
