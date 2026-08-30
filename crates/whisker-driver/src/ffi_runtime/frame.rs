use super::measurement::{mobile_font_features, mobile_font_variations, nonempty_ptr};
use super::resource::{empty_string, push_string};
use super::*;

#[derive(Debug)]
pub(super) struct MobileFrameError;
impl std::fmt::Display for MobileFrameError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("mobile Host rejected a frame")
    }
}
impl std::error::Error for MobileFrameError {}

#[derive(Debug)]
pub(super) enum MobilePresentError {
    Encoding(MobileFrameError),
    IncompatibleProtocol {
        packet: whisker_engine::whisker_protocol::ProtocolVersion,
        host: whisker_engine::whisker_protocol::ProtocolVersion,
    },
    UnsupportedCapability {
        capability: whisker_engine::whisker_protocol::RenderCapability,
        frame_id: u64,
    },
    HostRejected {
        frame_id: u64,
        status: u8,
        revision: u64,
    },
}

impl From<MobileFrameError> for MobilePresentError {
    fn from(error: MobileFrameError) -> Self {
        Self::Encoding(error)
    }
}

impl std::fmt::Display for MobilePresentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encoding(error) => error.fmt(formatter),
            Self::IncompatibleProtocol { packet, host } => write!(
                formatter,
                "mobile Host protocol {}.{} cannot accept frame protocol {}.{}",
                host.major, host.minor, packet.major, packet.minor
            ),
            Self::UnsupportedCapability {
                capability,
                frame_id,
            } => write!(
                formatter,
                "mobile Host does not advertise capability {} required by frame {frame_id}",
                capability.as_str()
            ),
            Self::HostRejected {
                frame_id,
                status,
                revision,
            } => write!(
                formatter,
                "mobile Host rejected frame {frame_id} with status {status} at revision {revision}"
            ),
        }
    }
}

impl std::error::Error for MobilePresentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Encoding(error) => Some(error),
            _ => None,
        }
    }
}

pub(super) struct MobileFrameSink {
    pub(super) present: PresentFrameCallback,
    pub(super) data: *mut c_void,
    pub(super) capabilities: whisker_engine::whisker_protocol::RenderCapabilities,
}

impl FrameSink for MobileFrameSink {
    type Error = MobilePresentError;
    fn capabilities(&self) -> whisker_engine::whisker_protocol::RenderCapabilities {
        self.capabilities
    }
    fn present(&mut self, packet: &FramePacket) -> Result<ApplyResult, Self::Error> {
        let capabilities = self.capabilities();
        if !capabilities.supports_protocol(packet.header.version) {
            return Err(MobilePresentError::IncompatibleProtocol {
                packet: packet.header.version,
                host: capabilities.protocol(),
            });
        }
        if let Some(capability) = capabilities.first_unsupported(packet) {
            return Err(MobilePresentError::UnsupportedCapability {
                capability,
                frame_id: packet.header.frame_id,
            });
        }
        let owned = MobileFrameOwned::new(packet)?;
        let mut response = MobileApplyResponse::default();
        if !(self.present)(self.data, &owned.value, &mut response) {
            return Err(MobilePresentError::HostRejected {
                frame_id: packet.header.frame_id,
                status: APPLY_REJECTED,
                revision: packet.header.base_revision,
            });
        }
        match response.status {
            APPLY_ACCEPTED if response.revision == packet.header.target_revision => {
                Ok(ApplyResult::Accepted {
                    revision: response.revision,
                })
            }
            APPLY_NEED_SNAPSHOT => Ok(ApplyResult::NeedSnapshot {
                receiver_revision: response.revision,
            }),
            _ => Err(MobilePresentError::HostRejected {
                frame_id: packet.header.frame_id,
                status: response.status,
                revision: response.revision,
            }),
        }
    }
}

// Every Box provides a stable pointee while the outer Vec grows; the borrowed
// C frame stores pointers into these allocations for the callback duration.
#[allow(clippy::vec_box)]
pub(super) struct MobileFrameOwned {
    value: MobileFrame,
    _arena: RawValueArena,
    _layouts: Vec<Box<MobileLayoutGeometry>>,
    _paints: Vec<Box<MobileBoxPaint>>,
    _box_shadows: Vec<Box<[MobileBoxShadow]>>,
    _clip_insets: Vec<Box<MobileClipInset>>,
    _clip_circles: Vec<Box<MobileClipCircle>>,
    _clip_ellipses: Vec<Box<MobileClipEllipse>>,
    _path_commands: Vec<Box<[MobilePathCommand]>>,
    _clip_path_commands: Vec<Box<MobileClipPathCommands>>,
    _clip_paths: Vec<Box<MobileClipPath>>,
    _gradient_stops: Vec<Box<[MobileGradientStop]>>,
    _radial_gradients: Vec<Box<MobileRadialGradient>>,
    _conic_gradients: Vec<Box<MobileConicGradient>>,
    _background_layers: Vec<Box<[MobileBackgroundLayer]>>,
    _background_resource_ids: Vec<Box<u64>>,
    _texts: Vec<Box<MobileText>>,
    _text_font_families: Vec<Box<[WhiskerStringRef]>>,
    _font_features: Vec<Box<[MobileFontFeature]>>,
    _font_variations: Vec<Box<[MobileFontVariation]>>,
    _transforms: Vec<Box<[f32; 16]>>,
    _values: Vec<Box<WhiskerValueRaw>>,
    _strings: Vec<CString>,
    pub(super) _operations: Vec<MobileOperation>,
}

fn empty_mobile_operation() -> MobileOperation {
    MobileOperation {
        tag: 0,
        flags: 0,
        node: 0,
        parent: 0,
        child: 0,
        index: 0,
        member: 0,
        integer: 0,
        scalar: 0.0,
        wide: 0,
        payload: std::ptr::null(),
        payload_count: 0,
    }
}

const fn mobile_cursor_keyword(keyword: whisker_engine::whisker_protocol::CursorKeyword) -> i32 {
    use whisker_engine::whisker_protocol::CursorKeyword;
    match keyword {
        CursorKeyword::Auto => 0,
        CursorKeyword::Default => 1,
        CursorKeyword::None => 2,
        CursorKeyword::ContextMenu => 3,
        CursorKeyword::Help => 4,
        CursorKeyword::Pointer => 5,
        CursorKeyword::Progress => 6,
        CursorKeyword::Wait => 7,
        CursorKeyword::Cell => 8,
        CursorKeyword::Crosshair => 9,
        CursorKeyword::Text => 10,
        CursorKeyword::VerticalText => 11,
        CursorKeyword::Alias => 12,
        CursorKeyword::Copy => 13,
        CursorKeyword::Move => 14,
        CursorKeyword::NoDrop => 15,
        CursorKeyword::NotAllowed => 16,
        CursorKeyword::Grab => 17,
        CursorKeyword::Grabbing => 18,
        CursorKeyword::ColResize => 19,
        CursorKeyword::RowResize => 20,
        CursorKeyword::NResize => 21,
        CursorKeyword::EResize => 22,
        CursorKeyword::SResize => 23,
        CursorKeyword::WResize => 24,
        CursorKeyword::NeResize => 25,
        CursorKeyword::NwResize => 26,
        CursorKeyword::SeResize => 27,
        CursorKeyword::SwResize => 28,
        CursorKeyword::EwResize => 29,
        CursorKeyword::NsResize => 30,
        CursorKeyword::NeswResize => 31,
        CursorKeyword::NwseResize => 32,
        CursorKeyword::ZoomIn => 33,
        CursorKeyword::ZoomOut => 34,
    }
}

impl MobileFrameOwned {
    pub(super) fn new(packet: &FramePacket) -> Result<Self, MobileFrameError> {
        let mut arena = RawValueArena::default();
        let mut layouts = Vec::<Box<MobileLayoutGeometry>>::new();
        let mut paints = Vec::<Box<MobileBoxPaint>>::new();
        let mut box_shadows = Vec::<Box<[MobileBoxShadow]>>::new();
        let mut clip_insets = Vec::<Box<MobileClipInset>>::new();
        let mut clip_circles = Vec::<Box<MobileClipCircle>>::new();
        let mut clip_ellipses = Vec::<Box<MobileClipEllipse>>::new();
        let mut path_commands = Vec::<Box<[MobilePathCommand]>>::new();
        let mut clip_path_commands = Vec::<Box<MobileClipPathCommands>>::new();
        let mut clip_paths = Vec::<Box<MobileClipPath>>::new();
        let mut gradient_stops = Vec::<Box<[MobileGradientStop]>>::new();
        let mut radial_gradients = Vec::<Box<MobileRadialGradient>>::new();
        let mut conic_gradients = Vec::<Box<MobileConicGradient>>::new();
        let mut background_layers = Vec::<Box<[MobileBackgroundLayer]>>::new();
        let mut background_resource_ids = Vec::<Box<u64>>::new();
        let mut texts = Vec::<Box<MobileText>>::new();
        let mut text_font_families = Vec::<Box<[WhiskerStringRef]>>::new();
        let mut font_features = Vec::<Box<[MobileFontFeature]>>::new();
        let mut font_variations = Vec::<Box<[MobileFontVariation]>>::new();
        let mut transforms = Vec::<Box<[f32; 16]>>::new();
        let mut values = Vec::<Box<WhiskerValueRaw>>::new();
        let mut strings = Vec::new();
        let mut operations = Vec::with_capacity(packet.operations.len() * 2);
        for operation in &packet.operations {
            let mut raw = empty_mobile_operation();
            match operation {
                Operation::CreateNode { node, element_type } => {
                    raw.tag = OP_CREATE;
                    raw.node = node.get();
                    raw.member = element_type.get();
                }
                Operation::DeleteNode { node } => {
                    raw.tag = OP_DELETE;
                    raw.node = node.get();
                }
                Operation::InsertChild {
                    parent,
                    child,
                    index,
                } => {
                    raw.tag = OP_INSERT;
                    raw.parent = parent.get();
                    raw.child = child.get();
                    raw.index = *index;
                }
                Operation::RemoveChild { parent, child } => {
                    raw.tag = OP_REMOVE;
                    raw.parent = parent.get();
                    raw.child = child.get();
                }
                Operation::MoveChild {
                    parent,
                    child,
                    index,
                } => {
                    raw.tag = OP_MOVE;
                    raw.parent = parent.get();
                    raw.child = child.get();
                    raw.index = *index;
                }
                Operation::SetLayout { node, geometry } => {
                    raw.tag = OP_LAYOUT;
                    raw.node = node.get();
                    layouts.push(Box::new(MobileLayoutGeometry {
                        border: mobile_rect(geometry.border_box),
                        content: mobile_rect(geometry.content_box),
                    }));
                    raw.payload = layouts.last().unwrap().as_ref() as *const _ as *const c_void;
                }
                Operation::SetBoxPaint { node, paint } => {
                    raw.tag = OP_PAINT;
                    raw.node = node.get();
                    paints.push(Box::new(mobile_paint(paint, &mut strings)));
                    raw.payload = paints.last().unwrap().as_ref() as *const _ as *const c_void;
                }
                Operation::SetBackgroundLayers { node, layers } => {
                    raw.tag = OP_BACKGROUND_LAYERS;
                    raw.node = node.get();
                    if !layers.is_empty() {
                        let mut mobile_layers = Vec::with_capacity(layers.len());
                        for layer in layers {
                            if layer.attachment != BackgroundAttachment::Scroll
                                || layer.blend_mode != BlendMode::Normal
                            {
                                return Err(MobileFrameError);
                            }
                            let empty = MobileLengthPercentage::default();
                            let (size_kind, size_width, size_height) = match layer.size {
                                BackgroundSize::Auto => (BACKGROUND_SIZE_AUTO, empty, empty),
                                BackgroundSize::Cover => (BACKGROUND_SIZE_COVER, empty, empty),
                                BackgroundSize::Contain => (BACKGROUND_SIZE_CONTAIN, empty, empty),
                                BackgroundSize::Explicit {
                                    width: Some(width),
                                    height: Some(height),
                                } => (
                                    BACKGROUND_SIZE_EXPLICIT,
                                    mobile_length(width),
                                    mobile_length(height),
                                ),
                                BackgroundSize::Explicit {
                                    width: Some(width),
                                    height: None,
                                } => (BACKGROUND_SIZE_WIDTH, mobile_length(width), empty),
                                BackgroundSize::Explicit {
                                    width: None,
                                    height: Some(height),
                                } => (BACKGROUND_SIZE_HEIGHT, empty, mobile_length(height)),
                                BackgroundSize::Explicit {
                                    width: None,
                                    height: None,
                                } => (BACKGROUND_SIZE_AUTO, empty, empty),
                            };
                            let repeat_x = mobile_background_repeat(layer.repeat_x);
                            let repeat_y = mobile_background_repeat(layer.repeat_y);
                            let origin = match layer.origin {
                                PaintBox::Border => BACKGROUND_BOX_BORDER,
                                PaintBox::Padding => BACKGROUND_BOX_PADDING,
                                PaintBox::Content => BACKGROUND_BOX_CONTENT,
                                _ => return Err(MobileFrameError),
                            };
                            let clip = match layer.clip {
                                PaintBox::Border => BACKGROUND_BOX_BORDER,
                                PaintBox::Padding => BACKGROUND_BOX_PADDING,
                                PaintBox::Content => BACKGROUND_BOX_CONTENT,
                                PaintBox::BorderArea => BACKGROUND_BOX_BORDER_AREA,
                                _ => return Err(MobileFrameError),
                            };
                            let image = match &layer.image {
                                PaintImage::Resource(resource) => {
                                    background_resource_ids.push(Box::new(resource.get()));
                                    MobileBackgroundImage {
                                        kind: BACKGROUND_RESOURCE,
                                        scalar: 0.0,
                                        payload: background_resource_ids.last().unwrap().as_ref()
                                            as *const _
                                            as *const c_void,
                                        payload_count: 1,
                                    }
                                }
                                PaintImage::LinearGradient {
                                    angle_degrees,
                                    repeating: false,
                                    stops,
                                } => {
                                    let stops = mobile_gradient_stops(stops, &mut strings)?;
                                    gradient_stops.push(stops);
                                    let stops = gradient_stops.last().unwrap();
                                    MobileBackgroundImage {
                                        kind: BACKGROUND_LINEAR,
                                        scalar: *angle_degrees,
                                        payload: stops.as_ptr().cast(),
                                        payload_count: stops.len(),
                                    }
                                }
                                PaintImage::RadialGradient {
                                    shape: RadialGradientShape::Ellipse,
                                    extent: RadialGradientExtent::Explicit,
                                    center,
                                    radii: Some((radius_x, radius_y)),
                                    repeating: false,
                                    stops,
                                } => {
                                    let stops = mobile_gradient_stops(stops, &mut strings)?;
                                    gradient_stops.push(stops);
                                    let stops = gradient_stops.last().unwrap();
                                    radial_gradients.push(Box::new(MobileRadialGradient {
                                        center_x: mobile_coordinate(center.x),
                                        center_y: mobile_coordinate(center.y),
                                        radius_x: mobile_length(*radius_x),
                                        radius_y: mobile_length(*radius_y),
                                        stops: stops.as_ptr(),
                                        stop_count: stops.len(),
                                    }));
                                    MobileBackgroundImage {
                                        kind: BACKGROUND_RADIAL,
                                        scalar: 0.0,
                                        payload: radial_gradients.last().unwrap().as_ref()
                                            as *const _
                                            as *const c_void,
                                        payload_count: 1,
                                    }
                                }
                                PaintImage::ConicGradient {
                                    from_degrees,
                                    center,
                                    repeating: false,
                                    stops,
                                } if stops.iter().all(|stop| {
                                    stop.position.is_some_and(|position| position.length == 0.0)
                                }) =>
                                {
                                    let stops = mobile_gradient_stops(stops, &mut strings)?;
                                    gradient_stops.push(stops);
                                    let stops = gradient_stops.last().unwrap();
                                    conic_gradients.push(Box::new(MobileConicGradient {
                                        center_x: mobile_coordinate(center.x),
                                        center_y: mobile_coordinate(center.y),
                                        stops: stops.as_ptr(),
                                        stop_count: stops.len(),
                                    }));
                                    MobileBackgroundImage {
                                        kind: BACKGROUND_CONIC,
                                        scalar: *from_degrees,
                                        payload: conic_gradients.last().unwrap().as_ref()
                                            as *const _
                                            as *const c_void,
                                        payload_count: 1,
                                    }
                                }
                                _ => return Err(MobileFrameError),
                            };
                            mobile_layers.push(MobileBackgroundLayer {
                                image,
                                position_x: mobile_coordinate(layer.position.x),
                                position_y: mobile_coordinate(layer.position.y),
                                size_width,
                                size_height,
                                size_kind,
                                repeat_x,
                                repeat_y,
                                origin,
                                clip,
                                attachment: BACKGROUND_ATTACHMENT_SCROLL,
                                blend_mode: BACKGROUND_BLEND_NORMAL,
                            });
                        }
                        background_layers.push(mobile_layers.into_boxed_slice());
                        let layers = background_layers.last().unwrap();
                        raw.payload = layers.as_ptr().cast();
                        raw.payload_count = layers.len();
                    }
                }
                Operation::SetVisualEffects { node, effects } => {
                    let mut remainder = effects.clone();
                    remainder.box_shadows.clear();
                    remainder.clip_path = None;
                    remainder.backdrop_blur = None;
                    remainder.image_rendering = ImageRendering::Auto;
                    if remainder != VisualEffects::default() {
                        return Err(MobileFrameError);
                    }
                    if !matches!(
                        effects.image_rendering,
                        ImageRendering::Auto
                            | ImageRendering::Pixelated
                            | ImageRendering::CrispEdges
                    ) {
                        return Err(MobileFrameError);
                    }
                    raw.tag = OP_BOX_SHADOWS;
                    raw.node = node.get();
                    box_shadows.push(
                        effects
                            .box_shadows
                            .iter()
                            .map(|shadow| MobileBoxShadow {
                                offset_x: shadow.offset_x,
                                offset_y: shadow.offset_y,
                                blur_radius: shadow.blur_radius,
                                spread_radius: shadow.spread_radius,
                                color: mobile_color(&shadow.color, &mut strings),
                                inset: u8::from(shadow.inset),
                                _pad: [0; 7],
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    );
                    let shadows = box_shadows.last().unwrap();
                    raw.payload = if shadows.is_empty() {
                        std::ptr::null()
                    } else {
                        shadows.as_ptr().cast()
                    };
                    raw.payload_count = shadows.len();
                    operations.push(raw);

                    raw = empty_mobile_operation();
                    raw.tag = OP_CLIP_PATH;
                    raw.node = node.get();
                    if let Some((reference_box, shape)) = effects.clip_path.as_ref() {
                        let reference_box = match reference_box {
                            PaintBox::Border => BACKGROUND_BOX_BORDER,
                            PaintBox::Padding => BACKGROUND_BOX_PADDING,
                            PaintBox::Content => BACKGROUND_BOX_CONTENT,
                            _ => return Err(MobileFrameError),
                        };
                        let (shape_kind, payload) = match shape {
                            ClipShape::Inset { edges, radii } => {
                                clip_insets.push(Box::new(MobileClipInset {
                                    edges: [
                                        mobile_coordinate(edges.top),
                                        mobile_coordinate(edges.right),
                                        mobile_coordinate(edges.bottom),
                                        mobile_coordinate(edges.left),
                                    ],
                                    radii_horizontal: [
                                        mobile_length(radii.top_left.horizontal),
                                        mobile_length(radii.top_right.horizontal),
                                        mobile_length(radii.bottom_right.horizontal),
                                        mobile_length(radii.bottom_left.horizontal),
                                    ],
                                    radii_vertical: [
                                        mobile_length(radii.top_left.vertical),
                                        mobile_length(radii.top_right.vertical),
                                        mobile_length(radii.bottom_right.vertical),
                                        mobile_length(radii.bottom_left.vertical),
                                    ],
                                }));
                                (
                                    CLIP_SHAPE_INSET,
                                    clip_insets.last().unwrap().as_ref() as *const _
                                        as *const c_void,
                                )
                            }
                            ClipShape::Circle { radius, center } => {
                                clip_circles.push(Box::new(MobileClipCircle {
                                    radius: mobile_length(*radius),
                                    center_x: mobile_coordinate(center.x),
                                    center_y: mobile_coordinate(center.y),
                                }));
                                (
                                    CLIP_SHAPE_CIRCLE,
                                    clip_circles.last().unwrap().as_ref() as *const _
                                        as *const c_void,
                                )
                            }
                            ClipShape::Ellipse {
                                radius_x,
                                radius_y,
                                center,
                            } => {
                                clip_ellipses.push(Box::new(MobileClipEllipse {
                                    radius_x: mobile_length(*radius_x),
                                    radius_y: mobile_length(*radius_y),
                                    center_x: mobile_coordinate(center.x),
                                    center_y: mobile_coordinate(center.y),
                                }));
                                (
                                    CLIP_SHAPE_ELLIPSE,
                                    clip_ellipses.last().unwrap().as_ref() as *const _
                                        as *const c_void,
                                )
                            }
                            ClipShape::Path {
                                fill_rule,
                                commands,
                            } => {
                                path_commands.push(
                                    commands
                                        .iter()
                                        .map(mobile_path_command)
                                        .collect::<Vec<_>>()
                                        .into_boxed_slice(),
                                );
                                let commands = path_commands.last().unwrap();
                                clip_path_commands.push(Box::new(MobileClipPathCommands {
                                    fill_rule: match fill_rule {
                                        FillRule::NonZero => FILL_RULE_NON_ZERO,
                                        FillRule::EvenOdd => FILL_RULE_EVEN_ODD,
                                    },
                                    _reserved: 0,
                                    commands: commands.as_ptr(),
                                    command_count: commands.len(),
                                }));
                                (
                                    CLIP_SHAPE_PATH,
                                    clip_path_commands.last().unwrap().as_ref() as *const _
                                        as *const c_void,
                                )
                            }
                            _ => return Err(MobileFrameError),
                        };
                        clip_paths.push(Box::new(MobileClipPath {
                            reference_box,
                            shape_kind,
                            payload,
                            payload_count: 1,
                        }));
                        raw.payload =
                            clip_paths.last().unwrap().as_ref() as *const _ as *const c_void;
                        raw.payload_count = 1;
                    }
                    operations.push(raw);

                    raw = empty_mobile_operation();
                    raw.tag = OP_BACKDROP_BLUR;
                    raw.node = node.get();
                    raw.scalar = effects.backdrop_blur.unwrap_or(0.0);
                    operations.push(raw);

                    raw = empty_mobile_operation();
                    raw.tag = OP_IMAGE_RENDERING;
                    raw.node = node.get();
                    raw.integer = match effects.image_rendering {
                        ImageRendering::Auto => IMAGE_RENDERING_AUTO,
                        ImageRendering::Pixelated => IMAGE_RENDERING_PIXELATED,
                        ImageRendering::CrispEdges => IMAGE_RENDERING_CRISP_EDGES,
                        _ => unreachable!("unsupported image-rendering rejected above"),
                    };
                }
                Operation::SetClip { node, clip } => {
                    raw.tag = OP_CLIP;
                    raw.node = node.get();
                    raw.flags = u32::from(matches!(
                        clip.horizontal,
                        whisker_engine::whisker_protocol::OverflowClip::Hidden
                    )) | (u32::from(matches!(
                        clip.vertical,
                        whisker_engine::whisker_protocol::OverflowClip::Hidden
                    )) << 1);
                }
                Operation::SetTransform { node, transform } => {
                    raw.tag = OP_TRANSFORM;
                    raw.node = node.get();
                    transforms.push(Box::new(transform.0));
                    raw.payload = transforms.last().unwrap().as_ref().as_ptr().cast();
                    raw.payload_count = 16;
                }
                Operation::SetOpacity { node, opacity } => {
                    raw.tag = OP_OPACITY;
                    raw.node = node.get();
                    raw.scalar = *opacity;
                }
                Operation::SetVisibility { node, visibility } => {
                    raw.tag = OP_VISIBILITY;
                    raw.node = node.get();
                    raw.integer = i32::from(matches!(
                        visibility,
                        whisker_engine::whisker_protocol::Visibility::Visible
                    ));
                }
                Operation::SetZOrder { node, z_order } => {
                    raw.tag = OP_Z_ORDER;
                    raw.node = node.get();
                    raw.integer = *z_order;
                }
                Operation::SetText { node, content } => {
                    raw.tag = OP_TEXT;
                    raw.node = node.get();
                    raw.payload = push_mobile_text(
                        content,
                        &mut strings,
                        &mut text_font_families,
                        &mut font_features,
                        &mut font_variations,
                        &mut texts,
                    )?;
                }
                Operation::SetTextStyle { node, style } => {
                    raw.tag = OP_TEXT_STYLE;
                    raw.node = node.get();
                    let content = TextContent {
                        payload: TextMeasurePayload {
                            text: String::new(),
                            style: style.style.clone(),
                            locale: style.locale.clone(),
                            direction: style.direction,
                            alignment: style.alignment,
                            indent: Default::default(),
                            wrap: MeasureTextWrap::Wrap,
                            word_break: MeasureTextWordBreak::Normal,
                            max_lines: None,
                            overflow: MeasureTextOverflow::Clip,
                        },
                        paint: style.paint.clone(),
                        prepared_content: None,
                    };
                    raw.payload = push_mobile_text(
                        &content,
                        &mut strings,
                        &mut text_font_families,
                        &mut font_features,
                        &mut font_variations,
                        &mut texts,
                    )?;
                }
                Operation::SetAccessibility {
                    node,
                    accessibility,
                } => {
                    raw.tag = OP_ACCESSIBILITY;
                    raw.node = node.get();
                    values.push(Box::new(arena.encode(&accessibility.to_value())));
                    raw.payload = values.last().unwrap().as_ref() as *const _ as *const c_void;
                }
                Operation::SetProperty {
                    node,
                    property,
                    value,
                } => {
                    raw.tag = OP_PROPERTY;
                    raw.node = node.get();
                    raw.member = property.get();
                    values.push(Box::new(arena.encode(value)));
                    raw.payload = values.last().unwrap().as_ref() as *const _ as *const c_void;
                }
                Operation::ClearProperty { node, property } => {
                    raw.tag = OP_CLEAR_PROPERTY;
                    raw.node = node.get();
                    raw.member = property.get();
                }
                Operation::SetEventMask { node, event_mask } => {
                    raw.tag = OP_EVENT_MASK;
                    raw.node = node.get();
                    raw.wide = *event_mask;
                }
                Operation::SetHitTest { node, behavior } => {
                    raw.tag = OP_HIT_TEST;
                    raw.node = node.get();
                    raw.integer = match behavior {
                        whisker_engine::whisker_protocol::HitTestBehavior::Auto => 0,
                        whisker_engine::whisker_protocol::HitTestBehavior::None => 1,
                        whisker_engine::whisker_protocol::HitTestBehavior::BoxOnly => 2,
                        whisker_engine::whisker_protocol::HitTestBehavior::DescendantsOnly => 3,
                    };
                }
                Operation::SetCursor { node, cursor } => {
                    if !cursor.resources.is_empty() {
                        return Err(MobileFrameError);
                    }
                    raw.tag = OP_CURSOR;
                    raw.node = node.get();
                    raw.integer = mobile_cursor_keyword(cursor.fallback);
                }
                Operation::SetPointerCapture { node, pointer } => {
                    raw.tag = OP_CAPTURE;
                    raw.node = node.get();
                    raw.wide = pointer.get();
                }
                Operation::ReleasePointerCapture { node, pointer } => {
                    raw.tag = OP_RELEASE_CAPTURE;
                    raw.node = node.get();
                    raw.wide = pointer.get();
                }
                Operation::InvokeCommand {
                    node,
                    command,
                    arguments,
                } => {
                    raw.tag = OP_COMMAND;
                    raw.node = node.get();
                    raw.member = command.get();
                    raw.wide = 0;
                    values.push(Box::new(arena.encode(arguments)));
                    raw.payload = values.last().unwrap().as_ref() as *const _ as *const c_void;
                }
                Operation::SetImage { .. } => {
                    return Err(MobileFrameError);
                }
            }
            operations.push(raw);
        }
        let value = MobileFrame {
            abi_major: MOBILE_ABI_MAJOR,
            abi_minor: MOBILE_ABI_MINOR,
            protocol_major: packet.header.version.major,
            protocol_minor: packet.header.version.minor,
            mode: match packet.header.mode {
                FrameMode::Snapshot => FRAME_SNAPSHOT,
                FrameMode::Delta => FRAME_DELTA,
            },
            _pad: [0; 7],
            surface: packet.header.surface.get(),
            scene_epoch: packet.header.scene_epoch,
            viewport_epoch: packet.header.viewport_epoch,
            frame_id: packet.header.frame_id,
            base_revision: packet.header.base_revision,
            target_revision: packet.header.target_revision,
            operations: operations.as_ptr(),
            operation_count: operations.len(),
        };
        Ok(Self {
            value,
            _arena: arena,
            _layouts: layouts,
            _paints: paints,
            _box_shadows: box_shadows,
            _clip_insets: clip_insets,
            _clip_circles: clip_circles,
            _clip_ellipses: clip_ellipses,
            _path_commands: path_commands,
            _clip_path_commands: clip_path_commands,
            _clip_paths: clip_paths,
            _gradient_stops: gradient_stops,
            _radial_gradients: radial_gradients,
            _conic_gradients: conic_gradients,
            _background_layers: background_layers,
            _background_resource_ids: background_resource_ids,
            _texts: texts,
            _text_font_families: text_font_families,
            _font_features: font_features,
            _font_variations: font_variations,
            _transforms: transforms,
            _values: values,
            _strings: strings,
            _operations: operations,
        })
    }
}

fn mobile_rect(value: whisker_engine::whisker_protocol::LayoutRect) -> MobileRect {
    MobileRect {
        x: value.x,
        y: value.y,
        width: value.width,
        height: value.height,
    }
}
fn mobile_coordinate(
    value: whisker_engine::whisker_protocol::PaintCoordinate,
) -> MobileLengthPercentage {
    MobileLengthPercentage {
        length: value.length,
        fraction: value.fraction,
    }
}
fn mobile_length(value: PaintLengthPercentage) -> MobileLengthPercentage {
    MobileLengthPercentage {
        length: value.length,
        fraction: value.fraction,
    }
}

fn mobile_path_point(
    points: &mut [MobileLengthPercentage; 6],
    offset: usize,
    value: PaintPosition,
) {
    points[offset] = mobile_coordinate(value.x);
    points[offset + 1] = mobile_coordinate(value.y);
}

fn mobile_path_command(value: &PathCommand) -> MobilePathCommand {
    let mut result = MobilePathCommand::default();
    match value {
        PathCommand::MoveTo(point) => {
            result.kind = PATH_MOVE_TO;
            mobile_path_point(&mut result.points, 0, *point);
        }
        PathCommand::LineTo(point) => {
            result.kind = PATH_LINE_TO;
            mobile_path_point(&mut result.points, 0, *point);
        }
        PathCommand::QuadraticTo { control, end } => {
            result.kind = PATH_QUADRATIC_TO;
            mobile_path_point(&mut result.points, 0, *control);
            mobile_path_point(&mut result.points, 2, *end);
        }
        PathCommand::CubicTo {
            control_1,
            control_2,
            end,
        } => {
            result.kind = PATH_CUBIC_TO;
            mobile_path_point(&mut result.points, 0, *control_1);
            mobile_path_point(&mut result.points, 2, *control_2);
            mobile_path_point(&mut result.points, 4, *end);
        }
        PathCommand::Close => result.kind = PATH_CLOSE,
    }
    result
}

fn mobile_background_repeat(value: ImageRepeat) -> u32 {
    match value {
        ImageRepeat::Repeat => BACKGROUND_REPEAT_REPEAT,
        ImageRepeat::NoRepeat => BACKGROUND_REPEAT_NO_REPEAT,
        ImageRepeat::Space => BACKGROUND_REPEAT_SPACE,
        ImageRepeat::Round => BACKGROUND_REPEAT_ROUND,
    }
}
fn mobile_gradient_stops(
    stops: &[whisker_engine::whisker_protocol::GradientStop],
    strings: &mut Vec<CString>,
) -> Result<Box<[MobileGradientStop]>, MobileFrameError> {
    if !(2..=4_096).contains(&stops.len()) {
        return Err(MobileFrameError);
    }
    stops
        .iter()
        .map(|stop| {
            let position = stop.position.ok_or(MobileFrameError)?;
            Ok(MobileGradientStop {
                color: mobile_color(&stop.color, strings),
                position: mobile_coordinate(position),
            })
        })
        .collect::<Result<Vec<_>, MobileFrameError>>()
        .map(Vec::into_boxed_slice)
}
pub(super) fn mobile_paint(
    value: &whisker_engine::whisker_protocol::BoxPaint,
    strings: &mut Vec<CString>,
) -> MobileBoxPaint {
    MobileBoxPaint {
        background: mobile_color(&value.background_color, strings),
        widths: [
            mobile_length(value.border_widths.top),
            mobile_length(value.border_widths.right),
            mobile_length(value.border_widths.bottom),
            mobile_length(value.border_widths.left),
        ],
        colors: [
            mobile_color(&value.border_colors.top, strings),
            mobile_color(&value.border_colors.right, strings),
            mobile_color(&value.border_colors.bottom, strings),
            mobile_color(&value.border_colors.left, strings),
        ],
        styles: [
            border_style(value.border_styles.top),
            border_style(value.border_styles.right),
            border_style(value.border_styles.bottom),
            border_style(value.border_styles.left),
        ],
        radii_horizontal: [
            mobile_length(value.border_radii.top_left.horizontal),
            mobile_length(value.border_radii.top_right.horizontal),
            mobile_length(value.border_radii.bottom_right.horizontal),
            mobile_length(value.border_radii.bottom_left.horizontal),
        ],
        radii_vertical: [
            mobile_length(value.border_radii.top_left.vertical),
            mobile_length(value.border_radii.top_right.vertical),
            mobile_length(value.border_radii.bottom_right.vertical),
            mobile_length(value.border_radii.bottom_left.vertical),
        ],
    }
}
// Each operation retains a raw pointer to its text until the Host consumes the
// complete frame, so every text value needs an address that survives Vec growth.
#[allow(clippy::vec_box)]
fn push_mobile_text(
    content: &TextContent,
    strings: &mut Vec<CString>,
    text_font_families: &mut Vec<Box<[WhiskerStringRef]>>,
    font_features: &mut Vec<Box<[MobileFontFeature]>>,
    font_variations: &mut Vec<Box<[MobileFontVariation]>>,
    texts: &mut Vec<Box<MobileText>>,
) -> Result<*const c_void, MobileFrameError> {
    if content.paint.decoration.lines.overline
        || (content.paint.decoration.lines.underline && content.paint.decoration.lines.line_through)
        || !matches!(
            content.paint.decoration.thickness,
            whisker_engine::whisker_protocol::TextDecorationThickness::Auto
        )
        || content.paint.shadows.len() > 1
    {
        return Err(MobileFrameError);
    }
    let shadow = content.paint.shadows.first();
    let shadow_color = match shadow {
        Some(value) => mobile_color(&value.color, strings),
        None => mobile_color(&PaintColor::default(), strings),
    };
    let decoration_color = mobile_color(&content.paint.decoration.color, strings);
    font_features.push(mobile_font_features(&content.payload.style.features));
    let features = font_features.last().expect("pushed font features");
    font_variations.push(mobile_font_variations(&content.payload.style.variations));
    let variations = font_variations.last().expect("pushed font variations");
    text_font_families.push(
        content
            .payload
            .style
            .font_families
            .iter()
            .map(|family| match family {
                MeasureFontFamily::System => push_string(strings, "system"),
                MeasureFontFamily::Named(value) => push_string(strings, value),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    let families = text_font_families.last().expect("pushed font families");
    texts.push(Box::new(MobileText {
        text: push_string(strings, &content.payload.text),
        font_families: nonempty_ptr(families),
        font_family_count: families.len(),
        font_size: content.payload.style.font_size,
        font_weight: content.payload.style.font_weight,
        font_style: match content.payload.style.font_style {
            MeasureFontStyle::Normal => 0,
            MeasureFontStyle::Italic => 1,
            MeasureFontStyle::Oblique => 2,
        },
        wrap: u8::from(matches!(content.payload.wrap, MeasureTextWrap::Wrap)),
        word_break: match content.payload.word_break {
            MeasureTextWordBreak::Normal => 0,
            MeasureTextWordBreak::BreakAll => 1,
            MeasureTextWordBreak::KeepAll => 2,
        },
        overflow: u8::from(matches!(
            content.payload.overflow,
            MeasureTextOverflow::Ellipsis
        )),
        max_lines: content.payload.max_lines.unwrap_or(0),
        line_height: match content.payload.style.line_height {
            MeasureLineHeight::Normal => 0.0,
            MeasureLineHeight::LogicalPixels(value) => value,
        },
        letter_spacing: content.payload.style.letter_spacing,
        font_features: nonempty_ptr(features),
        font_feature_count: features.len(),
        font_variations: nonempty_ptr(variations),
        font_variation_count: variations.len(),
        font_optical_sizing: u8::from(matches!(
            content.payload.style.optical_sizing,
            whisker_engine::whisker_protocol::FontOpticalSizing::None
        )),
        _font_pad: [0; 7],
        color: mobile_color(&content.paint.foreground, strings),
        shadow_offset_x: shadow.map_or(0.0, |value| value.offset_x),
        shadow_offset_y: shadow.map_or(0.0, |value| value.offset_y),
        shadow_blur_radius: shadow.map_or(0.0, |value| value.blur_radius),
        shadow_flags: u32::from(shadow.is_some()),
        shadow_color,
        decoration_flags: u32::from(content.paint.decoration.lines.underline)
            | (u32::from(content.paint.decoration.lines.line_through) << 1),
        decoration_style: match content.paint.decoration.style {
            whisker_engine::whisker_protocol::TextDecorationStyle::Solid => 0,
            whisker_engine::whisker_protocol::TextDecorationStyle::Double => 1,
            whisker_engine::whisker_protocol::TextDecorationStyle::Dotted => 2,
            whisker_engine::whisker_protocol::TextDecorationStyle::Dashed => 3,
            whisker_engine::whisker_protocol::TextDecorationStyle::Wavy => 4,
        },
        decoration_color,
        alignment: match content.payload.alignment {
            whisker_engine::whisker_protocol::MeasureTextAlignment::Start => 0,
            whisker_engine::whisker_protocol::MeasureTextAlignment::End => 1,
            whisker_engine::whisker_protocol::MeasureTextAlignment::Left => 2,
            whisker_engine::whisker_protocol::MeasureTextAlignment::Right => 3,
            whisker_engine::whisker_protocol::MeasureTextAlignment::Center => 4,
        },
        indent_logical_pixels: content.payload.indent.logical_pixels,
        indent_percentage: content.payload.indent.percentage,
        prepared_content: content.prepared_content.map_or(0, |value| value.get()),
        direction: match content.payload.direction {
            MeasureTextDirection::Auto => 0,
            MeasureTextDirection::LeftToRight => 1,
            MeasureTextDirection::RightToLeft => 2,
        },
        _direction_pad: 0,
    }));
    Ok(texts.last().expect("pushed mobile text").as_ref() as *const _ as *const c_void)
}

fn mobile_color(value: &PaintColor, strings: &mut Vec<CString>) -> MobileColor {
    match value {
        PaintColor::Named(name) => MobileColor {
            kind: 0,
            red: 0,
            green: 0,
            blue: 0,
            _pad: 0,
            alpha: 1.0,
            name: push_string(strings, name),
        },
        PaintColor::Srgba {
            red,
            green,
            blue,
            alpha,
        } => MobileColor {
            kind: 1,
            red: *red,
            green: *green,
            blue: *blue,
            _pad: 0,
            alpha: *alpha,
            name: empty_string(),
        },
        PaintColor::Hsla {
            hue_degrees,
            saturation,
            lightness,
            alpha,
        } => {
            let (red, green, blue) =
                hsl_to_rgb(*hue_degrees, *saturation / 100.0, *lightness / 100.0);
            MobileColor {
                kind: 1,
                red,
                green,
                blue,
                _pad: 0,
                alpha: *alpha,
                name: empty_string(),
            }
        }
    }
}
pub(super) fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> (u8, u8, u8) {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let h = hue.rem_euclid(360.0) / 60.0;
    let x = chroma * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = lightness - chroma / 2.0;
    (
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    )
}
fn border_style(value: BorderLineStyle) -> u32 {
    match value {
        BorderLineStyle::None => 0,
        BorderLineStyle::Hidden => 1,
        BorderLineStyle::Solid => 2,
        BorderLineStyle::Dashed => 3,
        BorderLineStyle::Dotted => 4,
        BorderLineStyle::Double => 5,
        BorderLineStyle::Groove => 6,
        BorderLineStyle::Ridge => 7,
        BorderLineStyle::Inset => 8,
        BorderLineStyle::Outset => 9,
    }
}
