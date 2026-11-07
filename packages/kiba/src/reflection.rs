use glam::{Mat4, Vec3, Vec4};
use wgpu::util::DeviceExt;

use crate::renderer::{Batch, DEPTH_FORMAT, HDR_FORMAT};

const CAPTURE_SIDE: u32 = 128;
const CAPTURE_MIPS: u32 = 6;
const REFLECTION_SIDE: u32 = 64;
/// Matches the mesh shader's `SKY_LAST_MIP`, so roughness picks the same level in either cube.
const REFLECTION_MIPS: u32 = 5;
const NEAR: f32 = 0.05;
const FAR: f32 = 2000.0;

/// Reflects the surroundings on glossy surfaces inside a box, spawned next to a
/// [`Transform`](crate::Transform) at the box's center.
///
/// It captures what is drawn around the center when first used and after
/// [`recapture`](Self::recapture), so things that move later stay where they were, and a model
/// still loading at that moment is missing until a recapture after its [`LoadState::Ready`](crate::LoadState::Ready). Inside the box,
/// reflections bend toward its walls, which suits a room or a street. Outside it, surfaces reflect
/// the [`Sky`](crate::Sky). A frame uses the first probe whose box holds the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReflectionProbe {
    /// The box's width, height, and depth, along the world axes.
    pub size: Vec3,
    revision: u32,
}

impl ReflectionProbe {
    pub fn new(size: Vec3) -> Self {
        Self { size, revision: 0 }
    }

    /// Captures again on the next frame that uses this probe.
    pub fn recapture(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub(crate) fn revision(&self) -> u32 {
        self.revision
    }
}

/// The probe a frame uses, and what to draw into it when it has not been captured.
#[derive(Clone, Debug)]
pub(crate) struct Reflection {
    pub(crate) id: ReflectionId,
    pub(crate) faces: Option<Box<[Vec<Batch>; 6]>>,
}

/// Everything that makes a capture stale when it changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ReflectionId {
    pub(crate) entity: u64,
    pub(crate) revision: u32,
    pub(crate) center: Vec3,
    pub(crate) size: Vec3,
}

impl ReflectionId {
    pub(crate) fn min(&self) -> Vec3 {
        self.center - self.size / 2.0
    }

    pub(crate) fn max(&self) -> Vec3 {
        self.center + self.size / 2.0
    }

    pub(crate) fn contains(&self, point: Vec3) -> bool {
        point.cmpge(self.min()).all() && point.cmple(self.max()).all()
    }
}

/// Each capture layer's view and projection from `center`, ordered +X, -X, +Y, -Y, +Z, -Z.
pub(crate) fn face_view_projections(center: Vec3) -> [Mat4; 6] {
    let projection = Mat4::perspective_rh(std::f32::consts::FRAC_PI_2, 1.0, FAR, NEAR);
    [
        (Vec3::X, Vec3::Y),
        (Vec3::NEG_X, Vec3::Y),
        (Vec3::Y, Vec3::Z),
        (Vec3::NEG_Y, Vec3::Z),
        (Vec3::Z, Vec3::Y),
        (Vec3::NEG_Z, Vec3::Y),
    ]
    .map(|(forward, up)| projection * Mat4::look_to_rh(center, forward, up))
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    faces: [Mat4; 6],
    pass_info: Vec4,
    ambient: Vec4,
}

/// Holds the captured scene and the cube surfaces reflect it from.
pub(crate) struct ReflectionTextures {
    capture: wgpu::Texture,
    depth: wgpu::TextureView,
    cube: wgpu::Texture,
    downsample: wgpu::RenderPipeline,
    prefilter: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    pub(crate) captured: Option<ReflectionId>,
}

fn layer_view(texture: &wgpu::Texture, layer: u32, mip: u32) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2),
        base_mip_level: mip,
        mip_level_count: Some(1),
        base_array_layer: layer,
        array_layer_count: Some(1),
        ..Default::default()
    })
}

impl ReflectionTextures {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("reflection.wgsl"));
        let pipeline = |label, entry_point| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry_point),
                    compilation_options: Default::default(),
                    targets: &[Some(HDR_FORMAT.into())],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let image = |label, side, mips, format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: side,
                    height: side,
                    depth_or_array_layers: 6,
                },
                mip_level_count: mips,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let depth = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("kiba reflection depth"),
                size: wgpu::Extent3d {
                    width: CAPTURE_SIDE,
                    height: CAPTURE_SIDE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default());
        Self {
            capture: image(
                "kiba reflection capture",
                CAPTURE_SIDE,
                CAPTURE_MIPS,
                HDR_FORMAT,
            ),
            depth,
            cube: image(
                "kiba reflection",
                REFLECTION_SIDE,
                REFLECTION_MIPS,
                HDR_FORMAT,
            ),
            downsample: pipeline("kiba reflection downsample", "fs_downsample"),
            prefilter: pipeline("kiba reflection prefilter", "fs_prefilter"),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("kiba reflection"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            captured: None,
        }
    }

    pub(crate) fn cube_view(&self) -> wgpu::TextureView {
        self.cube.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        })
    }

    /// The color and depth targets one capture layer draws into.
    pub(crate) fn layer_targets(&self, layer: u32) -> (wgpu::TextureView, &wgpu::TextureView) {
        (layer_view(&self.capture, layer, 0), &self.depth)
    }

    /// Shrinks the drawn layers and filters them into the cube by roughness, where the sky's
    /// `radiance` stands in for what nothing covered, or `ambient` when there is no sky.
    pub(crate) fn finish(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        radiance: &wgpu::TextureView,
        has_sky: bool,
        ambient: Vec3,
    ) {
        let faces = face_view_projections(Vec3::ZERO);
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    pipeline: &wgpu::RenderPipeline,
                    target: &wgpu::TextureView,
                    source: &wgpu::TextureView,
                    pass_info: Vec4| {
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("kiba reflection"),
                contents: bytemuck::bytes_of(&Params {
                    faces,
                    pass_info,
                    ambient: ambient.extend(0.0),
                }),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let layout = pipeline.get_bind_group_layout(0);
            let mut entries = vec![
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ];
            if std::ptr::eq(pipeline, &self.prefilter) {
                entries.push(wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(radiance),
                });
            }
            let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("kiba reflection"),
                layout: &layout,
                entries: &entries,
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("kiba reflection"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: Default::default(),
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bindings, &[]);
            pass.draw(0..3, 0..1);
        };
        for mip in 1..CAPTURE_MIPS {
            let source = self.capture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                base_mip_level: mip - 1,
                mip_level_count: Some(1),
                ..Default::default()
            });
            for layer in 0..6 {
                let target = layer_view(&self.capture, layer, mip);
                let info = Vec4::new(layer as f32, 0.0, 0.0, 0.0);
                pass(encoder, &self.downsample, &target, &source, info);
            }
        }
        let source = self.capture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let has_sky = f32::from(u8::from(has_sky));
        for mip in 0..REFLECTION_MIPS {
            let roughness = mip as f32 / (REFLECTION_MIPS - 1) as f32;
            for face in 0..6 {
                let target = layer_view(&self.cube, face, mip);
                let info = Vec4::new(face as f32, roughness, has_sky, 0.0);
                pass(encoder, &self.prefilter, &target, &source, info);
            }
        }
    }
}
