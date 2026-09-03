use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::element::{
    DesktopElementFactory, DesktopNativeElement, DesktopNativeEvent, DesktopViewDefinition,
    DesktopViewImplementation, built_in_element_factories,
};
use whisker::standard_element_registrations;
use whisker_protocol::{
    AccessibilityRole, AccessibilityState, CommandId, ElementCommandSchema, ElementEventSchema,
    ElementMeasurement, ElementPropertySchema, ElementRegistration, ElementValueKind, EventId,
    FrameHeader, MeasureFontFamily, MeasureFontStyle, MeasureLineHeight, MeasureTextDirection,
    MeasureTextOverflow, MeasureTextWrap, PaintCorners, PaintEdges, PaintLengthPercentage,
    PropertyId, ProtocolVersion, TextMeasurePayload, TextMeasureStyle, TextPaint,
};

fn element_type(name: &str) -> ElementTypeId {
    standard_element_registrations()
        .into_iter()
        .find(|registration| registration.name == name)
        .expect("standard element registration")
        .element_type
}

fn scene(surface: SurfaceId) -> DesktopScene {
    DesktopScene::new(
        surface,
        DesktopElementRegistry::bind(
            &standard_element_registrations(),
            &crate::element::built_in_element_factories(),
        )
        .unwrap(),
    )
}

fn id(value: u64) -> NodeId {
    NodeId::new(value).unwrap()
}

fn geometry(x: f32, y: f32, width: f32, height: f32) -> LayoutGeometry {
    LayoutGeometry {
        border_box: LayoutRect {
            x,
            y,
            width,
            height,
        },
        content_box: LayoutRect {
            x: 1.0,
            y: 2.0,
            width: (width - 2.0).max(0.0),
            height: (height - 4.0).max(0.0),
        },
    }
}

fn paint(color: PaintColor) -> BoxPaint {
    let zero = PaintLengthPercentage::default();
    BoxPaint {
        background_color: color,
        border_widths: PaintEdges {
            top: zero,
            right: zero,
            bottom: zero,
            left: zero,
        },
        border_colors: PaintEdges {
            top: PaintColor::default(),
            right: PaintColor::default(),
            bottom: PaintColor::default(),
            left: PaintColor::default(),
        },
        border_styles: PaintEdges {
            top: whisker_protocol::BorderLineStyle::None,
            right: whisker_protocol::BorderLineStyle::None,
            bottom: whisker_protocol::BorderLineStyle::None,
            left: whisker_protocol::BorderLineStyle::None,
        },
        border_radii: PaintCorners {
            top_left: whisker_protocol::PaintCornerRadius::circular(zero),
            top_right: whisker_protocol::PaintCornerRadius::circular(zero),
            bottom_right: whisker_protocol::PaintCornerRadius::circular(zero),
            bottom_left: whisker_protocol::PaintCornerRadius::circular(zero),
        },
    }
}

fn text() -> TextContent {
    TextContent {
        payload: TextMeasurePayload {
            text: "native".into(),
            style: TextMeasureStyle {
                font_families: vec![MeasureFontFamily::System],
                font_size: 14.0,
                font_weight: 400,
                font_style: MeasureFontStyle::Normal,
                line_height: MeasureLineHeight::Normal,
                letter_spacing: 0.0,
                ..TextMeasureStyle::default()
            },
            locale: None,
            direction: MeasureTextDirection::Auto,
            alignment: whisker_protocol::MeasureTextAlignment::Start,
            indent: Default::default(),
            wrap: MeasureTextWrap::Wrap,
            word_break: Default::default(),
            max_lines: None,
            overflow: MeasureTextOverflow::Clip,
        },
        paint: TextPaint::default(),
        prepared_content: None,
    }
}

fn packet(mode: FrameMode, base: u64, target: u64, operations: Vec<Operation>) -> FramePacket {
    FramePacket {
        header: FrameHeader {
            version: ProtocolVersion::CURRENT,
            surface: SurfaceId::new(1).unwrap(),
            scene_epoch: 1,
            frame_id: target,
            base_revision: base,
            target_revision: target,
            viewport_epoch: 1,
            mode,
        },
        operations,
    }
}

#[test]
fn pointer_capture_is_retained_and_released_by_the_desktop_surface() {
    let node = id(1);
    let pointer = whisker_protocol::PointerId::new(7).unwrap();
    let mut scene = scene(SurfaceId::new(1).unwrap());

    scene
        .present(&packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![
                Operation::CreateNode {
                    node,
                    element_type: element_type(whisker::VIEW_ELEMENT_NAME),
                },
                Operation::SetPointerCapture { node, pointer },
            ],
        ))
        .unwrap();
    assert_eq!(scene.pointer_captures.get(&pointer), Some(&node));

    scene
        .present(&packet(
            FrameMode::Delta,
            1,
            2,
            vec![Operation::ReleasePointerCapture { node, pointer }],
        ))
        .unwrap();
    assert!(!scene.pointer_captures.contains_key(&pointer));
}

const CHECKED: PropertyId = PropertyId::new(1).unwrap();
const DISABLED: PropertyId = PropertyId::new(2).unwrap();
const CHANGE: EventId = EventId::new(1).unwrap();
const TOGGLE: CommandId = CommandId::new(1).unwrap();

#[derive(Debug)]
struct ToggleNative {
    checked: bool,
    disabled: bool,
    events: DesktopEventEmitter,
}

impl DesktopNativeElement for ToggleNative {
    fn set_property(&mut self, property: PropertyId, value: &WhiskerValue) {
        let WhiskerValue::Bool(value) = value else {
            unreachable!()
        };
        match property {
            CHECKED => self.checked = *value,
            DISABLED => self.disabled = *value,
            _ => unreachable!(),
        }
    }

    fn clear_property(&mut self, property: PropertyId) {
        match property {
            CHECKED => self.checked = false,
            DISABLED => self.disabled = false,
            _ => unreachable!(),
        }
    }

    fn invoke_command(&mut self, command: CommandId, _arguments: &WhiskerValue) {
        assert_eq!(command, TOGGLE);
        if self.disabled {
            return;
        }
        self.checked = !self.checked;
        self.events.emit(DesktopNativeEvent {
            event: "change".into(),
            detail: WhiskerValue::map([("checked", WhiskerValue::Bool(self.checked))]),
        });
    }
}

fn toggle_scene_with_wake(event_wake: RuntimeWakeHandle) -> (DesktopScene, ElementTypeId) {
    let element_type = ElementTypeId::new(20).unwrap();
    let mut registrations = standard_element_registrations();
    registrations.push(ElementRegistration {
        element_type,
        name: "whisker.test/Toggle".into(),
        child_policy: whisker_protocol::ChildPolicy::None,
        measurement: ElementMeasurement::None,
        text_style: false,
        properties: vec![
            ElementPropertySchema {
                property: CHECKED,
                name: "checked".into(),
                value: ElementValueKind::Bool,
            },
            ElementPropertySchema {
                property: DISABLED,
                name: "disabled".into(),
                value: ElementValueKind::Bool,
            },
        ],
        events: vec![ElementEventSchema {
            event: CHANGE,
            name: "change".into(),
            detail: Some(ElementValueKind::Map),
        }],
        commands: vec![ElementCommandSchema {
            command: TOGGLE,
            name: "toggle".into(),
            arguments: ElementValueKind::Null,
        }],
    });
    let mut factories = built_in_element_factories();
    factories.push(DesktopElementFactory::native(
        "whisker.test/Toggle",
        |events| {
            Box::new(ToggleNative {
                checked: false,
                disabled: false,
                events,
            })
        },
    ));
    (
        DesktopScene::new_with_wake(
            SurfaceId::new(1).unwrap(),
            DesktopElementRegistry::bind(&registrations, &factories).unwrap(),
            event_wake,
        ),
        element_type,
    )
}

fn toggle_scene() -> (DesktopScene, ElementTypeId) {
    toggle_scene_with_wake(RuntimeWakeHandle::new(|| {}))
}

#[derive(Debug, Default)]
struct TextInputProbe {
    focused: bool,
    value: String,
    caret_request: Option<([f32; 2], f32)>,
}

#[derive(Debug)]
struct TextInputNative {
    probe: Arc<Mutex<TextInputProbe>>,
}

impl DesktopNativeElement for TextInputNative {
    fn set_property(&mut self, _property: PropertyId, _value: &WhiskerValue) {
        unreachable!("test input has no properties")
    }

    fn clear_property(&mut self, _property: PropertyId) {
        unreachable!("test input has no properties")
    }

    fn invoke_command(&mut self, _command: CommandId, _arguments: &WhiskerValue) {
        unreachable!("test input has no commands")
    }

    fn accepts_text_input(&self) -> bool {
        true
    }

    fn text_input_focused(&self) -> bool {
        self.probe.lock().unwrap().focused
    }

    fn set_text_input_focus(&mut self, focused: bool) {
        self.probe.lock().unwrap().focused = focused;
    }

    fn handle_text_input(&mut self, event: &DesktopTextInputEvent) {
        if let DesktopTextInputEvent::Commit(text) = event {
            self.probe.lock().unwrap().value.push_str(text);
        }
    }

    fn selected_text(&self) -> Option<String> {
        Some(self.probe.lock().unwrap().value.clone())
    }

    fn text_input_caret_rect(&self, logical_size: [f32; 2], scale: f32) -> Option<LayoutRect> {
        self.probe.lock().unwrap().caret_request = Some((logical_size, scale));
        Some(LayoutRect {
            x: 32.0,
            y: 4.0,
            width: 1.0,
            height: 18.0,
        })
    }
}

fn text_input_scene() -> (DesktopScene, ElementTypeId, Arc<Mutex<TextInputProbe>>) {
    let element_type = ElementTypeId::new(21).unwrap();
    let mut registrations = standard_element_registrations();
    registrations.push(ElementRegistration {
        element_type,
        name: "whisker.test/Input".into(),
        child_policy: whisker_protocol::ChildPolicy::None,
        measurement: ElementMeasurement::None,
        text_style: false,
        properties: vec![],
        events: vec![],
        commands: vec![],
    });
    let probe = Arc::new(Mutex::new(TextInputProbe::default()));
    let input_probe = Arc::clone(&probe);
    let mut factories = built_in_element_factories();
    factories.push(DesktopElementFactory::native(
        "whisker.test/Input",
        move |_| {
            Box::new(TextInputNative {
                probe: Arc::clone(&input_probe),
            })
        },
    ));
    (
        DesktopScene::new(
            SurfaceId::new(1).unwrap(),
            DesktopElementRegistry::bind(&registrations, &factories).unwrap(),
        ),
        element_type,
        probe,
    )
}

#[test]
fn text_input_focus_and_edits_are_routed_to_the_hit_native_element() {
    let node = id(1);
    let (mut scene, element_type, probe) = text_input_scene();
    scene
        .present(&packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![
                Operation::CreateNode { node, element_type },
                Operation::SetLayout {
                    node,
                    geometry: geometry(10.0, 20.0, 120.0, 44.0),
                },
            ],
        ))
        .unwrap();

    assert!(scene.focus_text_input_at([20.0, 30.0]));
    assert_eq!(
        scene.focused_text_input_caret_rect(2.0),
        Some(LayoutRect {
            x: 43.0,
            y: 26.0,
            width: 1.0,
            height: 18.0,
        })
    );
    assert_eq!(
        probe.lock().unwrap().caret_request,
        Some(([118.0, 40.0], 2.0))
    );
    assert!(scene.dispatch_text_input(&DesktopTextInputEvent::Commit("hi".into())));
    assert_eq!(scene.selected_text().as_deref(), Some("hi"));
    assert_eq!(probe.lock().unwrap().value, "hi");

    assert!(scene.focus_text_input_at([500.0, 500.0]));
    assert!(!probe.lock().unwrap().focused);
    assert!(!scene.dispatch_text_input(&DesktopTextInputEvent::Commit("!".into())));
}

#[test]
fn accessibility_semantics_are_retained_with_the_common_presentation() {
    let node = id(1);
    let mut scene = scene(SurfaceId::new(1).unwrap());
    let accessibility = Accessibility::new()
        .label("Settings")
        .role(AccessibilityRole::Button)
        .state(AccessibilityState::new().disabled(true));
    scene
        .present(&packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![
                Operation::CreateNode {
                    node,
                    element_type: element_type(whisker::VIEW_ELEMENT_NAME),
                },
                Operation::SetAccessibility {
                    node,
                    accessibility: accessibility.clone(),
                },
                Operation::SetLayout {
                    node,
                    geometry: geometry(12.0, 18.0, 90.0, 44.0),
                },
            ],
        ))
        .unwrap();

    assert_eq!(scene.nodes[&node].presentation.accessibility, accessibility);
    let snapshot = scene.accessibility_snapshot();
    assert_eq!(snapshot.roots, vec![node]);
    assert_eq!(snapshot.nodes.len(), 1);
    assert_eq!(snapshot.nodes[0].id, node);
    assert_eq!(
        snapshot.nodes[0].bounds,
        LayoutRect {
            x: 12.0,
            y: 18.0,
            width: 90.0,
            height: 44.0,
        }
    );
}

#[test]
fn native_toggle_applies_properties_invokes_command_and_routes_change() {
    let node = id(1);
    let wakes = Arc::new(AtomicUsize::new(0));
    let wake_count = Arc::clone(&wakes);
    let (mut scene, element_type) = toggle_scene_with_wake(RuntimeWakeHandle::new(move || {
        wake_count.fetch_add(1, Ordering::Relaxed);
    }));
    assert_eq!(
        scene.present(&packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![
                Operation::CreateNode { node, element_type },
                Operation::SetEventMask {
                    node,
                    event_mask: 1,
                },
                Operation::SetProperty {
                    node,
                    property: CHECKED,
                    value: WhiskerValue::Bool(true),
                },
                Operation::InvokeCommand {
                    node,
                    command: TOGGLE,
                    arguments: WhiskerValue::Null,
                },
            ],
        )),
        Ok(ApplyResult::Accepted { revision: 1 })
    );
    assert_eq!(
        scene.take_events(),
        vec![DesktopProviderEvent {
            target: node,
            name: "change".into(),
            detail: WhiskerValue::map([("checked", WhiskerValue::Bool(false),)]),
        }]
    );
    assert_eq!(wakes.load(Ordering::Relaxed), 1);
}

#[test]
fn native_toggle_rejects_wrong_property_shape_before_commit() {
    let node = id(1);
    let (mut scene, element_type) = toggle_scene();
    assert_eq!(
        scene.present(&packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![
                Operation::CreateNode { node, element_type },
                Operation::SetProperty {
                    node,
                    property: CHECKED,
                    value: WhiskerValue::String("true".into()),
                },
            ],
        )),
        Err(DesktopPresentError::Element(
            DesktopElementError::InvalidPropertyValue {
                node,
                property: CHECKED,
                expected: ElementValueKind::Bool,
            }
        ))
    );
    assert!(scene.nodes.is_empty());
}

#[test]
fn unregistered_background_resource_rejects_the_whole_frame() {
    let node = id(1);
    let mut scene = scene(SurfaceId::new(1).unwrap());
    let resource = ResourceId::new(u64::MAX).unwrap();
    let layer = BackgroundLayer {
        image: PaintImage::Resource(resource),
        position: Default::default(),
        size: BackgroundSize::Auto,
        repeat_x: ImageRepeat::Repeat,
        repeat_y: ImageRepeat::Repeat,
        origin: PaintBox::Padding,
        clip: PaintBox::Border,
        attachment: BackgroundAttachment::Scroll,
        blend_mode: BlendMode::Normal,
    };
    let frame = packet(
        FrameMode::Snapshot,
        0,
        1,
        vec![
            Operation::CreateNode {
                node,
                element_type: element_type(whisker::VIEW_ELEMENT_NAME),
            },
            Operation::SetBackgroundLayers {
                node,
                layers: vec![layer.clone()],
            },
        ],
    );

    assert_eq!(
        scene.present(&frame),
        Err(DesktopPresentError::Unsupported("background-layers"))
    );
    assert!(scene.nodes.is_empty());

    scene.register_raster_resource(resource);
    assert_eq!(
        scene.present(&frame),
        Ok(ApplyResult::Accepted { revision: 1 })
    );
    assert_eq!(
        scene.nodes[&node].presentation.background_layers,
        vec![layer]
    );
}

#[test]
fn scroll_container_offsets_paint_and_hit_testing_inside_its_viewport() {
    let scroll = id(1);
    let content = id(2);
    let target = id(3);
    let mut scene = scene(SurfaceId::new(1).unwrap());
    scene
        .present(&packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![
                Operation::CreateNode {
                    node: scroll,
                    element_type: element_type(whisker::SCROLL_VIEW_ELEMENT_NAME),
                },
                Operation::SetEventMask {
                    node: scroll,
                    event_mask: 1,
                },
                Operation::CreateNode {
                    node: content,
                    element_type: element_type(whisker::VIEW_ELEMENT_NAME),
                },
                Operation::CreateNode {
                    node: target,
                    element_type: element_type(whisker::VIEW_ELEMENT_NAME),
                },
                Operation::InsertChild {
                    parent: scroll,
                    child: content,
                    index: 0,
                },
                Operation::InsertChild {
                    parent: content,
                    child: target,
                    index: 0,
                },
                Operation::SetLayout {
                    node: scroll,
                    geometry: geometry(0.0, 0.0, 100.0, 100.0),
                },
                Operation::SetLayout {
                    node: content,
                    geometry: geometry(0.0, 0.0, 100.0, 300.0),
                },
                Operation::SetLayout {
                    node: target,
                    geometry: geometry(0.0, 150.0, 100.0, 40.0),
                },
                Operation::SetClip {
                    node: scroll,
                    clip: BoxClip {
                        horizontal: OverflowClip::Hidden,
                        vertical: OverflowClip::Hidden,
                    },
                },
                Operation::SetBoxPaint {
                    node: target,
                    paint: paint(PaintColor::Srgba {
                        red: 255,
                        green: 0,
                        blue: 0,
                        alpha: 1.0,
                    }),
                },
            ],
        ))
        .unwrap();

    assert_eq!(scene.hit_test([10.0, 60.0]), Some(content));
    assert!(scene.scroll_at([10.0, 60.0], [0.0, 120.0]));
    assert_eq!(scene.nodes[&scroll].scroll_offset, [0.0, 120.0]);
    assert_eq!(
        scene.take_events(),
        vec![DesktopProviderEvent {
            target: scroll,
            name: "scroll".to_owned(),
            detail: WhiskerValue::map([
                ("scrollLeft", WhiskerValue::Float(0.0)),
                ("scrollTop", WhiskerValue::Float(120.0)),
                ("scrollWidth", WhiskerValue::Float(100.0)),
                ("scrollHeight", WhiskerValue::Float(300.0)),
                ("viewportWidth", WhiskerValue::Float(98.0)),
                ("viewportHeight", WhiskerValue::Float(96.0)),
            ]),
        }]
    );
    assert_eq!(scene.hit_test([10.0, 60.0]), Some(target));
    assert!(scene.paint_commands().iter().any(|command| {
        matches!(
            command,
            PaintCommand::Box { rect, .. }
                if rect.x == 0.0 && rect.y == 30.0 && rect.width == 100.0
        )
    }));
}

#[test]
fn smooth_scroll_command_advances_on_host_frames() {
    let scroll = id(1);
    let content = id(2);
    let mut scene = scene(SurfaceId::new(1).unwrap());
    scene
        .present(&packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![
                Operation::CreateNode {
                    node: scroll,
                    element_type: element_type(whisker::SCROLL_VIEW_ELEMENT_NAME),
                },
                Operation::CreateNode {
                    node: content,
                    element_type: element_type(whisker::VIEW_ELEMENT_NAME),
                },
                Operation::InsertChild {
                    parent: scroll,
                    child: content,
                    index: 0,
                },
                Operation::SetLayout {
                    node: scroll,
                    geometry: geometry(0.0, 0.0, 100.0, 100.0),
                },
                Operation::SetLayout {
                    node: content,
                    geometry: geometry(0.0, 0.0, 100.0, 500.0),
                },
            ],
        ))
        .unwrap();

    scene
        .present(&packet(
            FrameMode::Delta,
            1,
            2,
            vec![Operation::InvokeCommand {
                node: scroll,
                command: whisker::SCROLL_TO_COMMAND,
                arguments: WhiskerValue::map([
                    ("offset", WhiskerValue::Float(240.0)),
                    ("smooth", WhiskerValue::Bool(true)),
                ]),
            }],
        ))
        .unwrap();

    assert_eq!(scene.nodes[&scroll].scroll_offset, [0.0, 0.0]);
    assert!(scene.has_active_scroll_animations());
    assert!(scene.advance_scroll_animations(100.0));
    let intermediate = scene.nodes[&scroll].scroll_offset[1];
    assert!(intermediate > 0.0 && intermediate < 240.0);
    assert!(!scene.advance_scroll_animations(1_000.0));
    assert_eq!(scene.nodes[&scroll].scroll_offset, [0.0, 240.0]);
}

#[test]
fn snap_stop_always_limits_one_wheel_sequence_to_the_adjacent_child() {
    let scroll = id(1);
    let first = id(2);
    let second = id(3);
    let third = id(4);
    let registrations = standard_element_registrations();
    let scroll_registration = registrations
        .iter()
        .find(|registration| registration.name == whisker::SCROLL_VIEW_ELEMENT_NAME)
        .unwrap();
    let orientation = scroll_registration
        .property_named("scroll-orientation")
        .unwrap()
        .property;
    let snap = scroll_registration
        .property_named("item-snap")
        .unwrap()
        .property;
    let snap_stop = scroll_registration
        .property_named("scroll-snap-stop")
        .unwrap()
        .property;
    let mut scene = scene(SurfaceId::new(1).unwrap());
    let mut operations = vec![
        Operation::CreateNode {
            node: scroll,
            element_type: scroll_registration.element_type,
        },
        Operation::SetEventMask {
            node: scroll,
            event_mask: 1,
        },
        Operation::SetProperty {
            node: scroll,
            property: orientation,
            value: WhiskerValue::String("horizontal".into()),
        },
        Operation::SetProperty {
            node: scroll,
            property: snap,
            value: WhiskerValue::map([
                ("factor", WhiskerValue::Float(0.0)),
                ("offset", WhiskerValue::Float(0.0)),
            ]),
        },
        Operation::SetProperty {
            node: scroll,
            property: snap_stop,
            value: WhiskerValue::String("always".into()),
        },
        Operation::SetLayout {
            node: scroll,
            geometry: geometry(0.0, 0.0, 320.0, 180.0),
        },
    ];
    for (node, x) in [(first, 0.0), (second, 296.0), (third, 592.0)] {
        operations.extend([
            Operation::CreateNode {
                node,
                element_type: element_type(whisker::VIEW_ELEMENT_NAME),
            },
            Operation::InsertChild {
                parent: scroll,
                child: node,
                index: node.get() as u32 - 2,
            },
            Operation::SetLayout {
                node,
                geometry: geometry(x, 0.0, 280.0, 180.0),
            },
        ]);
    }
    scene
        .present(&packet(FrameMode::Snapshot, 0, 1, operations))
        .unwrap();

    assert!(scene.scroll_at([10.0, 10.0], [0.0, 500.0]));
    assert_eq!(scene.nodes[&scroll].scroll_offset, [500.0, 0.0]);
    assert!(scene.settle_scroll_at([10.0, 10.0]));
    assert_eq!(scene.nodes[&scroll].scroll_offset, [296.0, 0.0]);
}

#[test]
fn accepted_projection_lowers_content_geometry_clip_and_opacity() {
    let root = id(1);
    let child = id(2);
    let box_type = element_type(whisker::VIEW_ELEMENT_NAME);
    let text_type = element_type(whisker::TEXT_ELEMENT_NAME);
    let mut scene = scene(SurfaceId::new(1).unwrap());
    let snapshot = packet(
        FrameMode::Snapshot,
        0,
        1,
        vec![
            Operation::CreateNode {
                node: root,
                element_type: box_type,
            },
            Operation::CreateNode {
                node: child,
                element_type: text_type,
            },
            Operation::InsertChild {
                parent: root,
                child,
                index: 0,
            },
            Operation::SetLayout {
                node: root,
                geometry: geometry(4.0, 5.0, 100.0, 80.0),
            },
            Operation::SetLayout {
                node: child,
                geometry: geometry(2.0, 3.0, 20.0, 10.0),
            },
            Operation::SetBoxPaint {
                node: root,
                paint: paint(PaintColor::Named("red".into())),
            },
            Operation::SetClip {
                node: root,
                clip: BoxClip {
                    horizontal: OverflowClip::Hidden,
                    vertical: OverflowClip::Visible,
                },
            },
            Operation::SetOpacity {
                node: root,
                opacity: 0.5,
            },
            Operation::SetOpacity {
                node: child,
                opacity: 0.5,
            },
            Operation::SetText {
                node: child,
                content: text(),
            },
        ],
    );
    assert_eq!(
        scene.present(&snapshot),
        Ok(ApplyResult::Accepted { revision: 1 })
    );
    let commands = scene.paint_commands();
    assert_eq!(commands.len(), 6);
    assert!(matches!(
        &commands[0],
        PaintCommand::BeginOpacityGroup { node, opacity }
            if *node == root && *opacity == 0.5
    ));
    assert!(matches!(
        &commands[1],
        PaintCommand::Box { rect, opacity, .. }
            if *rect == LayoutRect { x: 4.0, y: 5.0, width: 100.0, height: 80.0 }
                && *opacity == 1.0
    ));
    assert!(matches!(
        &commands[2],
        PaintCommand::BeginOpacityGroup { node, opacity }
            if *node == child && *opacity == 0.5
    ));
    assert!(matches!(
        &commands[3],
        PaintCommand::Text { rect, clip, opacity, .. }
            if *rect == LayoutRect { x: 7.0, y: 10.0, width: 18.0, height: 6.0 }
                && clip.left == Some(7.0)
                && clip.right == Some(25.0)
                && *opacity == 1.0
    ));
    assert!(matches!(
        &commands[4],
        PaintCommand::EndOpacityGroup { node } if *node == child
    ));
    assert!(matches!(
        &commands[5],
        PaintCommand::EndOpacityGroup { node } if *node == root
    ));
}

#[test]
fn cursor_hit_testing_respects_pointer_events_and_child_z_order() {
    let root = id(1);
    let ignored_child = id(2);
    let active_child = id(3);
    let element_type = element_type(whisker::VIEW_ELEMENT_NAME);
    let mut scene = scene(SurfaceId::new(1).unwrap());
    scene
        .present(&packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![
                Operation::CreateNode {
                    node: root,
                    element_type,
                },
                Operation::CreateNode {
                    node: ignored_child,
                    element_type,
                },
                Operation::CreateNode {
                    node: active_child,
                    element_type,
                },
                Operation::InsertChild {
                    parent: root,
                    child: ignored_child,
                    index: 0,
                },
                Operation::InsertChild {
                    parent: root,
                    child: active_child,
                    index: 1,
                },
                Operation::SetLayout {
                    node: root,
                    geometry: geometry(0.0, 0.0, 180.0, 90.0),
                },
                Operation::SetLayout {
                    node: ignored_child,
                    geometry: geometry(10.0, 10.0, 70.0, 70.0),
                },
                Operation::SetLayout {
                    node: active_child,
                    geometry: geometry(100.0, 10.0, 70.0, 70.0),
                },
                Operation::SetCursor {
                    node: root,
                    cursor: Cursor {
                        resources: Vec::new(),
                        fallback: whisker_protocol::CursorKeyword::Pointer,
                    },
                },
                Operation::SetCursor {
                    node: ignored_child,
                    cursor: Cursor {
                        resources: Vec::new(),
                        fallback: whisker_protocol::CursorKeyword::Text,
                    },
                },
                Operation::SetHitTest {
                    node: ignored_child,
                    behavior: HitTestBehavior::None,
                },
                Operation::SetCursor {
                    node: active_child,
                    cursor: Cursor {
                        resources: Vec::new(),
                        fallback: whisker_protocol::CursorKeyword::Grab,
                    },
                },
            ],
        ))
        .unwrap();

    assert_eq!(
        scene.cursor_at([20.0, 20.0]),
        Some(whisker_protocol::CursorKeyword::Pointer)
    );
    assert_eq!(
        scene.cursor_at([120.0, 20.0]),
        Some(whisker_protocol::CursorKeyword::Grab)
    );
    assert_eq!(scene.cursor_at([200.0, 20.0]), None);
}

#[test]
fn visible_descendant_paints_and_hit_tests_through_hidden_parent() {
    let root = id(1);
    let child = id(2);
    let element_type = element_type(whisker::VIEW_ELEMENT_NAME);
    let mut scene = scene(SurfaceId::new(1).unwrap());
    scene
        .present(&packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![
                Operation::CreateNode {
                    node: root,
                    element_type,
                },
                Operation::CreateNode {
                    node: child,
                    element_type,
                },
                Operation::InsertChild {
                    parent: root,
                    child,
                    index: 0,
                },
                Operation::SetLayout {
                    node: root,
                    geometry: geometry(10.0, 10.0, 80.0, 80.0),
                },
                Operation::SetLayout {
                    node: child,
                    geometry: geometry(10.0, 10.0, 30.0, 30.0),
                },
                Operation::SetBoxPaint {
                    node: root,
                    paint: paint(PaintColor::Named("red".into())),
                },
                Operation::SetBoxPaint {
                    node: child,
                    paint: paint(PaintColor::Named("green".into())),
                },
                Operation::SetVisibility {
                    node: root,
                    visibility: Visibility::Hidden,
                },
                Operation::SetVisibility {
                    node: child,
                    visibility: Visibility::Visible,
                },
                Operation::SetCursor {
                    node: root,
                    cursor: Cursor {
                        resources: Vec::new(),
                        fallback: whisker_protocol::CursorKeyword::Pointer,
                    },
                },
                Operation::SetCursor {
                    node: child,
                    cursor: Cursor {
                        resources: Vec::new(),
                        fallback: whisker_protocol::CursorKeyword::Text,
                    },
                },
            ],
        ))
        .unwrap();

    let commands = scene.paint_commands();
    assert_eq!(commands.len(), 1);
    assert!(matches!(&commands[0], PaintCommand::Box { rect, .. }
            if *rect == LayoutRect { x: 20.0, y: 20.0, width: 30.0, height: 30.0 }));
    assert_eq!(
        scene.cursor_at([25.0, 25.0]),
        Some(whisker_protocol::CursorKeyword::Text)
    );
    assert_eq!(scene.cursor_at([70.0, 70.0]), None);
}

#[test]
fn rejected_delta_does_not_partially_change_desktop_state() {
    let root = id(1);
    let element_type = element_type(whisker::VIEW_ELEMENT_NAME);
    let mut scene = scene(SurfaceId::new(1).unwrap());
    scene
        .present(&packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![
                Operation::CreateNode {
                    node: root,
                    element_type,
                },
                Operation::SetLayout {
                    node: root,
                    geometry: geometry(0.0, 0.0, 10.0, 10.0),
                },
                Operation::SetBoxPaint {
                    node: root,
                    paint: paint(PaintColor::Named("blue".into())),
                },
            ],
        ))
        .unwrap();
    let before_len = scene.paint_commands().len();
    let invalid = LayoutGeometry {
        border_box: LayoutRect {
            width: f32::NAN,
            ..LayoutRect::default()
        },
        ..LayoutGeometry::default()
    };
    assert_eq!(
        scene.present(&packet(
            FrameMode::Delta,
            1,
            2,
            vec![Operation::SetLayout {
                node: root,
                geometry: invalid
            }],
        )),
        Err(DesktopPresentError::Protocol(
            ValidationError::NonFiniteNumber
        ))
    );
    assert_eq!(scene.paint_commands().len(), before_len);
    assert!(
        matches!(scene.paint_commands()[0], PaintCommand::Box { rect, .. } if rect.width == 10.0)
    );
}

#[test]
fn unsupported_visual_payload_is_rejected_before_desktop_commit() {
    let root = id(1);
    let mut scene = scene(SurfaceId::new(1).unwrap());
    scene
        .present(&packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![Operation::CreateNode {
                node: root,
                element_type: element_type(whisker::VIEW_ELEMENT_NAME),
            }],
        ))
        .unwrap();

    assert_eq!(
        scene.present(&packet(
            FrameMode::Delta,
            1,
            2,
            vec![Operation::SetVisualEffects {
                node: root,
                effects: whisker_protocol::VisualEffects {
                    blend_mode: whisker_protocol::BlendMode::Multiply,
                    ..Default::default()
                },
            }],
        )),
        Err(DesktopPresentError::Unsupported("visual-effects"))
    );
    assert_eq!(scene.validation.revision(), 1);
}

#[test]
fn unknown_element_type_rejects_the_whole_frame() {
    let root = id(1);
    let mut scene = scene(SurfaceId::new(1).unwrap());
    let unknown = ElementTypeId::new(900).unwrap();

    assert_eq!(
        scene.present(&packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![Operation::CreateNode {
                node: root,
                element_type: unknown,
            }],
        )),
        Err(DesktopPresentError::Element(
            DesktopElementError::UnknownElementType {
                element_type: unknown,
            }
        ))
    );

    assert_eq!(
        scene.present(&packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![Operation::CreateNode {
                node: root,
                element_type: element_type(whisker::VIEW_ELEMENT_NAME),
            }],
        )),
        Ok(ApplyResult::Accepted { revision: 1 })
    );
}

#[test]
fn text_content_operation_is_dispatched_by_registered_element_type() {
    let root = id(1);
    let mut scene = scene(SurfaceId::new(1).unwrap());

    assert_eq!(
        scene.present(&packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![
                Operation::CreateNode {
                    node: root,
                    element_type: element_type(whisker::VIEW_ELEMENT_NAME),
                },
                Operation::SetText {
                    node: root,
                    content: text(),
                },
            ],
        )),
        Err(DesktopPresentError::Element(
            DesktopElementError::UnexpectedText { node: root }
        ))
    );

    assert_eq!(
        scene.present(&packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![
                Operation::CreateNode {
                    node: root,
                    element_type: element_type(whisker::TEXT_ELEMENT_NAME),
                },
                Operation::SetText {
                    node: root,
                    content: text(),
                },
                Operation::SetBoxPaint {
                    node: root,
                    paint: paint(PaintColor::Named("green".into())),
                },
            ],
        )),
        Ok(ApplyResult::Accepted { revision: 1 })
    );
    assert!(matches!(
        scene.paint_commands().as_slice(),
        [PaintCommand::Box { .. }, PaintCommand::Text { node, .. }] if *node == root
    ));
}

#[test]
fn validation_does_not_instantiate_module_elements() {
    let element_type = ElementTypeId::new(22).unwrap();
    let node = id(1);
    let constructions = Arc::new(AtomicUsize::new(0));
    let observed_constructions = Arc::clone(&constructions);
    let mut registrations = standard_element_registrations();
    registrations.push(ElementRegistration {
        element_type,
        name: "whisker.test/PlainText".into(),
        child_policy: whisker_protocol::ChildPolicy::PlainText,
        measurement: ElementMeasurement::None,
        text_style: false,
        properties: vec![],
        events: vec![],
        commands: vec![],
    });
    let mut factories = built_in_element_factories();
    factories.push(
        DesktopViewDefinition::new("whisker.test/PlainText", move |_| {
            observed_constructions.fetch_add(1, Ordering::Relaxed);
        })
        .plain_text()
        .into_desktop_factory(),
    );
    let mut scene = DesktopScene::new(
        SurfaceId::new(1).unwrap(),
        DesktopElementRegistry::bind(&registrations, &factories).unwrap(),
    );

    scene
        .present(&packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![
                Operation::CreateNode { node, element_type },
                Operation::SetText {
                    node,
                    content: text(),
                },
            ],
        ))
        .unwrap();

    assert_eq!(constructions.load(Ordering::Relaxed), 1);
}

#[test]
fn leaf_element_rejects_scene_children_without_partial_commit() {
    let parent = id(1);
    let child = id(2);
    let mut scene = scene(SurfaceId::new(1).unwrap());

    assert_eq!(
        scene.present(&packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![
                Operation::CreateNode {
                    node: parent,
                    element_type: element_type(whisker::TEXT_ELEMENT_NAME),
                },
                Operation::CreateNode {
                    node: child,
                    element_type: element_type(whisker::VIEW_ELEMENT_NAME),
                },
                Operation::InsertChild {
                    parent,
                    child,
                    index: 0,
                },
            ],
        )),
        Err(DesktopPresentError::Element(
            DesktopElementError::ChildrenNotAllowed { parent }
        ))
    );

    assert_eq!(
        scene.present(&packet(
            FrameMode::Snapshot,
            0,
            1,
            vec![Operation::CreateNode {
                node: parent,
                element_type: element_type(whisker::TEXT_ELEMENT_NAME),
            }],
        )),
        Ok(ApplyResult::Accepted { revision: 1 })
    );
}
