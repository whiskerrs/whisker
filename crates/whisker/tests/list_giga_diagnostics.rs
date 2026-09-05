//! Headless regression tests for GIGA's List and reader symptoms.
//! These exercise the public List -> Rust layout -> FramePacket boundary;
//! they do not emulate native drawing or scroll physics.

use std::{cell::RefCell, collections::HashMap, convert::Infallible, rc::Rc};
use whisker::SurfaceRuntime;
use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, Owner};
use whisker::runtime::view::{observe_layout, set_root, with_installed_renderer};
use whisker_engine::whisker_layout::LayoutSize;
use whisker_engine::whisker_protocol::{
    InputEvent, InputEventKind, MeasurementRequest, MeasurementResponse, Operation, SurfaceId,
    WhiskerValue,
};
use whisker_engine::whisker_style::StyleEnvironment;
use whisker_engine::{LayoutOptions, MeasurementProvider, RecordingRenderer};

struct NoText;
impl MeasurementProvider for NoText {
    type Error = Infallible;
    fn measure_batch(
        &mut self,
        _: SurfaceId,
        requests: &[MeasurementRequest],
        _: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        assert!(requests.is_empty());
        Ok(())
    }
}

fn frame(surface: &SurfaceRuntime, renderer: &mut RecordingRenderer, epoch: u32) {
    surface
        .render_frame(
            LayoutSize::new(390.0, 600.0),
            1,
            epoch,
            &mut NoText,
            renderer,
            LayoutOptions::default(),
        )
        .unwrap();
}

#[test]
fn giga_row_margin_does_not_move_a_retained_row() {
    for (axis, leading, margin) in [
        (ScrollAxis::Vertical, 0, 0),
        (ScrollAxis::Vertical, 0, 8),
        (ScrollAxis::Vertical, 4, 8),
        (ScrollAxis::Horizontal, 0, 8),
        (ScrollAxis::Horizontal, 4, 8),
    ] {
        __reset_for_tests();
        let owner = Owner::new(None);
        let surface = SurfaceRuntime::new(
            SurfaceId::new(94).unwrap(),
            StyleEnvironment::new(390.0, 600.0, 1.0, 14.0),
        );
        let positions = Rc::new(RefCell::new(HashMap::new()));
        with_installed_renderer(surface.renderer(), || {
            let root = owner.with(|| {
                let positions = positions.clone();
                render! {
                    List(
                        axis: axis,
                        style: css!(width: percent(100), height: px(600)),
                        each: || (0_u32..800).collect::<Vec<_>>(),
                        key: |row: &u32| *row,
                        children: move |row: ReadSignal<u32>| {
                            let index = row.get_untracked();
                            let positions = positions.clone();
                            let style = match axis {
                                ScrollAxis::Vertical => css!(height: px(60), margin_top: px(leading), margin_bottom: px(margin), flex_shrink: 0.0),
                                ScrollAxis::Horizontal => css!(width: px(60), margin_left: px(leading), margin_right: px(margin), flex_shrink: 0.0),
                            };
                            let node = render! { View(style: style) };
                            observe_layout(node, Box::new(move |observation| {
                                let position = match axis {
                                    ScrollAxis::Vertical => observation.geometry.border_box.y,
                                    ScrollAxis::Horizontal => observation.geometry.border_box.x,
                                };
                                positions.borrow_mut().insert(index, position);
                            }));
                            node
                        },
                    )
                }
            });
            set_root(root);
        });
        let mut renderer = RecordingRenderer::new(surface.surface());
        frame(&surface, &mut renderer, 1);
        frame(&surface, &mut renderer, 2);
        let scroll_type = surface
            .element_registrations()
            .iter()
            .find(|r| r.name == whisker::SCROLL_VIEW_ELEMENT_NAME)
            .unwrap()
            .element_type;
        let scroll_node = renderer.frames()[0]
            .packet
            .operations
            .iter()
            .find_map(|op| match op {
                Operation::CreateNode {
                    node, element_type, ..
                } if *element_type == scroll_type => Some(*node),
                _ => None,
            })
            .unwrap();
        let scroll = |offset: f64| {
            with_installed_renderer(surface.renderer(), || {
                surface
                    .dispatch_input(&InputEvent {
                        surface: surface.surface(),
                        timestamp_ms: offset,
                        kind: InputEventKind::Named("scroll".into()),
                        pointer: None,
                        target: Some(scroll_node),
                        detail: WhiskerValue::map([
                            (
                                if axis == ScrollAxis::Vertical {
                                    "scrollTop"
                                } else {
                                    "scrollLeft"
                                },
                                WhiskerValue::Float(offset),
                            ),
                            (
                                if axis == ScrollAxis::Vertical {
                                    "viewportHeight"
                                } else {
                                    "viewportWidth"
                                },
                                WhiskerValue::Float(if axis == ScrollAxis::Vertical {
                                    600.0
                                } else {
                                    390.0
                                }),
                            ),
                            (
                                if axis == ScrollAxis::Vertical {
                                    "scrollHeight"
                                } else {
                                    "scrollWidth"
                                },
                                WhiskerValue::Float(800.0 * f64::from(60 + leading + margin)),
                            ),
                        ]),
                    })
                    .unwrap();
                whisker::flush();
            })
        };
        scroll(1200.0);
        frame(&surface, &mut renderer, 3);
        frame(&surface, &mut renderer, 4);
        let before = positions.borrow()[&25];
        scroll(1260.0);
        frame(&surface, &mut renderer, 5);
        frame(&surface, &mut renderer, 6);
        let after = positions.borrow()[&25];
        eprintln!(
            "axis={axis:?}, leading={leading}, margin={margin}: retained row 25 content y {before} -> {after}; drift={}",
            after - before
        );
        with_installed_renderer(surface.renderer(), || owner.dispose());
        assert!(
            (after - before).abs() < 0.5,
            "scrolling must not move an unchanged retained row in content coordinates"
        );
    }
}

#[test]
fn giga_loading_replacement_keeps_header_visible() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(95).unwrap(),
        StyleEnvironment::new(390.0, 600.0, 1.0, 14.0),
    );
    let loaded = owner.with(|| signal(false));
    let handle = owner.with(ListHandle::<u32>::new);
    let header_heights = Rc::new(RefCell::new(Vec::new()));
    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            let header_heights = header_heights.clone();
            render! {
                List(
                    list_ref: handle.r(),
                    style: css!(width: percent(100), height: px(600)),
                    header: move || {
                        let heights = header_heights.clone();
                        let header = render! { View(style: css!(height: px(360))) };
                        observe_layout(header, Box::new(move |o| heights.borrow_mut().push(o.geometry.border_box.height)));
                        header
                    },
                    each: move || if loaded.get() { (0..800).collect::<Vec<_>>() } else { vec![999] },
                    key: |row: &u32| *row,
                    children: |row: ReadSignal<u32>| render! { View(style: css!(height: px(if row.get_untracked() == 999 { 640 } else { 60 }), margin_bottom: px(8))) },
                )
            }
        });
        set_root(root);
    });
    let mut renderer = RecordingRenderer::new(surface.surface());
    for epoch in 1..=3 {
        frame(&surface, &mut renderer, epoch);
    }
    with_installed_renderer(surface.renderer(), || {
        loaded.set(true);
        whisker::flush();
    });
    for epoch in 4..=8 {
        frame(&surface, &mut renderer, epoch);
        let snapshot = handle.snapshot().unwrap();
        eprintln!(
            "loaded frame {epoch}: offset={}, extent={}",
            snapshot.offset, snapshot.content_extent
        );
        assert_eq!(
            snapshot.offset, 0.0,
            "loading chapters must not scroll the header out of view"
        );
    }
    eprintln!("header heights={:?}", header_heights.borrow());
    assert!(
        header_heights
            .borrow()
            .iter()
            .all(|height| (*height - 360.0).abs() < 0.5)
    );
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn giga_rtl_initial_page_settles_before_runtime_goes_idle() {
    __reset_for_tests();
    let handle = Rc::new(RefCell::new(None::<ListHandle<u32>>));
    let surface = SurfaceRuntime::new(
        SurfaceId::new(96).unwrap(),
        StyleEnvironment::new(390.0, 600.0, 1.0, 14.0),
    );
    let wake = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let wake_callback = wake.clone();
    let mut runtime = whisker::RuntimeInstance::new(
        surface.clone(),
        whisker::runtime::RuntimeWakeHandle::new(move || {
            wake_callback.store(true, std::sync::atomic::Ordering::Relaxed);
        }),
    );
    runtime
        .mount(|| {
            let list = ListHandle::<u32>::new();
            *handle.borrow_mut() = Some(list.clone());
            render! {
                List(
                    list_ref: list.r(),
                    axis: ScrollAxis::Horizontal,
                    initial_scroll: ListScrollTarget::index(9, ScrollAlignment::Start),
                    style: css!(width: px(390), height: px(600)),
                    content_style: css!(height: percent(100)),
                    each: || (0_u32..10).rev().collect::<Vec<_>>(),
                    key: |row: &u32| *row,
                    children: |_row: ReadSignal<u32>| render! {
                        View(style: css!(width: px(390), height: percent(100), flex_shrink: 0.0))
                    },
                )
            }
        })
        .unwrap();
    let mut renderer = RecordingRenderer::new(surface.surface());
    for epoch in 1..=8 {
        wake.store(false, std::sync::atomic::Ordering::Relaxed);
        let drive = runtime
            .drive_frame(
                f64::from(epoch) * 16.0,
                StyleEnvironment::new(390.0, 600.0, 1.0, 14.0),
                1,
                epoch,
                &mut NoText,
                &mut renderer,
                LayoutOptions::default(),
            )
            .unwrap();
        let snapshot = handle.borrow().as_ref().unwrap().snapshot().unwrap();
        eprintln!(
            "RTL frame {epoch}: needs_frame={}, offset={}, first_key={:?}",
            drive.needs_frame, snapshot.offset, snapshot.first_visible_key
        );
        if !drive.needs_frame && !wake.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
    }
    let snapshot = handle.borrow().as_ref().unwrap().snapshot().unwrap();
    runtime.unmount().unwrap();
    assert_eq!(
        snapshot.offset, 3510.0,
        "RTL logical page 0 must be reached before the Host stops scheduling frames"
    );
}

#[test]
fn giga_reader_hold_emits_longpress() {
    use std::cell::Cell;
    use whisker_engine::whisker_protocol::{InputPoint, PointerId, PointerInput, PointerKind};
    __reset_for_tests();
    let surface = SurfaceRuntime::new(
        SurfaceId::new(97).unwrap(),
        StyleEnvironment::new(390.0, 600.0, 1.0, 14.0),
    );
    let holds = Rc::new(Cell::new(0));
    let held = holds.clone();
    let mut runtime = whisker::RuntimeInstance::new(
        surface.clone(),
        whisker::runtime::RuntimeWakeHandle::new(|| {}),
    );
    runtime.mount(|| {
        let builder = List::builder()
            .style(css!(width: px(390), height: px(600)))
            .axis(ScrollAxis::Horizontal)
            .each(|| vec![0_u32])
            .key(|row: &u32| *row)
            .children(|_: ReadSignal<u32>| render! { View(style: css!(width: px(390), height: px(600))) });
        builder
            .on_longpress(move |_| held.set(held.get() + 1))
            .build()
    }).unwrap();
    let mut renderer = RecordingRenderer::new(surface.surface());
    for epoch in 1..=3 {
        runtime
            .drive_frame(
                f64::from(epoch),
                StyleEnvironment::new(390.0, 600.0, 1.0, 14.0),
                1,
                epoch,
                &mut NoText,
                &mut renderer,
                LayoutOptions::default(),
            )
            .unwrap();
    }
    let pointer = |timestamp_ms, kind, buttons| InputEvent {
        surface: surface.surface(),
        timestamp_ms,
        kind,
        pointer: Some(PointerInput {
            id: PointerId::new(1).unwrap(),
            kind: PointerKind::Touch,
            position: InputPoint { x: 100.0, y: 100.0 },
            buttons,
            changed_button: -1,
        }),
        target: None,
        detail: WhiskerValue::Null,
    };
    let down = runtime
        .dispatch_input(&pointer(10.0, InputEventKind::PointerDown, 1))
        .unwrap();
    assert!(down.target.is_some(), "the page must be hit-testable");
    runtime
        .drive_frame(
            1010.0,
            StyleEnvironment::new(390.0, 600.0, 1.0, 14.0),
            1,
            4,
            &mut NoText,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();
    assert_eq!(
        holds.get(),
        1,
        "longpress must fire before the finger is released"
    );
    runtime
        .dispatch_input(&pointer(1110.0, InputEventKind::PointerUp, 0))
        .unwrap();
    let from_pointer = holds.get();
    // Control: the named-event listener and bubbling must work independently
    // of held-pointer recognition.
    runtime
        .dispatch_input(&InputEvent {
            surface: surface.surface(),
            timestamp_ms: 1200.0,
            kind: InputEventKind::Named("longpress".into()),
            pointer: pointer(1200.0, InputEventKind::PointerUp, 0).pointer,
            target: down.target,
            detail: WhiskerValue::Null,
        })
        .unwrap();
    let from_named_event = holds.get() - from_pointer;
    runtime.unmount().unwrap();
    eprintln!(
        "held pointer: {from_pointer} longpress events; direct named event: {from_named_event}"
    );
    assert_eq!(from_named_event, 1);
    assert_eq!(
        from_pointer, 1,
        "holding the reader must reach the UI toggle callback"
    );
}
