//! Draws a map's style into one surface.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use bytemuck::{Pod, Zeroable};
use glam::{DMat4, Mat4};
use whisker_surface::{FrameInfo, SurfaceSize};
use whisker_wgpu::{Gpu, Renderer, wgpu};

use crate::camera::View;
use crate::geo::{PlacedTile, globe_cells};
use crate::map::{MapInner, StyleState};
use crate::style::color::Rgba;
use crate::style::expr::Context;
use crate::style::{Layer, LayerKind};
use crate::tiles::bucket::{
    CircleVertex, EXTENT, ExtrusionVertex, FillVertex, Geometry, LineVertex,
};
use crate::tiles::{LoadedTile, TileContent};

pub(crate) mod labels;

pub(crate) use labels::PlacedLabel;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;
const MSAA_SAMPLES: u32 = 4;

/// The atmosphere's glow at the globe's edge and the space beyond, premultiplied sRGB.
const HALO: [f32; 4] = [0.78, 0.88, 1.0, 1.0];
const SPACE: [f32; 4] = [0.05, 0.08, 0.16, 1.0];

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct FrameUniform {
    viewport: [f32; 4],
    light: [f32; 4],
    light_color: [f32; 4],
    globe: [[f32; 4]; 4],
    globe_inverse: [[f32; 4]; 4],
    globe_params: [f32; 4],
    camera_dir: [f32; 4],
    halo: [f32; 4],
    space: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub(crate) struct DrawUniform {
    pub matrix: [[f32; 4]; 4],
    pub color: [f32; 4],
    pub params: [f32; 4],
    pub extra: [f32; 4],
    pub color2: [f32; 4],
    pub dash0: [f32; 4],
    pub dash1: [f32; 4],
    pub tile: [f32; 4],
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Program {
    ClearMask,
    Mask,
    Background,
    Fill,
    Line,
    Circle,
    Raster,
    /// Writes the extrusions' depth so that `Extrusion` colors only the surface in front.
    ExtrusionDepth,
    Extrusion,
    Symbol,
    /// The background on the globe's surface rather than across the screen.
    GlobeBackground,
    Atmosphere,
}

pub(crate) struct Shared {
    pub frame_layout: wgpu::BindGroupLayout,
    pub draw_layout: wgpu::BindGroupLayout,
    pub texture_layout: wgpu::BindGroupLayout,
    layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
    pub symbol_shader: wgpu::ShaderModule,
    pub linear: wgpu::Sampler,
    /// Tile-sized meshes cut into 1, 2, 4 … 32 cells each way, indexed by the power of two.
    grids: Vec<GpuMesh>,
    pub empty_texture: wgpu::BindGroup,
    pipelines: RefCell<HashMap<(Program, wgpu::TextureFormat, u32), wgpu::RenderPipeline>>,
}

thread_local! {
    static SHARED: RefCell<Option<Rc<Shared>>> = const { RefCell::new(None) };
}

fn shared(gpu: &Gpu) -> Rc<Shared> {
    SHARED.with_borrow_mut(|shared| {
        shared
            .get_or_insert_with(|| Rc::new(Shared::new(gpu)))
            .clone()
    })
}

pub(crate) fn buffer(
    gpu: &Gpu,
    label: &str,
    contents: &[u8],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (contents.len() as u64)
            .next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT)
            .max(4),
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    if contents.len().is_multiple_of(4) {
        gpu.queue.write_buffer(&buffer, 0, contents);
    } else {
        let mut padded = contents.to_vec();
        padded.resize(buffer.size() as usize, 0);
        gpu.queue.write_buffer(&buffer, 0, &padded);
    }
    buffer
}

impl Shared {
    fn new(gpu: &Gpu) -> Shared {
        let device = &gpu.device;
        let uniform = |dynamic: bool, size: usize| wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: dynamic,
                min_binding_size: wgpu::BufferSize::new(size as u64),
            },
            count: None,
        };
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("whisker-map frame"),
            entries: &[uniform(false, size_of::<FrameUniform>())],
        });
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("whisker-map draw"),
            entries: &[uniform(true, size_of::<DrawUniform>())],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("whisker-map texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("whisker-map"),
            bind_group_layouts: &[
                Some(&frame_layout),
                Some(&draw_layout),
                Some(&texture_layout),
            ],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("whisker-map"),
            source: wgpu::ShaderSource::Wgsl(include_str!("map.wgsl").into()),
        });
        let symbol_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("whisker-map symbols"),
            source: wgpu::ShaderSource::Wgsl(include_str!("symbol.wgsl").into()),
        });
        let linear = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("whisker-map linear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let grids = (0..6).map(|power| grid_mesh(gpu, 1 << power)).collect();
        let empty = upload_texture(
            gpu,
            &texture_layout,
            &linear,
            1,
            1,
            &[0, 0, 0, 0],
            wgpu::TextureFormat::Rgba8Unorm,
        );
        Shared {
            frame_layout,
            draw_layout,
            texture_layout,
            layout,
            shader,
            symbol_shader,
            linear,
            grids,
            empty_texture: empty.1,
            pipelines: RefCell::new(HashMap::new()),
        }
    }

    fn draw_grid(&self, pass: &mut wgpu::RenderPass, cells: u32) {
        let grid = &self.grids[(cells.trailing_zeros() as usize).min(self.grids.len() - 1)];
        pass.set_vertex_buffer(0, grid.vertices.slice(..));
        pass.set_index_buffer(grid.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..grid.count, 0, 0..1);
    }

    pub fn pipeline(
        &self,
        gpu: &Gpu,
        program: Program,
        format: wgpu::TextureFormat,
        samples: u32,
    ) -> wgpu::RenderPipeline {
        self.pipelines
            .borrow_mut()
            .entry((program, format, samples))
            .or_insert_with(|| self.create(gpu, program, format, samples))
            .clone()
    }

    fn create(
        &self,
        gpu: &Gpu,
        program: Program,
        format: wgpu::TextureFormat,
        samples: u32,
    ) -> wgpu::RenderPipeline {
        let attrs_fill = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];
        let attrs_line = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4];
        let attrs_circle = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4];
        let attrs_quad = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2];
        let attrs_extrusion =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4];
        let attrs_symbol = labels::SYMBOL_ATTRIBUTES;
        let (module, vs, fs, stride, attributes): (_, _, _, usize, &[wgpu::VertexAttribute]) =
            match program {
                Program::ClearMask | Program::Background => {
                    (&self.shader, "vs_screen", "fs_solid", 0, &[])
                }
                Program::Atmosphere => (&self.shader, "vs_screen", "fs_atmosphere", 0, &[]),
                Program::GlobeBackground => {
                    (&self.shader, "vs_screen", "fs_globe_background", 0, &[])
                }
                Program::Mask => (&self.shader, "vs_mask", "fs_solid", 16, &attrs_quad[..1]),
                Program::Fill => (
                    &self.shader,
                    "vs_fill",
                    "fs_fill",
                    size_of::<FillVertex>(),
                    &attrs_fill,
                ),
                Program::Line => (
                    &self.shader,
                    "vs_line",
                    "fs_line",
                    size_of::<LineVertex>(),
                    &attrs_line,
                ),
                Program::Circle => (
                    &self.shader,
                    "vs_circle",
                    "fs_circle",
                    size_of::<CircleVertex>(),
                    &attrs_circle,
                ),
                Program::Raster => (&self.shader, "vs_raster", "fs_raster", 16, &attrs_quad),
                Program::Extrusion | Program::ExtrusionDepth => (
                    &self.shader,
                    "vs_extrusion",
                    "fs_extrusion",
                    size_of::<ExtrusionVertex>(),
                    &attrs_extrusion,
                ),
                Program::Symbol => (
                    &self.symbol_shader,
                    "vs_symbol",
                    "fs_symbol",
                    size_of::<labels::SymbolVertex>(),
                    &attrs_symbol,
                ),
            };
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: stride as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes,
        })];
        let masking = matches!(program, Program::ClearMask | Program::Mask);
        let stencil_face = |compare, pass| wgpu::StencilFaceState {
            compare,
            fail_op: wgpu::StencilOperation::Keep,
            depth_fail_op: wgpu::StencilOperation::Keep,
            pass_op: pass,
        };
        let stencil = match program {
            Program::ClearMask | Program::Mask => {
                let face = stencil_face(
                    wgpu::CompareFunction::Always,
                    wgpu::StencilOperation::Replace,
                );
                wgpu::StencilState {
                    front: face,
                    back: face,
                    read_mask: 0xff,
                    write_mask: 0xff,
                }
            }
            Program::Fill
            | Program::Line
            | Program::Circle
            | Program::Raster
            | Program::Extrusion
            | Program::ExtrusionDepth => {
                let face = stencil_face(wgpu::CompareFunction::Equal, wgpu::StencilOperation::Keep);
                wgpu::StencilState {
                    front: face,
                    back: face,
                    read_mask: 0xff,
                    write_mask: 0,
                }
            }
            _ => wgpu::StencilState::default(),
        };
        let depth = matches!(program, Program::Extrusion | Program::ExtrusionDepth);
        gpu.device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("whisker-map"),
                layout: Some(&self.layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: if stride == 0 { &[] } else { &buffers },
                },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: if masking || program == Program::ExtrusionDepth {
                            wgpu::ColorWrites::empty()
                        } else {
                            wgpu::ColorWrites::ALL
                        },
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Cw,
                    cull_mode: if depth { Some(wgpu::Face::Back) } else { None },
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(program == Program::ExtrusionDepth),
                    depth_compare: Some(match program {
                        Program::ExtrusionDepth => wgpu::CompareFunction::LessEqual,
                        Program::Extrusion => wgpu::CompareFunction::Equal,
                        _ => wgpu::CompareFunction::Always,
                    }),
                    stencil,
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            })
    }
}

/// A tile's square, 0 to `EXTENT` with texture coordinates 0 to 1, cut into `cells` each way.
fn grid_mesh(gpu: &Gpu, cells: u32) -> GpuMesh {
    let mut vertices: Vec<[f32; 4]> = Vec::new();
    for j in 0..=cells {
        for i in 0..=cells {
            let (u, v) = (i as f32 / cells as f32, j as f32 / cells as f32);
            vertices.push([u * EXTENT, v * EXTENT, u, v]);
        }
    }
    let mut indices = Vec::new();
    let row = cells + 1;
    for j in 0..cells {
        for i in 0..cells {
            let a = j * row + i;
            indices.extend_from_slice(&[a, a + 1, a + row, a + 1, a + row + 1, a + row]);
        }
    }
    GpuMesh::new(gpu, &vertices, &indices)
}

/// Where a tile sits in the world, for projecting its units onto the globe.
fn tile_params(tile: PlacedTile) -> [f32; 4] {
    let n = f64::from(1u32 << tile.id.z);
    [
        ((f64::from(tile.id.x) + f64::from(tile.wrap) * n) / n) as f32,
        (f64::from(tile.id.y) / n) as f32,
        (1.0 / (n * f64::from(EXTENT))) as f32,
        0.0,
    ]
}

pub(crate) fn upload_texture(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    width: u32,
    height: u32,
    pixels: &[u8],
    format: wgpu::TextureFormat,
) -> (wgpu::Texture, wgpu::BindGroup) {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("whisker-map texture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let bytes_per_pixel = format.block_copy_size(None).unwrap_or(4);
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * bytes_per_pixel),
            rows_per_image: Some(height),
        },
        size,
    );
    let bind_group = texture_bind_group(gpu, layout, sampler, &texture);
    (texture, bind_group)
}

pub(crate) fn texture_bind_group(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    texture: &wgpu::Texture,
) -> wgpu::BindGroup {
    let view = texture.create_view(&Default::default());
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("whisker-map texture"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

pub(crate) struct GpuMesh {
    pub vertices: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    pub count: u32,
}

impl GpuMesh {
    pub fn new<V: Pod>(gpu: &Gpu, vertices: &[V], indices: &[u32]) -> GpuMesh {
        GpuMesh {
            vertices: buffer(
                gpu,
                "whisker-map vertices",
                bytemuck::cast_slice(vertices),
                wgpu::BufferUsages::VERTEX,
            ),
            indices: buffer(
                gpu,
                "whisker-map indices",
                bytemuck::cast_slice(indices),
                wgpu::BufferUsages::INDEX,
            ),
            count: indices.len() as u32,
        }
    }
}

/// A tile's GPU objects.
struct GpuTile {
    meshes: HashMap<usize, (Program, GpuMesh)>,
    raster: Option<wgpu::BindGroup>,
}

fn gpu_tile<'a>(gpu: &Gpu, shared: &Shared, tile: &'a LoadedTile) -> std::cell::Ref<'a, GpuTile> {
    if tile.gpu.borrow().is_none() {
        let mut meshes = HashMap::new();
        let mut raster = None;
        match &tile.content {
            TileContent::Vector { buckets, .. } => {
                for bucket in buckets {
                    let entry = match &bucket.geometry {
                        Geometry::Fill(m) => {
                            (Program::Fill, GpuMesh::new(gpu, &m.vertices, &m.indices))
                        }
                        Geometry::Line(m) => {
                            (Program::Line, GpuMesh::new(gpu, &m.vertices, &m.indices))
                        }
                        Geometry::Circle(m) => {
                            (Program::Circle, GpuMesh::new(gpu, &m.vertices, &m.indices))
                        }
                        Geometry::Extrusion(m) => (
                            Program::Extrusion,
                            GpuMesh::new(gpu, &m.vertices, &m.indices),
                        ),
                    };
                    meshes.insert(bucket.layer, entry);
                }
            }
            TileContent::Raster(image) => {
                raster = Some(
                    upload_texture(
                        gpu,
                        &shared.texture_layout,
                        &shared.linear,
                        image.width,
                        image.height,
                        &image.pixels,
                        wgpu::TextureFormat::Rgba8Unorm,
                    )
                    .1,
                );
            }
            TileContent::Dem(dem) => {
                let shade = crate::raster::hillshade_texture(dem, tile.id.z);
                raster = Some(
                    upload_texture(
                        gpu,
                        &shared.texture_layout,
                        &shared.linear,
                        dem.size,
                        dem.size,
                        &shade,
                        wgpu::TextureFormat::Rgba8Unorm,
                    )
                    .1,
                );
            }
        }
        *tile.gpu.borrow_mut() = Some(Box::new(GpuTile { meshes, raster }));
    }
    std::cell::Ref::map(tile.gpu.borrow(), |gpu| {
        gpu.as_ref()
            .and_then(|b| b.downcast_ref::<GpuTile>())
            .expect("tile GPU objects")
    })
}

struct Attachments {
    size: (u32, u32),
    depth: wgpu::TextureView,
    msaa: Option<wgpu::TextureView>,
}

/// Per source: the ideal tile zoom, and the loaded tiles to draw.
pub(crate) type TileSet = HashMap<String, (u8, Vec<(PlacedTile, Rc<LoadedTile>)>)>;

/// One queued draw in the frame's single pass.
#[derive(Clone)]
pub(crate) enum Step {
    ClearMask,
    Mask {
        uniform: u32,
        stencil: u32,
        cells: u32,
    },
    Draw {
        program: Program,
        uniform: u32,
        stencil: u32,
        mesh: MeshRef,
        texture: Option<TextureRef>,
    },
    Screen {
        program: Program,
        uniform: u32,
    },
}

#[derive(Clone)]
pub(crate) enum MeshRef {
    Tile(Rc<LoadedTile>, usize),
    /// A tile's square cut into this many cells each way.
    Grid(u32),
    Labels(usize),
}

#[derive(Clone)]
pub(crate) enum TextureRef {
    Tile(Rc<LoadedTile>),
    Labels(usize),
}

pub(crate) struct MapRenderer {
    map: Rc<MapInner>,
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
    size: SurfaceSize,
    attachments: Option<Attachments>,
    labels: labels::LabelRenderer,
}

fn draw_buffer(
    gpu: &Gpu,
    shared: &Shared,
    stride: u64,
    capacity: u64,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("whisker-map draws"),
        size: stride * capacity,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("whisker-map draws"),
        layout: &shared.draw_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &buffer,
                offset: 0,
                size: wgpu::BufferSize::new(size_of::<DrawUniform>() as u64),
            }),
        }],
    });
    (buffer, bind_group)
}

fn mat(m: DMat4) -> [[f32; 4]; 4] {
    Mat4::from_cols_array(&m.to_cols_array().map(|v| v as f32)).to_cols_array_2d()
}

/// The uniform data and steps one frame records before encoding them.
pub(crate) struct FrameRecorder {
    pub uniforms: Vec<DrawUniform>,
    pub steps: Vec<Step>,
}

impl FrameRecorder {
    pub fn push(&mut self, uniform: DrawUniform) -> u32 {
        self.uniforms.push(uniform);
        (self.uniforms.len() - 1) as u32
    }
}

impl MapRenderer {
    pub(crate) fn new(gpu: &Gpu, format: wgpu::TextureFormat, map: Rc<MapInner>) -> MapRenderer {
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
            label: Some("whisker-map frame"),
            size: size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("whisker-map frame"),
            layout: &shared.frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buffer.as_entire_binding(),
            }],
        });
        let alignment = u64::from(gpu.device.limits().min_uniform_buffer_offset_alignment);
        let draw_stride = (size_of::<DrawUniform>() as u64).next_multiple_of(alignment);
        let (draw_buffer, draw_bind_group) = draw_buffer(gpu, &shared, draw_stride, 256);
        let labels = labels::LabelRenderer::new();
        MapRenderer {
            map,
            shared,
            format,
            samples,
            encode_srgb: format.is_srgb(),
            frame_buffer,
            frame_bind_group,
            draw_buffer,
            draw_bind_group,
            draw_stride,
            draw_capacity: 256,
            size: SurfaceSize {
                width: 1,
                height: 1,
                scale: 1.0,
            },
            attachments: None,
            labels,
        }
    }

    fn prepare_attachments(&mut self, gpu: &Gpu) {
        let size = (self.size.width.max(1), self.size.height.max(1));
        if self.attachments.as_ref().is_some_and(|a| a.size != size) {
            self.attachments = None;
        }
        let (format, samples) = (self.format, self.samples);
        self.attachments.get_or_insert_with(|| {
            let texture = |label, format| {
                gpu.device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d {
                            width: size.0,
                            height: size.1,
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
                size,
                depth: texture("whisker-map depth", DEPTH_FORMAT),
                msaa: (samples > 1).then(|| texture("whisker-map msaa", format)),
            }
        });
    }

    /// The tiles each source draws this frame, ancestors before descendants.
    fn collect_tiles(view: &View, style: &StyleState) -> TileSet {
        let zoom = view.zoom();
        let mut tiles_by_source = HashMap::new();
        for (id, source) in &style.sources {
            source.begin_frame();
            let z = source.ideal_zoom(zoom);
            let min_z = source.source.borrow().min_zoom;
            let wanted = if view.globe > 0.0 {
                view.covering_tiles_globe(z, 400)
            } else {
                view.covering_tiles(z, min_z, 400)
            };
            let mut tiles = source.tiles_for(&wanted);
            tiles.truncate(254);
            source.evict();
            tiles_by_source.insert(id.clone(), (z, tiles));
        }
        tiles_by_source
    }

    /// Records every style layer's draws for this frame.
    fn record(
        &mut self,
        gpu: &Gpu,
        view: &View,
        style: &StyleState,
        tiles_by_source: &TileSet,
    ) -> FrameRecorder {
        let mut rec = FrameRecorder {
            uniforms: Vec::new(),
            steps: Vec::new(),
        };
        let zoom = view.zoom();
        let ctx = Context::zoom(zoom);
        let cells = |placed: &PlacedTile| {
            if view.globe > 0.0 {
                globe_cells(placed.id.z)
            } else {
                1
            }
        };
        if view.globe > 0.0 {
            let uniform = rec.push(DrawUniform::default());
            rec.steps.push(Step::Screen {
                program: Program::Atmosphere,
                uniform,
            });
        }

        let mut masked: Option<&str> = None;
        for (index, layer) in style.layers.iter().enumerate() {
            if !layer.shown_at(zoom) {
                continue;
            }
            if layer.kind == LayerKind::Background {
                let color = layer.get("background-color").color(&ctx, Rgba::BLACK);
                let opacity = layer.get("background-opacity").number(&ctx, 1.0) as f32;
                let uniform = rec.push(DrawUniform {
                    color: color.premultiplied(1.0),
                    params: [0.0, opacity, 0.0, 0.0],
                    ..Default::default()
                });
                rec.steps.push(Step::Screen {
                    program: if view.globe > 0.0 {
                        Program::GlobeBackground
                    } else {
                        Program::Background
                    },
                    uniform,
                });
                continue;
            }
            if layer.kind == LayerKind::Symbol {
                self.labels.record_layer(index, &mut rec);
                continue;
            }
            let Some(source_id) = layer.source.as_deref() else {
                continue;
            };
            let Some((_, tiles)) = tiles_by_source.get(source_id) else {
                continue;
            };
            if tiles.is_empty() {
                continue;
            }
            if masked != Some(source_id) {
                rec.steps.push(Step::ClearMask);
                for (i, (placed, _)) in tiles.iter().enumerate() {
                    let uniform = rec.push(DrawUniform {
                        matrix: mat(view.tile_matrix(*placed, f64::from(EXTENT))),
                        tile: tile_params(*placed),
                        ..Default::default()
                    });
                    rec.steps.push(Step::Mask {
                        uniform,
                        stencil: i as u32 + 1,
                        cells: cells(placed),
                    });
                }
                masked = Some(source_id);
            }
            let first_draw = rec.steps.len();
            for (i, (placed, tile)) in tiles.iter().enumerate() {
                let stencil = i as u32 + 1;
                let matrix = mat(view.tile_matrix(*placed, f64::from(EXTENT)));
                let units = f64::from(EXTENT) / (512.0 * 2f64.powf(zoom - f64::from(placed.id.z)));
                match layer.kind {
                    LayerKind::Raster | LayerKind::Hillshade => {
                        if !matches!(tile.content, TileContent::Raster(_) | TileContent::Dem(_)) {
                            continue;
                        }
                        let mut uniform = raster_uniform(layer, &ctx, matrix);
                        uniform.tile = tile_params(*placed);
                        let uniform = rec.push(uniform);
                        rec.steps.push(Step::Draw {
                            program: Program::Raster,
                            uniform,
                            stencil,
                            mesh: MeshRef::Grid(cells(placed)),
                            texture: Some(TextureRef::Tile(tile.clone())),
                        });
                    }
                    _ => {
                        let gpu_tile = gpu_tile(gpu, &self.shared, tile);
                        let Some((program, _)) = gpu_tile.meshes.get(&index) else {
                            continue;
                        };
                        let program = *program;
                        drop(gpu_tile);
                        let mut uniform = match program {
                            Program::Fill => fill_uniform(layer, &ctx, matrix),
                            Program::Line => line_uniform(layer, &ctx, matrix, units),
                            Program::Circle => circle_uniform(layer, &ctx, matrix),
                            Program::Extrusion => extrusion_uniform(layer, &ctx, matrix, view),
                            _ => continue,
                        };
                        uniform.tile = tile_params(*placed);
                        let uniform = rec.push(uniform);
                        rec.steps.push(Step::Draw {
                            program,
                            uniform,
                            stencil,
                            mesh: MeshRef::Tile(tile.clone(), index),
                            texture: None,
                        });
                    }
                }
            }
            if layer.kind == LayerKind::FillExtrusion {
                let color = rec.steps[first_draw..].to_vec();
                for step in &mut rec.steps[first_draw..] {
                    if let Step::Draw { program, .. } = step {
                        *program = Program::ExtrusionDepth;
                    }
                }
                rec.steps.extend(color);
            }
        }
        rec
    }
}

fn layer_color(layer: &Layer, ctx: &Context, color: &str, opacity: &str) -> ([f32; 4], f32, bool) {
    let c = layer.get(color);
    let o = layer.get(opacity);
    let dynamic = c.feature_dependent || o.feature_dependent;
    if dynamic {
        ([0.0; 4], 1.0, true)
    } else {
        (
            c.color(ctx, Rgba::BLACK).premultiplied(1.0),
            o.number(ctx, 1.0) as f32,
            false,
        )
    }
}

fn fill_uniform(layer: &Layer, ctx: &Context, matrix: [[f32; 4]; 4]) -> DrawUniform {
    let (color, opacity, dynamic) = layer_color(layer, ctx, "fill-color", "fill-opacity");
    DrawUniform {
        matrix,
        color,
        params: [f32::from(u8::from(dynamic)), opacity, 0.0, 0.0],
        ..Default::default()
    }
}

fn line_uniform(layer: &Layer, ctx: &Context, matrix: [[f32; 4]; 4], units: f64) -> DrawUniform {
    let (color, opacity, dynamic) = layer_color(layer, ctx, "line-color", "line-opacity");
    let width = layer.get("line-width").number(ctx, 1.0) as f32;
    let gap = layer.get("line-gap-width").number(ctx, 0.0) as f32;
    let blur = layer.get("line-blur").number(ctx, 0.0) as f32;
    let dashes = layer.get("line-dasharray").numbers(ctx);
    let mut dash = [0.0f32; 8];
    for (slot, d) in dash.iter_mut().zip(&dashes) {
        *slot = *d as f32;
    }
    DrawUniform {
        matrix,
        color,
        params: [f32::from(u8::from(dynamic)), opacity, units as f32, width],
        extra: [gap, 0.0, blur, dashes.len().min(8) as f32],
        dash0: [dash[0], dash[1], dash[2], dash[3]],
        dash1: [dash[4], dash[5], dash[6], dash[7]],
        ..Default::default()
    }
}

fn circle_uniform(layer: &Layer, ctx: &Context, matrix: [[f32; 4]; 4]) -> DrawUniform {
    let (color, opacity, dynamic) = layer_color(layer, ctx, "circle-color", "circle-opacity");
    let radius = layer.get("circle-radius").number(ctx, 5.0) as f32;
    let stroke = layer.get("circle-stroke-width").number(ctx, 0.0) as f32;
    let blur = layer.get("circle-blur").number(ctx, 0.0) as f32;
    let stroke_color = layer
        .get("circle-stroke-color")
        .color(ctx, Rgba::TRANSPARENT);
    let stroke_opacity = layer.get("circle-stroke-opacity").number(ctx, 1.0) as f32;
    DrawUniform {
        matrix,
        color,
        params: [f32::from(u8::from(dynamic)), opacity, 0.0, radius],
        extra: [stroke, blur, 0.0, 0.0],
        color2: stroke_color.premultiplied(stroke_opacity),
        ..Default::default()
    }
}

fn raster_uniform(layer: &Layer, ctx: &Context, matrix: [[f32; 4]; 4]) -> DrawUniform {
    if layer.kind == LayerKind::Hillshade {
        let exaggeration = layer.get("hillshade-exaggeration").number(ctx, 0.5) as f32;
        return DrawUniform {
            matrix,
            params: [0.0, exaggeration, 0.0, 0.0],
            extra: [0.0, 1.0, 0.0, 0.0],
            ..Default::default()
        };
    }
    DrawUniform {
        matrix,
        params: [
            0.0,
            layer.get("raster-opacity").number(ctx, 1.0) as f32,
            0.0,
            0.0,
        ],
        extra: [
            layer.get("raster-brightness-min").number(ctx, 0.0) as f32,
            layer.get("raster-brightness-max").number(ctx, 1.0) as f32,
            layer.get("raster-saturation").number(ctx, 0.0) as f32,
            layer.get("raster-contrast").number(ctx, 0.0) as f32,
        ],
        ..Default::default()
    }
}

fn extrusion_uniform(
    layer: &Layer,
    ctx: &Context,
    matrix: [[f32; 4]; 4],
    view: &View,
) -> DrawUniform {
    let c = layer.get("fill-extrusion-color");
    let opacity = layer.get("fill-extrusion-opacity").number(ctx, 1.0) as f32;
    let gradient = layer.get("fill-extrusion-vertical-gradient").bool(ctx);
    DrawUniform {
        matrix,
        color: c.color(ctx, Rgba::BLACK).0,
        params: [
            f32::from(u8::from(c.feature_dependent)),
            opacity,
            view.pixels_per_meter() as f32,
            0.0,
        ],
        extra: [f32::from(u8::from(gradient)), 0.0, 0.0, 0.0],
        ..Default::default()
    }
}

fn light_uniform(style: &StyleState, view: &View) -> ([f32; 4], [f32; 4]) {
    let light = style.light;
    let [radial, azimuth, polar] = light.position;
    let mut azimuth = (azimuth + 90.0).to_radians();
    if light.viewport_anchor {
        azimuth += f64::from(view.camera.bearing).to_radians();
    }
    let polar = polar.to_radians();
    let dir = [
        (radial * azimuth.cos() * polar.sin()) as f32,
        (radial * azimuth.sin() * polar.sin()) as f32,
        (radial * polar.cos()) as f32,
    ];
    let [r, g, b, _] = light.color.0;
    (
        [dir[0], -dir[1], dir[2], light.intensity as f32],
        [r, g, b, 1.0],
    )
}

impl Renderer for MapRenderer {
    fn resize(&mut self, _gpu: &Gpu, size: SurfaceSize) {
        self.size = size;
        self.map.set_size(size);
    }

    fn render(&mut self, gpu: &Gpu, target: &wgpu::TextureView, frame: FrameInfo) {
        let map = self.map.clone();
        let view = map.begin_frame(&frame, self.size);
        let style = map.style();
        let shared = self.shared.clone();

        let mut rec = match &style {
            Some(style) => {
                let tiles = Self::collect_tiles(&view, style);
                self.labels
                    .prepare(gpu, &shared, &map, &view, style, &tiles);
                let rec = self.record(gpu, &view, style, &tiles);
                map.publish_frame(tiles, std::mem::take(&mut self.labels.placed));
                rec
            }
            None => FrameRecorder {
                uniforms: Vec::new(),
                steps: Vec::new(),
            },
        };
        if let Some(style) = &style {
            self.labels.record_overlay(&mut rec, style);
        }
        let (light, light_color) = style
            .as_ref()
            .map(|style| light_uniform(style, &view))
            .unwrap_or(([0.0, 0.0, 1.0, 0.5], [1.0; 4]));
        let frame_uniform = FrameUniform {
            viewport: [
                view.width as f32,
                view.height as f32,
                self.size.scale,
                f32::from(u8::from(self.encode_srgb)),
            ],
            light,
            light_color,
            globe: mat(view.globe_matrix),
            globe_inverse: mat(view.globe_inverse),
            globe_params: [
                view.globe as f32,
                view.horizon as f32,
                view.camera.center.lat.to_radians().cos() as f32,
                view.globe_radius as f32,
            ],
            camera_dir: view.camera_dir.as_vec3().extend(0.0).to_array(),
            halo: HALO,
            space: SPACE,
        };
        gpu.queue
            .write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&frame_uniform));

        let count = rec.uniforms.len().max(1) as u64;
        if count > self.draw_capacity {
            let capacity = count.next_power_of_two();
            (self.draw_buffer, self.draw_bind_group) =
                draw_buffer(gpu, &shared, self.draw_stride, capacity);
            self.draw_capacity = capacity;
        }
        let mut staging = vec![0u8; (self.draw_stride * count) as usize];
        for (i, uniform) in rec.uniforms.iter().enumerate() {
            let offset = i * self.draw_stride as usize;
            let bytes = bytemuck::bytes_of(uniform);
            staging[offset..offset + bytes.len()].copy_from_slice(bytes);
        }
        gpu.queue.write_buffer(&self.draw_buffer, 0, &staging);

        self.prepare_attachments(gpu);
        let Some(attachments) = &self.attachments else {
            return;
        };
        let (view_target, resolve_target) = match &attachments.msaa {
            Some(msaa) => (msaa, Some(target)),
            None => (target, None),
        };
        let clear = map.clear_color();
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("whisker-map"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("whisker-map"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: view_target,
                    depth_slice: None,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
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
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0),
                        store: wgpu::StoreOp::Discard,
                    }),
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.frame_bind_group, &[]);
            pass.set_bind_group(2, &shared.empty_texture, &[]);
            let (format, samples) = (self.format, self.samples);
            let mut current: Option<Program> = None;
            let stride = self.draw_stride;
            let offset = |uniform: u32| (u64::from(uniform) * stride) as u32;
            let mut set_program = |pass: &mut wgpu::RenderPass, program: Program| {
                if current != Some(program) {
                    pass.set_pipeline(&shared.pipeline(gpu, program, format, samples));
                    current = Some(program);
                }
            };
            for step in &rec.steps {
                match step {
                    Step::ClearMask => {
                        set_program(&mut pass, Program::ClearMask);
                        pass.set_stencil_reference(0);
                        pass.set_bind_group(1, &self.draw_bind_group, &[0]);
                        pass.draw(0..3, 0..1);
                    }
                    Step::Mask {
                        uniform,
                        stencil,
                        cells,
                    } => {
                        set_program(&mut pass, Program::Mask);
                        pass.set_stencil_reference(*stencil);
                        pass.set_bind_group(1, &self.draw_bind_group, &[offset(*uniform)]);
                        shared.draw_grid(&mut pass, *cells);
                    }
                    Step::Screen { program, uniform } => {
                        set_program(&mut pass, *program);
                        pass.set_bind_group(1, &self.draw_bind_group, &[offset(*uniform)]);
                        pass.draw(0..3, 0..1);
                    }
                    Step::Draw {
                        program,
                        uniform,
                        stencil,
                        mesh,
                        texture,
                    } => {
                        set_program(&mut pass, *program);
                        pass.set_stencil_reference(*stencil);
                        pass.set_bind_group(1, &self.draw_bind_group, &[offset(*uniform)]);
                        match texture {
                            Some(TextureRef::Tile(tile)) => {
                                let gpu_tile = gpu_tile(gpu, &shared, tile);
                                if let Some(group) = &gpu_tile.raster {
                                    pass.set_bind_group(2, group, &[]);
                                }
                            }
                            Some(TextureRef::Labels(i)) => {
                                if let Some(group) = self.labels.texture(*i) {
                                    pass.set_bind_group(2, group, &[]);
                                }
                            }
                            None => {}
                        }
                        match mesh {
                            MeshRef::Grid(cells) => shared.draw_grid(&mut pass, *cells),
                            MeshRef::Tile(tile, layer) => {
                                let gpu_tile = gpu_tile(gpu, &shared, tile);
                                if let Some((_, mesh)) = gpu_tile.meshes.get(layer) {
                                    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                                    pass.set_index_buffer(
                                        mesh.indices.slice(..),
                                        wgpu::IndexFormat::Uint32,
                                    );
                                    pass.draw_indexed(0..mesh.count, 0, 0..1);
                                }
                            }
                            MeshRef::Labels(i) => {
                                if let Some(mesh) = self.labels.mesh(*i) {
                                    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                                    pass.set_index_buffer(
                                        mesh.indices.slice(..),
                                        wgpu::IndexFormat::Uint32,
                                    );
                                    pass.draw_indexed(0..mesh.count, 0, 0..1);
                                }
                            }
                        }
                        if texture.is_some() {
                            pass.set_bind_group(2, &shared.empty_texture, &[]);
                        }
                    }
                }
            }
        }
        gpu.queue.submit([encoder.finish()]);
        map.end_frame(&view);
    }
}
