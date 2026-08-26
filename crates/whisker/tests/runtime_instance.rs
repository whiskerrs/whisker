use std::cell::{Cell, RefCell};
use std::convert::Infallible;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use whisker::css::{
    Angle, Background as CssBackground, BackgroundAttachment as CssBackgroundAttachment,
    BackgroundClip as CssBackgroundClip, BackgroundLayer as CssBackgroundLayer,
    BackgroundOrigin as CssBackgroundOrigin, BackgroundRepeat as CssBackgroundRepeat,
    BackgroundSize as CssBackgroundSize, BackgroundSizeAxis as CssBackgroundSizeAxis, CalcExpr,
    ColorStop, CssString, Gradient, ImageRef, LengthPercentage, NamedColor, Percentage, Position,
    RadialShape,
};
use whisker::prelude::*;
use whisker::{
    RuntimeBindingError, RuntimeDriveError, RuntimeInstance, RuntimeLifecycle, SurfaceRuntime,
};
use whisker_engine::whisker_protocol::{
    ApplyResult, BackgroundAttachment, BackgroundLayer, BackgroundSize, BlendMode, FrameMode,
    FramePacket, ImageRepeat, InputEvent, InputEventKind, InputPoint, MeasuredSize,
    MeasurementMetrics, MeasurementPayload, MeasurementReady, MeasurementRequest,
    MeasurementRequestId, MeasurementResponse, Operation, PaintBox, PaintCoordinate, PaintImage,
    PaintLengthPercentage, PaintPosition, PointerId, PointerInput, PointerKind, RenderCapabilities,
    ResourceCommand, ResourceDimensions, ResourceEvent, ResourceId, ResourceKind, ResourceRequest,
    ResourceSource, SurfaceId, WhiskerValue,
};
use whisker_engine::whisker_style::{StyleEnvironment, StyleResolutionError};
use whisker_engine::{FrameSink, LayoutOptions, MeasurementProvider, RecordingRenderer};
use whisker_runtime::RuntimeWakeHandle;

#[derive(Default)]
struct NoMeasurement;

impl MeasurementProvider for NoMeasurement {
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
    requests: Vec<(MeasurementRequest, MeasurementRequestId)>,
}

#[derive(Default)]
struct ReadyTextMeasurement {
    calls: Vec<Vec<MeasurementRequest>>,
}

impl MeasurementProvider for ReadyTextMeasurement {
    type Error = Infallible;

    fn measure_batch(
        &mut self,
        _surface: SurfaceId,
        requests: &[MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        self.calls.push(requests.to_vec());
        responses.extend(requests.iter().map(|request| {
            let MeasurementPayload::Text(text) = &request.payload else {
                panic!("test Host only supports Text measurement")
            };
            MeasurementResponse::Ready {
                key: request.key,
                environment_epoch: request.environment_epoch,
                metrics: MeasurementMetrics::from_size(MeasuredSize::new(
                    text.text.chars().count() as f32 * text.style.font_size * 0.5,
                    text.style.font_size * 1.2,
                )),
            }
        }));
        Ok(())
    }
}

impl MeasurementProvider for PendingTextMeasurement {
    type Error = Infallible;

    fn measure_batch(
        &mut self,
        _surface: SurfaceId,
        requests: &[MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        for request in requests {
            let request_id = MeasurementRequestId::new(100 + request.key.get()).unwrap();
            responses.push(MeasurementResponse::Pending {
                key: request.key,
                environment_epoch: request.environment_epoch,
                request_id,
                provisional: None,
            });
            self.requests.push((request.clone(), request_id));
        }
        Ok(())
    }
}

fn surface(id: u64) -> SurfaceRuntime {
    SurfaceRuntime::new(
        SurfaceId::new(id).unwrap(),
        StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
    )
}

const BACKGROUND_URL: &str = "https://example.com/background.png";

fn url_background_style() -> Css {
    Css::new()
        .width(px(100))
        .height(px(100))
        .background_image(ImageRef::Url(CssString::new(BACKGROUND_URL)))
}

fn ready_raster(resource: ResourceId, generation: u64) -> ResourceEvent {
    ResourceEvent::Ready {
        resource,
        generation,
        dimensions: Some(ResourceDimensions {
            width: 2.0,
            height: 2.0,
            scale: 1.0,
        }),
    }
}

fn render_ready_background(surface_id: u64, style: Css) -> BackgroundLayer {
    let surface = surface(surface_id);
    let mut runtime = RuntimeInstance::new(surface.clone(), RuntimeWakeHandle::new(|| {}));
    runtime
        .mount(move || render! { view(style: style) })
        .unwrap();
    let commands = surface.take_resource_commands();
    assert_eq!(commands.len(), 1, "one URL must produce one Host load");
    let ResourceCommand::Load(request) = &commands[0] else {
        panic!("URL background must produce a load")
    };
    let resource = request.resource;
    runtime
        .dispatch_resource_event(&ready_raster(resource, request.generation))
        .unwrap();

    let mut measurements = NoMeasurement;
    let mut sink = RecordingRenderer::new(surface.surface());
    runtime
        .drive_frame(
            1.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();
    sink.frames()
        .iter()
        .rev()
        .flat_map(|frame| frame.packet.operations.iter().rev())
        .find_map(|operation| match operation {
            Operation::SetBackgroundLayers { layers, .. } if !layers.is_empty() => {
                Some(layers[0].clone())
            }
            _ => None,
        })
        .expect("Ready URL must be lowered into SetBackgroundLayers")
}

fn render_gradient(surface_id: u64, gradient: Gradient) -> PaintImage {
    let surface = surface(surface_id);
    let mut runtime = RuntimeInstance::new(surface.clone(), RuntimeWakeHandle::new(|| {}));
    runtime
        .mount(move || {
            render! { view(style: Css::new().width(px(100)).height(px(100)).background_image(gradient.clone())) }
        })
        .unwrap();
    assert!(surface.take_resource_commands().is_empty());
    let mut measurements = NoMeasurement;
    let mut sink = RecordingRenderer::new(surface.surface());
    runtime
        .drive_frame(
            1.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();
    sink.frames()
        .iter()
        .rev()
        .flat_map(|frame| frame.packet.operations.iter().rev())
        .find_map(|operation| match operation {
            Operation::SetBackgroundLayers { layers, .. } if layers.len() == 1 => {
                Some(layers[0].image.clone())
            }
            _ => None,
        })
        .expect("gradient must be lowered into SetBackgroundLayers")
}

fn dispatch_control_tap(runtime: &RuntimeInstance, surface: &SurfaceRuntime, timestamp_ms: f64) {
    let dispatch = runtime
        .dispatch_input(&InputEvent {
            surface: surface.surface(),
            timestamp_ms,
            kind: InputEventKind::Tap,
            pointer: Some(PointerInput {
                id: PointerId::new(99).unwrap(),
                kind: PointerKind::Touch,
                position: InputPoint { x: 1.0, y: 1.0 },
                buttons: 1,
                changed_button: 0,
            }),
            target: surface.root(),
            detail: WhiskerValue::Null,
        })
        .expect("control tap must route through the runtime context");
    assert!(dispatch.consumed);
}

struct NeedSnapshotOnceSink {
    request_snapshot: bool,
}

impl FrameSink for NeedSnapshotOnceSink {
    type Error = Infallible;

    fn capabilities(&self) -> RenderCapabilities {
        RenderCapabilities::all_frame_native()
    }

    fn present(&mut self, packet: &FramePacket) -> Result<ApplyResult, Self::Error> {
        if self.request_snapshot {
            self.request_snapshot = false;
            Ok(ApplyResult::NeedSnapshot {
                receiver_revision: packet.header.base_revision,
            })
        } else {
            Ok(ApplyResult::Accepted {
                revision: packet.header.target_revision,
            })
        }
    }
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
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
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
                StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
                1,
                1,
                &mut measurements,
                &mut sink,
                LayoutOptions::default(),
            )
            .is_err()
    );
    runtime.resume().unwrap();
    assert_eq!(runtime.lifecycle(), RuntimeLifecycle::Running);
    runtime.unmount().unwrap();
    assert_eq!(runtime.lifecycle(), RuntimeLifecycle::Unmounted);
}

#[test]
fn current_resource_completion_wakes_running_but_not_paused_runtime() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let wake_count = Arc::clone(&wakes);
    let surface = surface(39);
    let mut runtime = RuntimeInstance::new(
        surface.clone(),
        RuntimeWakeHandle::new(move || {
            wake_count.fetch_add(1, Ordering::SeqCst);
        }),
    );
    runtime.mount(|| render! { view() }).unwrap();
    let resource = ResourceId::new(5).unwrap();
    surface
        .enqueue_resource_command(ResourceCommand::Load(ResourceRequest {
            resource,
            generation: 1,
            kind: ResourceKind::RasterImage,
            source: ResourceSource::BundledAsset("image.png".into()),
        }))
        .unwrap();
    let ready = ResourceEvent::Ready {
        resource,
        generation: 1,
        dimensions: Some(ResourceDimensions {
            width: 10.0,
            height: 5.0,
            scale: 1.0,
        }),
    };

    let before_running = wakes.load(Ordering::SeqCst);
    runtime.dispatch_resource_event(&ready).unwrap();
    assert!(wakes.load(Ordering::SeqCst) > before_running);

    surface
        .enqueue_resource_command(ResourceCommand::Load(ResourceRequest {
            resource,
            generation: 2,
            kind: ResourceKind::RasterImage,
            source: ResourceSource::BundledAsset("replacement.png".into()),
        }))
        .unwrap();
    runtime.pause().unwrap();
    let before_paused = wakes.load(Ordering::SeqCst);
    runtime
        .dispatch_resource_event(&ResourceEvent::Ready {
            resource,
            generation: 2,
            dimensions: None,
        })
        .unwrap();
    assert_eq!(wakes.load(Ordering::SeqCst), before_paused);
}

#[test]
fn background_url_loads_out_of_frame_and_is_emitted_only_after_ready() {
    let surface = surface(40);
    let mut runtime = RuntimeInstance::new(surface.clone(), RuntimeWakeHandle::new(|| {}));
    runtime
        .mount(|| {
            render! {
                view(style: Css::new()
                    .width(px(100))
                    .height(px(100))
                    .background_image(ImageRef::Url(CssString::new(
                        "https://example.com/background.png",
                    ))))
            }
        })
        .unwrap();

    let commands = surface.take_resource_commands();
    assert_eq!(commands.len(), 1);
    let ResourceCommand::Load(request) = &commands[0] else {
        panic!("background URL must acquire a raster resource")
    };
    assert_eq!(request.generation, 1);
    assert_eq!(request.kind, ResourceKind::RasterImage);
    assert_eq!(
        request.source,
        ResourceSource::Url("https://example.com/background.png".into())
    );
    let resource = request.resource;

    let mut measurements = NoMeasurement;
    let mut sink = RecordingRenderer::new(surface.surface());
    runtime
        .drive_frame(
            1.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(sink.frames()[0].packet.operations.iter().all(|operation| {
        !matches!(operation, Operation::SetBackgroundLayers { layers, .. } if !layers.is_empty())
    }));

    assert_eq!(
        runtime
            .dispatch_resource_event(&ResourceEvent::Ready {
                resource,
                generation: 1,
                dimensions: Some(ResourceDimensions {
                    width: 2.0,
                    height: 2.0,
                    scale: 1.0,
                }),
            })
            .unwrap(),
        whisker::ResourceEventApply::Applied
    );
    runtime
        .drive_frame(
            2.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();
    let root = surface.root().unwrap();
    assert!(sink.frames()[1].packet.operations.iter().any(|operation| {
        matches!(
            operation,
            Operation::SetBackgroundLayers { node, layers }
                if *node == root
                    && layers.len() == 1
                    && layers[0].image == PaintImage::Resource(resource)
                    && layers[0].position == PaintPosition::default()
                    && layers[0].size == BackgroundSize::Auto
                    && layers[0].repeat_x == ImageRepeat::Repeat
                    && layers[0].repeat_y == ImageRepeat::Repeat
                    && layers[0].origin == PaintBox::Padding
                    && layers[0].clip == PaintBox::Border
                    && layers[0].attachment == BackgroundAttachment::Scroll
                    && layers[0].blend_mode == BlendMode::Normal
        )
    }));
    assert!(surface.take_resource_commands().is_empty());
}

#[test]
fn background_url_lowers_explicit_geometry_repeat_boxes_and_attachment() {
    let x = LengthPercentage::calc(CalcExpr::value(px(12)).add(CalcExpr::value(Percentage(25.0))));
    let y = LengthPercentage::calc(CalcExpr::value(px(8)).add(CalcExpr::value(Percentage(75.0))));
    let layer = render_ready_background(
        44,
        Css::new()
            .width(px(100))
            .height(px(100))
            .background_image(ImageRef::Url(CssString::new(BACKGROUND_URL)))
            .background_position_x(x)
            .background_position_y(y)
            .background_size(CssBackgroundSize::Explicit(
                px(40).into(),
                Percentage(50.0).into(),
            ))
            .background_repeat(CssBackgroundRepeat::RepeatX)
            .background_origin(CssBackgroundOrigin::ContentBox)
            .background_clip(CssBackgroundClip::PaddingBox)
            .background_attachment(CssBackgroundAttachment::Scroll),
    );

    assert_eq!(
        layer.position,
        PaintPosition {
            x: PaintCoordinate {
                length: 12.0,
                fraction: 0.25,
            },
            y: PaintCoordinate {
                length: 8.0,
                fraction: 0.75,
            },
        }
    );
    assert_eq!(
        layer.size,
        BackgroundSize::Explicit {
            width: Some(PaintLengthPercentage {
                length: 40.0,
                fraction: 0.0,
            }),
            height: Some(PaintLengthPercentage {
                length: 0.0,
                fraction: 0.5,
            }),
        }
    );
    assert_eq!(layer.repeat_x, ImageRepeat::Repeat);
    assert_eq!(layer.repeat_y, ImageRepeat::NoRepeat);
    assert_eq!(layer.origin, PaintBox::Content);
    assert_eq!(layer.clip, PaintBox::Padding);
    assert_eq!(layer.attachment, BackgroundAttachment::Scroll);
}

#[test]
fn background_shorthand_preserves_geometry_for_each_url_layer() {
    let front_url = "https://example.com/front.png";
    let back_url = "https://example.com/back.png";
    let style = Css::new().width(px(100)).height(px(100)).background(
        CssBackground::new()
            .layer(
                CssBackgroundLayer::new(ImageRef::Url(CssString::new(front_url)))
                    .position(Position::Coords(Percentage(25.0).into(), px(8).into()))
                    .size(CssBackgroundSize::Cover)
                    .repeat(CssBackgroundRepeat::NoRepeat)
                    .origin(CssBackgroundOrigin::ContentBox)
                    .clip(CssBackgroundClip::PaddingBox),
            )
            .layer(
                CssBackgroundLayer::new(ImageRef::Url(CssString::new(back_url)))
                    .position(Position::Coords(Percentage(75.0).into(), px(12).into()))
                    .size(CssBackgroundSize::Contain)
                    .repeat(CssBackgroundRepeat::RepeatY)
                    .origin(CssBackgroundOrigin::BorderBox)
                    .clip(CssBackgroundClip::ContentBox),
            ),
    );
    let surface = surface(46);
    let mut runtime = RuntimeInstance::new(surface.clone(), RuntimeWakeHandle::new(|| {}));
    runtime
        .mount(move || render! { view(style: style) })
        .unwrap();

    let commands = surface.take_resource_commands();
    assert_eq!(commands.len(), 2);
    let mut front_resource = None;
    let mut back_resource = None;
    for command in &commands {
        let ResourceCommand::Load(request) = command else {
            panic!("background URL must produce a load")
        };
        match &request.source {
            ResourceSource::Url(url) if url == front_url => front_resource = Some(request.resource),
            ResourceSource::Url(url) if url == back_url => back_resource = Some(request.resource),
            source => panic!("unexpected background source: {source:?}"),
        }
        runtime
            .dispatch_resource_event(&ready_raster(request.resource, request.generation))
            .unwrap();
    }
    let front_resource = front_resource.unwrap();
    let back_resource = back_resource.unwrap();

    let mut measurements = NoMeasurement;
    let mut sink = RecordingRenderer::new(surface.surface());
    runtime
        .drive_frame(
            1.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();
    let layers = sink
        .frames()
        .iter()
        .rev()
        .flat_map(|frame| frame.packet.operations.iter().rev())
        .find_map(|operation| match operation {
            Operation::SetBackgroundLayers { layers, .. } if layers.len() == 2 => Some(layers),
            _ => None,
        })
        .expect("both ready layers must be emitted together");

    assert_eq!(layers[0].image, PaintImage::Resource(front_resource));
    assert_eq!(
        layers[0].position.x,
        PaintCoordinate {
            length: 0.0,
            fraction: 0.25
        }
    );
    assert_eq!(
        layers[0].position.y,
        PaintCoordinate {
            length: 8.0,
            fraction: 0.0
        }
    );
    assert_eq!(layers[0].size, BackgroundSize::Cover);
    assert_eq!(
        (layers[0].repeat_x, layers[0].repeat_y),
        (ImageRepeat::NoRepeat, ImageRepeat::NoRepeat)
    );
    assert_eq!(
        (layers[0].origin, layers[0].clip),
        (PaintBox::Content, PaintBox::Padding)
    );

    assert_eq!(layers[1].image, PaintImage::Resource(back_resource));
    assert_eq!(
        layers[1].position.x,
        PaintCoordinate {
            length: 0.0,
            fraction: 0.75
        }
    );
    assert_eq!(
        layers[1].position.y,
        PaintCoordinate {
            length: 12.0,
            fraction: 0.0
        }
    );
    assert_eq!(layers[1].size, BackgroundSize::Contain);
    assert_eq!(
        (layers[1].repeat_x, layers[1].repeat_y),
        (ImageRepeat::NoRepeat, ImageRepeat::Repeat)
    );
    assert_eq!(
        (layers[1].origin, layers[1].clip),
        (PaintBox::Border, PaintBox::Content)
    );
}

#[test]
fn background_gradients_lower_without_host_resources() {
    let stops = || {
        vec![
            ColorStop::new(NamedColor::Red.into()),
            ColorStop::new(NamedColor::Blue.into()),
        ]
    };
    let linear = render_gradient(
        47,
        Gradient::Linear {
            direction: whisker::css::LinearDirection::ToRight,
            stops: stops(),
        },
    );
    let PaintImage::LinearGradient {
        angle_degrees,
        repeating,
        stops: linear_stops,
    } = linear
    else {
        panic!("expected linear gradient")
    };
    assert_eq!(angle_degrees, 90.0);
    assert!(!repeating);
    assert_eq!(linear_stops.len(), 2);
    assert_eq!(linear_stops[0].position.unwrap().fraction, 0.0);
    assert_eq!(linear_stops[1].position.unwrap().fraction, 1.0);

    let radial = render_gradient(
        48,
        Gradient::Radial {
            shape: RadialShape::EllipseSized(px(40).into(), Percentage(25.0).into()),
            stops: stops(),
        },
    );
    let PaintImage::RadialGradient {
        shape,
        extent,
        center,
        radii,
        ..
    } = radial
    else {
        panic!("expected radial gradient")
    };
    assert_eq!(
        shape,
        whisker_engine::whisker_protocol::RadialGradientShape::Ellipse
    );
    assert_eq!(
        extent,
        whisker_engine::whisker_protocol::RadialGradientExtent::Explicit
    );
    assert_eq!(center.x.fraction, 0.5);
    let (radius_x, radius_y) = radii.unwrap();
    assert_eq!(radius_x.length, 40.0);
    assert_eq!(radius_y.fraction, 0.25);

    let conic = render_gradient(
        49,
        Gradient::Conic {
            from: Some(Angle::Turn(0.25)),
            at: Some((Percentage(25.0).into(), Percentage(75.0).into())),
            stops: stops(),
        },
    );
    let PaintImage::ConicGradient {
        from_degrees,
        center,
        stops: conic_stops,
        ..
    } = conic
    else {
        panic!("expected conic gradient")
    };
    assert_eq!(from_degrees, 90.0);
    assert_eq!((center.x.fraction, center.y.fraction), (0.25, 0.75));
    assert_eq!(conic_stops.len(), 2);
}

#[test]
fn background_url_lowers_remaining_repeat_modes_with_explicit_size() {
    let cases = [
        (
            "repeat",
            CssBackgroundRepeat::Repeat,
            ImageRepeat::Repeat,
            ImageRepeat::Repeat,
        ),
        (
            "no-repeat",
            CssBackgroundRepeat::NoRepeat,
            ImageRepeat::NoRepeat,
            ImageRepeat::NoRepeat,
        ),
        (
            "repeat-y",
            CssBackgroundRepeat::RepeatY,
            ImageRepeat::NoRepeat,
            ImageRepeat::Repeat,
        ),
    ];

    for (index, (name, css_repeat, repeat_x, repeat_y)) in cases.into_iter().enumerate() {
        let layer = render_ready_background(
            45 + index as u64,
            Css::new()
                .width(px(100))
                .height(px(100))
                .background_image(ImageRef::Url(CssString::new(BACKGROUND_URL)))
                .background_size(CssBackgroundSize::Explicit(
                    px(32).into(),
                    Percentage(25.0).into(),
                ))
                .background_repeat(css_repeat),
        );
        assert_eq!(
            layer.size,
            BackgroundSize::Explicit {
                width: Some(PaintLengthPercentage {
                    length: 32.0,
                    fraction: 0.0,
                }),
                height: Some(PaintLengthPercentage {
                    length: 0.0,
                    fraction: 0.25,
                }),
            },
            "{name}"
        );
        assert_eq!(layer.repeat_x, repeat_x, "{name}");
        assert_eq!(layer.repeat_y, repeat_y, "{name}");
    }
}

#[test]
fn background_url_lowers_intrinsic_cover_contain_and_one_axis_auto_sizes() {
    let cases = [
        ("auto", CssBackgroundSize::Auto, BackgroundSize::Auto),
        ("cover", CssBackgroundSize::Cover, BackgroundSize::Cover),
        (
            "contain",
            CssBackgroundSize::Contain,
            BackgroundSize::Contain,
        ),
        (
            "width with auto height",
            CssBackgroundSize::Explicit(px(60).into(), CssBackgroundSizeAxis::Auto),
            BackgroundSize::Explicit {
                width: Some(PaintLengthPercentage {
                    length: 60.0,
                    fraction: 0.0,
                }),
                height: None,
            },
        ),
        (
            "auto width with height",
            CssBackgroundSize::Explicit(CssBackgroundSizeAxis::Auto, px(30).into()),
            BackgroundSize::Explicit {
                width: None,
                height: Some(PaintLengthPercentage {
                    length: 30.0,
                    fraction: 0.0,
                }),
            },
        ),
    ];

    for (index, (name, css_size, expected)) in cases.into_iter().enumerate() {
        let layer = render_ready_background(
            50 + index as u64,
            Css::new()
                .width(px(100))
                .height(px(80))
                .background_image(ImageRef::Url(CssString::new(BACKGROUND_URL)))
                .background_size(css_size)
                .background_repeat(CssBackgroundRepeat::NoRepeat),
        );
        assert_eq!(layer.size, expected, "{name}");
    }
}

#[test]
fn background_url_is_shared_and_released_only_after_the_last_clear_is_accepted() {
    let surface = surface(41);
    let desired_visibility = Rc::new(Cell::new((true, true)));
    let mounted_visibility = Rc::clone(&desired_visibility);
    let mut runtime = RuntimeInstance::new(surface.clone(), RuntimeWakeHandle::new(|| {}));
    runtime
        .mount(move || {
            let first = RwSignal::new(true);
            let second = RwSignal::new(true);
            render! {
                view(
                    style: css!(width: px(200), height: px(100)),
                    on_tap: move |_| {
                        let (show_first, show_second) = mounted_visibility.get();
                        first.set(show_first);
                        second.set(show_second);
                    },
                ) {
                    Show(when: move || first.get()) {
                        view(style: url_background_style())
                    }
                    Show(when: move || second.get()) {
                        view(style: url_background_style())
                    }
                }
            }
        })
        .unwrap();

    let commands = surface.take_resource_commands();
    assert_eq!(commands.len(), 1, "equal URLs must share one Host load");
    let ResourceCommand::Load(request) = &commands[0] else {
        panic!("shared background URL must load once")
    };
    assert_eq!(request.generation, 1);
    assert_eq!(request.source, ResourceSource::Url(BACKGROUND_URL.into()));
    let resource = request.resource;
    assert_eq!(
        runtime
            .dispatch_resource_event(&ready_raster(resource, 1))
            .unwrap(),
        whisker::ResourceEventApply::Applied
    );

    let mut measurements = NoMeasurement;
    let mut sink = RecordingRenderer::new(surface.surface());
    runtime
        .drive_frame(
            1.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(surface.take_resource_commands().is_empty());

    desired_visibility.set((false, true));
    dispatch_control_tap(&runtime, &surface, 1.5);
    runtime
        .drive_frame(
            2.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(
        surface.take_resource_commands().is_empty(),
        "deleting one of two users must retain the shared generation"
    );

    desired_visibility.set((true, true));
    dispatch_control_tap(&runtime, &surface, 2.5);
    runtime
        .drive_frame(
            3.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(
        surface.take_resource_commands().is_empty(),
        "reacquiring a still-shared URL must not reload it"
    );

    desired_visibility.set((false, true));
    dispatch_control_tap(&runtime, &surface, 3.5);
    runtime
        .drive_frame(
            4.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(surface.take_resource_commands().is_empty());

    desired_visibility.set((false, false));
    dispatch_control_tap(&runtime, &surface, 4.5);
    assert!(
        surface.take_resource_commands().is_empty(),
        "Release must not precede the frame which removes the final user"
    );
    let removal_frame_index = sink.frames().len();
    let removed = runtime
        .drive_frame(
            5.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        removed.frame.presentation,
        Some(ApplyResult::Accepted { .. })
    ));
    assert!(
        sink.frames()[removal_frame_index]
            .packet
            .operations
            .iter()
            .any(|operation| matches!(operation, Operation::DeleteNode { .. }))
    );
    assert_eq!(
        surface.take_resource_commands(),
        vec![ResourceCommand::Release {
            resource,
            generation: 1,
        }],
        "the accepted clear is the earliest safe Release point"
    );
}

#[test]
fn background_url_reacquired_before_clear_ack_keeps_the_current_generation() {
    let surface = surface(42);
    let desired_visibility = Rc::new(Cell::new(true));
    let mounted_visibility = Rc::clone(&desired_visibility);
    let mut runtime = RuntimeInstance::new(surface.clone(), RuntimeWakeHandle::new(|| {}));
    runtime
        .mount(move || {
            let visible = RwSignal::new(true);
            render! {
                view(
                    style: css!(width: px(100), height: px(100)),
                    on_tap: move |_| visible.set(mounted_visibility.get()),
                ) {
                    Show(when: move || visible.get()) {
                        view(style: url_background_style())
                    }
                }
            }
        })
        .unwrap();
    let commands = surface.take_resource_commands();
    let ResourceCommand::Load(request) = &commands[0] else {
        panic!("initial background URL must load")
    };
    assert_eq!(request.generation, 1);
    assert_eq!(request.source, ResourceSource::Url(BACKGROUND_URL.into()));
    let resource = request.resource;
    runtime
        .dispatch_resource_event(&ready_raster(resource, request.generation))
        .unwrap();

    let mut measurements = NoMeasurement;
    let mut accepted = RecordingRenderer::new(surface.surface());
    runtime
        .drive_frame(
            1.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut accepted,
            LayoutOptions::default(),
        )
        .unwrap();

    desired_visibility.set(false);
    dispatch_control_tap(&runtime, &surface, 1.5);
    let mut recovery = NeedSnapshotOnceSink {
        request_snapshot: true,
    };
    let clear = runtime
        .drive_frame(
            2.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut recovery,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        clear.frame.presentation,
        Some(ApplyResult::NeedSnapshot { .. })
    ));
    assert!(
        surface.take_resource_commands().is_empty(),
        "an unacknowledged clear must not release its resource"
    );

    desired_visibility.set(true);
    dispatch_control_tap(&runtime, &surface, 2.5);
    let reacquired = runtime
        .drive_frame(
            3.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut recovery,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        reacquired.frame.presentation,
        Some(ApplyResult::Accepted { .. })
    ));
    assert!(
        surface.take_resource_commands().is_empty(),
        "reacquiring before Release must cancel retirement without a new Load"
    );
}

#[test]
fn stale_ready_for_a_released_background_does_not_affect_its_replacement() {
    let surface = surface(43);
    let desired_visibility = Rc::new(Cell::new(true));
    let mounted_visibility = Rc::clone(&desired_visibility);
    let mut runtime = RuntimeInstance::new(surface.clone(), RuntimeWakeHandle::new(|| {}));
    runtime
        .mount(move || {
            let visible = RwSignal::new(true);
            render! {
                view(
                    style: css!(width: px(100), height: px(100)),
                    on_tap: move |_| visible.set(mounted_visibility.get()),
                ) {
                    Show(when: move || visible.get()) {
                        view(style: url_background_style())
                    }
                }
            }
        })
        .unwrap();
    let first_commands = surface.take_resource_commands();
    let ResourceCommand::Load(first_load) = &first_commands[0] else {
        panic!("initial background URL must load")
    };
    let resource = first_load.resource;
    assert_eq!(first_load.generation, 1);
    runtime
        .dispatch_resource_event(&ready_raster(resource, first_load.generation))
        .unwrap();

    let mut measurements = NoMeasurement;
    let mut sink = RecordingRenderer::new(surface.surface());
    runtime
        .drive_frame(
            1.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();

    desired_visibility.set(false);
    dispatch_control_tap(&runtime, &surface, 1.5);
    assert!(surface.take_resource_commands().is_empty());
    runtime
        .drive_frame(
            2.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();
    assert_eq!(
        surface.take_resource_commands(),
        vec![ResourceCommand::Release {
            resource,
            generation: 1,
        }]
    );

    desired_visibility.set(true);
    dispatch_control_tap(&runtime, &surface, 2.5);
    runtime
        .drive_frame(
            3.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();
    let second_commands = surface.take_resource_commands();
    assert_eq!(second_commands.len(), 1);
    let ResourceCommand::Load(second_load) = &second_commands[0] else {
        panic!("reacquiring a released URL must start a new load")
    };
    assert_ne!(
        second_load.resource, resource,
        "a resource ID published in FramePacket must not be reused"
    );
    assert_eq!(second_load.generation, 1);
    assert_eq!(
        second_load.source,
        ResourceSource::Url(BACKGROUND_URL.into())
    );
    let replacement = second_load.resource;

    let frames_before_stale = sink.frames().len();
    assert_eq!(
        runtime
            .dispatch_resource_event(&ready_raster(resource, 1))
            .unwrap(),
        whisker::ResourceEventApply::Stale,
        "a completion for the released ID must never become paintable again"
    );
    runtime
        .drive_frame(
            4.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(sink.frames()[frames_before_stale..].iter().all(|frame| {
        frame.packet.operations.iter().all(|operation| {
            !matches!(
                operation,
                Operation::SetBackgroundLayers { layers, .. }
                    if layers.iter().any(|layer| {
                        matches!(layer.image, PaintImage::Resource(id) if id == resource || id == replacement)
                    })
            )
        })
    }));

    assert_eq!(
        runtime
            .dispatch_resource_event(&ready_raster(replacement, 1))
            .unwrap(),
        whisker::ResourceEventApply::Applied
    );
    runtime
        .drive_frame(
            5.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(
        sink.frames()
            .last()
            .unwrap()
            .packet
            .operations
            .iter()
            .any(|operation| matches!(
                operation,
                Operation::SetBackgroundLayers { layers, .. }
                    if layers.iter().any(|layer| layer.image == PaintImage::Resource(replacement))
            ))
    );
}

#[test]
fn host_viewport_updates_re_resolve_styles_layout_and_measurement() {
    let surface = surface(36);
    let mut runtime = RuntimeInstance::new(surface.clone(), RuntimeWakeHandle::new(|| {}));
    runtime
        .mount(|| {
            render! {
                view(style: css!(width: vw(50), height: vh(50))) {
                    text(value: "viewport", style: css!(font_size: rpx(75)))
                }
            }
        })
        .unwrap();

    let mut measurements = ReadyTextMeasurement::default();
    let mut sink = RecordingRenderer::new(surface.surface());
    runtime
        .drive_frame(
            1.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();
    let root = surface.root().unwrap();
    let first_call_count = measurements.calls.len();
    assert!(first_call_count > 0);
    assert!(matches!(
        sink.frames()[0].packet.header.mode,
        FrameMode::Snapshot
    ));
    assert!(
        sink.frames()[0]
            .packet
            .operations
            .iter()
            .any(|operation| matches!(
                operation,
                Operation::SetLayout { node, geometry }
                    if *node == root
                        && geometry.border_box.width == 100.0
                        && geometry.border_box.height == 50.0
            ))
    );
    let first_text_sizes = measurements
        .calls
        .iter()
        .flatten()
        .map(|request| match &request.payload {
            MeasurementPayload::Text(text) => text.style.font_size,
            _ => unreachable!(),
        })
        .collect::<Vec<_>>();
    assert!(
        first_text_sizes
            .iter()
            .all(|size| (*size - 20.0).abs() < 0.001),
        "unexpected initial text sizes: {first_text_sizes:?}"
    );

    runtime
        .drive_frame(
            2.0,
            StyleEnvironment::new(400.0, 300.0, 2.0, 14.0),
            2,
            2,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();
    assert_eq!(
        surface.environment(),
        StyleEnvironment::new(400.0, 300.0, 2.0, 14.0)
    );
    assert!(measurements.calls.len() > first_call_count);
    assert!(
        measurements.calls[first_call_count..]
            .iter()
            .flatten()
            .all(|request| {
                matches!(
                    &request.payload,
                    MeasurementPayload::Text(text) if (text.style.font_size - 40.0).abs() < 0.001
                )
            })
    );
    let resized_call_count = measurements.calls.len();
    let delta = &sink.frames()[1].packet;
    assert_eq!(delta.header.mode, FrameMode::Delta);
    assert_eq!(delta.header.viewport_epoch, 2);
    assert!(delta.operations.iter().any(|operation| matches!(
        operation,
        Operation::SetLayout { node, geometry }
            if *node == root
                && geometry.border_box.width == 200.0
                && geometry.border_box.height == 150.0
    )));
    assert!(delta.operations.iter().any(|operation| matches!(
        operation,
        Operation::SetText { content, .. }
            if (content.payload.style.font_size - 40.0).abs() < 0.001
    )));

    let idle = runtime
        .drive_frame(
            3.0,
            StyleEnvironment::new(400.0, 300.0, 2.0, 14.0),
            2,
            2,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(idle.frame.presentation.is_none());
    assert_eq!(measurements.calls.len(), resized_call_count);

    let scale_changed = runtime
        .drive_frame(
            4.0,
            StyleEnvironment::new(400.0, 300.0, 3.0, 14.0),
            3,
            3,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(measurements.calls.len() > resized_call_count);
    assert!(scale_changed.frame.presentation.is_none());

    let accepted_environment = surface.environment();
    let accepted_frames = sink.frames().len();
    let invalid = runtime.drive_frame(
        5.0,
        StyleEnvironment::new(f32::NAN, 300.0, 3.0, 14.0),
        4,
        4,
        &mut measurements,
        &mut sink,
        LayoutOptions::default(),
    );
    assert!(matches!(
        invalid,
        Err(RuntimeDriveError::Environment(RuntimeBindingError::Style(
            StyleResolutionError::InvalidEnvironment
        )))
    ));
    assert_eq!(surface.environment(), accepted_environment);
    assert_eq!(sink.frames().len(), accepted_frames);
}

#[test]
fn render_root_flex_grow_fills_host_viewport_without_a_protocol_node() {
    let surface = surface(37);
    let mut runtime = RuntimeInstance::new(surface.clone(), RuntimeWakeHandle::new(|| {}));
    runtime
        .mount(|| render! { view(style: css!(flex_grow: 1.0)) })
        .unwrap();

    let mut measurements = NoMeasurement;
    let mut sink = RecordingRenderer::new(surface.surface());
    runtime
        .drive_frame(
            1.0,
            StyleEnvironment::new(320.0, 240.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();

    let root = surface.root().unwrap();
    let packet = &sink.frames()[0].packet;
    assert_eq!(
        packet
            .operations
            .iter()
            .filter(|operation| matches!(operation, Operation::CreateNode { .. }))
            .count(),
        1
    );
    assert!(packet.operations.iter().any(|operation| matches!(
        operation,
        Operation::SetLayout { node, geometry }
            if *node == root
                && geometry.border_box.x == 0.0
                && geometry.border_box.y == 0.0
                && geometry.border_box.width == 320.0
                && geometry.border_box.height == 240.0
    )));
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
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
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
        detail: WhiskerValue::Null,
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
fn raw_touch_stream_synthesizes_tap_but_drag_does_not() {
    let surface = surface(36);
    let mut runtime = RuntimeInstance::new(surface.clone(), RuntimeWakeHandle::new(|| {}));
    let taps = Rc::new(Cell::new(0));
    let tap_count = Rc::clone(&taps);

    runtime
        .mount(move || {
            render! {
                view(
                    style: css!(width: px(100), height: px(100)),
                    on_tap: move |_| tap_count.set(tap_count.get() + 1),
                )
            }
        })
        .unwrap();

    let mut measurements = NoMeasurement;
    let mut sink = RecordingRenderer::new(surface.surface());
    runtime
        .drive_frame(
            1.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();

    let pointer = |timestamp_ms, kind, x, y, buttons| InputEvent {
        surface: surface.surface(),
        timestamp_ms,
        kind,
        pointer: Some(PointerInput {
            id: PointerId::new(1).unwrap(),
            kind: PointerKind::Touch,
            position: InputPoint { x, y },
            buttons,
            changed_button: -1,
        }),
        target: None,
        detail: WhiskerValue::Null,
    };

    runtime
        .dispatch_input(&pointer(2.0, InputEventKind::PointerDown, 10.0, 10.0, 1))
        .unwrap();
    let up = runtime
        .dispatch_input(&pointer(30.0, InputEventKind::PointerUp, 10.0, 10.0, 0))
        .unwrap();
    assert!(up.consumed);
    assert_eq!(taps.get(), 1);

    runtime
        .dispatch_input(&pointer(40.0, InputEventKind::PointerDown, 10.0, 10.0, 1))
        .unwrap();
    runtime
        .dispatch_input(&pointer(50.0, InputEventKind::PointerMove, 30.0, 10.0, 1))
        .unwrap();
    runtime
        .dispatch_input(&pointer(60.0, InputEventKind::PointerUp, 30.0, 10.0, 0))
        .unwrap();
    assert_eq!(taps.get(), 1);
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
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
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
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
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
                            detail: WhiskerValue::Null,
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
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
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
        detail: WhiskerValue::Null,
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
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(!blocked.frame.layout.has_layout());
    assert!(blocked.frame.presentation.is_none());
    assert!(!measurements.requests.is_empty());
    let before = wakes.load(Ordering::SeqCst);
    for (request, request_id) in &measurements.requests {
        runtime
            .measurement_ready(&MeasurementReady {
                key: request.key,
                request_id: *request_id,
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
    }
    assert!(wakes.load(Ordering::SeqCst) > before);

    let mut ready_measurements = ReadyTextMeasurement::default();
    let complete = runtime
        .drive_frame(
            2.0,
            StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
            1,
            1,
            &mut ready_measurements,
            &mut sink,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(complete.frame.layout.has_layout());
    assert!(complete.frame.presentation.is_some());
}
