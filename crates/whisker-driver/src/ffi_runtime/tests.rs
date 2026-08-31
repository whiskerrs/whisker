use super::*;
use std::cell::{Cell, RefCell};
use whisker_engine::whisker_protocol::{
    BackgroundLayer, FontFeature, FontOpticalSizing, FontTag, FontVariation, FrameHeader,
    GradientStop, PaintCoordinate, PaintPosition, ProtocolVersion, RenderCapability, TextContent,
    TextMeasurePayload, TextMeasureStyle, TextPaint, TextShadow, TextStyleSnapshot,
};

#[test]
fn mobile_pointer_input_decodes_without_host_specific_types() {
    let event = mobile_pointer_event(42.5, 0, 7, 1, 24.0, 16.0, 1, -1).unwrap();
    assert_eq!(event.kind, InputEventKind::PointerDown);
    assert_eq!(event.target, None);
    let pointer = event.pointer.unwrap();
    assert_eq!(pointer.id.get(), 7);
    assert_eq!(pointer.kind, PointerKind::Touch);
    assert_eq!(pointer.position, InputPoint { x: 24.0, y: 16.0 });
    assert_eq!(pointer.buttons, 1);
    assert_eq!(pointer.changed_button, -1);

    assert!(mobile_pointer_event(0.0, 4, 1, 0, 0.0, 0.0, 0, -1).is_none());
    assert!(mobile_pointer_event(0.0, 0, 0, 0, 0.0, 0.0, 0, -1).is_none());
    assert!(mobile_pointer_event(0.0, 0, 1, 4, 0.0, 0.0, 0, -1).is_none());
    assert!(mobile_pointer_event(f64::NAN, 0, 1, 0, 0.0, 0.0, 0, -1).is_none());
    assert!(mobile_pointer_event(0.0, 0, 1, 0, f32::NAN, 0.0, 0, -1).is_none());
}

fn linear_background(name: &str) -> BackgroundLayer {
    BackgroundLayer {
        image: PaintImage::LinearGradient {
            angle_degrees: 180.0,
            repeating: false,
            stops: [0.0, 1.0]
                .into_iter()
                .map(|fraction| GradientStop {
                    color: PaintColor::Named(name.into()),
                    position: Some(PaintCoordinate {
                        length: 0.0,
                        fraction,
                    }),
                })
                .collect(),
        },
        position: PaintPosition::default(),
        size: BackgroundSize::Auto,
        repeat_x: ImageRepeat::Repeat,
        repeat_y: ImageRepeat::Repeat,
        origin: PaintBox::Padding,
        clip: PaintBox::Border,
        attachment: BackgroundAttachment::Scroll,
        blend_mode: BlendMode::Normal,
    }
}

fn text_with_shadow(shadows: Vec<TextShadow>) -> TextContent {
    TextContent {
        payload: TextMeasurePayload {
            text: "shadow".into(),
            style: TextMeasureStyle::default(),
            locale: None,
            direction: whisker_engine::whisker_protocol::MeasureTextDirection::Auto,
            alignment: Default::default(),
            indent: Default::default(),
            wrap: MeasureTextWrap::Wrap,
            word_break: Default::default(),
            max_lines: None,
            overflow: MeasureTextOverflow::Clip,
        },
        paint: TextPaint {
            foreground: PaintColor::Named("black".into()),
            shadows,
            ..TextPaint::default()
        },
        prepared_content: None,
    }
}

#[test]
fn viewport_rejects_invalid_host_metrics() {
    assert!(Viewport::new(320.0, 640.0, 2.0).is_some());
    assert!(Viewport::new(-1.0, 640.0, 2.0).is_none());
    assert!(Viewport::new(320.0, 640.0, 0.0).is_none());
}

#[test]
fn mobile_host_profile_negotiates_protocol_and_support_masks() {
    let raw = MobileHostCapabilities {
        abi_major: MOBILE_ABI_MAJOR,
        abi_minor: MOBILE_ABI_MINOR,
        protocol_major: 1,
        protocol_minor: 3,
        native: CAPABILITY_VISUAL_EFFECTS,
        emulated: CAPABILITY_BACKDROP_BLUR,
    };

    let profile = decode_host_capabilities(&raw).unwrap();

    assert_eq!(profile.protocol(), ProtocolVersion { major: 1, minor: 3 });
    assert_eq!(
        profile.support(RenderCapability::VisualEffects),
        whisker_engine::whisker_protocol::CapabilitySupport::Native
    );
    assert_eq!(
        profile.support(RenderCapability::BackdropBlur),
        whisker_engine::whisker_protocol::CapabilitySupport::Emulated
    );
}

#[test]
fn mobile_capability_constants_match_the_semantic_protocol() {
    assert_eq!(FRAME_PROTOCOL_MAJOR, ProtocolVersion::CURRENT.major);
    assert_eq!(FRAME_PROTOCOL_MINOR, ProtocolVersion::CURRENT.minor);
    let expected = [
        (
            RenderCapability::EllipticalBorderRadius,
            CAPABILITY_ELLIPTICAL_BORDER_RADIUS,
        ),
        (
            RenderCapability::BackgroundLayers,
            CAPABILITY_BACKGROUND_LAYERS,
        ),
        (RenderCapability::VisualEffects, CAPABILITY_VISUAL_EFFECTS),
        (RenderCapability::TextEffects, CAPABILITY_TEXT_EFFECTS),
        (RenderCapability::TextTypography, CAPABILITY_TEXT_TYPOGRAPHY),
        (RenderCapability::Cursor, CAPABILITY_CURSOR),
        (
            RenderCapability::ResourceLifecycle,
            CAPABILITY_RESOURCE_LIFECYCLE,
        ),
        (
            RenderCapability::LinearGradients,
            CAPABILITY_LINEAR_GRADIENTS,
        ),
        (
            RenderCapability::RadialGradients,
            CAPABILITY_RADIAL_GRADIENTS,
        ),
        (RenderCapability::ConicGradients, CAPABILITY_CONIC_GRADIENTS),
        (
            RenderCapability::BackgroundGeometry,
            CAPABILITY_BACKGROUND_GEOMETRY,
        ),
        (
            RenderCapability::BackgroundLayerStacking,
            CAPABILITY_BACKGROUND_LAYER_STACKING,
        ),
        (
            RenderCapability::BackgroundImageResources,
            CAPABILITY_BACKGROUND_IMAGE_RESOURCES,
        ),
        (RenderCapability::BackdropBlur, CAPABILITY_BACKDROP_BLUR),
    ];

    assert_eq!(expected.len(), RenderCapability::ALL.len());
    for (capability, wire) in expected {
        assert_eq!(capability.mask(), wire, "{}", capability.as_str());
    }
}

extern "C" fn count_present_calls(
    data: *mut c_void,
    _frame: *const MobileFrame,
    _response: *mut MobileApplyResponse,
) -> bool {
    let calls = unsafe { &*data.cast::<Cell<usize>>() };
    calls.set(calls.get() + 1);
    true
}

#[test]
fn unsupported_mobile_capability_is_rejected_before_crossing_the_host_seam() {
    let calls = Cell::new(0_usize);
    let capabilities = RenderCapabilities::new(
        ProtocolVersion::CURRENT,
        [whisker_engine::whisker_protocol::CapabilityEntry {
            capability: RenderCapability::VisualEffects,
            support: whisker_engine::whisker_protocol::CapabilitySupport::Native,
        }],
    )
    .unwrap();
    let mut sink = MobileFrameSink {
        present: count_present_calls,
        data: (&calls as *const Cell<usize>).cast_mut().cast(),
        capabilities,
    };
    let effects = VisualEffects {
        backdrop_blur: Some(12.0),
        ..VisualEffects::default()
    };
    let packet = FramePacket {
        header: FrameHeader {
            version: ProtocolVersion::CURRENT,
            surface: SurfaceId::new(1).unwrap(),
            scene_epoch: 1,
            frame_id: 7,
            base_revision: 0,
            target_revision: 1,
            viewport_epoch: 1,
            mode: FrameMode::Snapshot,
        },
        operations: vec![Operation::SetVisualEffects {
            node: NodeId::new(1).unwrap(),
            effects,
        }],
    };

    let error = sink.present(&packet).unwrap_err();

    assert_eq!(calls.get(), 0);
    assert_eq!(
        error.to_string(),
        "mobile Host does not advertise capability backdrop-blur required by frame 7"
    );
}
#[test]
fn hsla_is_lowered_without_host_string_parsing() {
    assert_eq!(hsl_to_rgb(0.0, 1.0, 0.5), (255, 0, 0));
    assert_eq!(hsl_to_rgb(120.0, 1.0, 0.5), (0, 255, 0));
}

#[derive(Debug, PartialEq, Eq)]
struct CapturedResourceCommand {
    command: u32,
    kind: u32,
    source: u32,
    resource: u64,
    generation: u64,
    identifier: Vec<u8>,
    data: Vec<u8>,
}

extern "C" fn capture_resource_command(
    data: *mut c_void,
    command: *const MobileResourceCommand,
) -> bool {
    let commands = unsafe { &*(data.cast::<RefCell<Vec<CapturedResourceCommand>>>()) };
    let command = unsafe { &*command };
    let identifier = if command.identifier.len == 0 {
        Vec::new()
    } else {
        unsafe {
            std::slice::from_raw_parts(command.identifier.ptr.cast::<u8>(), command.identifier.len)
                .to_vec()
        }
    };
    let bytes = if command.data.len == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(command.data.ptr, command.data.len).to_vec() }
    };
    commands.borrow_mut().push(CapturedResourceCommand {
        command: command.command,
        kind: command.kind,
        source: command.source,
        resource: command.resource,
        generation: command.generation,
        identifier,
        data: bytes,
    });
    true
}

#[test]
fn mobile_resource_commands_are_typed_and_borrowed_only_for_the_callback() {
    let captured = RefCell::new(Vec::new());
    let host = MobileResourceHost {
        callback: capture_resource_command,
        data: (&captured as *const RefCell<Vec<CapturedResourceCommand>>)
            .cast_mut()
            .cast(),
    };
    let resource = ResourceId::new(42).unwrap();
    host.send(&ResourceCommand::Load(
        whisker_engine::whisker_protocol::ResourceRequest {
            resource,
            generation: 3,
            kind: ResourceKind::RasterImage,
            source: ResourceSource::Bytes {
                media_type: "image/png".into(),
                data: vec![1, 2, 3],
            },
        },
    ));
    host.send(&ResourceCommand::Release {
        resource,
        generation: 3,
    });

    assert_eq!(
        captured.into_inner(),
        vec![
            CapturedResourceCommand {
                command: RESOURCE_COMMAND_LOAD,
                kind: RESOURCE_RASTER_IMAGE,
                source: RESOURCE_SOURCE_BYTES,
                resource: 42,
                generation: 3,
                identifier: b"image/png".to_vec(),
                data: vec![1, 2, 3],
            },
            CapturedResourceCommand {
                command: RESOURCE_COMMAND_RELEASE,
                kind: 0,
                source: RESOURCE_SOURCE_NONE,
                resource: 42,
                generation: 3,
                identifier: Vec::new(),
                data: Vec::new(),
            },
        ]
    );
}

#[test]
fn mobile_resource_events_decode_without_json() {
    let diagnostic = b"offline";
    let failed = MobileResourceEvent {
        status: RESOURCE_EVENT_FAILED,
        failure_code: RESOURCE_FAILURE_NETWORK,
        resource: 9,
        generation: 2,
        width: 0.0,
        height: 0.0,
        scale: 0.0,
        dimensions_mask: 0,
        diagnostic: WhiskerStringRef {
            ptr: diagnostic.as_ptr().cast(),
            len: diagnostic.len(),
        },
    };
    assert_eq!(
        decode_resource_event(&failed),
        Some(ResourceEvent::Failed {
            resource: ResourceId::new(9).unwrap(),
            generation: 2,
            code: ResourceFailureCode::Network,
            diagnostic: Some("offline".into()),
        })
    );

    let ready = MobileResourceEvent {
        status: RESOURCE_EVENT_READY,
        failure_code: RESOURCE_FAILURE_NONE,
        resource: 9,
        generation: 2,
        width: 20.0,
        height: 10.0,
        scale: 2.0,
        dimensions_mask: RESOURCE_DIMENSIONS_PRESENT,
        diagnostic: WhiskerStringRef {
            ptr: std::ptr::null(),
            len: 0,
        },
    };
    assert_eq!(
        decode_resource_event(&ready),
        Some(ResourceEvent::Ready {
            resource: ResourceId::new(9).unwrap(),
            generation: 2,
            dimensions: Some(ResourceDimensions {
                width: 20.0,
                height: 10.0,
                scale: 2.0,
            }),
        })
    );
}

#[test]
fn mobile_box_paint_preserves_elliptical_radii() {
    let mut paint = whisker_engine::whisker_protocol::BoxPaint::default();
    paint.border_radii.top_left = whisker_engine::whisker_protocol::PaintCornerRadius {
        horizontal: PaintLengthPercentage {
            length: 40.0,
            fraction: 0.0,
        },
        vertical: PaintLengthPercentage {
            length: 10.0,
            fraction: 0.0,
        },
    };
    let raw = mobile_paint(&paint, &mut Vec::new());
    assert_eq!(raw.radii_horizontal[0].length, 40.0);
    assert_eq!(raw.radii_vertical[0].length, 10.0);
}

#[test]
fn mobile_frame_encodes_keyword_cursor_and_rejects_resource_cursor() {
    let header = FrameHeader {
        version: ProtocolVersion::CURRENT,
        surface: SurfaceId::new(1).unwrap(),
        scene_epoch: 1,
        frame_id: 1,
        base_revision: 0,
        target_revision: 1,
        viewport_epoch: 1,
        mode: FrameMode::Snapshot,
    };
    let node = NodeId::new(1).unwrap();
    let packet = FramePacket {
        header,
        operations: vec![Operation::SetCursor {
            node,
            cursor: whisker_engine::whisker_protocol::Cursor {
                resources: Vec::new(),
                fallback: whisker_engine::whisker_protocol::CursorKeyword::ZoomOut,
            },
        }],
    };
    let frame = MobileFrameOwned::new(&packet).unwrap();
    assert_eq!(frame._operations[0].tag, OP_CURSOR);
    assert_eq!(frame._operations[0].node, node.get());
    assert_eq!(frame._operations[0].integer, 34);

    let packet = FramePacket {
        header,
        operations: vec![Operation::SetCursor {
            node,
            cursor: whisker_engine::whisker_protocol::Cursor {
                resources: vec![whisker_engine::whisker_protocol::CursorResource {
                    resource: ResourceId::new(1).unwrap(),
                    hotspot: Some((1, 2)),
                }],
                fallback: whisker_engine::whisker_protocol::CursorKeyword::Pointer,
            },
        }],
    };
    assert!(MobileFrameOwned::new(&packet).is_err());
}

#[test]
fn mobile_frame_encodes_one_text_shadow_without_string_protocols() {
    let shadow = TextShadow {
        offset_x: 3.0,
        offset_y: -2.0,
        blur_radius: 5.0,
        color: PaintColor::Srgba {
            red: 10,
            green: 20,
            blue: 30,
            alpha: 0.4,
        },
    };
    let mut content = text_with_shadow(vec![shadow]);
    content.payload.wrap = MeasureTextWrap::NoWrap;
    content.payload.word_break = MeasureTextWordBreak::KeepAll;
    content.payload.max_lines = Some(2);
    content.payload.overflow = MeasureTextOverflow::Ellipsis;
    content.payload.direction = MeasureTextDirection::RightToLeft;
    content.payload.alignment = whisker_engine::whisker_protocol::MeasureTextAlignment::End;
    content.payload.style.features = vec![FontFeature {
        tag: FontTag::new(*b"kern").unwrap(),
        value: 0,
    }];
    content.payload.style.variations = vec![FontVariation {
        tag: FontTag::new(*b"wght").unwrap(),
        value: 650.0,
    }];
    content.payload.style.font_families = vec![
        MeasureFontFamily::Named("Whisker Fixture Sans".into()),
        MeasureFontFamily::System,
    ];
    content.payload.style.font_style = MeasureFontStyle::Italic;
    content.payload.style.line_height = MeasureLineHeight::LogicalPixels(28.0);
    content.payload.style.letter_spacing = 1.5;
    content.payload.style.optical_sizing = FontOpticalSizing::Auto;
    let packet = FramePacket {
        header: FrameHeader {
            version: ProtocolVersion::CURRENT,
            surface: SurfaceId::new(1).unwrap(),
            scene_epoch: 1,
            frame_id: 1,
            base_revision: 0,
            target_revision: 1,
            viewport_epoch: 1,
            mode: FrameMode::Snapshot,
        },
        operations: vec![Operation::SetText {
            node: NodeId::new(1).unwrap(),
            content,
        }],
    };

    let frame = MobileFrameOwned::new(&packet).unwrap();
    let operation = &frame._operations[0];
    assert_eq!(operation.tag, OP_TEXT);
    let text = unsafe { &*operation.payload.cast::<MobileText>() };
    assert_eq!(text.font_family_count, 2);
    let families =
        unsafe { std::slice::from_raw_parts(text.font_families, text.font_family_count) };
    let family = |value: WhiskerStringRef| unsafe {
        String::from_utf8(std::slice::from_raw_parts(value.ptr.cast::<u8>(), value.len).to_vec())
            .unwrap()
    };
    assert_eq!(family(families[0]), "Whisker Fixture Sans");
    assert_eq!(family(families[1]), "system");
    assert_eq!(text.font_style, 1);
    assert_eq!(text.line_height, 28.0);
    assert_eq!(text.letter_spacing, 1.5);
    assert_eq!(text.shadow_flags, 1);
    assert_eq!(text.shadow_offset_x, 3.0);
    assert_eq!(text.shadow_offset_y, -2.0);
    assert_eq!(text.shadow_blur_radius, 5.0);
    assert_eq!(text.shadow_color.kind, 1);
    assert_eq!(text.shadow_color.red, 10);
    assert_eq!(text.shadow_color.green, 20);
    assert_eq!(text.shadow_color.blue, 30);
    assert_eq!(text.shadow_color.alpha, 0.4);
    assert_eq!(text.wrap, 0);
    assert_eq!(text.word_break, 2);
    assert_eq!(text.max_lines, 2);
    assert_eq!(text.overflow, 1);
    assert_eq!(text.font_feature_count, 1);
    let feature = unsafe { &*text.font_features };
    assert_eq!(feature.tag, *b"kern");
    assert_eq!(feature.value, 0);
    assert_eq!(text.font_variation_count, 1);
    let variation = unsafe { &*text.font_variations };
    assert_eq!(variation.tag, *b"wght");
    assert_eq!(variation.value, 650.0);
    assert_eq!(text.font_optical_sizing, 0);
    assert_eq!(text.alignment, 1);
    assert_eq!(text.direction, 2);
}

#[test]
fn mobile_frame_encodes_resolved_text_style_as_its_own_operation() {
    let content = text_with_shadow(Vec::new());
    let style = TextStyleSnapshot {
        style: content.payload.style,
        locale: content.payload.locale,
        direction: MeasureTextDirection::LeftToRight,
        alignment: whisker_engine::whisker_protocol::MeasureTextAlignment::Center,
        paint: content.paint,
    };
    let packet = FramePacket {
        header: FrameHeader {
            version: ProtocolVersion::CURRENT,
            surface: SurfaceId::new(1).unwrap(),
            scene_epoch: 1,
            frame_id: 1,
            base_revision: 0,
            target_revision: 1,
            viewport_epoch: 1,
            mode: FrameMode::Snapshot,
        },
        operations: vec![Operation::SetTextStyle {
            node: NodeId::new(1).unwrap(),
            style,
        }],
    };

    let frame = MobileFrameOwned::new(&packet).unwrap();
    let operation = &frame._operations[0];
    assert_eq!(operation.tag, OP_TEXT_STYLE);
    let text = unsafe { &*operation.payload.cast::<MobileText>() };
    assert_eq!(text.text.len, 0);
    assert_eq!(text.direction, 1);
    assert_eq!(text.alignment, 4);
}

#[test]
fn mobile_measure_batch_owns_ordered_font_fallbacks() {
    let mut payload = text_with_shadow(Vec::new()).payload;
    payload.style.font_families = vec![
        MeasureFontFamily::Named("Whisker Fixture Sans".into()),
        MeasureFontFamily::System,
    ];
    payload.direction = MeasureTextDirection::LeftToRight;
    payload.alignment = whisker_engine::whisker_protocol::MeasureTextAlignment::Center;
    let request = MeasurementRequest {
        key: whisker_engine::whisker_protocol::MeasurementKey::new(1).unwrap(),
        node: NodeId::new(1).unwrap(),
        element_type: whisker_engine::whisker_protocol::ElementTypeId::new(2).unwrap(),
        environment_epoch: 3,
        constraints: whisker_engine::whisker_protocol::MeasureConstraints {
            known_dimensions: [None, None],
            available_space: [AvailableSpace::Definite(240.0), AvailableSpace::MaxContent],
        },
        payload: MeasurementPayload::Text(payload),
    };

    let batch = MobileMeasureBatch::new(&[request]);
    let raw = &batch.requests[0];
    assert_eq!(raw.font_family_count, 2);
    let families = unsafe { std::slice::from_raw_parts(raw.font_families, raw.font_family_count) };
    let decode = |value: WhiskerStringRef| unsafe {
        String::from_utf8(std::slice::from_raw_parts(value.ptr.cast::<u8>(), value.len).to_vec())
            .unwrap()
    };
    assert_eq!(decode(families[0]), "Whisker Fixture Sans");
    assert_eq!(decode(families[1]), "system");
    assert_eq!(raw.direction, 1);
    assert_eq!(raw.alignment, 4);
}

#[test]
fn mobile_frame_rejects_multiple_text_shadows() {
    let shadow = TextShadow {
        offset_x: 0.0,
        offset_y: 1.0,
        blur_radius: 0.0,
        color: PaintColor::default(),
    };
    let packet = FramePacket {
        header: FrameHeader {
            version: ProtocolVersion::CURRENT,
            surface: SurfaceId::new(1).unwrap(),
            scene_epoch: 1,
            frame_id: 1,
            base_revision: 0,
            target_revision: 1,
            viewport_epoch: 1,
            mode: FrameMode::Snapshot,
        },
        operations: vec![Operation::SetText {
            node: NodeId::new(1).unwrap(),
            content: text_with_shadow(vec![shadow.clone(), shadow]),
        }],
    };

    assert!(MobileFrameOwned::new(&packet).is_err());
}

#[test]
fn mobile_frame_exposes_background_layers_as_one_contiguous_slice() {
    let node = NodeId::new(1).unwrap();
    let resource = whisker_engine::whisker_protocol::ResourceId::new(u64::MAX - 1).unwrap();
    let mut resource_background = linear_background("transparent");
    resource_background.image = PaintImage::Resource(resource);
    let packet = FramePacket {
        header: FrameHeader {
            version: ProtocolVersion::CURRENT,
            surface: SurfaceId::new(1).unwrap(),
            scene_epoch: 1,
            frame_id: 1,
            base_revision: 0,
            target_revision: 1,
            viewport_epoch: 1,
            mode: FrameMode::Snapshot,
        },
        operations: vec![Operation::SetBackgroundLayers {
            node,
            layers: vec![
                linear_background("red"),
                linear_background("blue"),
                resource_background,
            ],
        }],
    };
    let frame = MobileFrameOwned::new(&packet).unwrap();
    let operation = &frame._operations[0];
    assert_eq!(operation.tag, OP_BACKGROUND_LAYERS);
    assert_eq!(operation.payload_count, 3);
    let layers = unsafe {
        std::slice::from_raw_parts(
            operation.payload.cast::<MobileBackgroundLayer>(),
            operation.payload_count,
        )
    };
    assert_eq!(layers[0].image.kind, BACKGROUND_LINEAR);
    assert_eq!(layers[1].image.kind, BACKGROUND_LINEAR);
    assert_eq!(layers[2].image.kind, BACKGROUND_RESOURCE);
    assert_ne!(layers[0].image.payload, layers[1].image.payload);
    assert_eq!(layers[2].image.payload_count, 1);
    assert_eq!(
        unsafe { *layers[2].image.payload.cast::<u64>() },
        resource.get()
    );
}

#[test]
fn mobile_frame_exposes_box_shadows_as_one_contiguous_slice() {
    let node = NodeId::new(1).unwrap();
    let packet = FramePacket {
        header: FrameHeader {
            version: ProtocolVersion::CURRENT,
            surface: SurfaceId::new(1).unwrap(),
            scene_epoch: 1,
            frame_id: 1,
            base_revision: 0,
            target_revision: 1,
            viewport_epoch: 1,
            mode: FrameMode::Snapshot,
        },
        operations: vec![
            Operation::SetVisualEffects {
                node,
                effects: VisualEffects {
                    box_shadows: vec![
                        whisker_engine::whisker_protocol::BoxShadow {
                            offset_x: 4.0,
                            offset_y: 5.0,
                            blur_radius: 0.0,
                            spread_radius: 2.0,
                            color: PaintColor::Named("black".into()),
                            inset: false,
                        },
                        whisker_engine::whisker_protocol::BoxShadow {
                            offset_x: -1.0,
                            offset_y: 3.0,
                            blur_radius: 6.0,
                            spread_radius: -2.0,
                            color: PaintColor::Srgba {
                                red: 10,
                                green: 20,
                                blue: 30,
                                alpha: 0.5,
                            },
                            inset: true,
                        },
                    ],
                    backdrop_blur: Some(7.0),
                    image_rendering: ImageRendering::Pixelated,
                    ..Default::default()
                },
            },
            Operation::SetVisualEffects {
                node,
                effects: VisualEffects::default(),
            },
        ],
    };

    let frame = MobileFrameOwned::new(&packet).unwrap();
    let operation = &frame._operations[0];
    assert_eq!(operation.tag, OP_BOX_SHADOWS);
    assert_eq!(operation.payload_count, 2);
    let shadows = unsafe {
        std::slice::from_raw_parts(
            operation.payload.cast::<MobileBoxShadow>(),
            operation.payload_count,
        )
    };
    assert_eq!(shadows[0].offset_x, 4.0);
    assert_eq!(shadows[0].spread_radius, 2.0);
    assert_eq!(shadows[0].color.kind, 0);
    assert_eq!(shadows[1].blur_radius, 6.0);
    assert_eq!(shadows[1].inset, 1);
    assert_eq!(shadows[1].color.kind, 1);
    assert_eq!(shadows[1].color.alpha, 0.5);
    assert_eq!(frame._operations[1].tag, OP_CLIP_PATH);
    assert_eq!(frame._operations[1].payload_count, 0);
    assert!(frame._operations[1].payload.is_null());
    assert_eq!(frame._operations[2].tag, OP_BACKDROP_BLUR);
    assert_eq!(frame._operations[2].scalar, 7.0);
    assert_eq!(frame._operations[3].tag, OP_IMAGE_RENDERING);
    assert_eq!(frame._operations[3].integer, IMAGE_RENDERING_PIXELATED);
    assert_eq!(frame._operations[4].tag, OP_BOX_SHADOWS);
    assert_eq!(frame._operations[4].payload_count, 0);
    assert!(frame._operations[4].payload.is_null());
    assert_eq!(frame._operations[5].tag, OP_CLIP_PATH);
    assert_eq!(frame._operations[5].payload_count, 0);
    assert!(frame._operations[5].payload.is_null());
    assert_eq!(frame._operations[6].tag, OP_BACKDROP_BLUR);
    assert_eq!(frame._operations[6].scalar, 0.0);
    assert_eq!(frame._operations[7].tag, OP_IMAGE_RENDERING);
    assert_eq!(frame._operations[7].integer, IMAGE_RENDERING_AUTO);
}

#[test]
fn mobile_frame_exposes_rounded_inset_clip_path_as_typed_payload() {
    use whisker_engine::whisker_protocol::{PaintCornerRadius, PaintCorners, PaintEdges};

    let node = NodeId::new(1).unwrap();
    let coordinate = PaintCoordinate {
        length: 2.0,
        fraction: 0.1,
    };
    let radius = PaintCornerRadius::circular(PaintLengthPercentage {
        length: 4.0,
        fraction: 0.2,
    });
    let packet = FramePacket {
        header: FrameHeader {
            version: ProtocolVersion::CURRENT,
            surface: SurfaceId::new(1).unwrap(),
            scene_epoch: 1,
            frame_id: 1,
            base_revision: 0,
            target_revision: 1,
            viewport_epoch: 1,
            mode: FrameMode::Snapshot,
        },
        operations: vec![Operation::SetVisualEffects {
            node,
            effects: VisualEffects {
                clip_path: Some((
                    PaintBox::Padding,
                    ClipShape::Inset {
                        edges: PaintEdges {
                            top: coordinate,
                            right: coordinate,
                            bottom: coordinate,
                            left: coordinate,
                        },
                        radii: PaintCorners {
                            top_left: radius,
                            top_right: radius,
                            bottom_right: radius,
                            bottom_left: radius,
                        },
                    },
                )),
                ..Default::default()
            },
        }],
    };

    let frame = MobileFrameOwned::new(&packet).unwrap();
    assert_eq!(frame._operations.len(), 4);
    assert_eq!(frame._operations[0].tag, OP_BOX_SHADOWS);
    let operation = &frame._operations[1];
    assert_eq!(operation.tag, OP_CLIP_PATH);
    assert_eq!(operation.payload_count, 1);
    let clip = unsafe { &*operation.payload.cast::<MobileClipPath>() };
    assert_eq!(clip.reference_box, BACKGROUND_BOX_PADDING);
    assert_eq!(clip.shape_kind, CLIP_SHAPE_INSET);
    assert_eq!(clip.payload_count, 1);
    let inset = unsafe { &*clip.payload.cast::<MobileClipInset>() };
    assert_eq!(inset.edges[0].length, 2.0);
    assert_eq!(inset.edges[0].fraction, 0.1);
    assert_eq!(inset.radii_horizontal[0].length, 4.0);
    assert_eq!(inset.radii_vertical[0].fraction, 0.2);
    assert_eq!(frame._operations[2].tag, OP_BACKDROP_BLUR);
    assert_eq!(frame._operations[3].tag, OP_IMAGE_RENDERING);
}

#[test]
fn mobile_frame_exposes_path_commands_and_fill_rule_as_typed_payload() {
    let position = |x, y| PaintPosition {
        x: PaintCoordinate {
            length: x,
            fraction: 0.0,
        },
        y: PaintCoordinate {
            length: y,
            fraction: 0.0,
        },
    };
    let packet = FramePacket {
        header: FrameHeader {
            version: ProtocolVersion::CURRENT,
            surface: SurfaceId::new(1).unwrap(),
            scene_epoch: 1,
            frame_id: 1,
            base_revision: 0,
            target_revision: 1,
            viewport_epoch: 1,
            mode: FrameMode::Snapshot,
        },
        operations: vec![Operation::SetVisualEffects {
            node: NodeId::new(1).unwrap(),
            effects: VisualEffects {
                clip_path: Some((
                    PaintBox::Border,
                    ClipShape::Path {
                        fill_rule: FillRule::EvenOdd,
                        commands: vec![
                            PathCommand::MoveTo(position(1.0, 2.0)),
                            PathCommand::QuadraticTo {
                                control: position(3.0, 4.0),
                                end: position(5.0, 6.0),
                            },
                            PathCommand::CubicTo {
                                control_1: position(7.0, 8.0),
                                control_2: position(9.0, 10.0),
                                end: position(11.0, 12.0),
                            },
                            PathCommand::Close,
                        ],
                    },
                )),
                ..Default::default()
            },
        }],
    };

    let frame = MobileFrameOwned::new(&packet).unwrap();
    let operation = &frame._operations[1];
    let clip = unsafe { &*operation.payload.cast::<MobileClipPath>() };
    assert_eq!(clip.shape_kind, CLIP_SHAPE_PATH);
    let path = unsafe { &*clip.payload.cast::<MobileClipPathCommands>() };
    assert_eq!(path.fill_rule, FILL_RULE_EVEN_ODD);
    let commands = unsafe { std::slice::from_raw_parts(path.commands, path.command_count) };
    assert_eq!(commands.len(), 4);
    assert_eq!(commands[0].kind, PATH_MOVE_TO);
    assert_eq!(commands[0].points[0].length, 1.0);
    assert_eq!(commands[1].kind, PATH_QUADRATIC_TO);
    assert_eq!(commands[1].points[2].length, 5.0);
    assert_eq!(commands[2].kind, PATH_CUBIC_TO);
    assert_eq!(commands[2].points[5].length, 12.0);
    assert_eq!(commands[3].kind, PATH_CLOSE);
}

#[test]
fn mobile_frame_preserves_every_intrinsic_background_size_kind() {
    let mut auto = linear_background("red");
    auto.position.x.length = 3.0;
    auto.repeat_x = ImageRepeat::NoRepeat;
    let mut cover = linear_background("green");
    cover.size = BackgroundSize::Cover;
    let mut contain = linear_background("blue");
    contain.size = BackgroundSize::Contain;
    let mut width = linear_background("yellow");
    width.size = BackgroundSize::Explicit {
        width: Some(PaintLengthPercentage {
            length: 60.0,
            fraction: 0.0,
        }),
        height: None,
    };
    let mut height = linear_background("black");
    height.size = BackgroundSize::Explicit {
        width: None,
        height: Some(PaintLengthPercentage {
            length: 30.0,
            fraction: 0.0,
        }),
    };
    let packet = FramePacket {
        header: FrameHeader {
            version: ProtocolVersion::CURRENT,
            surface: SurfaceId::new(1).unwrap(),
            scene_epoch: 1,
            frame_id: 1,
            base_revision: 0,
            target_revision: 1,
            viewport_epoch: 1,
            mode: FrameMode::Snapshot,
        },
        operations: vec![Operation::SetBackgroundLayers {
            node: NodeId::new(1).unwrap(),
            layers: vec![auto, cover, contain, width, height],
        }],
    };

    let frame = MobileFrameOwned::new(&packet).unwrap();
    let operation = &frame._operations[0];
    let layers = unsafe {
        std::slice::from_raw_parts(
            operation.payload.cast::<MobileBackgroundLayer>(),
            operation.payload_count,
        )
    };
    assert_eq!(layers[0].size_kind, BACKGROUND_SIZE_AUTO);
    assert_eq!(layers[0].position_x.length, 3.0);
    assert_eq!(layers[0].repeat_x, BACKGROUND_REPEAT_NO_REPEAT);
    assert_eq!(layers[1].size_kind, BACKGROUND_SIZE_COVER);
    assert_eq!(layers[2].size_kind, BACKGROUND_SIZE_CONTAIN);
    assert_eq!(layers[3].size_kind, BACKGROUND_SIZE_WIDTH);
    assert_eq!(layers[3].size_width.length, 60.0);
    assert_eq!(layers[4].size_kind, BACKGROUND_SIZE_HEIGHT);
    assert_eq!(layers[4].size_height.length, 30.0);
}
