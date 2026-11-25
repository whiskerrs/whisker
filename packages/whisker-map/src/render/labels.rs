//! Symbol layers: text and icons placed in screen space each frame.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::DVec2;
use web_time::Instant;
use whisker_wgpu::{Gpu, wgpu};

use super::{
    DrawUniform, FrameRecorder, GpuMesh, MeshRef, Program, Shared, Step, TextureRef, TileSet,
};
use crate::camera::View;
use crate::geo::PlacedTile;
use crate::map::{MapInner, StyleState};
use crate::sprite::SpriteImage;
use crate::style::color::Rgba;
use crate::style::expr::Context;
use crate::style::{Layer, LayerKind};
use crate::symbols::SymbolFeature;
use crate::text::{BORDER, ONE_EM, Shaping, shape};
use crate::tiles::bucket::EXTENT;
use crate::tiles::{LoadedTile, TileContent};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default, Debug)]
pub(crate) struct SymbolVertex {
    /// Logical pixels from the top left of the view.
    pub pos: [f32; 2],
    /// Texels in the glyph atlas or sprite sheet.
    pub uv: [f32; 2],
    pub color: [f32; 4],
    pub halo: [f32; 4],
    /// x 1 for SDF glyphs, y font scale, z halo width, w opacity.
    pub params: [f32; 4],
    /// x halo blur.
    pub extra: [f32; 2],
}

pub(crate) const SYMBOL_ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x2
];

const ATLAS_SIZE: u32 = 2048;
const FADE_SECONDS: f32 = 0.3;
/// How far outside the view, in logical pixels, labels are still considered.
const VIEW_MARGIN: f64 = 100.0;

/// Glyph bitmaps packed into one single-channel texture as they are first drawn.
struct GlyphAtlas {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    rects: HashMap<(Arc<str>, u32), [u32; 4]>,
    shelves: Vec<Shelf>,
    next_y: u32,
    full: bool,
}

struct Shelf {
    y: u32,
    height: u32,
    x: u32,
}

impl GlyphAtlas {
    fn new(gpu: &Gpu, shared: &Shared) -> GlyphAtlas {
        let size = ATLAS_SIZE.min(gpu.device.limits().max_texture_dimension_2d);
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("whisker-map glyphs"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let bind_group =
            super::texture_bind_group(gpu, &shared.texture_layout, &shared.linear, &texture);
        GlyphAtlas {
            texture,
            bind_group,
            rects: HashMap::new(),
            shelves: Vec::new(),
            next_y: 0,
            full: false,
        }
    }

    fn size(&self) -> u32 {
        self.texture.width()
    }

    fn reset(&mut self) {
        self.rects.clear();
        self.shelves.clear();
        self.next_y = 0;
        self.full = false;
    }

    /// The glyph's rect in the atlas, uploading it on first use.
    fn rect(&mut self, gpu: &Gpu, font: &Arc<str>, glyph: &crate::text::Glyph) -> Option<[u32; 4]> {
        if let Some(rect) = self.rects.get(&(font.clone(), glyph.id)) {
            return Some(*rect);
        }
        if glyph.width == 0 {
            return None;
        }
        let (w, h) = (glyph.width + 2 * BORDER, glyph.height + 2 * BORDER);
        let (x, y) = self.allocate(w + 1, h + 1)?;
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &glyph.bitmap,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let rect = [x, y, w, h];
        self.rects.insert((font.clone(), glyph.id), rect);
        Some(rect)
    }

    fn allocate(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        let size = self.size();
        if let Some(shelf) = self
            .shelves
            .iter_mut()
            .find(|s| s.height >= h && s.height <= h + 4 && s.x + w <= size)
        {
            let x = shelf.x;
            shelf.x += w;
            return Some((x, shelf.y));
        }
        if self.next_y + h > size {
            self.full = true;
            return None;
        }
        let y = self.next_y;
        self.next_y += h;
        self.shelves.push(Shelf { y, height: h, x: w });
        Some((0, y))
    }
}

/// Screen-space boxes already taken this frame, bucketed by cell.
struct CollisionGrid {
    cells: HashMap<(i32, i32), Vec<usize>>,
    boxes: Vec<[f32; 4]>,
}

const CELL: f32 = 64.0;

impl CollisionGrid {
    fn new() -> CollisionGrid {
        CollisionGrid {
            cells: HashMap::new(),
            boxes: Vec::new(),
        }
    }

    fn cells(b: [f32; 4]) -> impl Iterator<Item = (i32, i32)> {
        let (x0, y0) = ((b[0] / CELL).floor() as i32, (b[1] / CELL).floor() as i32);
        let (x1, y1) = ((b[2] / CELL).floor() as i32, (b[3] / CELL).floor() as i32);
        (y0..=y1).flat_map(move |y| (x0..=x1).map(move |x| (x, y)))
    }

    fn hits(&self, b: [f32; 4]) -> bool {
        Self::cells(b).any(|cell| {
            self.cells.get(&cell).is_some_and(|ids| {
                ids.iter().any(|&i| {
                    let o = self.boxes[i];
                    b[0] < o[2] && o[0] < b[2] && b[1] < o[3] && o[1] < b[3]
                })
            })
        })
    }

    fn insert(&mut self, b: [f32; 4]) {
        let id = self.boxes.len();
        self.boxes.push(b);
        for cell in Self::cells(b) {
            self.cells.entry(cell).or_default().push(id);
        }
    }
}

/// One layer's quads for one texture.
#[derive(Default)]
struct Batch {
    vertices: Vec<SymbolVertex>,
    indices: Vec<u32>,
}

impl Batch {
    fn quad(&mut self, corners: [DVec2; 4], uv: [f32; 4], template: SymbolVertex) {
        let base = self.vertices.len() as u32;
        let uvs = [
            [uv[0], uv[1]],
            [uv[0] + uv[2], uv[1]],
            [uv[0], uv[1] + uv[3]],
            [uv[0] + uv[2], uv[1] + uv[3]],
        ];
        for (corner, uv) in corners.iter().zip(uvs) {
            self.vertices.push(SymbolVertex {
                pos: [corner.x as f32, corner.y as f32],
                uv,
                ..template
            });
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
    }
}

/// A symbol layer's properties at this frame's zoom.
struct LayerProps {
    text_size: f32,
    icon_size: f32,
    text_color: Rgba,
    halo_color: [f32; 4],
    halo_width: f32,
    halo_blur: f32,
    text_opacity: f32,
    icon_opacity: f32,
    text_allow: bool,
    icon_allow: bool,
    text_ignore: bool,
    icon_ignore: bool,
    text_optional: bool,
    icon_optional: bool,
    text_padding: f32,
    icon_padding: f32,
    text_follows_line: bool,
    icon_follows_line: bool,
    max_angle: f64,
    icon_anchor: crate::text::Anchor,
    icon_offset: [f32; 2],
    icon_rotate: f64,
}

impl LayerProps {
    fn new(layer: &Layer, ctx: &Context) -> LayerProps {
        let offset = layer.get("icon-offset").numbers(ctx);
        let follows_line =
            |name: &str| !matches!(layer.get(name).string(ctx).as_deref(), Some("viewport"));
        LayerProps {
            text_size: layer.get("text-size").number(ctx, 16.0) as f32,
            icon_size: layer.get("icon-size").number(ctx, 1.0) as f32,
            text_color: layer.get("text-color").color(ctx, Rgba::BLACK),
            halo_color: premultiplied(
                layer.get("text-halo-color").color(ctx, Rgba::TRANSPARENT),
                1.0,
            ),
            halo_width: layer.get("text-halo-width").number(ctx, 0.0) as f32,
            halo_blur: layer.get("text-halo-blur").number(ctx, 0.0) as f32,
            text_opacity: layer.get("text-opacity").number(ctx, 1.0) as f32,
            icon_opacity: layer.get("icon-opacity").number(ctx, 1.0) as f32,
            text_allow: layer.get("text-allow-overlap").bool(ctx),
            icon_allow: layer.get("icon-allow-overlap").bool(ctx),
            text_ignore: layer.get("text-ignore-placement").bool(ctx),
            icon_ignore: layer.get("icon-ignore-placement").bool(ctx),
            text_optional: layer.get("text-optional").bool(ctx),
            icon_optional: layer.get("icon-optional").bool(ctx),
            text_padding: layer.get("text-padding").number(ctx, 2.0) as f32,
            icon_padding: layer
                .get("icon-padding")
                .numbers(ctx)
                .first()
                .copied()
                .unwrap_or(2.0) as f32,
            text_follows_line: follows_line("text-rotation-alignment"),
            icon_follows_line: follows_line("icon-rotation-alignment"),
            max_angle: layer.get("text-max-angle").number(ctx, 45.0).to_radians(),
            icon_anchor: crate::text::Anchor::parse(
                &layer.get("icon-anchor").string(ctx).unwrap_or_default(),
            ),
            icon_offset: [
                offset.first().copied().unwrap_or(0.0) as f32,
                offset.get(1).copied().unwrap_or(0.0) as f32,
            ],
            icon_rotate: layer.get("icon-rotate").number(ctx, 0.0).to_radians(),
        }
    }
}

/// A label drawn this frame, for finding what a tap hit.
pub(crate) struct PlacedLabel {
    pub bounds: [f32; 4],
    pub source: String,
    pub tile: Rc<LoadedTile>,
    pub symbol: usize,
}

struct Fade {
    opacity: f32,
    seen: u64,
}

pub(crate) struct LabelRenderer {
    atlas: Option<GlyphAtlas>,
    sprite_groups: Vec<(wgpu::BindGroup, [f32; 2])>,
    meshes: Vec<Option<GpuMesh>>,
    textures: Vec<wgpu::BindGroup>,
    /// Per style layer: the mesh, texture, and texture size of each draw, icons first.
    draws: HashMap<usize, Vec<(usize, usize, [f32; 2])>>,
    fades: HashMap<u64, Fade>,
    frame: u64,
    last: Option<Instant>,
    pub placed: Vec<PlacedLabel>,
}

impl LabelRenderer {
    pub fn new() -> LabelRenderer {
        LabelRenderer {
            atlas: None,
            sprite_groups: Vec::new(),
            meshes: Vec::new(),
            textures: Vec::new(),
            draws: HashMap::new(),
            fades: HashMap::new(),
            frame: 0,
            last: None,
            placed: Vec::new(),
        }
    }

    pub fn prepare(
        &mut self,
        gpu: &Gpu,
        shared: &Rc<Shared>,
        map: &Rc<MapInner>,
        view: &View,
        style: &StyleState,
        tiles: &TileSet,
    ) {
        self.meshes.clear();
        self.draws.clear();
        self.placed.clear();
        self.frame += 1;
        let now = Instant::now();
        let dt = self
            .last
            .replace(now)
            .map_or(0.0, |last| now.duration_since(last).as_secs_f32().min(0.1));
        let atlas = self
            .atlas
            .get_or_insert_with(|| GlyphAtlas::new(gpu, shared));
        if atlas.full {
            atlas.reset();
        }
        style
            .sprites
            .load(&style.sprite, map.logical_scale(), style.on_change.clone());
        let sheets = style.sprites.sheets.borrow().clone();
        while self.sprite_groups.len() < sheets.len() {
            let sheet = &sheets[self.sprite_groups.len()];
            let (_, group) = super::upload_texture(
                gpu,
                &shared.texture_layout,
                &shared.linear,
                sheet.width,
                sheet.height,
                &sheet.pixels,
                wgpu::TextureFormat::Rgba8Unorm,
            );
            self.sprite_groups
                .push((group, [sheet.width as f32, sheet.height as f32]));
        }
        self.textures.clear();
        self.textures.push(atlas.bind_group.clone());
        for (group, _) in &self.sprite_groups {
            self.textures.push(group.clone());
        }
        let atlas_size = atlas.size() as f32;

        let zoom = view.zoom();
        let ctx = Context::zoom(zoom);
        let mut grid = CollisionGrid::new();
        let mut animating = false;
        let mut placer = Placer {
            gpu,
            view,
            style,
            atlas: self.atlas.as_mut().expect("atlas"),
            grid: &mut grid,
            fades: &mut self.fades,
            frame: self.frame,
            dt,
            animating: &mut animating,
        };
        let mut layer_batches: Vec<(usize, Vec<(usize, Batch)>)> = Vec::new();
        let mut placed = Vec::new();
        for (index, layer) in style.layers.iter().enumerate().rev() {
            if layer.kind != LayerKind::Symbol || !layer.shown_at(zoom) {
                continue;
            }
            let Some(source_id) = layer.source.as_deref() else {
                continue;
            };
            let Some((ideal, source_tiles)) = tiles.get(source_id) else {
                continue;
            };
            let props = LayerProps::new(layer, &ctx);
            let mut batches: HashMap<usize, Batch> = HashMap::new();
            for (tile_placed, tile) in source_tiles.iter().filter(|(p, _)| p.id.z == *ideal) {
                let TileContent::Vector { symbols, .. } = &tile.content else {
                    continue;
                };
                for (symbol_index, symbol) in symbols.iter().enumerate() {
                    if symbol.layer != index {
                        continue;
                    }
                    for bounds in placer.place(&props, *tile_placed, symbol, &mut batches) {
                        placed.push(PlacedLabel {
                            bounds,
                            source: source_id.to_string(),
                            tile: tile.clone(),
                            symbol: symbol_index,
                        });
                    }
                }
            }
            let mut batches: Vec<(usize, Batch)> = batches
                .into_iter()
                .filter(|(_, b)| !b.indices.is_empty())
                .collect();
            // Icons draw under text, as MapLibre does.
            batches.sort_by_key(|(texture, _)| std::cmp::Reverse(*texture));
            layer_batches.push((index, batches));
        }
        let frame = self.frame;
        self.fades.retain(|_, fade| fade.seen == frame);
        for (index, batches) in layer_batches {
            let mut draws = Vec::new();
            for (texture, batch) in batches {
                let size = if texture == 0 {
                    [atlas_size, atlas_size]
                } else {
                    self.sprite_groups[texture - 1].1
                };
                self.meshes
                    .push(Some(GpuMesh::new(gpu, &batch.vertices, &batch.indices)));
                draws.push((self.meshes.len() - 1, texture, size));
            }
            self.draws.insert(index, draws);
        }
        self.placed = placed;
        if animating {
            map.keep_drawing();
        }
    }

    pub fn record_layer(&mut self, index: usize, rec: &mut FrameRecorder) {
        let Some(draws) = self.draws.get(&index) else {
            return;
        };
        for &(mesh, texture, size) in draws {
            let uniform = rec.push(DrawUniform {
                params: [size[0], size[1], 0.0, 0.0],
                ..Default::default()
            });
            rec.steps.push(Step::Draw {
                program: Program::Symbol,
                uniform,
                stencil: 0,
                mesh: MeshRef::Labels(mesh),
                texture: Some(TextureRef::Labels(texture)),
            });
        }
    }

    pub fn record_overlay(&mut self, _rec: &mut FrameRecorder, _style: &StyleState) {}

    pub fn texture(&self, index: usize) -> Option<&wgpu::BindGroup> {
        self.textures.get(index)
    }

    pub fn mesh(&self, index: usize) -> Option<&GpuMesh> {
        self.meshes.get(index).and_then(Option::as_ref)
    }
}

struct Placer<'a> {
    gpu: &'a Gpu,
    view: &'a View,
    style: &'a StyleState,
    atlas: &'a mut GlyphAtlas,
    grid: &'a mut CollisionGrid,
    fades: &'a mut HashMap<u64, Fade>,
    frame: u64,
    dt: f32,
    animating: &'a mut bool,
}

fn premultiplied(color: Rgba, opacity: f32) -> [f32; 4] {
    color.premultiplied(opacity)
}

fn padded(b: [f32; 4], padding: f32) -> [f32; 4] {
    [
        b[0] - padding,
        b[1] - padding,
        b[2] + padding,
        b[3] + padding,
    ]
}

fn union(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

impl Placer<'_> {
    /// Places each of the symbol's anchors that fits, returning the bounds of those placed.
    fn place(
        &mut self,
        props: &LayerProps,
        tile: PlacedTile,
        symbol: &SymbolFeature,
        batches: &mut HashMap<usize, Batch>,
    ) -> Vec<[f32; 4]> {
        let mut placed = Vec::new();
        let shaping = match &symbol.text {
            Some(text) => match symbol.shaping.get() {
                Some(shaping) => Some(shaping.clone()),
                None => {
                    let Some(store) = &self.style.glyphs else {
                        return placed;
                    };
                    let wrap = !symbol.along_line;
                    let Some(shaping) =
                        shape(text, &symbol.font, store, &symbol.layout.for_shaping(wrap))
                    else {
                        return placed;
                    };
                    let shaping = Arc::new(shaping);
                    let _ = symbol.shaping.set(shaping.clone());
                    Some(shaping)
                }
            },
            None => None,
        };
        let icon = match &symbol.icon {
            Some(name) => match self.style.sprites.image(name) {
                Some(image) => Some(image),
                None if self.style.sprites.sheets.borrow().is_empty()
                    && !self.style.sprite.is_empty() =>
                {
                    return placed;
                }
                None => None,
            },
            None => None,
        };
        if shaping.is_none() && icon.is_none() {
            return placed;
        }
        let text_size = symbol.text_size.unwrap_or(props.text_size);
        let font_scale = text_size / ONE_EM;
        let icon_size = symbol.icon_size.unwrap_or(props.icon_size);
        let text_color = premultiplied(symbol.text_color.unwrap_or(props.text_color), 1.0);
        let along_path = symbol.along_line && props.text_follows_line;
        let icon_follows_line = symbol.along_line && props.icon_follows_line;

        let anchors = if symbol.line.is_some() {
            let text_length = shaping
                .as_ref()
                .map_or(0.0, |s| (s.right - s.left) * font_scale);
            let icon_length = icon.map_or(0.0, |i| i.width as f32 / i.pixel_ratio * icon_size);
            let overscale =
                1u32 << (self.view.zoom().floor() as i32 - i32::from(tile.id.z)).clamp(0, 16);
            symbol.anchors_along(overscale, text_length.max(icon_length))
        } else {
            symbol.anchors.clone()
        };
        for anchor in &anchors {
            let Some(screen) =
                self.view
                    .tile_point_to_screen(tile, anchor.point, f64::from(EXTENT))
            else {
                continue;
            };
            if screen.x < -VIEW_MARGIN
                || screen.y < -VIEW_MARGIN
                || screen.x > self.view.width + VIEW_MARGIN
                || screen.y > self.view.height + VIEW_MARGIN
            {
                continue;
            }
            let key = label_key(symbol, tile, anchor.point);
            let line_screen = if symbol.along_line {
                symbol.line.as_ref().and_then(|line| {
                    line.iter()
                        .map(|p| self.view.tile_point_to_screen(tile, *p, f64::from(EXTENT)))
                        .collect::<Option<Vec<DVec2>>>()
                })
            } else {
                None
            };
            let line_angle = line_screen.as_ref().map_or(0.0, |line| {
                let a = line[anchor.segment];
                let b = line[(anchor.segment + 1).min(line.len() - 1)];
                (b - a).y.atan2((b - a).x)
            });

            // Text quads and collision boxes.
            let mut text_quads: Vec<([DVec2; 4], [f32; 4])> = Vec::new();
            let mut text_boxes: Vec<[f32; 4]> = Vec::new();
            if let Some(shaping) = &shaping {
                if along_path {
                    let Some(line) = &line_screen else { continue };
                    match self.along_path(
                        shaping,
                        font_scale,
                        line,
                        anchor.segment,
                        screen,
                        props.max_angle,
                    ) {
                        Some((quads, boxes)) => {
                            text_quads = quads;
                            text_boxes = boxes
                                .into_iter()
                                .map(|b| padded(b, props.text_padding))
                                .collect();
                        }
                        None => continue,
                    }
                } else {
                    for glyph in &shaping.glyphs {
                        let Some(rect) = self.atlas.rect(self.gpu, &glyph.font, &glyph.glyph)
                        else {
                            continue;
                        };
                        let q = Shaping::quad(glyph);
                        let corner = |x: f32, y: f32| {
                            screen
                                + DVec2::new(f64::from(x * font_scale), f64::from(y * font_scale))
                        };
                        text_quads.push((
                            [
                                corner(q[0], q[1]),
                                corner(q[2], q[1]),
                                corner(q[0], q[3]),
                                corner(q[2], q[3]),
                            ],
                            [
                                rect[0] as f32,
                                rect[1] as f32,
                                rect[2] as f32,
                                rect[3] as f32,
                            ],
                        ));
                    }
                    let (x, y) = (screen.x as f32, screen.y as f32);
                    text_boxes.push(padded(
                        [
                            x + shaping.left * font_scale,
                            y + shaping.top * font_scale,
                            x + shaping.right * font_scale,
                            y + shaping.bottom * font_scale,
                        ],
                        props.text_padding,
                    ));
                }
            }

            // Icon quad and collision box.
            let mut icon_quad: Option<([DVec2; 4], [f32; 4], SpriteImage)> = None;
            if let Some(image) = icon {
                let w = image.width as f32 / image.pixel_ratio * icon_size;
                let h = image.height as f32 / image.pixel_ratio * icon_size;
                let a = props.icon_anchor;
                let (ox, oy) = (
                    props.icon_offset[0] * icon_size,
                    props.icon_offset[1] * icon_size,
                );
                let x0 = -a.horizontal * w + ox;
                let y0 = -a.vertical * h + oy;
                let rotate = props.icon_rotate + if icon_follows_line { line_angle } else { 0.0 };
                let (sin, cos) = rotate.sin_cos();
                let corner = |x: f32, y: f32| {
                    let (x, y) = (f64::from(x), f64::from(y));
                    screen + DVec2::new(x * cos - y * sin, x * sin + y * cos)
                };
                let corners = [
                    corner(x0, y0),
                    corner(x0 + w, y0),
                    corner(x0, y0 + h),
                    corner(x0 + w, y0 + h),
                ];
                icon_quad = Some((
                    corners,
                    [
                        image.x as f32,
                        image.y as f32,
                        image.width as f32,
                        image.height as f32,
                    ],
                    image,
                ));
            }
            let icon_box = icon_quad.as_ref().map(|(corners, _, _)| {
                let b = corners
                    .iter()
                    .fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, c| {
                        [
                            b[0].min(c.x as f32),
                            b[1].min(c.y as f32),
                            b[2].max(c.x as f32),
                            b[3].max(c.y as f32),
                        ]
                    });
                padded(b, props.icon_padding)
            });

            let has_text = !text_quads.is_empty();
            let has_icon = icon_quad.is_some();
            let mut place_text =
                has_text && (props.text_allow || !text_boxes.iter().any(|b| self.grid.hits(*b)));
            let mut place_icon =
                has_icon && (props.icon_allow || !icon_box.is_some_and(|b| self.grid.hits(b)));
            if has_text && has_icon {
                if !props.text_optional && !props.icon_optional {
                    let both = place_text && place_icon;
                    place_text = both;
                    place_icon = both;
                } else if !props.text_optional {
                    place_icon = place_icon && place_text;
                } else if !props.icon_optional {
                    place_text = place_text && place_icon;
                }
            }
            if !place_text && !place_icon {
                continue;
            }
            if place_text && !props.text_ignore {
                for b in &text_boxes {
                    self.grid.insert(*b);
                }
            }
            if place_icon
                && !props.icon_ignore
                && let Some(b) = icon_box
            {
                self.grid.insert(b);
            }

            let fade = self.fades.entry(key).or_insert(Fade {
                opacity: 0.0,
                seen: 0,
            });
            fade.opacity = (fade.opacity + self.dt / FADE_SECONDS).min(1.0);
            fade.seen = self.frame;
            if fade.opacity < 1.0 {
                *self.animating = true;
            }
            let opacity = fade.opacity;

            let mut bounds: Option<[f32; 4]> = None;
            if place_icon && let Some((corners, uv, image)) = icon_quad {
                let batch = batches.entry(image.sheet + 1).or_default();
                batch.quad(
                    corners,
                    uv,
                    SymbolVertex {
                        params: [0.0, 1.0, 0.0, opacity * props.icon_opacity],
                        ..Default::default()
                    },
                );
                bounds = icon_box;
            }
            if place_text {
                let batch = batches.entry(0).or_default();
                let template = SymbolVertex {
                    color: text_color,
                    halo: props.halo_color,
                    params: [
                        1.0,
                        font_scale,
                        props.halo_width,
                        opacity * props.text_opacity,
                    ],
                    extra: [props.halo_blur, 0.0],
                    ..Default::default()
                };
                for (corners, uv) in &text_quads {
                    batch.quad(*corners, *uv, template);
                }
                for b in &text_boxes {
                    bounds = Some(bounds.map_or(*b, |o| union(o, *b)));
                }
            }
            if let Some(bounds) = bounds {
                placed.push(bounds);
            }
        }
        placed
    }

    /// Glyph quads that follow the projected line through `anchor`, kept upright, with one
    /// collision box per glyph; `None` when the label runs off the line or bends too sharply.
    #[allow(clippy::type_complexity)]
    fn along_path(
        &mut self,
        shaping: &Shaping,
        font_scale: f32,
        line: &[DVec2],
        segment: usize,
        anchor: DVec2,
        max_angle: f64,
    ) -> Option<(Vec<([DVec2; 4], [f32; 4])>, Vec<[f32; 4]>)> {
        let a = line[segment];
        let b = line[(segment + 1).min(line.len() - 1)];
        let flip = (b - a).x < 0.0;
        let scale = f64::from(font_scale);
        let mut quads = Vec::new();
        let mut boxes = Vec::new();
        let mut previous_angle: Option<f64> = None;
        for glyph in &shaping.glyphs {
            let rect = self.atlas.rect(self.gpu, &glyph.font, &glyph.glyph);
            let center = f64::from(glyph.x + glyph.glyph.advance / 2.0) * scale;
            let distance = if flip { -center } else { center };
            let (point, mut angle) = walk(line, segment, anchor, distance)?;
            if flip {
                angle += std::f64::consts::PI;
            }
            if let Some(previous) = previous_angle {
                let mut d = angle - previous;
                while d > std::f64::consts::PI {
                    d -= std::f64::consts::TAU;
                }
                while d < -std::f64::consts::PI {
                    d += std::f64::consts::TAU;
                }
                if d.abs() > max_angle {
                    return None;
                }
            }
            previous_angle = Some(angle);
            let half = f64::from(ONE_EM) * scale / 2.0;
            boxes.push([
                (point.x - half) as f32,
                (point.y - half) as f32,
                (point.x + half) as f32,
                (point.y + half) as f32,
            ]);
            let Some(rect) = rect else { continue };
            let q = Shaping::quad(glyph);
            let (sin, cos) = angle.sin_cos();
            let corner = |x: f32, y: f32| {
                let x = f64::from(x) * scale - center;
                let y = f64::from(y) * scale;
                point + DVec2::new(x * cos - y * sin, x * sin + y * cos)
            };
            quads.push((
                [
                    corner(q[0], q[1]),
                    corner(q[2], q[1]),
                    corner(q[0], q[3]),
                    corner(q[2], q[3]),
                ],
                [
                    rect[0] as f32,
                    rect[1] as f32,
                    rect[2] as f32,
                    rect[3] as f32,
                ],
            ));
        }
        Some((quads, boxes))
    }
}

/// The point `distance` along `line` from `anchor`, which lies on `line[segment]..line[segment + 1]`,
/// and the direction of the segment it lands on.
fn walk(line: &[DVec2], segment: usize, anchor: DVec2, distance: f64) -> Option<(DVec2, f64)> {
    let direction = |a: DVec2, b: DVec2| (b - a).y.atan2((b - a).x);
    let mut remaining = distance.abs();
    let mut current = anchor;
    if distance >= 0.0 {
        let mut next = segment + 1;
        while next < line.len() {
            let to = line[next];
            let length = (to - current).length();
            if remaining <= length {
                let from = line[next - 1];
                let k = if length > 0.0 {
                    remaining / length
                } else {
                    0.0
                };
                return Some((current + (to - current) * k, direction(from, to)));
            }
            remaining -= length;
            current = to;
            next += 1;
        }
        None
    } else {
        let mut previous = segment as isize;
        while previous >= 0 {
            let to = line[previous as usize];
            let length = (to - current).length();
            if remaining <= length {
                let after = line[(previous as usize + 1).min(line.len() - 1)];
                let k = if length > 0.0 {
                    remaining / length
                } else {
                    0.0
                };
                return Some((current + (to - current) * k, direction(to, after)));
            }
            remaining -= length;
            current = to;
            previous -= 1;
        }
        None
    }
}

fn label_key(symbol: &SymbolFeature, tile: PlacedTile, point: [f32; 2]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    symbol.layer.hash(&mut hasher);
    symbol.text.hash(&mut hasher);
    symbol.icon.hash(&mut hasher);
    let n = f64::from(1u32 << tile.id.z);
    let x =
        (f64::from(tile.id.x) + f64::from(tile.wrap) * n + f64::from(point[0]) / f64::from(EXTENT))
            / n;
    let y = (f64::from(tile.id.y) + f64::from(point[1]) / f64::from(EXTENT)) / n;
    ((x * 1e6).round() as i64).hash(&mut hasher);
    ((y * 1e6).round() as i64).hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_along_a_polyline() {
        let line = [
            DVec2::new(0.0, 0.0),
            DVec2::new(10.0, 0.0),
            DVec2::new(10.0, 10.0),
        ];
        let anchor = DVec2::new(5.0, 0.0);
        let (p, angle) = walk(&line, 0, anchor, 8.0).unwrap();
        assert!((p - DVec2::new(10.0, 3.0)).length() < 1e-9);
        assert!((angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        let (p, angle) = walk(&line, 0, anchor, -4.0).unwrap();
        assert!((p - DVec2::new(1.0, 0.0)).length() < 1e-9);
        assert!(angle.abs() < 1e-9);
        assert!(walk(&line, 0, anchor, 30.0).is_none());
        assert!(walk(&line, 0, anchor, -6.0).is_none());
    }

    #[test]
    fn collision_grid_finds_overlaps() {
        let mut grid = CollisionGrid::new();
        grid.insert([0.0, 0.0, 100.0, 20.0]);
        assert!(grid.hits([90.0, 10.0, 200.0, 30.0]));
        assert!(!grid.hits([101.0, 0.0, 200.0, 20.0]));
        assert!(!grid.hits([-50.0, 21.0, -1.0, 40.0]));
    }
}
