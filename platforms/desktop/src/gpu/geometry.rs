use super::*;

pub(super) struct BackgroundTileGeometry {
    pub(super) rect: LayoutRect,
    pub(super) stride: [f32; 2],
    pub(super) domain: LayoutRect,
    pub(super) flags: u32,
}

pub(super) struct BackgroundAxisGeometry {
    pub(super) origin: f32,
    pub(super) tile_size: f32,
    pub(super) stride: f32,
    pub(super) flags: u32,
}

pub(super) fn background_axis_geometry(
    start: f32,
    area_size: f32,
    image_size: f32,
    position: whisker_protocol::PaintCoordinate,
    repeat: whisker_protocol::ImageRepeat,
    no_repeat_flag: u32,
    space_flag: u32,
) -> BackgroundAxisGeometry {
    use whisker_protocol::ImageRepeat;

    match repeat {
        ImageRepeat::Repeat | ImageRepeat::NoRepeat => {
            let origin = start + position.length + position.fraction * (area_size - image_size);
            BackgroundAxisGeometry {
                origin,
                tile_size: image_size,
                stride: image_size,
                flags: if repeat == ImageRepeat::NoRepeat {
                    no_repeat_flag
                } else {
                    0
                },
            }
        }
        ImageRepeat::Space => {
            let count = (area_size / image_size).floor();
            if count >= 2.0 {
                BackgroundAxisGeometry {
                    origin: start,
                    tile_size: image_size,
                    stride: (area_size - image_size) / (count - 1.0),
                    flags: space_flag,
                }
            } else {
                BackgroundAxisGeometry {
                    origin: start + position.length + position.fraction * (area_size - image_size),
                    tile_size: image_size,
                    stride: image_size,
                    flags: no_repeat_flag,
                }
            }
        }
        ImageRepeat::Round => {
            let count = (area_size / image_size).round().max(1.0);
            let tile_size = area_size / count;
            BackgroundAxisGeometry {
                origin: start + position.length + position.fraction * (area_size - tile_size),
                tile_size,
                stride: tile_size,
                flags: 0,
            }
        }
    }
}

pub(super) fn background_tile_geometry(
    positioning_rect: LayoutRect,
    layer: &whisker_protocol::BackgroundLayer,
    intrinsic_size: Option<[f32; 2]>,
) -> Option<BackgroundTileGeometry> {
    use whisker_protocol::{BackgroundSize, ImageRepeat};

    let resolve = |value: whisker_protocol::PaintLengthPercentage, extent: f32| {
        value.length + value.fraction * extent
    };
    let [mut width, mut height] = match layer.size {
        BackgroundSize::Auto => {
            intrinsic_size.unwrap_or([positioning_rect.width, positioning_rect.height])
        }
        BackgroundSize::Cover | BackgroundSize::Contain => {
            let intrinsic = intrinsic_size?;
            if intrinsic[0] <= 0.0 || intrinsic[1] <= 0.0 {
                return None;
            }
            let width_scale = positioning_rect.width / intrinsic[0];
            let height_scale = positioning_rect.height / intrinsic[1];
            let scale = if layer.size == BackgroundSize::Cover {
                width_scale.max(height_scale)
            } else {
                width_scale.min(height_scale)
            };
            [intrinsic[0] * scale, intrinsic[1] * scale]
        }
        BackgroundSize::Explicit { width, height } => {
            let explicit_width = width.map(|value| resolve(value, positioning_rect.width));
            let explicit_height = height.map(|value| resolve(value, positioning_rect.height));
            match (explicit_width, explicit_height, intrinsic_size) {
                (Some(width), Some(height), _) => [width, height],
                (Some(width), None, Some(intrinsic)) if intrinsic[0] > 0.0 => {
                    [width, width * intrinsic[1] / intrinsic[0]]
                }
                (None, Some(height), Some(intrinsic)) if intrinsic[1] > 0.0 => {
                    [height * intrinsic[0] / intrinsic[1], height]
                }
                (None, None, Some(intrinsic)) => intrinsic,
                _ => return None,
            }
        }
    };
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let height_tracks_rounded_width = matches!(
        layer.size,
        BackgroundSize::Explicit {
            width: Some(_),
            height: None
        }
    ) && layer.repeat_x == ImageRepeat::Round
        && layer.repeat_y != ImageRepeat::Round;
    let width_tracks_rounded_height = matches!(
        layer.size,
        BackgroundSize::Explicit {
            width: None,
            height: Some(_)
        }
    ) && layer.repeat_y == ImageRepeat::Round
        && layer.repeat_x != ImageRepeat::Round;

    let mut x = background_axis_geometry(
        positioning_rect.x,
        positioning_rect.width,
        width,
        layer.position.x,
        layer.repeat_x,
        1,
        4,
    );
    if height_tracks_rounded_width {
        height *= x.tile_size / width;
    }
    let y = background_axis_geometry(
        positioning_rect.y,
        positioning_rect.height,
        height,
        layer.position.y,
        layer.repeat_y,
        2,
        8,
    );
    if width_tracks_rounded_height {
        width *= y.tile_size / height;
        x = background_axis_geometry(
            positioning_rect.x,
            positioning_rect.width,
            width,
            layer.position.x,
            layer.repeat_x,
            1,
            4,
        );
    }
    Some(BackgroundTileGeometry {
        rect: LayoutRect {
            x: x.origin,
            y: y.origin,
            width: x.tile_size,
            height: y.tile_size,
        },
        stride: [x.stride, y.stride],
        domain: positioning_rect,
        flags: x.flags | y.flags,
    })
}

pub(crate) fn background_gradient_draw(
    positioning_rect: LayoutRect,
    layer: &whisker_protocol::BackgroundLayer,
    opacity: f32,
) -> Option<LinearGradientDraw> {
    let tile = background_tile_geometry(positioning_rect, layer, None)?;
    let mut gradient = match &layer.image {
        PaintImage::LinearGradient {
            angle_degrees,
            repeating,
            stops,
        } => linear_gradient_draw(tile.rect, *angle_degrees, *repeating, stops, opacity),
        PaintImage::RadialGradient {
            center,
            radii: Some(radii),
            stops,
            ..
        } => radial_gradient_draw(tile.rect, *center, *radii, stops, opacity),
        PaintImage::ConicGradient {
            from_degrees,
            center,
            repeating: false,
            stops,
        } => conic_gradient_draw(tile.rect, *from_degrees, *center, stops, opacity),
        _ => return None,
    };
    gradient.tile_stride = [tile.stride[0], tile.stride[1], 0.0, 0.0];
    gradient.tile_domain = [
        tile.domain.x,
        tile.domain.y,
        tile.domain.width,
        tile.domain.height,
    ];
    gradient.geometry_flags = tile.flags;
    Some(gradient)
}

pub(crate) fn background_resource_draw(
    positioning_rect: LayoutRect,
    layer: &whisker_protocol::BackgroundLayer,
    intrinsic_size: [f32; 2],
    opacity: f32,
) -> Option<(LinearGradientDraw, ResourceId)> {
    let PaintImage::Resource(resource) = &layer.image else {
        return None;
    };
    let tile = background_tile_geometry(positioning_rect, layer, Some(intrinsic_size))?;
    Some((
        LinearGradientDraw {
            start_end: [opacity, 0.0, 0.0, 0.0],
            tile_rect: [tile.rect.x, tile.rect.y, tile.rect.width, tile.rect.height],
            tile_stride: [tile.stride[0], tile.stride[1], 0.0, 0.0],
            tile_domain: [
                tile.domain.x,
                tile.domain.y,
                tile.domain.width,
                tile.domain.height,
            ],
            stops: Vec::new(),
            kind: 4,
            geometry_flags: tile.flags,
        },
        *resource,
    ))
}

pub(super) fn push_quad_draw(
    vertices: &mut Vec<BoxVertex>,
    draws: &mut Vec<DrawCommand>,
    primitive: BoxPrimitive,
    transform: Transform,
    clip: LogicalClip,
    shape_clips: &ShapeClipStack,
    background: (Option<LinearGradientDraw>, Option<ResourceId>, bool),
) {
    let (gradient, resource, pixelated) = background;
    let start = vertices.len() as u32;
    push_transformed_quad(vertices, primitive, transform);
    draws.push(DrawCommand::Quads {
        vertices: start..vertices.len() as u32,
        clip,
        shape_clips: shape_clips.clone(),
        gradient,
        resource,
        native: None,
        pixelated,
    });
}

pub(super) struct NativeRasterDraw<'a> {
    pub(super) node: NodeId,
    pub(super) rect: LayoutRect,
    pub(super) transform: Transform,
    pub(super) clip: LogicalClip,
    pub(super) shape_clips: &'a ShapeClipStack,
    pub(super) opacity: f32,
}

pub(super) fn push_native_raster_draw(
    vertices: &mut Vec<BoxVertex>,
    draws: &mut Vec<DrawCommand>,
    draw: NativeRasterDraw<'_>,
) {
    let NativeRasterDraw {
        node,
        rect,
        transform,
        clip,
        shape_clips,
        opacity,
    } = draw;
    let paint = whisker_protocol::BoxPaint::default();
    let primitive = background_gradient_primitive(rect, rect, &paint, PaintBox::Border);
    let start = vertices.len() as u32;
    push_transformed_quad(vertices, primitive, transform);
    draws.push(DrawCommand::Quads {
        vertices: start..vertices.len() as u32,
        clip,
        shape_clips: shape_clips.clone(),
        gradient: Some(LinearGradientDraw {
            start_end: [opacity, 0.0, 0.0, 0.0],
            tile_rect: [rect.x, rect.y, rect.width, rect.height],
            tile_stride: [rect.width, rect.height, 0.0, 0.0],
            tile_domain: [rect.x, rect.y, rect.width, rect.height],
            stops: Vec::new(),
            kind: 4,
            geometry_flags: 3,
        }),
        resource: None,
        native: Some(node),
        pixelated: false,
    });
}

pub(super) struct NativeRasterCache {
    pub(super) generation: u64,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) image: GpuImageResource,
}

pub(crate) struct GpuRenderer {
    pub(super) surface: Surface<'static>,
    pub(super) device: Device,
    pub(super) queue: Queue,
    pub(super) config: SurfaceConfiguration,
    pub(super) box_gpu: BoxGpuPipeline,
    pub(super) backdrop_gpu: BackdropGpuPipeline,
    pub(super) text_viewport: Viewport,
    pub(super) text_atlas: TextAtlas,
    pub(super) text_renderers: HashMap<NodeId, TextRenderer>,
    pub(super) image_resources: HashMap<ResourceId, GpuImageResource>,
    pub(super) native_rasters: HashMap<NodeId, NativeRasterCache>,
}

#[cfg(test)]
pub(super) fn push_quad(vertices: &mut Vec<BoxVertex>, primitive: BoxPrimitive) {
    push_transformed_quad(vertices, primitive, Transform::IDENTITY);
}

pub(super) fn push_transformed_quad(
    vertices: &mut Vec<BoxVertex>,
    primitive: BoxPrimitive,
    transform: Transform,
) {
    let rect = primitive.outer_rect;
    let left = rect.x;
    let top = rect.y;
    let right = rect.x + rect.width;
    let bottom = rect.y + rect.height;
    for position in [
        [left, top],
        [right, top],
        [left, bottom],
        [left, bottom],
        [right, top],
        [right, bottom],
    ] {
        vertices.push(BoxVertex {
            position,
            color: primitive.color,
            outer_rect: [rect.x, rect.y, rect.width, rect.height],
            outer_radii_x: primitive.outer_radii_x,
            outer_radii_y: primitive.outer_radii_y,
            inner_rect: [
                primitive.inner_rect.x,
                primitive.inner_rect.y,
                primitive.inner_rect.width,
                primitive.inner_rect.height,
            ],
            inner_radii_x: primitive.inner_radii_x,
            inner_radii_y: primitive.inner_radii_y,
            border_widths: primitive.border_widths,
            mode: primitive.kind.shader_mode(),
            border_colors: primitive.border_colors,
            border_styles: primitive.border_styles,
            transformed_position: transform_point(transform, position),
        });
    }
}

pub(super) fn transform_point(transform: Transform, [x, y]: [f32; 2]) -> [f32; 4] {
    [
        transform.0[0] * x + transform.0[4] * y + transform.0[12],
        transform.0[1] * x + transform.0[5] * y + transform.0[13],
        transform.0[2] * x + transform.0[6] * y + transform.0[14],
        transform.0[3] * x + transform.0[7] * y + transform.0[15],
    ]
}

pub(super) fn text_decoration_rects(
    left: f32,
    width: f32,
    y: f32,
    thickness: f32,
    style: whisker_protocol::TextDecorationStyle,
) -> Vec<LayoutRect> {
    let line = |x: f32, y: f32, width: f32| LayoutRect {
        x,
        y: y - thickness * 0.5,
        width: width.max(0.0),
        height: thickness,
    };
    match style {
        whisker_protocol::TextDecorationStyle::Solid => vec![line(left, y, width)],
        whisker_protocol::TextDecorationStyle::Double => vec![
            line(left, y - thickness, width),
            line(left, y + thickness, width),
        ],
        whisker_protocol::TextDecorationStyle::Dotted
        | whisker_protocol::TextDecorationStyle::Dashed => {
            let segment = if matches!(style, whisker_protocol::TextDecorationStyle::Dotted) {
                thickness
            } else {
                thickness * 4.0
            };
            let gap = thickness * 2.0;
            let mut result = Vec::new();
            let mut x = left;
            while x < left + width {
                result.push(line(x, y, segment.min(left + width - x)));
                x += segment + gap;
            }
            result
        }
        whisker_protocol::TextDecorationStyle::Wavy => {
            let segment = (thickness * 0.75).max(0.75);
            let wavelength = (thickness * 4.0).max(3.0);
            let mut result = Vec::new();
            let mut x = left;
            while x < left + width {
                let center = x + segment * 0.5;
                let phase = (center - left) / wavelength * std::f32::consts::TAU;
                result.push(line(
                    x,
                    y + phase.sin() * thickness,
                    (segment + thickness * 0.25).min(left + width - x),
                ));
                x += segment;
            }
            result
        }
    }
}

pub(super) fn text_bounds(clip: LogicalClip, width: u32, height: u32, scale: f32) -> TextBounds {
    TextBounds {
        left: (clip.left.unwrap_or(0.0) * scale).floor().max(0.0) as i32,
        top: (clip.top.unwrap_or(0.0) * scale).floor().max(0.0) as i32,
        right: (clip.right.unwrap_or(width as f32 / scale) * scale)
            .ceil()
            .clamp(0.0, width as f32) as i32,
        bottom: (clip.bottom.unwrap_or(height as f32 / scale) * scale)
            .ceil()
            .clamp(0.0, height as f32) as i32,
    }
}

#[cfg(all(test, feature = "host-conformance"))]
pub(crate) async fn render_box_primitives_offscreen(
    primitives: &[BoxPrimitive],
    logical_size: [u32; 2],
) -> Result<Vec<u8>, GpuError> {
    let clipped = primitives
        .iter()
        .copied()
        .map(|primitive| {
            (
                primitive,
                LogicalClip::default(),
                Transform::IDENTITY,
                ShapeClipStack::default(),
                None,
                None,
                false,
                Vec::new(),
            )
        })
        .collect::<Vec<_>>();
    render_clipped_box_primitives_offscreen(&clipped, logical_size, &HashMap::new()).await
}

#[cfg(all(test, feature = "host-conformance"))]
pub(crate) async fn render_clipped_box_primitives_offscreen(
    primitives: &[ClippedBoxPrimitive],
    logical_size: [u32; 2],
    resources: &HashMap<ResourceId, RasterResource>,
) -> Result<Vec<u8>, GpuError> {
    render_clipped_box_primitives_with_backdrops_offscreen(primitives, &[], logical_size, resources)
        .await
}

#[cfg(all(test, feature = "host-conformance"))]
pub(crate) async fn render_clipped_box_primitives_with_backdrops_offscreen(
    primitives: &[ClippedBoxPrimitive],
    backdrops: &[BackdropCheckpoint],
    logical_size: [u32; 2],
    resources: &HashMap<ResourceId, RasterResource>,
) -> Result<Vec<u8>, GpuError> {
    let [width, height] = logical_size;
    if width == 0 || height == 0 {
        return Err(GpuError(
            "offscreen checkpoint has an empty viewport".into(),
        ));
    }

    let instance = Instance::new(&InstanceDescriptor::default());
    let adapter = instance
        .request_adapter(&RequestAdapterOptions::default())
        .await
        .map_err(|error| GpuError(format!("select offscreen Desktop GPU adapter: {error}")))?;
    let (device, queue) = adapter
        .request_device(&DeviceDescriptor::default())
        .await
        .map_err(|error| GpuError(format!("create offscreen Desktop GPU device: {error}")))?;
    let format = TextureFormat::Rgba8Unorm;
    let box_gpu = BoxGpuPipeline::new(&device, &queue, format);
    let backdrop_gpu = BackdropGpuPipeline::new(&device, format);
    box_gpu.update_viewport(
        &queue,
        [width as f32, height as f32],
        [width as f32, height as f32],
    );

    let mut vertices = Vec::new();
    let mut draws = Vec::new();
    let mut active_opacity_groups = Vec::<(NodeId, f32)>::new();
    for (primitive, clip, transform, shape_clips, gradient, resource, pixelated, groups) in
        primitives
    {
        let shared = active_opacity_groups
            .iter()
            .zip(groups)
            .take_while(|(left, right)| left.0 == right.0)
            .count();
        for (node, _) in active_opacity_groups.drain(shared..).rev() {
            draws.push(DrawCommand::EndOpacityGroup { node });
        }
        for (node, opacity) in &groups[shared..] {
            draws.push(DrawCommand::BeginOpacityGroup {
                node: *node,
                opacity: *opacity,
            });
        }
        active_opacity_groups.extend_from_slice(&groups[shared..]);
        let start = vertices.len() as u32;
        push_transformed_quad(&mut vertices, *primitive, *transform);
        draws.push(DrawCommand::Quads {
            vertices: start..vertices.len() as u32,
            clip: *clip,
            shape_clips: shape_clips.clone(),
            gradient: gradient.clone(),
            resource: *resource,
            native: None,
            pixelated: *pixelated,
        });
    }
    for (node, _) in active_opacity_groups.into_iter().rev() {
        draws.push(DrawCommand::EndOpacityGroup { node });
    }
    let image_resources = resources
        .iter()
        .map(|(resource, raster)| (*resource, box_gpu.upload_image(&device, &queue, raster)))
        .collect::<HashMap<_, _>>();
    let vertex_buffer = (!vertices.is_empty()).then(|| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whisker Desktop conformance box vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: BufferUsages::VERTEX,
        })
    });
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("whisker Desktop conformance checkpoint"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT
            | TextureUsages::TEXTURE_BINDING
            | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor::default());
    let scratch = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("whisker Desktop conformance backdrop scratch"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let scratch_view = scratch.create_view(&TextureViewDescriptor::default());
    let mut opacity_depth = 0usize;
    let mut maximum_opacity_depth = 0usize;
    for draw in &draws {
        match draw {
            DrawCommand::BeginOpacityGroup { .. } => {
                opacity_depth += 1;
                maximum_opacity_depth = maximum_opacity_depth.max(opacity_depth);
            }
            DrawCommand::EndOpacityGroup { .. } => {
                opacity_depth = opacity_depth
                    .checked_sub(1)
                    .expect("conformance draw emits balanced opacity groups");
            }
            _ => {}
        }
    }
    assert_eq!(
        opacity_depth, 0,
        "conformance draw emits balanced opacity groups"
    );
    let opacity_textures = (0..maximum_opacity_depth)
        .map(|_| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("whisker Desktop conformance opacity group"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        })
        .collect::<Vec<_>>();
    let opacity_views = opacity_textures
        .iter()
        .map(|texture| texture.create_view(&TextureViewDescriptor::default()))
        .collect::<Vec<_>>();
    let unpadded_bytes_per_row = width * 4;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("whisker Desktop conformance readback"),
        size: u64::from(padded_bytes_per_row) * u64::from(height),
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("whisker Desktop conformance encoder"),
    });
    let shape_clip_bind_group = box_gpu.shape_clip_bind_group(&device, &draws);
    {
        encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("whisker Desktop conformance clear"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }
    if let Some(vertex_buffer) = &vertex_buffer {
        let mut active_opacity_groups = Vec::new();
        for (draw_index, draw) in draws.into_iter().enumerate() {
            match draw {
                DrawCommand::BeginOpacityGroup { node, opacity } => {
                    let target = &opacity_views[active_opacity_groups.len()];
                    let _clear = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("whisker Desktop conformance clear opacity group"),
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: target,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    active_opacity_groups.push((node, opacity));
                }
                DrawCommand::EndOpacityGroup { node } => {
                    let (open_node, opacity) = active_opacity_groups
                        .pop()
                        .expect("conformance draw emits balanced opacity groups");
                    assert_eq!(open_node, node, "opacity groups remain nested");
                    let source = &opacity_views[active_opacity_groups.len()];
                    let target = active_opacity_groups
                        .len()
                        .checked_sub(1)
                        .map_or(&view, |depth| &opacity_views[depth]);
                    let (_buffer, bind_group) = backdrop_gpu.bind_group(
                        &device,
                        source,
                        BackdropUniform {
                            direction: [0.0, 0.0],
                            radius: 0.0,
                            opacity,
                        },
                    );
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("whisker Desktop conformance composite opacity group"),
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: target,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Load,
                                store: StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    pass.set_pipeline(&backdrop_gpu.composite_pipeline);
                    pass.set_bind_group(0, &bind_group, &[]);
                    pass.draw(0..3, 0..1);
                }
                DrawCommand::Quads {
                    vertices: range,
                    clip,
                    resource,
                    pixelated,
                    ..
                } => {
                    let Some((x, y, clip_width, clip_height)) =
                        offscreen_scissor(clip, width, height)
                    else {
                        continue;
                    };
                    let target = active_opacity_groups
                        .len()
                        .checked_sub(1)
                        .map_or(&view, |depth| &opacity_views[depth]);
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("whisker Desktop conformance boxes"),
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: target,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Load,
                                store: StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    pass.set_scissor_rect(x, y, clip_width, clip_height);
                    pass.set_pipeline(&box_gpu.pipeline);
                    pass.set_bind_group(0, &box_gpu.viewport_bind_group, &[]);
                    pass.set_bind_group(1, &shape_clip_bind_group, &[]);
                    let image = resource
                        .and_then(|resource| image_resources.get(&resource))
                        .unwrap_or(&box_gpu.fallback_image);
                    pass.set_bind_group(
                        2,
                        if pixelated {
                            &image.nearest_bind_group
                        } else {
                            &image.linear_bind_group
                        },
                        &[],
                    );
                    pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                    pass.draw(range, draw_index as u32..draw_index as u32 + 1);
                }
                DrawCommand::Text { .. } | DrawCommand::BackdropBlur { .. } => unreachable!(),
            }
        }
        assert!(
            active_opacity_groups.is_empty(),
            "conformance draw emits balanced opacity groups"
        );
    }
    for (rect, radius, clip) in backdrops {
        let (_horizontal_buffer, horizontal_group) = backdrop_gpu.bind_group(
            &device,
            &view,
            BackdropUniform {
                direction: [1.0, 0.0],
                radius: *radius,
                opacity: 1.0,
            },
        );
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("whisker Desktop conformance horizontal backdrop blur"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &scratch_view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&backdrop_gpu.pipeline);
            pass.set_bind_group(0, &horizontal_group, &[]);
            pass.draw(0..3, 0..1);
        }
        let (_vertical_buffer, vertical_group) = backdrop_gpu.bind_group(
            &device,
            &scratch_view,
            BackdropUniform {
                direction: [0.0, 1.0],
                radius: *radius,
                opacity: 1.0,
            },
        );
        let Some((x, y, clip_width, clip_height)) =
            offscreen_scissor(clip.intersect(*rect, true, true), width, height)
        else {
            continue;
        };
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("whisker Desktop conformance vertical backdrop blur"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_scissor_rect(x, y, clip_width, clip_height);
        pass.set_pipeline(&backdrop_gpu.pipeline);
        pass.set_bind_group(0, &vertical_group, &[]);
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::wait())
        .map_err(|error| GpuError(format!("wait for Desktop GPU checkpoint: {error}")))?;
    receiver
        .recv()
        .map_err(|error| GpuError(format!("receive Desktop GPU checkpoint: {error}")))?
        .map_err(|error| GpuError(format!("map Desktop GPU checkpoint: {error}")))?;

    let mapped = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for row in mapped.chunks_exact(padded_bytes_per_row as usize) {
        pixels.extend_from_slice(&row[..unpadded_bytes_per_row as usize]);
    }
    drop(mapped);
    readback.unmap();
    Ok(pixels)
}

#[cfg(all(test, feature = "host-conformance"))]
pub(super) fn offscreen_scissor(
    clip: LogicalClip,
    width: u32,
    height: u32,
) -> Option<(u32, u32, u32, u32)> {
    let left = clip.left.unwrap_or(0.0).floor().max(0.0) as u32;
    let top = clip.top.unwrap_or(0.0).floor().max(0.0) as u32;
    let right = clip
        .right
        .unwrap_or(width as f32)
        .ceil()
        .clamp(0.0, width as f32) as u32;
    let bottom = clip
        .bottom
        .unwrap_or(height as f32)
        .ceil()
        .clamp(0.0, height as f32) as u32;
    (right > left && bottom > top).then_some((left, top, right - left, bottom - top))
}
