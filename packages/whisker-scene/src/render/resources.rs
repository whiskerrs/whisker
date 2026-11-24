//! GPU copies of geometry, textures and material bind groups, created on first use.

use std::collections::HashMap;
use std::rc::Rc;

use whisker_wgpu::{Gpu, wgpu};

use super::pipeline::{Shared, TEXTURE_SLOTS};
use glam::Mat4;

use super::Draw;
use crate::core::{
    CUBE_LEVELS, CUBE_SIZE, Deform, Environment, Geometry, Instances, Material, Texture,
};

pub(crate) struct GpuMesh {
    pub(super) vertices: wgpu::Buffer,
    pub(super) indices: wgpu::Buffer,
    pub(super) count: u32,
}

pub(crate) struct InstanceBuffer {
    buffer: wgpu::Buffer,
}

pub(super) fn instance_buffer(gpu: &Gpu, transforms: &[Mat4]) -> wgpu::Buffer {
    let columns: Vec<[f32; 16]> = transforms.iter().map(Mat4::to_cols_array).collect();
    let bytes: &[u8] = bytemuck::cast_slice(&columns);
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("whisker-scene instances"),
        size: (bytes.len() as u64).max(64),
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue.write_buffer(&buffer, 0, bytes);
    buffer
}

fn instances<'a>(gpu: &Gpu, instances: &'a Instances) -> &'a wgpu::Buffer {
    &instances
        .data()
        .gpu
        .get_or_init(|| InstanceBuffer {
            buffer: instance_buffer(gpu, instances.transforms()),
        })
        .buffer
}

/// Binds `draw`'s vertex, instance and index buffers, returning its index and instance counts, or
/// `None` when it draws nothing.
pub(super) fn bind_mesh(
    gpu: &Gpu,
    shared: &Shared,
    pass: &mut wgpu::RenderPass<'_>,
    draw: &Draw,
) -> Option<(u32, u32)> {
    let mesh = mesh(gpu, &draw.geometry);
    let count = draw
        .instances
        .as_ref()
        .map_or(1, |instances| instances.transforms().len() as u32);
    if mesh.count == 0 || count == 0 {
        return None;
    }
    match &draw.deform {
        Some(deform) => pass.set_vertex_buffer(0, deformed_vertices(gpu, deform).slice(..)),
        None => pass.set_vertex_buffer(0, mesh.vertices.slice(..)),
    }
    match &draw.instances {
        Some(draw_instances) => pass.set_vertex_buffer(1, instances(gpu, draw_instances).slice(..)),
        None => pass.set_vertex_buffer(1, shared.one_instance.slice(..)),
    }
    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
    Some((mesh.count, count))
}

/// A vertex buffer holding one node's deformed vertices.
pub(crate) struct DeformBuffer {
    pub(super) buffer: wgpu::Buffer,
}

pub(crate) struct GpuTexture {
    pub(super) view: wgpu::TextureView,
}

pub(super) fn upload_texture(
    gpu: &Gpu,
    width: u32,
    height: u32,
    pixels: &[u8],
    srgb: bool,
) -> GpuTexture {
    let levels = mip_chain(width, height, pixels);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("whisker-scene texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: if srgb {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        },
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, (w, h, pixels)) in levels.iter().enumerate() {
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(*h),
            },
            wgpu::Extent3d {
                width: *w,
                height: *h,
                depth_or_array_layers: 1,
            },
        );
    }
    GpuTexture {
        view: texture.create_view(&Default::default()),
    }
}

/// The environment's prefiltered cube, uploaded on first use.
pub(super) fn environment_cube<'a>(gpu: &Gpu, environment: &'a Environment) -> &'a GpuTexture {
    let data = environment.data();
    data.gpu.get_or_init(|| {
        let levels: Vec<Vec<[u16; 4]>> = data
            .baked()
            .levels
            .iter()
            .map(|level| {
                level
                    .iter()
                    .map(|c| [half(c.x), half(c.y), half(c.z), half(1.0)])
                    .collect()
            })
            .collect();
        upload_cube(gpu, CUBE_SIZE, &levels)
    })
}

/// A cube with `levels` of six faces each, halving from `size`, as RGBA16 floats.
pub(super) fn upload_cube(gpu: &Gpu, size: u32, levels: &[Vec<[u16; 4]>]) -> GpuTexture {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("whisker-scene environment"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, texels) in levels.iter().enumerate() {
        let side = (size >> level).max(1);
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(texels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(side * 8),
                rows_per_image: Some(side),
            },
            wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 6,
            },
        );
    }
    GpuTexture {
        view: texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        }),
    }
}

/// `value` as an IEEE half float, rounded toward zero and saturating at the largest finite half.
pub(super) fn half(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let magnitude = value.abs();
    if magnitude.is_nan() {
        return sign | 0x7e00;
    }
    if magnitude >= 65504.0 {
        return sign | 0x7bff;
    }
    if magnitude < 6.103_515_6e-5 {
        return sign | (magnitude / 5.960_464_5e-8) as u16;
    }
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = (bits >> 13) & 0x3ff;
    sign | ((exponent as u16) << 10) | mantissa as u16
}

const _: () = assert!(CUBE_LEVELS <= CUBE_SIZE.trailing_zeros() + 1);

/// The image and each half-size level below it, averaged 2×2, down to 1×1.
fn mip_chain(
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Vec<(u32, u32, std::borrow::Cow<'_, [u8]>)> {
    let mut levels = vec![(width, height, std::borrow::Cow::Borrowed(pixels))];
    while let Some((w, h, src)) = levels.last()
        && (*w > 1 || *h > 1)
    {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(w - 1);
                        let sy = (y * 2 + dy).min(h - 1);
                        sum += u32::from(src[((sy * w + sx) * 4 + c) as usize]);
                    }
                    next[((y * nw + x) * 4 + c) as usize] = (sum / 4) as u8;
                }
            }
        }
        levels.push((nw, nh, std::borrow::Cow::Owned(next)));
    }
    levels
}

pub(super) fn mesh<'a>(gpu: &Gpu, geometry: &'a Geometry) -> &'a GpuMesh {
    let data = geometry.data();
    data.gpu.get_or_init(|| {
        let buffer = |label, contents: &[u8], usage| {
            let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (contents.len() as u64)
                    .next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT)
                    .max(4),
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut padded = contents.to_vec();
            padded.resize(buffer.size() as usize, 0);
            gpu.queue.write_buffer(&buffer, 0, &padded);
            buffer
        };
        GpuMesh {
            vertices: buffer(
                "whisker-scene vertices",
                bytemuck::cast_slice(&data.vertices),
                wgpu::BufferUsages::VERTEX,
            ),
            indices: buffer(
                "whisker-scene indices",
                bytemuck::cast_slice(&data.indices),
                wgpu::BufferUsages::INDEX,
            ),
            count: data.indices.len() as u32,
        }
    })
}

/// The buffer of `deform`'s current vertices, rewritten when they changed since its last upload.
pub(super) fn deformed_vertices(gpu: &Gpu, deform: &Deform) -> wgpu::Buffer {
    let mut slot = deform.gpu.borrow_mut();
    let version = deform.version.get();
    let size = deform.with_vertices(|vertices| std::mem::size_of_val(vertices) as u64);
    if slot
        .as_ref()
        .is_none_or(|(buffer, _)| buffer.buffer.size() < size.max(4))
    {
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("whisker-scene deformed vertices"),
            size: size.next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT).max(4),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        *slot = Some((DeformBuffer { buffer }, u64::MAX));
    }
    let (buffer, uploaded) = slot.as_mut().expect("created above");
    if *uploaded != version {
        deform.with_vertices(|vertices| {
            gpu.queue
                .write_buffer(&buffer.buffer, 0, bytemuck::cast_slice(vertices));
        });
        *uploaded = version;
    }
    buffer.buffer.clone()
}

pub(super) fn texture<'a>(gpu: &Gpu, texture: &'a Texture, srgb: bool) -> &'a GpuTexture {
    let data = texture.data();
    let cell = if srgb { &data.srgb } else { &data.linear };
    cell.get_or_init(|| upload_texture(gpu, data.width, data.height, &data.pixels, srgb))
}

/// The textures a material binds, in shader slot order, with whether each is read as sRGB.
fn material_textures(material: &Material) -> [(Option<&Texture>, bool); TEXTURE_SLOTS] {
    match material {
        Material::Unlit { .. } => [
            (None, true),
            (None, false),
            (None, false),
            (None, true),
            (None, false),
        ],
        Material::Standard(m) => [
            (m.base_color_texture.as_ref(), true),
            (m.metallic_roughness_texture.as_ref(), false),
            (m.normal_texture.as_ref(), false),
            (m.emissive_texture.as_ref(), true),
            (m.occlusion_texture.as_ref(), false),
        ],
    }
}

type MaterialKey = [usize; TEXTURE_SLOTS];

/// Material bind groups keyed by their textures, kept while some draw of the last frame used them.
#[derive(Default)]
pub(super) struct MaterialCache {
    entries: HashMap<MaterialKey, (wgpu::BindGroup, Vec<Texture>, bool)>,
}

impl MaterialCache {
    pub(super) fn bind_group(
        &mut self,
        gpu: &Gpu,
        shared: &Shared,
        material: &Material,
    ) -> &wgpu::BindGroup {
        let slots = material_textures(material);
        let key: MaterialKey = slots
            .map(|(texture, _)| texture.map_or(0, |texture| Rc::as_ptr(texture.rc()) as usize));
        let entry = self.entries.entry(key).or_insert_with(|| {
            let views: Vec<&wgpu::TextureView> = slots
                .iter()
                .enumerate()
                .map(|(slot, (texture, srgb))| match texture {
                    Some(texture) => &self::texture(gpu, texture, *srgb).view,
                    None if slot == 2 => &shared.flat_normal.view,
                    None => &shared.white.view,
                })
                .collect();
            let mut entries: Vec<wgpu::BindGroupEntry> = views
                .iter()
                .enumerate()
                .map(|(binding, view)| wgpu::BindGroupEntry {
                    binding: binding as u32,
                    resource: wgpu::BindingResource::TextureView(view),
                })
                .collect();
            entries.push(wgpu::BindGroupEntry {
                binding: TEXTURE_SLOTS as u32,
                resource: wgpu::BindingResource::Sampler(&shared.sampler),
            });
            let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("whisker-scene material"),
                layout: &shared.texture_layout,
                entries: &entries,
            });
            let kept = slots
                .iter()
                .filter_map(|(texture, _)| texture.cloned())
                .collect();
            (bind_group, kept, false)
        });
        entry.2 = true;
        &entry.0
    }

    /// Drops the bind groups no draw used since the last call.
    pub(super) fn sweep(&mut self) {
        self.entries.retain(|_, (_, _, used)| std::mem::take(used));
    }
}
