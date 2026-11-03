use glam::{Mat4, Vec3, Vec4};
use wgpu::util::DeviceExt;

use crate::renderer::{DEPTH_FORMAT, Frame, HDR_FORMAT};

const RADIANCE_SIDE: u32 = 32;
const RADIANCE_MIPS: u32 = 5;
const IRRADIANCE_SIDE: u32 = 16;

/// Colors the background and lights every surface from all around, used alone as `(Sky::default(),)`.
///
/// It replaces [`Sun::ambient`](crate::Sun::ambient), and the sun's disk appears in it. Changing one
/// recalculates its lighting, so avoid changing it every frame; a day cycle that updates it once a second
/// looks smooth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sky {
    /// Linear RGB straight up.
    pub zenith: Vec3,
    /// Linear RGB at eye level.
    pub horizon: Vec3,
    /// Linear RGB below the horizon.
    pub ground: Vec3,
}

impl Default for Sky {
    /// A clear midday sky.
    fn default() -> Self {
        Self {
            zenith: Vec3::new(0.12, 0.28, 0.65),
            horizon: Vec3::new(0.65, 0.78, 0.95),
            ground: Vec3::new(0.2, 0.18, 0.16),
        }
    }
}

impl Sky {
    /// The colors of a clear sky with the sun heading along `direction`, from noon through dusk to night.
    pub fn from_sun(direction: Vec3) -> Self {
        let noon = Self::default();
        let height = -direction.normalize_or_zero().y;
        let night = Vec3::new(0.004, 0.006, 0.015);
        let daylight = smoothstep(-0.05, 0.3, height);
        let dusk = smoothstep(-0.15, 0.02, height);
        let glow = night.lerp(Vec3::new(1.0, 0.42, 0.18), dusk);
        Self {
            zenith: night.lerp(noon.zenith, daylight),
            horizon: glow.lerp(noon.horizon, daylight),
            ground: (night * 0.5).lerp(noon.ground, daylight),
        }
    }
}

fn smoothstep(from: f32, to: f32, value: f32) -> f32 {
    let t = ((value - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Fades distant surfaces into the air, used alone as `(Fog::default(),)`.
///
/// Set `color` near [`Sky::horizon`] so that far surfaces meet the sky without an edge, and copy it again
/// whenever the sky changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fog {
    /// Linear RGB of the air.
    pub color: Vec3,
    /// How thick the air is. About 3 divided by this is the distance where surfaces are nearly gone.
    pub density: f32,
}

impl Default for Fog {
    /// A light haze that hides surfaces at about 1.5 km, in the color of the default sky.
    fn default() -> Self {
        Self {
            color: Sky::default().horizon,
            density: 0.002,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SkyUniform {
    inverse_view_projection: Mat4,
    camera_position: Vec4,
    to_sun: Vec4,
    sun_color: Vec4,
    zenith: Vec4,
    horizon: Vec4,
    ground: Vec4,
    bake: Vec4,
}

impl SkyUniform {
    fn new(sky: &Sky) -> Self {
        Self {
            inverse_view_projection: Mat4::IDENTITY,
            camera_position: Vec4::ZERO,
            to_sun: Vec4::ZERO,
            sun_color: Vec4::ZERO,
            zenith: sky.zenith.extend(0.0),
            horizon: sky.horizon.extend(0.0),
            ground: sky.ground.extend(0.0),
            bake: Vec4::ZERO,
        }
    }
}

fn cube_texture(device: &wgpu::Device, label: &str, side: u32, mips: u32) -> wgpu::Texture {
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
        format: HDR_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn cube_view(texture: &wgpu::Texture) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::Cube),
        ..Default::default()
    })
}

pub(crate) struct SkyRenderer {
    draw_pipeline: wgpu::RenderPipeline,
    bake_pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bindings: wgpu::BindGroup,
    radiance: wgpu::Texture,
    irradiance: wgpu::Texture,
    baked: Option<Sky>,
}

impl SkyRenderer {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("sky.wgsl"));
        let pipeline = |label, entry_point, depth_stencil| {
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
                depth_stencil,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let draw_pipeline = pipeline(
            "kiba sky",
            "fs_sky",
            Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::GreaterEqual,
                stencil: Default::default(),
                bias: Default::default(),
            }),
        );
        let bake_pipeline = pipeline("kiba sky bake", "fs_bake", None);
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("kiba sky"),
            size: size_of::<SkyUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("kiba sky"),
            layout: &draw_pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        Self {
            draw_pipeline,
            bake_pipeline,
            uniform,
            bindings,
            radiance: cube_texture(device, "kiba sky radiance", RADIANCE_SIDE, RADIANCE_MIPS),
            irradiance: cube_texture(device, "kiba sky irradiance", IRRADIANCE_SIDE, 1),
            baked: None,
        }
    }

    pub(crate) fn radiance_view(&self) -> wgpu::TextureView {
        cube_view(&self.radiance)
    }

    pub(crate) fn irradiance_view(&self) -> wgpu::TextureView {
        cube_view(&self.irradiance)
    }

    /// Readies this frame's sky, recalculating its lighting if it changed.
    pub(crate) fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) {
        let Some(sky) = &frame.sky else {
            return;
        };
        if self.baked != Some(*sky) {
            self.bake(device, queue, sky);
            self.baked = Some(*sky);
        }
        let uniform = SkyUniform {
            inverse_view_projection: frame.view_projection.inverse(),
            camera_position: frame.camera_position.extend(frame.exposure),
            to_sun: (-frame.sun.direction).normalize_or_zero().extend(0.0),
            sun_color: frame.sun.color.extend(0.0),
            ..SkyUniform::new(sky)
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
    }

    /// Fills the background of a pass in which everything else has already drawn.
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.draw_pipeline);
        pass.set_bind_group(0, &self.bindings, &[]);
        pass.draw(0..3, 0..1);
    }

    fn bake(&self, device: &wgpu::Device, queue: &wgpu::Queue, sky: &Sky) {
        let mut encoder = device.create_command_encoder(&Default::default());
        let mut face_pass = |texture: &wgpu::Texture, mip: u32, roughness: f32| {
            for face in 0..6 {
                let uniform = SkyUniform {
                    bake: Vec4::new(face as f32, roughness, 0.0, 0.0),
                    ..SkyUniform::new(sky)
                };
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("kiba sky bake"),
                    contents: bytemuck::bytes_of(&uniform),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("kiba sky bake"),
                    layout: &self.bake_pipeline.get_bind_group_layout(0),
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    }],
                });
                let view = texture.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_mip_level: mip,
                    mip_level_count: Some(1),
                    base_array_layer: face,
                    array_layer_count: Some(1),
                    ..Default::default()
                });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("kiba sky bake"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: Default::default(),
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&self.bake_pipeline);
                pass.set_bind_group(0, &bindings, &[]);
                pass.draw(0..3, 0..1);
            }
        };
        for mip in 0..RADIANCE_MIPS {
            face_pass(&self.radiance, mip, mip as f32 / (RADIANCE_MIPS - 1) as f32);
        }
        face_pass(&self.irradiance, 0, -1.0);
        queue.submit([encoder.finish()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sky_follows_the_sun_from_noon_to_night() {
        let noon = Sky::from_sun(Vec3::NEG_Y);
        let dusk = Sky::from_sun(Vec3::new(1.0, -0.02, 0.0));
        let night = Sky::from_sun(Vec3::new(1.0, 0.5, 0.0));
        assert_eq!(noon, Sky::default());
        assert!(
            dusk.horizon.x > dusk.horizon.z,
            "dusk glows red: {:?}",
            dusk.horizon
        );
        assert!(night.zenith.length() < 0.05 && night.horizon.length() < 0.05);
    }
}
