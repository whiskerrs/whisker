use std::cell::RefCell;
use std::convert::Infallible;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use whisker::prelude::*;
use whisker::{RuntimeInstance, RuntimeLifecycle, SurfaceRuntime};
use whisker_engine::whisker_layout::LayoutSize;
use whisker_engine::whisker_protocol::{
    InputEvent, InputEventKind, InputPoint, MeasuredSize, MeasurementMetrics, MeasurementReady,
    MeasurementRequest, MeasurementRequestId, MeasurementResponse, PointerId, PointerInput,
    PointerKind, ProtocolValue, SurfaceId,
};
use whisker_engine::whisker_style::StyleEnvironment;
use whisker_engine::{HostLayoutOptions, MeasurementHost, RecordingRenderer};
use whisker_runtime::RuntimeWakeHandle;

#[derive(Default)]
struct NoMeasurement;

impl MeasurementHost for NoMeasurement {
    type Error = Infallible;

    fn measure_batch(
        &mut self,
        _surface: SurfaceId,
        requests: &[MeasurementRequest],
        _responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        assert!(requests.is_empty());
        Ok(())
    }
}

#[derive(Default)]
struct PendingTextMeasurement {
    request: Option<MeasurementRequest>,
}

impl MeasurementHost for PendingTextMeasurement {
    type Error = Infallible;

    fn measure_batch(
        &mut self,
        _surface: SurfaceId,
        requests: &[MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        assert_eq!(requests.len(), 1);
        let request = requests[0].clone();
        responses.push(MeasurementResponse::Pending {
            key: request.key,
            environment_epoch: request.environment_epoch,
            request_id: MeasurementRequestId::new(77).unwrap(),
            provisional: None,
        });
        self.request = Some(request);
        Ok(())
    }
}

fn surface(id: u64) -> SurfaceRuntime {
    SurfaceRuntime::new(
        SurfaceId::new(id).unwrap(),
        StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
    )
}

#[test]
fn host_drives_mount_frame_pause_resume_and_unmount() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let wake_count = Arc::clone(&wakes);
    let surface = surface(31);
    let mut runtime = RuntimeInstance::new(
        surface.clone(),
        RuntimeWakeHandle::new(move || {
            wake_count.fetch_add(1, Ordering::SeqCst);
        }),
    );

    runtime
        .mount(|| render! { view(style: css!(width: px(100), height: px(100))) })
        .unwrap();
    assert_eq!(runtime.lifecycle(), RuntimeLifecycle::Running);
    assert!(wakes.load(Ordering::SeqCst) > 0);

    let mut measurements = NoMeasurement;
    let mut sink = RecordingRenderer::new(surface.surface());
    let first = runtime
        .drive_frame(
            1.0,
            LayoutSize::new(200.0, 100.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            HostLayoutOptions::default(),
        )
        .unwrap();
    assert!(first.frame.presentation.is_some());
    assert!(!first.needs_frame);

    runtime.pause().unwrap();
    assert_eq!(runtime.lifecycle(), RuntimeLifecycle::Paused);
    assert!(
        runtime
            .drive_frame(
                2.0,
                LayoutSize::new(200.0, 100.0),
                1,
                1,
                &mut measurements,
                &mut sink,
                HostLayoutOptions::default(),
            )
            .is_err()
    );
    runtime.resume().unwrap();
    assert_eq!(runtime.lifecycle(), RuntimeLifecycle::Running);
    runtime.unmount().unwrap();
    assert_eq!(runtime.lifecycle(), RuntimeLifecycle::Unmounted);
}

#[test]
fn pointer_hit_test_routes_capture_target_and_bubble_in_rust() {
    let surface = surface(32);
    let mut runtime = RuntimeInstance::new(surface.clone(), RuntimeWakeHandle::new(|| {}));
    let log = Rc::new(RefCell::new(Vec::new()));
    let root_capture = Rc::clone(&log);
    let root_bubble = Rc::clone(&log);
    let child_target = Rc::clone(&log);

    runtime
        .mount(move || {
            render! {
                view(
                    style: css!(width: px(100), height: px(100)),
                    on_capture_tap: move |_| root_capture.borrow_mut().push("root-capture"),
                    on_tap: move |_| root_bubble.borrow_mut().push("root-bubble"),
                ) {
                    view(
                        style: css!(width: px(50), height: px(50)),
                        on_tap: move |_| child_target.borrow_mut().push("child-target"),
                    )
                }
            }
        })
        .unwrap();

    let mut measurements = NoMeasurement;
    let mut sink = RecordingRenderer::new(surface.surface());
    runtime
        .drive_frame(
            1.0,
            LayoutSize::new(200.0, 100.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            HostLayoutOptions::default(),
        )
        .unwrap();

    let event = InputEvent {
        surface: surface.surface(),
        timestamp_ms: 2.0,
        kind: InputEventKind::Tap,
        pointer: Some(PointerInput {
            id: PointerId::new(1).unwrap(),
            kind: PointerKind::Touch,
            position: InputPoint { x: 10.0, y: 10.0 },
            buttons: 1,
            changed_button: 0,
        }),
        target: None,
        detail: ProtocolValue::Null,
    };
    let dispatch = runtime.dispatch_input(&event).unwrap();

    assert!(dispatch.consumed);
    assert_eq!(dispatch.listener_count, 3);
    assert_eq!(
        log.borrow().as_slice(),
        ["root-capture", "child-target", "root-bubble"]
    );
}

#[test]
fn background_completion_parks_while_paused_and_resumes_on_host_drive() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let wake_count = Arc::clone(&wakes);
    let surface = surface(33);
    let mut runtime = RuntimeInstance::new(
        surface.clone(),
        RuntimeWakeHandle::new(move || {
            wake_count.fetch_add(1, Ordering::SeqCst);
        }),
    );
    let result = Rc::new(std::cell::Cell::new(0));
    let task_result = Rc::clone(&result);
    let (started_tx, started_rx) = mpsc::channel();
    let (finish_tx, finish_rx) = mpsc::channel();
    let (completed_tx, completed_rx) = mpsc::channel();

    runtime
        .mount(move || {
            whisker::spawn_local(async move {
                let value = whisker::run_blocking(move || {
                    started_tx.send(()).unwrap();
                    finish_rx.recv().unwrap();
                    completed_tx.send(()).unwrap();
                    42
                })
                .await;
                task_result.set(value);
            });
            render! { view(style: css!(width: px(100), height: px(100))) }
        })
        .unwrap();

    let mut measurements = NoMeasurement;
    let mut sink = RecordingRenderer::new(surface.surface());
    runtime
        .drive_frame(
            1.0,
            LayoutSize::new(200.0, 100.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            HostLayoutOptions::default(),
        )
        .unwrap();
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    runtime.pause().unwrap();
    let before_completion = wakes.load(Ordering::SeqCst);
    finish_tx.send(()).unwrap();
    completed_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    std::thread::sleep(Duration::from_millis(10));
    assert_eq!(wakes.load(Ordering::SeqCst), before_completion);
    assert_eq!(result.get(), 0);

    runtime.resume().unwrap();
    assert!(wakes.load(Ordering::SeqCst) > before_completion);
    runtime
        .drive_frame(
            2.0,
            LayoutSize::new(200.0, 100.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            HostLayoutOptions::default(),
        )
        .unwrap();
    assert_eq!(result.get(), 42);
}

#[test]
fn reentrant_host_input_is_queued_until_the_event_boundary() {
    let surface = surface(34);
    let runtime = Rc::new(RefCell::new(RuntimeInstance::new(
        surface.clone(),
        RuntimeWakeHandle::new(|| {}),
    )));
    let log = Rc::new(RefCell::new(Vec::new()));
    let tap_log = Rc::clone(&log);
    let click_log = Rc::clone(&log);
    let nested_runtime = Rc::downgrade(&runtime);
    let nested_surface = surface.surface();

    runtime
        .borrow_mut()
        .mount(move || {
            render! {
                view(
                    style: css!(width: px(100), height: px(100)),
                    on_tap: move |_| {
                        tap_log.borrow_mut().push("tap");
                        let nested = InputEvent {
                            surface: nested_surface,
                            timestamp_ms: 3.0,
                            kind: InputEventKind::Click,
                            pointer: Some(PointerInput {
                                id: PointerId::new(1).unwrap(),
                                kind: PointerKind::Touch,
                                position: InputPoint { x: 10.0, y: 10.0 },
                                buttons: 0,
                                changed_button: 0,
                            }),
                            target: None,
                            detail: ProtocolValue::Null,
                        };
                        let queued = nested_runtime
                            .upgrade()
                            .unwrap()
                            .borrow()
                            .dispatch_input(&nested)
                            .unwrap();
                        assert!(queued.queued);
                    },
                    on_click: move |_| click_log.borrow_mut().push("click"),
                )
            }
        })
        .unwrap();
    let mut measurements = NoMeasurement;
    let mut sink = RecordingRenderer::new(surface.surface());
    runtime
        .borrow()
        .drive_frame(
            1.0,
            LayoutSize::new(200.0, 100.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            HostLayoutOptions::default(),
        )
        .unwrap();
    let tap = InputEvent {
        surface: surface.surface(),
        timestamp_ms: 2.0,
        kind: InputEventKind::Tap,
        pointer: Some(PointerInput {
            id: PointerId::new(1).unwrap(),
            kind: PointerKind::Touch,
            position: InputPoint { x: 10.0, y: 10.0 },
            buttons: 1,
            changed_button: 0,
        }),
        target: None,
        detail: ProtocolValue::Null,
    };

    runtime.borrow().dispatch_input(&tap).unwrap();
    assert_eq!(log.borrow().as_slice(), ["tap", "click"]);
}

#[test]
fn deferred_measurement_event_wakes_and_completes_the_next_frame() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let wake_count = Arc::clone(&wakes);
    let surface = surface(35);
    let mut runtime = RuntimeInstance::new(
        surface.clone(),
        RuntimeWakeHandle::new(move || {
            wake_count.fetch_add(1, Ordering::SeqCst);
        }),
    );
    runtime
        .mount(|| render! { text(value: "deferred", style: css!(font_size: px(16))) })
        .unwrap();
    let mut measurements = PendingTextMeasurement::default();
    let mut sink = RecordingRenderer::new(surface.surface());
    let blocked = runtime
        .drive_frame(
            1.0,
            LayoutSize::new(200.0, 100.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            HostLayoutOptions::default(),
        )
        .unwrap();
    assert!(!blocked.frame.layout.has_layout());
    assert!(blocked.frame.presentation.is_none());
    let request = measurements.request.clone().unwrap();
    let before = wakes.load(Ordering::SeqCst);
    runtime
        .measurement_ready(&MeasurementReady {
            key: request.key,
            request_id: MeasurementRequestId::new(77).unwrap(),
            environment_epoch: request.environment_epoch,
            metrics: MeasurementMetrics {
                size: MeasuredSize::new(64.0, 20.0),
                first_baseline: Some(15.0),
                last_baseline: Some(15.0),
                overflow: None,
                prepared_content: None,
            },
        })
        .unwrap();
    assert!(wakes.load(Ordering::SeqCst) > before);

    let complete = runtime
        .drive_frame(
            2.0,
            LayoutSize::new(200.0, 100.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            HostLayoutOptions::default(),
        )
        .unwrap();
    assert!(complete.frame.layout.has_layout());
    assert!(complete.frame.presentation.is_some());
}
