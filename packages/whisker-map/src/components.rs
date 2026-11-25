//! `MapView` and the components placed inside it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use glam::DVec2;
use whisker::css::{Overflow, PositionKind, Transform, TransformFn};
use whisker::event::{BindType, TouchEvent};
use whisker::prelude::*;
use whisker::runtime::event::bind_typed;
use whisker::runtime::view::Element;
use whisker::runtime::view::renderer::{release_pointer_capture, set_pointer_capture};
use whisker::{Children, RwSignal, Signal, Style};
use whisker_surface::{SurfaceView, WheelEvent};
use whisker_wgpu::LayerHandle;

use crate::camera::MapCamera;
use crate::events::{EventCallback, MapEvents, SourceDataCallback};
use crate::geo::LatLng;
use crate::gesture::{Gestures, PointerKind, Sample, wheel};
use crate::handle::MapRef;
use crate::map::{LayerSlot, MapInner, MarkerSlot};
use crate::query::TapCallback;
use crate::render::MapRenderer;
use crate::style::MapStyle;

#[derive(Clone)]
struct MapContext {
    map: Rc<MapInner>,
}

/// Shows a MapLibre style, moved by touch and mouse gestures.
///
/// `camera` goes both ways: writing it moves the map, and gestures write where the map moved to.
/// The components among `children`, such as [`Marker`], are placed over the map.
#[component]
pub fn map_view(
    map_style: MapStyle,
    camera: RwSignal<MapCamera>,
    #[prop(default = true)] gestures: bool,
    style: Style,
    on_tap: Option<TapCallback>,
    on_move_end: Option<EventCallback>,
    on_idle: Option<EventCallback>,
    on_source_data: Option<SourceDataCallback>,
    map_ref: Option<MapRef>,
    children: Children,
) -> Element {
    let map = MapInner::new(camera.get_untracked());
    map.set_events(MapEvents {
        move_end: on_move_end.clone(),
        idle: on_idle.clone(),
        source_data: on_source_data.clone(),
    });
    map.bind_camera(camera);
    map.load_style(map_style.clone());
    let surface = {
        let map = map.clone();
        whisker_wgpu::surface(move |gpu, format| MapRenderer::new(gpu, format, map))
    };
    map.set_surface(surface.downgrade());
    if let Some(map_ref) = map_ref {
        map_ref.bind(&map);
    }
    {
        let map = map.clone();
        effect(move || map.camera_from_app(camera.get()));
    }
    provide_context(MapContext { map: map.clone() });

    let fill: Style = Css::new().width(percent(100)).height(percent(100)).into();
    let stage: Style = Css::new()
        .width(percent(100))
        .height(percent(100))
        .position(PositionKind::Relative)
        .overflow(Overflow::Hidden)
        .into();
    let projected = children.clone();
    let surface_view = if gestures {
        let map = map.clone();
        let on_wheel = move |event: WheelEvent| {
            let d = event.detail;
            wheel(&map, DVec2::new(d.x, d.y), d.delta_y, d.ctrl_key);
        };
        render! { SurfaceView(surface: surface, style: fill, on_wheel: on_wheel) }
    } else {
        render! { SurfaceView(surface: surface, style: fill) }
    };
    let root = render! {
        View(style: style.clone()) {
            View(style: stage) {
                { surface_view }
                Fragment { { projected() } }
            }
        }
    };
    if gestures || on_tap.is_some() {
        bind_gestures(root, map, gestures, on_tap.clone());
    }
    root
}

fn bind_gestures(root: Element, map: Rc<MapInner>, moves: bool, on_tap: Option<TapCallback>) {
    let gestures = Rc::new(RefCell::new(Gestures::new(moves)));
    let handler = move |phase: Phase| {
        let map = map.clone();
        let gestures = gestures.clone();
        let on_tap = on_tap.clone();
        move |event: TouchEvent| {
            let kind = if event.pointer_type == "mouse" {
                PointerKind::Mouse
            } else {
                PointerKind::Touch
            };
            let Some(offset) = event.offset else {
                return;
            };
            let sample = Sample {
                id: event.pointer_id,
                position: DVec2::new(offset.x, offset.y),
                time_ms: event.timestamp,
                kind,
                buttons: event.buttons,
            };
            let tap = {
                let mut gestures = gestures.borrow_mut();
                let tap = match phase {
                    Phase::Down => {
                        set_pointer_capture(root, event.pointer_id);
                        gestures.down(&map, sample);
                        None
                    }
                    Phase::Move => {
                        gestures.moved(&map, sample);
                        None
                    }
                    Phase::Up => {
                        release_pointer_capture(root, event.pointer_id);
                        gestures.up(&map, sample)
                    }
                    Phase::Cancel => {
                        release_pointer_capture(root, event.pointer_id);
                        gestures.cancel(sample);
                        None
                    }
                };
                map.set_touching(gestures.is_active());
                tap
            };
            let Some(point) = tap else {
                return;
            };
            if map.tap_layers(point) {
                return;
            }
            if let Some(on_tap) = &on_tap {
                (on_tap.0)(map.tap_at(point));
            }
        }
    };
    for (name, phase) in [
        ("touchstart", Phase::Down),
        ("touchmove", Phase::Move),
        ("touchend", Phase::Up),
        ("touchcancel", Phase::Cancel),
        ("pointerdown", Phase::Down),
        ("pointermove", Phase::Move),
        ("pointerup", Phase::Up),
        ("pointercancel", Phase::Cancel),
    ] {
        bind_typed(root, name, BindType::Bind, handler(phase));
    }
}

#[derive(Clone, Copy)]
enum Phase {
    Down,
    Move,
    Up,
    Cancel,
}

/// Places `children` over the map with their bottom center at `position`.
///
/// # Panics
///
/// Outside a [`MapView`].
#[component]
pub fn marker(position: Signal<LatLng>, children: Children) -> Element {
    let map = use_context::<MapContext>()
        .unwrap_or_else(|| panic!("Marker must be placed inside a MapView"))
        .map;
    let slot = Rc::new(MarkerSlot {
        position: Cell::new(
            position
                .try_get_untracked()
                .unwrap_or(LatLng::new(0.0, 0.0)),
        ),
        screen: signal(None),
    });
    map.add_marker(&slot);
    {
        let slot = slot.clone();
        let map = map.clone();
        effect(move || {
            slot.position.set(position.get());
            map.request_frame();
        });
    }
    let screen = slot.screen;
    let _kept = StoredValue::new(slot);
    let projected = children.clone();
    render! {
        View(
            style: computed(move || {
                let base = Css::new().position(PositionKind::Absolute).left(px(0)).top(px(0));
                match screen.get() {
                    Some((x, y)) => base.transform(
                        Transform::new()
                            .push(TransformFn::Translate(px(x).into(), px(y).into()))
                            .push(TransformFn::Translate(percent(-50).into(), percent(-100).into())),
                    ),
                    None => base.display_none(),
                }
            }),
        ) {
            Fragment { { projected() } }
        }
    }
}

/// Draws `layer` inside the map, in meters around `anchor`: +X east, +Y up and -Z north.
///
/// The layer shares the map's depth, so extrusions drawn before it hide it and it hides those
/// drawn after it where it is nearer. It draws beneath the style layer `before`, or above every
/// style layer, labels included, when that is `None` or names no layer.
///
/// # Panics
///
/// Outside a [`MapView`].
#[component]
pub fn map_layer(
    layer: LayerHandle,
    anchor: Signal<LatLng>,
    #[prop(default = None.into())] before: Signal<Option<String>>,
) -> Element {
    let map = use_context::<MapContext>()
        .unwrap_or_else(|| panic!("MapLayer must be placed inside a MapView"))
        .map;
    let slot = Rc::new(LayerSlot {
        layer: layer.clone(),
        anchor: Cell::new(anchor.try_get_untracked().unwrap_or(LatLng::new(0.0, 0.0))),
        before: RefCell::new(before.try_get_untracked().flatten()),
        target: Cell::new(None),
    });
    map.add_layer(slot.clone());
    {
        let slot = slot.clone();
        let map = map.clone();
        effect(move || {
            slot.anchor.set(anchor.get());
            *slot.before.borrow_mut() = before.get();
            map.request_frame();
        });
    }
    on_cleanup(move || map.remove_layer(&slot));
    render! { Fragment {} }
}
