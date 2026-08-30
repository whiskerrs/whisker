use super::*;

impl GpuRenderer {
    pub(crate) async fn new(
        target: impl Into<SurfaceTarget<'static>>,
        physical_size: [u32; 2],
    ) -> Result<Self, GpuError> {
        let instance = Instance::new(&InstanceDescriptor::default());
        let surface = instance
            .create_surface(target)
            .map_err(|error| GpuError(format!("create Desktop GPU surface: {error}")))?;
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..RequestAdapterOptions::default()
            })
            .await
            .map_err(|error| GpuError(format!("select Desktop GPU adapter: {error}")))?;
        let (device, queue) = adapter
            .request_device(&DeviceDescriptor::default())
            .await
            .map_err(|error| GpuError(format!("create Desktop GPU device: {error}")))?;
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| !format.is_srgb())
            .ok_or_else(|| {
                GpuError(
                    "Desktop GPU surface exposes no non-sRGB format for CSS compositing".into(),
                )
            })?;
        let present_mode = capabilities
            .present_modes
            .iter()
            .copied()
            .find(|mode| *mode == PresentMode::Fifo)
            .unwrap_or(PresentMode::AutoVsync);
        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            width: physical_size[0].max(1),
            height: physical_size[1].max(1),
            present_mode,
            alpha_mode: CompositeAlphaMode::Opaque,
            view_formats: Vec::new(),
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let box_gpu = BoxGpuPipeline::new(&device, &queue, format);
        let backdrop_gpu = BackdropGpuPipeline::new(&device, format);

        let text_cache = Cache::new(&device);
        let text_viewport = Viewport::new(&device, &text_cache);
        let text_atlas = TextAtlas::new(&device, &queue, &text_cache, format);
        Ok(Self {
            surface,
            device,
            queue,
            config,
            box_gpu,
            backdrop_gpu,
            text_viewport,
            text_atlas,
            text_renderers: HashMap::new(),
            image_resources: HashMap::new(),
            native_rasters: HashMap::new(),
        })
    }

    pub(crate) fn register_raster_resource(
        &mut self,
        resource: ResourceId,
        raster: &RasterResource,
    ) {
        let image = self.box_gpu.upload_image(&self.device, &self.queue, raster);
        self.image_resources.insert(resource, image);
    }

    pub(crate) fn release_raster_resource(&mut self, resource: ResourceId) {
        self.image_resources.remove(&resource);
    }

    pub(crate) fn resize(&mut self, physical_size: [u32; 2]) {
        if physical_size[0] == 0 || physical_size[1] == 0 {
            return;
        }
        self.config.width = physical_size[0];
        self.config.height = physical_size[1];
        self.surface.configure(&self.device, &self.config);
    }

    pub(crate) fn render(
        &mut self,
        commands: &[PaintCommand<'_>],
        text: &mut NativeTextHost,
        logical_size: [f32; 2],
        scale: f32,
    ) -> Result<(), GpuError> {
        if self.config.width == 0 || self.config.height == 0 {
            return Ok(());
        }
        self.box_gpu.update_viewport(
            &self.queue,
            logical_size,
            [self.config.width as f32, self.config.height as f32],
        );
        self.text_viewport.update(
            &self.queue,
            Resolution {
                width: self.config.width,
                height: self.config.height,
            },
        );

        let mut vertices = Vec::new();
        let mut draws = Vec::new();
        for (index, command) in commands.iter().enumerate() {
            match command {
                PaintCommand::BeginOpacityGroup { node, opacity } => {
                    draws.push(DrawCommand::BeginOpacityGroup {
                        node: *node,
                        opacity: *opacity,
                    });
                }
                PaintCommand::EndOpacityGroup { node } => {
                    draws.push(DrawCommand::EndOpacityGroup { node: *node });
                }
                PaintCommand::BackdropBlur { rect, radius, clip } => {
                    draws.push(DrawCommand::BackdropBlur {
                        rect: *rect,
                        radius: *radius,
                        clip: *clip,
                    });
                }
                PaintCommand::Box {
                    rect,
                    content_rect,
                    paint,
                    background_layers,
                    visual_effects,
                    clip,
                    shape_clips,
                    transform,
                    opacity,
                } => {
                    let default_paint = whisker_protocol::BoxPaint::default();
                    let paint = paint.unwrap_or(&default_paint);
                    for shadow in visual_effects
                        .box_shadows
                        .iter()
                        .rev()
                        .filter(|shadow| !shadow.inset)
                    {
                        if let Some(primitive) =
                            box_shadow_primitive(*rect, paint, shadow, *opacity)
                        {
                            push_quad_draw(
                                &mut vertices,
                                &mut draws,
                                primitive,
                                *transform,
                                *clip,
                                shape_clips,
                                (None, None, false),
                            );
                        }
                    }
                    let mut box_primitives = Vec::new();
                    lower_box(*rect, paint, *opacity, |primitive| {
                        box_primitives.push(primitive)
                    });
                    for primitive in box_primitives
                        .iter()
                        .copied()
                        .filter(|primitive| primitive.kind == BoxPrimitiveKind::Fill)
                    {
                        push_quad_draw(
                            &mut vertices,
                            &mut draws,
                            primitive,
                            *transform,
                            *clip,
                            shape_clips,
                            (None, None, false),
                        );
                    }
                    let box_geometry = resolve_box_geometry(*rect, paint);
                    for layer in background_layers.iter().rev() {
                        let positioning_rect = match layer.origin {
                            PaintBox::Border => box_geometry.outer_rect,
                            PaintBox::Padding => box_geometry.inner_rect,
                            PaintBox::Content => *content_rect,
                            _ => continue,
                        };
                        let (gradient, resource) = match &layer.image {
                            PaintImage::Resource(resource) => {
                                let Some(image) = self.image_resources.get(resource) else {
                                    continue;
                                };
                                let Some((draw, resource)) = background_resource_draw(
                                    positioning_rect,
                                    layer,
                                    image.intrinsic_size,
                                    *opacity,
                                ) else {
                                    continue;
                                };
                                (draw, Some(resource))
                            }
                            _ => {
                                let Some(draw) =
                                    background_gradient_draw(positioning_rect, layer, *opacity)
                                else {
                                    continue;
                                };
                                (draw, None)
                            }
                        };
                        push_quad_draw(
                            &mut vertices,
                            &mut draws,
                            background_gradient_primitive(*rect, *content_rect, paint, layer.clip),
                            *transform,
                            *clip,
                            shape_clips,
                            (
                                Some(gradient),
                                resource,
                                resource.is_some()
                                    && matches!(
                                        visual_effects.image_rendering,
                                        whisker_protocol::ImageRendering::Pixelated
                                            | whisker_protocol::ImageRendering::CrispEdges
                                    ),
                            ),
                        );
                    }
                    for shadow in visual_effects
                        .box_shadows
                        .iter()
                        .rev()
                        .filter(|shadow| shadow.inset)
                    {
                        if let Some(primitive) =
                            box_shadow_primitive(*rect, paint, shadow, *opacity)
                        {
                            push_quad_draw(
                                &mut vertices,
                                &mut draws,
                                primitive,
                                *transform,
                                *clip,
                                shape_clips,
                                (None, None, false),
                            );
                        }
                    }
                    for primitive in box_primitives
                        .into_iter()
                        .filter(|primitive| primitive.kind == BoxPrimitiveKind::Border)
                    {
                        push_quad_draw(
                            &mut vertices,
                            &mut draws,
                            primitive,
                            *transform,
                            *clip,
                            shape_clips,
                            (None, None, false),
                        );
                    }
                }
                PaintCommand::Text {
                    node,
                    rect,
                    content,
                    clip,
                    transform,
                    shape_clips,
                    opacity,
                    ..
                } => {
                    let decoration = &content.paint.decoration;
                    if (decoration.lines.underline || decoration.lines.line_through)
                        && let Some(prepared) = content
                            .prepared_content
                            .and_then(|id| text.prepared.get(&id))
                    {
                        let thickness = (content.payload.style.font_size / 16.0).max(1.0);
                        for run in prepared.buffer.layout_runs() {
                            let Some(line_x) =
                                run.glyphs.iter().map(|glyph| glyph.x).reduce(f32::min)
                            else {
                                continue;
                            };
                            let baseline = rect.y + run.line_y;
                            let y = if decoration.lines.underline {
                                baseline + thickness * 1.5
                            } else {
                                baseline - content.payload.style.font_size * 0.3
                            };
                            for line in text_decoration_rects(
                                rect.x + line_x,
                                run.line_w,
                                y,
                                thickness,
                                decoration.style,
                            ) {
                                push_quad_draw(
                                    &mut vertices,
                                    &mut draws,
                                    solid_rect_primitive(
                                        line,
                                        gpu_color(&decoration.color, *opacity),
                                    ),
                                    *transform,
                                    *clip,
                                    shape_clips,
                                    (None, None, false),
                                );
                            }
                        }
                    }
                    draws.push(DrawCommand::Text { index, node: *node })
                }
                PaintCommand::Raster {
                    node,
                    rect,
                    rasterizer,
                    clip,
                    shape_clips,
                    transform,
                    opacity,
                } => {
                    let width = (rect.width * scale).ceil().max(1.0) as u32;
                    let height = (rect.height * scale).ceil().max(1.0) as u32;
                    let Some(raster) = rasterizer.rasterize(width, height) else {
                        self.native_rasters.remove(node);
                        continue;
                    };
                    let stale = self.native_rasters.get(node).is_none_or(|cached| {
                        cached.generation != raster.generation()
                            || cached.width != raster.width()
                            || cached.height != raster.height()
                    });
                    if stale {
                        let upload = RasterResource::new(
                            raster.width(),
                            raster.height(),
                            raster.pixels().to_vec(),
                        )?;
                        let image = self
                            .box_gpu
                            .upload_image(&self.device, &self.queue, &upload);
                        self.native_rasters.insert(
                            *node,
                            NativeRasterCache {
                                generation: raster.generation(),
                                width: raster.width(),
                                height: raster.height(),
                                image,
                            },
                        );
                    }
                    push_native_raster_draw(
                        &mut vertices,
                        &mut draws,
                        NativeRasterDraw {
                            node: *node,
                            rect: *rect,
                            transform: *transform,
                            clip: *clip,
                            shape_clips,
                            opacity: *opacity,
                        },
                    );
                }
            }
        }
        let live_native_nodes = draws
            .iter()
            .filter_map(|draw| match draw {
                DrawCommand::Quads {
                    native: Some(node), ..
                } => Some(*node),
                _ => None,
            })
            .collect::<HashSet<_>>();
        self.native_rasters
            .retain(|node, _| live_native_nodes.contains(node));
        let live_text_nodes = draws
            .iter()
            .filter_map(|draw| match draw {
                DrawCommand::Text { node, .. } => Some(*node),
                DrawCommand::BeginOpacityGroup { .. }
                | DrawCommand::EndOpacityGroup { .. }
                | DrawCommand::Quads { .. }
                | DrawCommand::BackdropBlur { .. } => None,
            })
            .collect::<HashSet<_>>();
        self.text_renderers
            .retain(|node, _| live_text_nodes.contains(node));
        let mut prepared_text_nodes = HashSet::new();
        for draw in &draws {
            let DrawCommand::Text { index, node } = draw else {
                continue;
            };
            let PaintCommand::Text {
                rect,
                content,
                clip,
                opacity,
                ..
            } = &commands[*index]
            else {
                unreachable!();
            };
            let Some(id) = content.prepared_content else {
                continue;
            };
            let Some(prepared) = text.prepared.get(&id) else {
                continue;
            };
            if !self.text_renderers.contains_key(node) {
                self.text_renderers.insert(
                    *node,
                    TextRenderer::new(
                        &mut self.text_atlas,
                        &self.device,
                        MultisampleState::default(),
                        None,
                    ),
                );
            }
            let bounds = text_bounds(*clip, self.config.width, self.config.height, scale);
            let color = text_color(&content.paint.foreground, *opacity);
            let mut areas = Vec::with_capacity(content.paint.shadows.len() + 1);
            for shadow in &content.paint.shadows {
                if shadow.blur_radius <= 0.0 {
                    let shadow_color = text_color(&shadow.color, *opacity);
                    areas.push(TextArea {
                        buffer: &prepared.buffer,
                        left: (rect.x + shadow.offset_x) * scale,
                        top: (rect.y + shadow.offset_y) * scale,
                        scale,
                        bounds,
                        default_color: shadow_color,
                        custom_glyphs: &[],
                    });
                } else {
                    // Glyphon does not expose a blur primitive. Approximate the
                    // single Lynx shadow with a compact, normalized sample disk.
                    let radius = shadow.blur_radius.min(12.0);
                    let offsets = [
                        (0.0, 0.0),
                        (-radius, 0.0),
                        (radius, 0.0),
                        (0.0, -radius),
                        (0.0, radius),
                        (-radius * 0.7, -radius * 0.7),
                        (radius * 0.7, -radius * 0.7),
                        (-radius * 0.7, radius * 0.7),
                        (radius * 0.7, radius * 0.7),
                    ];
                    let sampled_color = text_color(&shadow.color, *opacity / offsets.len() as f32);
                    for (x, y) in offsets {
                        areas.push(TextArea {
                            buffer: &prepared.buffer,
                            left: (rect.x + shadow.offset_x + x) * scale,
                            top: (rect.y + shadow.offset_y + y) * scale,
                            scale,
                            bounds,
                            default_color: sampled_color,
                            custom_glyphs: &[],
                        });
                    }
                }
            }
            areas.push(TextArea {
                buffer: &prepared.buffer,
                left: rect.x * scale,
                top: rect.y * scale,
                scale,
                bounds,
                default_color: color,
                custom_glyphs: &[],
            });
            self.text_renderers
                .get_mut(node)
                .expect("renderer inserted for live text node")
                .prepare(
                    &self.device,
                    &self.queue,
                    &mut text.font_system,
                    &mut self.text_atlas,
                    &self.text_viewport,
                    areas,
                    &mut text.swash_cache,
                )
                .map_err(|error| GpuError(format!("prepare glyph atlas: {error}")))?;
            prepared_text_nodes.insert(*node);
        }
        let vertex_buffer = (!vertices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("whisker Desktop box vertices"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: BufferUsages::VERTEX,
                })
        });
        let shape_clip_bind_group = self.box_gpu.shape_clip_bind_group(&self.device, &draws);

        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(SurfaceError::Lost | SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                self.surface.get_current_texture().map_err(|error| {
                    GpuError(format!("acquire Desktop GPU frame after recovery: {error}"))
                })?
            }
            Err(SurfaceError::Timeout) => return Ok(()),
            Err(error) => return Err(GpuError(format!("acquire Desktop GPU frame: {error}"))),
        };
        let view = frame.texture.create_view(&TextureViewDescriptor::default());
        let scene_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("whisker Desktop composited scene"),
            size: wgpu::Extent3d {
                width: self.config.width,
                height: self.config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let scratch_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("whisker Desktop backdrop scratch"),
            size: wgpu::Extent3d {
                width: self.config.width,
                height: self.config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let scene_view = scene_texture.create_view(&TextureViewDescriptor::default());
        let scratch_view = scratch_texture.create_view(&TextureViewDescriptor::default());
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
                        .expect("scene emits balanced opacity groups");
                }
                _ => {}
            }
        }
        assert_eq!(opacity_depth, 0, "scene emits balanced opacity groups");
        let opacity_textures = (0..maximum_opacity_depth)
            .map(|_| {
                self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("whisker Desktop opacity group"),
                    size: wgpu::Extent3d {
                        width: self.config.width,
                        height: self.config.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.config.format,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
            })
            .collect::<Vec<_>>();
        let opacity_views = opacity_textures
            .iter()
            .map(|texture| texture.create_view(&TextureViewDescriptor::default()))
            .collect::<Vec<_>>();
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("whisker Desktop frame"),
            });
        {
            let _clear = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("whisker Desktop clear"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &scene_view,
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

        let mut active_opacity_groups = Vec::new();
        for (draw_index, draw) in draws.into_iter().enumerate() {
            match draw {
                DrawCommand::BeginOpacityGroup { node, opacity } => {
                    let view = &opacity_views[active_opacity_groups.len()];
                    let _clear = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("whisker Desktop clear opacity group"),
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view,
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
                        .expect("scene emits balanced opacity groups");
                    assert_eq!(open_node, node, "scene opacity groups remain nested");
                    let source = &opacity_views[active_opacity_groups.len()];
                    let target = active_opacity_groups
                        .len()
                        .checked_sub(1)
                        .map_or(&scene_view, |depth| &opacity_views[depth]);
                    let (_buffer, bind_group) = self.backdrop_gpu.bind_group(
                        &self.device,
                        source,
                        BackdropUniform {
                            direction: [0.0, 0.0],
                            radius: 0.0,
                            opacity,
                        },
                    );
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("whisker Desktop composite opacity group"),
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
                    pass.set_pipeline(&self.backdrop_gpu.composite_pipeline);
                    pass.set_bind_group(0, &bind_group, &[]);
                    pass.draw(0..3, 0..1);
                }
                DrawCommand::BackdropBlur { rect, radius, clip } => {
                    let target = active_opacity_groups
                        .len()
                        .checked_sub(1)
                        .map_or(&scene_view, |depth| &opacity_views[depth]);
                    let horizontal = BackdropUniform {
                        direction: [1.0, 0.0],
                        radius: radius * scale,
                        opacity: 1.0,
                    };
                    let (_horizontal_buffer, horizontal_group) =
                        self.backdrop_gpu
                            .bind_group(&self.device, target, horizontal);
                    {
                        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                            label: Some("whisker Desktop backdrop horizontal blur"),
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
                        pass.set_pipeline(&self.backdrop_gpu.pipeline);
                        pass.set_bind_group(0, &horizontal_group, &[]);
                        pass.draw(0..3, 0..1);
                    }
                    let vertical = BackdropUniform {
                        direction: [0.0, 1.0],
                        radius: radius * scale,
                        opacity: 1.0,
                    };
                    let (_vertical_buffer, vertical_group) =
                        self.backdrop_gpu
                            .bind_group(&self.device, &scratch_view, vertical);
                    let Some((x, y, width, height)) =
                        self.scissor(clip.intersect(rect, true, true), scale)
                    else {
                        continue;
                    };
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("whisker Desktop backdrop vertical blur"),
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
                    pass.set_scissor_rect(x, y, width, height);
                    pass.set_pipeline(&self.backdrop_gpu.pipeline);
                    pass.set_bind_group(0, &vertical_group, &[]);
                    pass.draw(0..3, 0..1);
                }
                DrawCommand::Quads {
                    vertices,
                    clip,
                    resource,
                    native,
                    pixelated,
                    ..
                } => {
                    let target = active_opacity_groups
                        .len()
                        .checked_sub(1)
                        .map_or(&scene_view, |depth| &opacity_views[depth]);
                    let Some((x, y, width, height)) = self.scissor(clip, scale) else {
                        continue;
                    };
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("whisker Desktop boxes"),
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
                    pass.set_scissor_rect(x, y, width, height);
                    pass.set_pipeline(&self.box_gpu.pipeline);
                    pass.set_bind_group(0, &self.box_gpu.viewport_bind_group, &[]);
                    pass.set_bind_group(1, &shape_clip_bind_group, &[]);
                    let image = native
                        .and_then(|node| self.native_rasters.get(&node).map(|entry| &entry.image))
                        .or_else(|| {
                            resource.and_then(|resource| self.image_resources.get(&resource))
                        })
                        .unwrap_or(&self.box_gpu.fallback_image);
                    pass.set_bind_group(
                        2,
                        if pixelated {
                            &image.nearest_bind_group
                        } else {
                            &image.linear_bind_group
                        },
                        &[],
                    );
                    pass.set_vertex_buffer(
                        0,
                        vertex_buffer
                            .as_ref()
                            .expect("quad draw has vertices")
                            .slice(..),
                    );
                    pass.draw(vertices, draw_index as u32..draw_index as u32 + 1);
                }
                DrawCommand::Text { node, .. } => {
                    if !prepared_text_nodes.contains(&node) {
                        continue;
                    }
                    let target = active_opacity_groups
                        .len()
                        .checked_sub(1)
                        .map_or(&scene_view, |depth| &opacity_views[depth]);
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("whisker Desktop text"),
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
                    self.text_renderers
                        .get(&node)
                        .expect("prepared text renderer remains retained")
                        .render(&self.text_atlas, &self.text_viewport, &mut pass)
                        .map_err(|error| GpuError(format!("encode glyph draw: {error}")))?;
                }
            }
        }
        assert!(
            active_opacity_groups.is_empty(),
            "scene emits balanced opacity groups"
        );
        let present_uniform = BackdropUniform {
            direction: [0.0, 0.0],
            radius: 0.0,
            opacity: 1.0,
        };
        let (_present_buffer, present_group) =
            self.backdrop_gpu
                .bind_group(&self.device, &scene_view, present_uniform);
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("whisker Desktop present composited scene"),
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
            pass.set_pipeline(&self.backdrop_gpu.pipeline);
            pass.set_bind_group(0, &present_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        self.text_atlas.trim();
        Ok(())
    }

    fn scissor(&self, clip: LogicalClip, scale: f32) -> Option<(u32, u32, u32, u32)> {
        let left = (clip.left.unwrap_or(0.0) * scale).floor().max(0.0) as u32;
        let top = (clip.top.unwrap_or(0.0) * scale).floor().max(0.0) as u32;
        let right = (clip.right.unwrap_or(self.config.width as f32 / scale) * scale)
            .ceil()
            .clamp(0.0, self.config.width as f32) as u32;
        let bottom = (clip.bottom.unwrap_or(self.config.height as f32 / scale) * scale)
            .ceil()
            .clamp(0.0, self.config.height as f32) as u32;
        if right > left && bottom > top {
            Some((left, top, right - left, bottom - top))
        } else {
            None
        }
    }
}
