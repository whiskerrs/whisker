use std::convert::Infallible;

use whisker::css::{BorderStyle, Overflow};
use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, Owner};
use whisker::runtime::view::{
    BindType, create_element_by_name, set_attribute_bool, set_event_listener, set_root,
    try_invoke_element_method, with_installed_renderer,
};
use whisker::{
    ElementModuleDefinition, ElementProviderMetadata, ElementRegistry, ElementTag,
    RuntimeBindingError, SurfaceRuntime,
};
use whisker_engine::RecordingRenderer;
use whisker_engine::whisker_layout::LayoutSize;
use whisker_engine::whisker_protocol::{
    CommandId, ElementCommandSchema, ElementEventSchema, ElementMeasurement, ElementPropertySchema,
    ElementSchema, ElementValueKind, EventId, MeasuredSize, MeasurementMetrics, MeasurementPayload,
    MeasurementRequest, MeasurementResponse, Operation, PaintColor, PreparedContentId, PropertyId,
    SurfaceId, WhiskerValue,
};
use whisker_engine::whisker_style::StyleEnvironment;
use whisker_engine::{LayoutOptions, MeasurementProvider};

#[derive(Default)]
struct TextHost {
    calls: Vec<Vec<MeasurementRequest>>,
}

#[whisker::module_component(
    name = "whisker.test/AutoRegistered",
    measurement = None,
)]
fn auto_registered(enabled: Signal<bool>, style: whisker::Style) {}

#[whisker::module_component(
    name = "whisker.test/NativeLabel",
    measurement = Text,
)]
fn native_label(children: whisker::TextChildren) {}

impl MeasurementProvider for TextHost {
    type Error = Infallible;

    fn measure_batch(
        &mut self,
        _surface: SurfaceId,
        requests: &[MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        self.calls.push(requests.to_vec());
        responses.extend(requests.iter().map(|request| {
            let MeasurementPayload::Text(payload) = &request.payload else {
                panic!("render! Text emitted a non-text measurement")
            };
            assert_eq!(payload.style.font_size, 20.0);
            MeasurementResponse::Ready {
                key: request.key,
                environment_epoch: request.environment_epoch,
                metrics: MeasurementMetrics {
                    size: MeasuredSize::new(payload.text.chars().count() as f32 * 10.0, 24.0),
                    first_baseline: Some(18.0),
                    last_baseline: Some(18.0),
                    overflow: None,
                    prepared_content: PreparedContentId::new(request.key.get()),
                },
            }
        }));
        Ok(())
    }
}

#[test]
fn module_component_registers_its_schema_with_the_active_surface() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(11).expect("test surface"),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );
    assert!(
        surface
            .element_registrations()
            .iter()
            .all(|registration| registration.name != auto_registered_schema::NAME)
    );

    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                AutoRegistered(
                    enabled: true,
                    style: css!(width: px(40), height: px(20)),
                )
            }
        });
        set_root(root);
    });
    assert_eq!(surface.binding_error(), None);
    let registration = surface
        .element_registrations()
        .into_iter()
        .find(|registration| registration.name == auto_registered_schema::NAME)
        .expect("module component schema registered during authoring");

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("auto-registered module frame");
    assert!(renderer.frames()[0].packet.operations.iter().any(
        |operation| matches!(operation, Operation::CreateNode { element_type, .. } if *element_type == registration.element_type)
    ));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn custom_plain_text_children_lower_to_measurement_and_set_text() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(15).unwrap(),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );

    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                NativeLabel(style: css!(font_size: px(20))) { "custom text" }
            }
        });
        set_root(root);
    });
    assert_eq!(surface.binding_error(), None);

    let registration = surface
        .element_registrations()
        .into_iter()
        .find(|registration| registration.name == native_label_schema::NAME)
        .unwrap();
    assert_eq!(registration.child_policy, whisker::ChildPolicy::PlainText);

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();

    assert!(renderer.frames()[0].packet.operations.iter().any(|operation| {
        matches!(operation, Operation::SetText { content, .. } if content.payload.text == "custom text")
    }));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn builtin_text_accepts_dynamic_into_view_text_children() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(17).unwrap(),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );

    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! { text(style: css!(font_size: px(20))) { { "dynamic".to_string() } } }
        });
        set_root(root);
    });
    assert_eq!(surface.binding_error(), None);

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();
    assert!(renderer.frames()[0].packet.operations.iter().any(|operation| {
        matches!(operation, Operation::SetText { content, .. } if content.payload.text == "dynamic")
    }));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn render_text_reaches_measured_frame_and_paint_only_delta() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(7).expect("test surface"),
        StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
    );
    let value = owner.with(|| signal(String::from("hello")));
    let root = with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                view(style: css!(color: Color::rgba(220, 30, 40, 0.75))) {
                    text(
                        value: value,
                        style: css!(font_size: px(20)),
                    )
                }
            }
        });
        set_root(root);
        root
    });
    assert_eq!(surface.binding_error(), None);

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    let frame = surface
        .render_frame(
            LayoutSize::new(200.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("render! text frame");
    assert!(frame.layout.has_layout());
    assert!(matches!(
        frame.presentation,
        Some(whisker_engine::whisker_protocol::ApplyResult::Accepted { revision: 1 })
    ));
    let measurement_calls = host.calls.len();
    assert!(measurement_calls > 0);

    let snapshot = &renderer.frames()[0].packet;
    let text_node = snapshot
        .operations
        .iter()
        .find_map(|operation| match operation {
            Operation::SetText { node, content } => {
                assert_eq!(content.payload.text, "hello");
                assert_eq!(content.payload.style.font_size, 20.0);
                assert_eq!(
                    content.paint.foreground,
                    PaintColor::Srgba {
                        red: 220,
                        green: 30,
                        blue: 40,
                        alpha: 0.75,
                    }
                );
                assert!(content.prepared_content.is_some());
                Some(*node)
            }
            _ => None,
        })
        .expect("snapshot SetText");
    assert!(snapshot.operations.iter().any(
        |operation| matches!(operation, Operation::InsertChild { child, .. } if *child == text_node)
    ));
    assert!(snapshot.operations.iter().any(
        |operation| matches!(operation, Operation::SetLayout { node, geometry } if *node == text_node && geometry.border_box.width == 50.0 && geometry.border_box.height == 24.0)
    ));
    with_installed_renderer(surface.renderer(), || {
        value.set(String::from("hello world"));
        whisker::flush();
    });
    surface
        .drive_layout(
            LayoutSize::new(200.0, 100.0),
            1,
            &mut host,
            LayoutOptions::default(),
        )
        .expect("reactive render! text update");
    assert!(host.calls.len() > measurement_calls);
    assert!(matches!(
        surface.present(1, &mut renderer),
        Ok(Some(
            whisker_engine::whisker_protocol::ApplyResult::Accepted { revision: 2 }
        ))
    ));
    let text_delta = &renderer.frames()[1].packet;
    assert!(text_delta.operations.iter().any(|operation| matches!(
        operation,
        Operation::SetText { node, content }
            if *node == text_node
                && content.payload.text == "hello world"
                && content.paint.foreground == PaintColor::Srgba {
                    red: 220,
                    green: 30,
                    blue: 40,
                    alpha: 0.75,
                }
                && content.prepared_content.is_some()
    )));
    assert!(text_delta.operations.iter().any(
        |operation| matches!(operation, Operation::SetLayout { node, geometry } if *node == text_node && geometry.border_box.width == 110.0 && geometry.border_box.height == 24.0)
    ));
    let measurement_calls = host.calls.len();

    with_installed_renderer(surface.renderer(), || {
        whisker::apply_style(root, Css::new().color(Color::rgba(20, 60, 230, 1.0)));
    });
    let progress = surface
        .drive_layout(
            LayoutSize::new(200.0, 100.0),
            1,
            &mut host,
            LayoutOptions::default(),
        )
        .expect("paint-only update");
    assert!(progress.has_layout());
    assert_eq!(
        host.calls.len(),
        measurement_calls,
        "paint must not trigger measurement"
    );

    assert!(matches!(
        surface.present(1, &mut renderer),
        Ok(Some(
            whisker_engine::whisker_protocol::ApplyResult::Accepted { revision: 3 }
        ))
    ));
    let delta = &renderer.frames()[2].packet;
    assert!(matches!(
        delta.operations.as_slice(),
        [Operation::SetText { node, content }]
            if *node == text_node
                && content.paint.foreground == PaintColor::Srgba {
                    red: 20,
                    green: 60,
                    blue: 230,
                    alpha: 1.0,
                }
                && content.prepared_content.is_some()
    ));
    with_installed_renderer(surface.renderer(), || owner.dispose());
    assert_eq!(surface.binding_error(), None);
}

fn painted_box(background: Color) -> Css {
    Css::new()
        .width(px(120))
        .height(px(48))
        .background_color(background)
        .border_top_width(px(2))
        .border_right_width(px(2))
        .border_bottom_width(px(2))
        .border_left_width(px(2))
        .border_top_color(Color::rgb(10, 20, 30))
        .border_right_color(Color::rgb(10, 20, 30))
        .border_bottom_color(Color::rgb(10, 20, 30))
        .border_left_color(Color::rgb(10, 20, 30))
        .border_top_style(BorderStyle::Solid)
        .border_right_style(BorderStyle::Solid)
        .border_bottom_style(BorderStyle::Solid)
        .border_left_style(BorderStyle::Solid)
        .border_radius(px(8))
        .overflow(Overflow::Hidden)
        .opacity(0.75)
        .z_index(4)
}

#[test]
fn render_box_paint_and_clip_reach_the_frame_sink() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(8).expect("test surface"),
        StyleEnvironment::new(200.0, 100.0, 1.0, 14.0),
    );
    let root = with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| render! { view(style: painted_box(Color::rgb(200, 30, 40))) });
        set_root(root);
        root
    });
    let mut host = TextHost::default();
    surface
        .drive_layout(
            LayoutSize::new(200.0, 100.0),
            1,
            &mut host,
            LayoutOptions::default(),
        )
        .expect("box layout");
    assert!(host.calls.is_empty());

    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .present(1, &mut renderer)
        .expect("present painted snapshot")
        .expect("painted snapshot exists");
    let root_node = surface.root().expect("surface root");
    let snapshot = &renderer.frames()[0].packet;
    assert!(snapshot.operations.iter().any(|operation| matches!(
        operation,
        Operation::SetBoxPaint { node, paint }
            if *node == root_node
                && paint.background_color == PaintColor::Srgba {
                    red: 200,
                    green: 30,
                    blue: 40,
                    alpha: 1.0,
                }
                && paint.border_widths.top.length == 2.0
                && paint.border_radii.top_left.length == 8.0
    )));
    assert!(snapshot.operations.iter().any(|operation| matches!(
        operation,
        Operation::SetClip { node, clip }
            if *node == root_node
                && clip.horizontal == whisker_engine::whisker_protocol::OverflowClip::Hidden
                && clip.vertical == whisker_engine::whisker_protocol::OverflowClip::Hidden
    )));
    assert!(snapshot.operations.iter().any(
        |operation| matches!(operation, Operation::SetOpacity { node, opacity } if *node == root_node && *opacity == 0.75)
    ));
    assert!(snapshot.operations.iter().any(
        |operation| matches!(operation, Operation::SetZOrder { node, z_order } if *node == root_node && *z_order == 4)
    ));

    with_installed_renderer(surface.renderer(), || {
        whisker::apply_style(root, painted_box(Color::rgb(20, 60, 230)));
    });
    surface
        .drive_layout(
            LayoutSize::new(200.0, 100.0),
            1,
            &mut host,
            LayoutOptions::default(),
        )
        .expect("paint-only box update");
    assert!(host.calls.is_empty());
    surface
        .present(1, &mut renderer)
        .expect("present box delta")
        .expect("box delta exists");
    assert!(matches!(
        renderer.frames()[1].packet.operations.as_slice(),
        [Operation::SetBoxPaint { node, paint }]
            if *node == root_node
                && paint.background_color == PaintColor::Srgba {
                    red: 20,
                    green: 60,
                    blue: 230,
                    alpha: 1.0,
                }
    ));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn frame_element_type_comes_from_the_surface_registry() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(10).expect("test surface"),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );
    let scroll_type = surface
        .element_registrations()
        .into_iter()
        .find(|registration| registration.name == "whisker.ui/ScrollView")
        .expect("standard ScrollView registration")
        .element_type;
    assert_ne!(
        scroll_type.get(),
        ElementTag::ScrollView as u32,
        "wire IDs must not depend on authoring-tag discriminants"
    );

    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| render! { scroll_view() });
        set_root(root);
    });
    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("ScrollView frame");

    assert!(renderer.frames()[0].packet.operations.iter().any(
        |operation| matches!(operation, Operation::CreateNode { element_type, .. } if *element_type == scroll_type)
    ));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn module_element_uses_the_same_retained_frame_path_as_builtins() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let registry = ElementRegistry::standard_builder()
        .register_module(ElementModuleDefinition::new(
            "example.maps",
            [ElementProviderMetadata::named(ElementSchema {
                name: "example.maps/Map".into(),
                child_policy: whisker::ChildPolicy::Elements,
                measurement: ElementMeasurement::None,
                properties: Vec::new(),
                events: Vec::new(),
                commands: Vec::new(),
            })],
        ))
        .build()
        .expect("valid module element registry");
    let map_type = registry
        .registration_for_name("example.maps/Map")
        .expect("map authoring binding")
        .element_type;
    let surface = SurfaceRuntime::with_element_registry(
        SurfaceId::new(12).expect("test surface"),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
        registry,
    );

    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| create_element_by_name("example.maps/Map"));
        set_root(root);
    });
    assert_eq!(surface.binding_error(), None);

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("module element frame");

    assert!(renderer.frames()[0].packet.operations.iter().any(
        |operation| matches!(operation, Operation::CreateNode { element_type, .. } if *element_type == map_type)
    ));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn external_element_properties_events_and_commands_share_the_retained_frame_path() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let checked = PropertyId::new(1).unwrap();
    let change = EventId::new(1).unwrap();
    let toggle = CommandId::new(1).unwrap();
    let registry = ElementRegistry::standard_builder()
        .register_provider(ElementProviderMetadata::named(ElementSchema {
            name: "whisker.test/Toggle".into(),
            child_policy: whisker::ChildPolicy::None,
            measurement: ElementMeasurement::None,
            properties: vec![ElementPropertySchema {
                property: checked,
                name: "checked".into(),
                value: ElementValueKind::Bool,
            }],
            events: vec![ElementEventSchema {
                event: change,
                name: "change".into(),
                detail: Some(ElementValueKind::Map),
            }],
            commands: vec![ElementCommandSchema {
                command: toggle,
                name: "toggle".into(),
                arguments: ElementValueKind::Null,
            }],
        }))
        .build()
        .unwrap();
    let surface = SurfaceRuntime::with_element_registry(
        SurfaceId::new(13).unwrap(),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
        registry,
    );

    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| create_element_by_name("whisker.test/Toggle"));
        set_attribute_bool(root, "checked", true);
        set_event_listener(root, "change", BindType::Bind, Box::new(|_| {}));
        assert_eq!(
            try_invoke_element_method(root, "toggle", WhiskerValue::args([])),
            Some(WhiskerValue::Null)
        );
        set_root(root);
    });
    assert_eq!(surface.binding_error(), None);

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .unwrap();
    let operations = &renderer.frames()[0].packet.operations;
    assert!(operations.iter().any(|operation| matches!(
        operation,
        Operation::SetProperty { property, value: WhiskerValue::Bool(true), .. }
            if *property == checked
    )));
    assert!(operations.iter().any(|operation| matches!(
        operation,
        Operation::SetEventMask { event_mask, .. } if *event_mask == 1
    )));
    assert!(operations.iter().any(|operation| matches!(
        operation,
        Operation::InvokeCommand {
            command,
            arguments: WhiskerValue::Null,
            ..
        } if *command == toggle
    )));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn text_leaf_contract_is_enforced_before_frame_generation() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(11).expect("test surface"),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );

    with_installed_renderer(surface.renderer(), || {
        owner.with(|| render! { text { view() } });
    });

    assert!(matches!(
        surface.binding_error(),
        Some(RuntimeBindingError::ChildrenNotAllowed { .. })
    ));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn element_children_reject_raw_text_before_frame_generation() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(16).unwrap(),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );

    with_installed_renderer(surface.renderer(), || {
        owner.with(|| render! { view { "not implicitly wrapped" } });
    });

    assert!(matches!(
        surface.binding_error(),
        Some(RuntimeBindingError::InvalidRawTextParent { .. })
    ));
    with_installed_renderer(surface.renderer(), || owner.dispose());
}

#[test]
fn wpt_border_radius_sum_of_radii_001_reaches_layout_and_frame_protocol() {
    __reset_for_tests();
    let owner = Owner::new(None);
    let surface = SurfaceRuntime::new(
        SurfaceId::new(9).expect("test surface"),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );
    with_installed_renderer(surface.renderer(), || {
        let root = owner.with(|| {
            render! {
                view(style: Css::new()
                    .width(px(100))
                    .height(px(100))
                    .background_color(Color::rgba(0, 0, 0, 0.0))
                    .border_top_width(px(10))
                    .border_right_width(px(10))
                    .border_bottom_width(px(10))
                    .border_left_width(px(10))
                    .border_top_color(Color::rgb(0, 0, 0))
                    .border_right_color(Color::rgb(0, 0, 0))
                    .border_bottom_color(Color::rgb(0, 0, 0))
                    .border_left_color(Color::rgb(0, 0, 0))
                    .border_top_style(BorderStyle::Solid)
                    .border_right_style(BorderStyle::Solid)
                    .border_bottom_style(BorderStyle::Solid)
                    .border_left_style(BorderStyle::Solid)
                    .border_top_left_radius(px(60))
                    .border_top_right_radius(px(150))
                    .border_bottom_right_radius(px(30))
                    .border_bottom_left_radius(px(30)))
            }
        });
        set_root(root);
    });

    let mut host = TextHost::default();
    let mut renderer = RecordingRenderer::new(surface.surface());
    surface
        .render_frame(
            LayoutSize::new(100.0, 100.0),
            1,
            1,
            &mut host,
            &mut renderer,
            LayoutOptions::default(),
        )
        .expect("WPT-derived rounded box frame");
    assert!(host.calls.is_empty());

    let root = surface.root().expect("surface root");
    let packet = &renderer.frames()[0].packet;
    assert!(packet.operations.iter().any(|operation| matches!(
        operation,
        Operation::SetLayout { node, geometry }
            if *node == root
                && geometry.border_box.width == 100.0
                && geometry.border_box.height == 100.0
    )));
    assert!(packet.operations.iter().any(|operation| matches!(
        operation,
        Operation::SetBoxPaint { node, paint }
            if *node == root
                && paint.border_widths.top.length == 10.0
                && paint.border_radii.top_left.length == 60.0
                && paint.border_radii.top_right.length == 150.0
                && paint.border_radii.bottom_right.length == 30.0
                && paint.border_radii.bottom_left.length == 30.0
    )));

    with_installed_renderer(surface.renderer(), || owner.dispose());
}
