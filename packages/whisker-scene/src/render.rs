//! A forward renderer that draws one [`Scene`] into one surface.

mod collect;
mod pipeline;
mod post;
mod resources;
mod shadow;
mod visibility;

use std::rc::Rc;

use glam::{Mat4, Vec3, Vec4};
use whisker::css::Color;
use whisker_surface::{FrameInfo, SurfaceSize};
use whisker_wgpu::{Gpu, Renderer, wgpu};

pub(crate) use collect::{Collected, Draw, camera_for, collect};
pub(crate) use pipeline::translucent;
pub(crate) use resources::{DeformBuffer, GpuMesh, GpuTexture};

use crate::core::{CUBE_LEVELS, Environment, Fog, Scene, SceneInner, linear_rgba, srgb_rgba};
use collect::{pick_camera, radiance};
use pipeline::{
    DirectionalUniform, DrawUniform, FrameUniform, PipelineKey, PointUniform, Shared, double_sided,
    draw_buffer, draw_uniform, shared,
};
use post::PostTargets;
use resources::{MaterialCache, deformed_vertices, environment_cube, mesh};
use shadow::{Eye, MAX_SHADOW_LAYERS, ShadowMaps, ShadowPlan};
use visibility::{Frustum, Ordered, draw_order};

const MAX_DIRECTIONAL: usize = 4;
const MAX_POINT: usize = 16;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24Plus;
const MSAA_SAMPLES: u32 = 4;

struct Attachments {
    size: (u32, u32),
    format: wgpu::TextureFormat,
    samples: u32,
    depth: wgpu::TextureView,
    msaa: Option<wgpu::TextureView>,
}

pub(crate) struct SceneRenderer {
    scene: Scene,
    shared: Rc<Shared>,
    format: wgpu::TextureFormat,
    samples: u32,
    encode_srgb: bool,
    frame_buffer: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    draw_buffer: wgpu::Buffer,
    draw_bind_group: wgpu::BindGroup,
    draw_stride: u64,
    draw_capacity: u64,
    staging: Vec<u8>,
    size: (u32, u32),
    attachments: Option<Attachments>,
    materials: MaterialCache,
    shadow_maps: ShadowMaps,
    shadow_view_buffer: wgpu::Buffer,
    shadow_view_bind_group: wgpu::BindGroup,
    shadow_view_stride: u64,
    bound_environment: Option<Environment>,
    post: Option<PostTargets>,
}

impl SceneRenderer {
    pub(crate) fn new(gpu: &Gpu, format: wgpu::TextureFormat, scene: Scene) -> SceneRenderer {
        let shared = shared(gpu);
        let multisample = |format: wgpu::TextureFormat| {
            gpu.adapter
                .get_texture_format_features(format)
                .flags
                .sample_count_supported(MSAA_SAMPLES)
        };
        let samples = if multisample(format) && multisample(DEPTH_FORMAT) {
            MSAA_SAMPLES
        } else {
            1
        };
        let frame_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("whisker-scene frame"),
            size: size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shadow_maps = ShadowMaps::new(gpu, 1, 1);
        let frame_bind_group = frame_bind_group(
            gpu,
            &shared,
            &frame_buffer,
            &shadow_maps,
            &shared.no_environment,
        );
        let alignment = u64::from(gpu.device.limits().min_uniform_buffer_offset_alignment);
        let draw_stride = (size_of::<DrawUniform>() as u64).next_multiple_of(alignment);
        let (draw_buffer, draw_bind_group) = draw_buffer(gpu, &shared, draw_stride, 16);
        let shadow_view_stride = (size_of::<Mat4>() as u64).next_multiple_of(alignment);
        let shadow_view_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("whisker-scene shadow views"),
            // One slot past the shadow layers holds the camera, for the depth prepass.
            size: shadow_view_stride * (MAX_SHADOW_LAYERS as u64 + 1),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shadow_view_bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("whisker-scene shadow views"),
            layout: &shared.shadow_view_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &shadow_view_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(size_of::<Mat4>() as u64),
                }),
            }],
        });
        SceneRenderer {
            scene,
            shared,
            format,
            samples,
            encode_srgb: !format.is_srgb(),
            frame_buffer,
            frame_bind_group,
            draw_buffer,
            draw_bind_group,
            draw_stride,
            draw_capacity: 16,
            staging: Vec::new(),
            size: (1, 1),
            attachments: None,
            materials: MaterialCache::default(),
            shadow_maps,
            shadow_view_buffer,
            shadow_view_bind_group,
            shadow_view_stride,
            bound_environment: None,
            post: None,
        }
    }

    /// Rebuilds the frame bind group when the shadow maps or the environment it binds change.
    fn prepare_frame_bindings(
        &mut self,
        gpu: &Gpu,
        plan: &ShadowPlan,
        environment: Option<&Environment>,
    ) {
        let maps_fit = self.shadow_maps.fits(plan);
        if maps_fit && self.bound_environment.as_ref() == environment {
            return;
        }
        if !maps_fit {
            self.shadow_maps = ShadowMaps::new(gpu, plan.size, plan.layers.len());
        }
        self.bound_environment = environment.cloned();
        let cube = match environment {
            Some(environment) => environment_cube(gpu, environment),
            None => &self.shared.no_environment,
        };
        self.frame_bind_group = frame_bind_group(
            gpu,
            &self.shared,
            &self.frame_buffer,
            &self.shadow_maps,
            cube,
        );
    }

    fn prepare_attachments(&mut self, gpu: &Gpu, format: wgpu::TextureFormat, samples: u32) {
        if self
            .attachments
            .as_ref()
            .is_some_and(|a| a.size != self.size || a.format != format || a.samples != samples)
        {
            self.attachments = None;
        }
        let (width, height) = self.size;
        self.attachments.get_or_insert_with(|| {
            let texture = |label, format| {
                gpu.device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: samples,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            };
            Attachments {
                size: (width, height),
                format,
                samples,
                depth: texture("whisker-scene depth", DEPTH_FORMAT),
                msaa: (samples > 1).then(|| texture("whisker-scene msaa", format)),
            }
        });
    }
}

impl Renderer for SceneRenderer {
    fn resize(&mut self, _gpu: &Gpu, size: SurfaceSize) {
        self.size = (size.width.max(1), size.height.max(1));
    }

    fn render(&mut self, gpu: &Gpu, target: &wgpu::TextureView, frame: FrameInfo) {
        let scene = self.scene.clone();
        let inner = scene.inner();
        inner.run_frame_callbacks(&frame);
        let collected = collect(inner.root());
        for draw in &collected.draws {
            if let Some(deform) = &draw.deform {
                deform.update(&draw.geometry);
            }
        }
        let (camera_world, projection) = pick_camera(inner.active_camera(), &collected.cameras);
        let aspect = self.size.0 as f32 / self.size.1 as f32;
        let eye = Eye {
            world: camera_world,
            projection,
            aspect,
        };
        let plan = shadow::plan(
            &collected,
            eye,
            gpu.device.limits().max_texture_dimension_2d,
        );
        let environment = inner.environment();
        self.prepare_frame_bindings(gpu, &plan, environment.as_ref().map(|(e, _)| e));
        let sky = environment.is_some() && inner.environment_background();
        let view = camera_world.inverse();
        let view_proj = projection.to_matrix(aspect) * view;
        self.write_frame_uniform(gpu, &collected, &plan, view_proj, eye, inner);
        let order = draw_order(&collected.draws, view, &Frustum::new(view_proj));
        self.write_draw_uniforms(gpu, &collected.draws);

        let backdrop = Backdrop {
            clear: self.clear_color(inner.background()),
            sky,
        };
        let settings = inner.post_processing();
        if !settings.active() {
            self.post = None;
        } else if self
            .post
            .as_ref()
            .is_none_or(|post| post.size() != self.size)
        {
            self.post = Some(PostTargets::new(gpu, self.size));
        }
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("whisker-scene"),
            });
        self.encode_shadows(gpu, &mut encoder, &collected.draws, &plan);
        match self
            .post
            .as_ref()
            .map(|post| (post.scene.clone(), post.format, post.samples))
        {
            None => {
                self.prepare_attachments(gpu, self.format, self.samples);
                let target = Target {
                    view: target,
                    format: self.format,
                    samples: self.samples,
                };
                self.encode_forward(
                    gpu,
                    &mut encoder,
                    target,
                    &collected.draws,
                    &order,
                    backdrop,
                );
            }
            Some((scene_view, format, samples)) => {
                self.prepare_attachments(gpu, format, samples);
                let scene_target = Target {
                    view: &scene_view,
                    format,
                    samples,
                };
                self.encode_forward(
                    gpu,
                    &mut encoder,
                    scene_target,
                    &collected.draws,
                    &order,
                    backdrop,
                );
                if settings.ambient_occlusion.is_some() || settings.depth_of_field.is_some() {
                    self.encode_depth_prepass(
                        gpu,
                        &mut encoder,
                        &collected.draws,
                        &order,
                        view_proj,
                    );
                }
                let projection = projection.to_matrix(aspect);
                self.encode_post(
                    gpu,
                    &mut encoder,
                    target,
                    &settings,
                    projection,
                    inner.tone_mapping(),
                );
            }
        }
        gpu.queue.submit([encoder.finish()]);
        self.materials.sweep();
    }
}

impl SceneRenderer {
    fn write_frame_uniform(
        &self,
        gpu: &Gpu,
        collected: &Collected,
        plan: &ShadowPlan,
        view_proj: Mat4,
        eye: Eye,
        inner: &SceneInner,
    ) {
        let (ambient_color, ambient_intensity) = inner.ambient();
        let (sky, ground, hemisphere) = inner.hemisphere();
        let (tone_mapping, exposure) = inner.tone_mapping();
        let tone_mapping = post::tone_curve(tone_mapping);
        let (fog_color, fog) = match inner.fog() {
            None => (Color::rgb(0, 0, 0), [0.0; 4]),
            Some(Fog::Linear { color, near, far }) => (color, [1.0, near, far, 0.0]),
            Some(Fog::Exponential { color, density }) => (color, [2.0, density, 0.0, 0.0]),
        };
        let (environment, irradiance) = match inner.environment() {
            Some((environment, intensity)) => {
                let mut irradiance = [[0.0; 4]; 9];
                for (slot, c) in irradiance
                    .iter_mut()
                    .zip(environment.data().baked().irradiance)
                {
                    *slot = (c * intensity).extend(0.0).to_array();
                }
                ([intensity, 1.0, (CUBE_LEVELS - 1) as f32, 0.0], irradiance)
            }
            None => ([0.0; 4], [[0.0; 4]; 9]),
        };
        let mut uniform = FrameUniform {
            view_proj: view_proj.to_cols_array_2d(),
            inverse_view_proj: view_proj.inverse().to_cols_array_2d(),
            environment,
            irradiance,
            camera_position: eye
                .world
                .transform_point3(Vec3::ZERO)
                .extend(1.0)
                .to_array(),
            camera_forward: eye
                .world
                .transform_vector3(Vec3::NEG_Z)
                .normalize_or(Vec3::NEG_Z)
                .extend(0.0)
                .to_array(),
            ambient: radiance(ambient_color, ambient_intensity),
            sky: radiance(sky, hemisphere),
            ground: radiance(ground, hemisphere),
            tone: [
                exposure,
                tone_mapping,
                f32::from(u8::from(self.post.is_some())),
                0.0,
            ],
            fog_color: linear_rgba(fog_color),
            fog,
            counts: [
                collected.directional.len().min(MAX_DIRECTIONAL) as u32,
                collected.point.len().min(MAX_POINT) as u32,
                u32::from(self.encode_srgb),
                0,
            ],
            shadow: [1.0 / self.shadow_maps.size as f32, 0.0, 0.0, 0.0],
            directional: [DirectionalUniform::default(); MAX_DIRECTIONAL],
            point: [PointUniform::default(); MAX_POINT],
            shadow_matrices: [[[0.0; 4]; 4]; MAX_SHADOW_LAYERS],
        };
        for (index, light) in collected
            .directional
            .iter()
            .take(MAX_DIRECTIONAL)
            .enumerate()
        {
            let cascades = plan.directional.get(index).and_then(Option::as_ref);
            uniform.directional[index] = DirectionalUniform {
                direction: light.toward.extend(0.0).to_array(),
                color: light.radiance,
                shadow: cascades.map_or([-1.0, 0.0, 0.0, 0.0], |c| {
                    [
                        c.first as f32,
                        c.count as f32,
                        c.shadow.bias,
                        c.shadow.softness,
                    ]
                }),
                splits: cascades.map_or([0.0; 4], |c| c.splits),
                texel: cascades.map_or([0.0; 4], |c| c.texel),
            };
        }
        for (index, light) in collected.point.iter().take(MAX_POINT).enumerate() {
            let faces = plan.point.get(index).and_then(Option::as_ref);
            let (direction, scale, offset) = match light.cone {
                Some(cone) => {
                    let (inner, outer) = (cone.inner_angle.cos(), cone.outer_angle.cos());
                    let scale = 1.0 / (inner - outer).max(1e-4);
                    (cone.direction, scale, -outer * scale)
                }
                None => (Vec3::ZERO, 0.0, 1.0),
            };
            let [r, g, b, _] = light.radiance;
            uniform.point[index] = PointUniform {
                position: light.position.extend(light.range).to_array(),
                color: [r, g, b, offset],
                shadow: faces.map_or([-1.0, 0.0, 0.0, 0.0], |f| {
                    [
                        f.first as f32,
                        f32::from(u8::from(f.spot)),
                        f.shadow.bias,
                        f.shadow.softness,
                    ]
                }),
                direction: direction.extend(scale).to_array(),
            };
        }
        for (slot, matrix) in uniform.shadow_matrices.iter_mut().zip(&plan.layers) {
            *slot = matrix.to_cols_array_2d();
        }
        gpu.queue
            .write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&uniform));
    }

    /// Writes one uniform per draw, at the dynamic offset of the draw's index.
    fn write_draw_uniforms(&mut self, gpu: &Gpu, draws: &[Draw]) {
        let count = draws.len() as u64;
        if count > self.draw_capacity {
            let capacity = count.next_power_of_two();
            (self.draw_buffer, self.draw_bind_group) =
                draw_buffer(gpu, &self.shared, self.draw_stride, capacity);
            self.draw_capacity = capacity;
        }
        self.staging.clear();
        self.staging.resize((self.draw_stride * count) as usize, 0);
        for (index, draw) in draws.iter().enumerate() {
            let offset = index * self.draw_stride as usize;
            let uniform = draw_uniform(draw.model, &draw.material, draw.receive_shadow);
            let bytes = bytemuck::bytes_of(&uniform);
            self.staging[offset..offset + bytes.len()].copy_from_slice(bytes);
        }
        if !self.staging.is_empty() {
            gpu.queue.write_buffer(&self.draw_buffer, 0, &self.staging);
        }
    }

    fn clear_color(&self, background: Color) -> wgpu::Color {
        let rgba = if self.encode_srgb && self.post.is_none() {
            srgb_rgba(background)
        } else {
            linear_rgba(background)
        };
        let [r, g, b, a] = Vec4::from(rgba).as_dvec4().to_array();
        wgpu::Color { r, g, b, a }
    }

    fn encode_forward(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: Target<'_>,
        draws: &[Draw],
        order: &[Ordered],
        backdrop: Backdrop,
    ) {
        let shared = self.shared.clone();
        let Target {
            view: target,
            format,
            samples,
        } = target;
        let Some(attachments) = &self.attachments else {
            return;
        };
        let (view_target, resolve_target) = match &attachments.msaa {
            Some(msaa) => (msaa, Some(target)),
            None => (target, None),
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("whisker-scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: view_target,
                depth_slice: None,
                resolve_target,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(backdrop.clear),
                    store: if resolve_target.is_some() {
                        wgpu::StoreOp::Discard
                    } else {
                        wgpu::StoreOp::Store
                    },
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &attachments.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        if backdrop.sky {
            pass.set_pipeline(&shared.sky_pipeline(gpu, format, samples));
            pass.draw(0..3, 0..1);
        }
        for ordered in order {
            let Draw {
                model,
                geometry,
                material,
                deform,
                ..
            } = &draws[ordered.index];
            let mesh = mesh(gpu, geometry);
            if mesh.count == 0 {
                continue;
            }
            let deformed = deform.as_ref().map(|deform| deformed_vertices(gpu, deform));
            pass.set_pipeline(&shared.pipeline(
                gpu,
                PipelineKey {
                    format,
                    samples,
                    blend: ordered.blended,
                    double_sided: double_sided(material),
                    mirrored: model.determinant() < 0.0,
                },
            ));
            let offset = (ordered.index as u64 * self.draw_stride) as u32;
            pass.set_bind_group(1, &self.draw_bind_group, &[offset]);
            pass.set_bind_group(2, self.materials.bind_group(gpu, &shared, material), &[]);
            match &deformed {
                Some(buffer) => pass.set_vertex_buffer(0, buffer.slice(..)),
                None => pass.set_vertex_buffer(0, mesh.vertices.slice(..)),
            }
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.count, 0, 0..1);
        }
    }
}

/// The image the forward pass resolves into.
struct Target<'a> {
    view: &'a wgpu::TextureView,
    format: wgpu::TextureFormat,
    samples: u32,
}

/// What the forward pass draws behind the scene: the clear color, then the sky when shown.
struct Backdrop {
    clear: wgpu::Color,
    sky: bool,
}

fn frame_bind_group(
    gpu: &Gpu,
    shared: &Shared,
    buffer: &wgpu::Buffer,
    shadow_maps: &ShadowMaps,
    environment: &GpuTexture,
) -> wgpu::BindGroup {
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("whisker-scene frame"),
        layout: &shared.frame_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&shadow_maps.view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&shared.shadow_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&environment.view),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Sampler(&shared.environment_sampler),
            },
        ],
    })
}

#[cfg(test)]
mod tests;
