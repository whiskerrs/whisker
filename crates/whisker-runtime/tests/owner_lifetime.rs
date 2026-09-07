use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use whisker_runtime::module::{ModuleHost, PlatformModule, with_module_host};
use whisker_runtime::reactive::{Owner, RwSignal, on_cleanup, resource};
use whisker_runtime::tasks::run_until_stalled;
use whisker_runtime::value::WhiskerValue;
use whisker_runtime::{RuntimeContext, RuntimeWakeHandle};

#[test]
fn resource_does_not_resume_fetcher_after_owner_disposal() {
    let runtime = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
    runtime.enter(|| {
        let owner = Owner::new(None);
        let (tx, rx) = futures_channel::oneshot::channel::<()>();
        let rx = Rc::new(RefCell::new(Some(rx)));
        let resumed = Rc::new(Cell::new(false));
        owner.with(|| {
            let source = RwSignal::new(42);
            let resumed = resumed.clone();
            resource(move || {
                let rx = rx.borrow_mut().take().unwrap();
                let resumed = resumed.clone();
                async move {
                    rx.await.unwrap();
                    resumed.set(true);
                    Ok(source.get())
                }
            });
        });
        run_until_stalled();
        owner.dispose();
        let _ = tx.send(());
        run_until_stalled();
        assert!(!resumed.get());
    });
}

#[test]
fn module_dispatch_skips_listener_removed_during_same_event() {
    let runtime = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
    runtime.enter(|| {
        let host = ModuleHost::new(|_, _, _, _, _| false, |_, _, _| {});
        with_module_host(&host, || {
            let a = Owner::new(None);
            let b = Owner::new(None);
            let hits = Rc::new(Cell::new(0));
            for (owner, other) in [(a, b), (b, a)] {
                owner.with(|| {
                    let value = RwSignal::new(1);
                    let hits = hits.clone();
                    let sub = PlatformModule::named("audit").on_event("change", move |_| {
                        other.dispose();
                        hits.set(hits.get() + value.get());
                    });
                    on_cleanup(move || drop(sub));
                });
            }
            host.dispatch_event("audit", "change", WhiskerValue::Null);
            assert_eq!(hits.get(), 1);
            a.dispose();
            b.dispose();
        });
    });
}

#[test]
fn input_dispatch_skips_listener_after_capture_unmounts_target() {
    use whisker_engine::whisker_style::StyleEnvironment;
    use whisker_protocol::{InputEvent, InputEventKind, SurfaceId};
    use whisker_runtime::view::{BindType, create_element, set_event_listener};
    use whisker_runtime::{ElementTag, RuntimeInstance, SurfaceRuntime};
    let surface = SurfaceRuntime::new(
        SurfaceId::new(1).unwrap(),
        StyleEnvironment::new(320.0, 480.0, 1.0, 14.0),
    );
    let mut runtime = RuntimeInstance::new(surface.clone(), RuntimeWakeHandle::new(|| {}));
    let hits = Rc::new(Cell::new(0));
    let captured_hits = hits.clone();
    runtime
        .mount(move || {
            let owner = Owner::new(None);
            owner.with(|| {
                let element = create_element(ElementTag::View);
                let value = RwSignal::new(1);
                set_event_listener(
                    element,
                    "click",
                    BindType::CaptureBind,
                    Box::new(move |_| owner.dispose()),
                );
                set_event_listener(
                    element,
                    "click",
                    BindType::Bind,
                    Box::new(move |_| captured_hits.set(value.get())),
                );
                element
            })
        })
        .unwrap();
    runtime
        .dispatch_input(&InputEvent {
            surface: surface.surface(),
            timestamp_ms: 1.0,
            kind: InputEventKind::Click,
            pointer: None,
            target: surface.root(),
            detail: WhiskerValue::Null,
        })
        .unwrap();
    assert_eq!(hits.get(), 0);
}

#[test]
fn mount_callback_is_cancelled_when_owner_disposes_before_flush() {
    let runtime = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
    runtime.enter(|| {
        let owner = Owner::new(None);
        owner.with(|| {
            let value = RwSignal::new(1);
            whisker_runtime::reactive::on_mount(move || {
                let _ = value.get();
            });
        });
        owner.dispose();
        whisker_runtime::reactive::flush_mounts();
    });
}

struct Dropped(Rc<Cell<usize>>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
fn context() -> RuntimeContext {
    RuntimeContext::new(RuntimeWakeHandle::new(|| {}))
}

#[test]
fn pending_and_unpolled_tasks_release_captures_immediately() {
    for poll in [false, true] {
        let runtime = context();
        runtime.enter(|| {
            let owner = Owner::new(None);
            let drops = Rc::new(Cell::new(0));
            let capture = Dropped(drops.clone());
            owner.with(|| {
                whisker_runtime::tasks::spawn_local(async move {
                    let _capture = capture;
                    std::future::pending::<()>().await;
                })
            });
            if poll {
                run_until_stalled();
            }
            owner.dispose();
            assert_eq!(drops.get(), 1);
            run_until_stalled();
            assert_eq!(drops.get(), 1);
        });
    }
}

#[test]
fn polls_restore_owner_for_nested_tasks_and_pause_does_not_cancel_them() {
    let runtime = context();
    runtime.enter(|| {
        let owner = Owner::new(None);
        let drops = Rc::new(Cell::new(0));
        let capture = Dropped(drops.clone());
        owner.with(|| {
            whisker_runtime::tasks::spawn_local(async move {
                assert_eq!(Owner::current(), Some(owner));
                whisker_runtime::tasks::spawn_local(async move {
                    assert_eq!(Owner::current(), Some(owner));
                    let _capture = capture;
                    std::future::pending::<()>().await;
                });
            })
        });
        owner.pause();
        run_until_stalled();
        assert_eq!(drops.get(), 0);
        owner.dispose();
        assert_eq!(drops.get(), 1);
    });
}

#[test]
fn disposal_inside_a_poll_waits_until_that_poll_returns() {
    let runtime = context();
    runtime.enter(|| {
        let owner = Owner::new(None);
        let value = owner.with(|| RwSignal::new(42));
        let finished = Rc::new(Cell::new(false));
        let result = finished.clone();
        owner.with(|| {
            whisker_runtime::tasks::spawn_local(async move {
                owner.dispose();
                assert_eq!(value.get(), 42);
                result.set(true);
            })
        });
        run_until_stalled();
        assert!(finished.get());
        assert_eq!(value.try_get(), None);
    });
}

#[test]
fn resource_replacement_drops_a_pending_fetch_without_a_wake() {
    let runtime = context();
    runtime.enter(|| {
        let owner = Owner::new(None);
        let drops = Rc::new(Cell::new(0));
        let source = owner.with(|| {
            let source = RwSignal::new(0);
            let drops = drops.clone();
            resource(move || {
                let _ = source.get();
                let capture = Dropped(drops.clone());
                async move {
                    let _capture = capture;
                    std::future::pending::<()>().await;
                    Ok(())
                }
            });
            source
        });
        run_until_stalled();
        source.set(1);
        whisker_runtime::reactive::flush();
        assert_eq!(drops.get(), 1);
        owner.dispose();
        assert_eq!(drops.get(), 2);
    });
}

#[test]
fn owner_cleanup_stops_native_observation_even_with_a_retained_subscription() {
    let runtime = context();
    let observations = Rc::new(RefCell::new(Vec::new()));
    let log = observations.clone();
    let host = ModuleHost::new(
        |_, _, _, _, _| false,
        move |_, _, observing| log.borrow_mut().push(observing),
    );
    let subscription = runtime.enter(|| {
        with_module_host(&host, || {
            let owner = Owner::new(None);
            let subscription = owner
                .with(|| PlatformModule::named("test").on_event("change", |_| panic!("disposed")));
            owner.dispose();
            assert!(!host.dispatch_event("test", "change", WhiskerValue::Null));
            subscription
        })
    });
    assert_eq!(*observations.borrow(), vec![true, false]);
    drop(subscription);
    assert_eq!(*observations.borrow(), vec![true, false]);
}

#[test]
fn callback_disposal_keeps_current_reads_valid_and_skips_nested_delivery() {
    let runtime = context();
    runtime.enter(|| {
        let host = ModuleHost::new(|_, _, _, _, _| false, |_, _, _| {});
        with_module_host(&host, || {
            let owner = Owner::new(None);
            let calls = Rc::new(Cell::new(0));
            let value = owner.with(|| RwSignal::new(42));
            let nested = host.clone();
            let result = calls.clone();
            let _sub = owner.with(|| {
                PlatformModule::named("test").on_event("change", move |_| {
                    owner.dispose();
                    nested.dispatch_event("test", "change", WhiskerValue::Null);
                    result.set(result.get() + 1);
                    assert_eq!(value.get(), 42);
                })
            });
            host.dispatch_event("test", "change", WhiskerValue::Null);
            assert_eq!(calls.get(), 1);
            assert_eq!(value.try_get(), None);
        });
    });
}

#[test]
fn callback_panic_restores_execution_and_owner_state() {
    let runtime = context();
    runtime.enter(|| {
        let owner = Owner::new(None);
        let value = owner.with(|| RwSignal::new(42));
        let callback = owner.with(|| {
            whisker_runtime::lifetime::Scoped::new(move || {
                owner.dispose();
                panic!("callback failed");
            })
        });
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback.with(|f| f())))
                .is_err()
        );
        assert_eq!(Owner::current(), None);
        assert_eq!(value.try_get(), None);
        let next = Owner::new(None);
        let value = next.with(|| RwSignal::new(1));
        next.dispose();
        assert_eq!(value.try_get(), None);
    });
}

#[test]
fn try_reads_track_live_values_and_distinguish_a_stored_none_from_disposal() {
    let runtime = context();
    runtime.enter(|| {
        let owner = Owner::new(None);
        let (value, tracked, untracked) = owner.with(|| {
            let value = RwSignal::new(None::<usize>);
            let tracked = Rc::new(Cell::new(0));
            let untracked = Rc::new(Cell::new(0));
            let t = tracked.clone();
            whisker_runtime::reactive::effect(move || {
                let _ = value.try_get();
                t.set(t.get() + 1);
            });
            let u = untracked.clone();
            whisker_runtime::reactive::effect(move || {
                let _ = value.try_get_untracked();
                u.set(u.get() + 1);
            });
            (value, tracked, untracked)
        });
        assert_eq!(value.try_get(), Some(None));
        value.set(Some(3));
        whisker_runtime::reactive::flush();
        assert_eq!(tracked.get(), 2);
        assert_eq!(untracked.get(), 1);
        assert_eq!(value.read_only().try_get(), Some(Some(3)));
        owner.dispose();
        assert_eq!(value.try_get_untracked(), None);
        assert_eq!(value.read_only().try_get(), None);
        assert!(!value.try_set(Some(4)));
    });
}

#[test]
fn a_parent_task_must_use_try_reads_for_a_shorter_lived_child_signal() {
    let runtime = context();
    runtime.enter(|| {
        let parent = Owner::new(None);
        let child = Owner::new(Some(parent));
        let value = child.with(|| RwSignal::new(42));
        let ran = Rc::new(Cell::new(false));
        let result = ran.clone();
        parent.with(|| {
            whisker_runtime::tasks::spawn_local(async move {
                assert_eq!(value.try_get(), None);
                result.set(true);
            })
        });
        child.dispose();
        run_until_stalled();
        assert!(ran.get());
        parent.dispose();
    });
}

#[test]
fn spawning_without_a_runtime_is_rejected() {
    assert!(std::panic::catch_unwind(|| whisker_runtime::tasks::spawn_local(async {})).is_err());
}

#[test]
fn closing_owners_reject_new_work_before_the_current_callback_returns() {
    let runtime = context();
    runtime.enter(|| {
        let owner = Owner::new(None);
        let callback = owner.with(|| {
            whisker_runtime::lifetime::Scoped::new(move || {
                owner.dispose();
                assert!(
                    std::panic::catch_unwind(|| whisker_runtime::tasks::spawn_local(async {}))
                        .is_err()
                );
                assert!(std::panic::catch_unwind(|| Owner::new(None)).is_err());
            })
        });
        callback.with(|f| f());
    });
}

#[test]
fn explicit_element_release_cancels_already_planned_bubble_callbacks() {
    use whisker_engine::whisker_style::StyleEnvironment;
    use whisker_protocol::{InputEvent, InputEventKind, SurfaceId};
    use whisker_runtime::view::{BindType, create_element, release_element, set_event_listener};
    use whisker_runtime::{ElementTag, RuntimeInstance, SurfaceRuntime};
    let surface = SurfaceRuntime::new(
        SurfaceId::new(1).unwrap(),
        StyleEnvironment::new(320.0, 480.0, 1.0, 14.0),
    );
    let mut runtime = RuntimeInstance::new(surface.clone(), RuntimeWakeHandle::new(|| {}));
    runtime
        .mount(|| {
            let element = create_element(ElementTag::View);
            set_event_listener(
                element,
                "click",
                BindType::CaptureBind,
                Box::new(move |_| release_element(element)),
            );
            set_event_listener(
                element,
                "click",
                BindType::Bind,
                Box::new(|_| panic!("released listener was dispatched")),
            );
            element
        })
        .unwrap();
    runtime
        .dispatch_input(&InputEvent {
            surface: surface.surface(),
            timestamp_ms: 1.0,
            kind: InputEventKind::Click,
            pointer: None,
            target: surface.root(),
            detail: WhiskerValue::Null,
        })
        .unwrap();
}

#[test]
fn explicit_unsubscribe_skips_other_callbacks_in_the_same_snapshot() {
    let runtime = context();
    runtime.enter(|| {
        let host = ModuleHost::new(|_, _, _, _, _| false, |_, _, _| {});
        with_module_host(&host, || {
            let subscriptions = Rc::new(RefCell::new(Vec::new()));
            let hits = Rc::new(Cell::new(0));
            for _ in 0..2 {
                let retained = subscriptions.clone();
                let hits = hits.clone();
                let sub = PlatformModule::named("test").on_event("change", move |_| {
                    let removed = std::mem::take(&mut *retained.borrow_mut());
                    drop(removed);
                    hits.set(hits.get() + 1);
                });
                subscriptions.borrow_mut().push(sub);
            }
            host.dispatch_event("test", "change", WhiskerValue::Null);
            assert_eq!(hits.get(), 1);
        });
    });
}
