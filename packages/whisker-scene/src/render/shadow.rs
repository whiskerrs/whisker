//! Shadow maps: which layers each shadowing light renders into, and the depth pass that fills them.

use std::f32::consts::FRAC_PI_2;

use glam::{Mat4, Vec3};
use whisker_wgpu::{Gpu, wgpu};

use super::collect::{Collected, Cone};
use super::pipeline::alpha_tested;
use super::resources::bind_mesh;
use super::visibility::{Frustum, visible, world_sphere};
use super::{Draw, MAX_DIRECTIONAL, MAX_POINT, SceneRenderer};
use crate::core::{Projection, Shadow};

/// Layers of the one shadow map array, spent as directional cascades and point-light faces.
pub(super) const MAX_SHADOW_LAYERS: usize = 16;
pub(super) const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const MAX_CASCADES: usize = 4;
/// Weight of logarithmic over even cascade splits.
const SPLIT_LAMBDA: f32 = 0.75;

/// Directions and up vectors of a point light's faces, in the order the shader picks them.
const FACES: [(Vec3, Vec3); 6] = [
    (Vec3::X, Vec3::NEG_Y),
    (Vec3::NEG_X, Vec3::NEG_Y),
    (Vec3::Y, Vec3::Z),
    (Vec3::NEG_Y, Vec3::NEG_Z),
    (Vec3::Z, Vec3::NEG_Y),
    (Vec3::NEG_Z, Vec3::NEG_Y),
];

pub(super) struct Cascades {
    pub(super) first: usize,
    pub(super) count: usize,
    /// The view depth each cascade reaches.
    pub(super) splits: [f32; MAX_CASCADES],
    /// World size of one texel in each cascade.
    pub(super) texel: [f32; MAX_CASCADES],
    pub(super) shadow: Shadow,
}

/// Six cube faces for a point light, or one layer for a spot light.
pub(super) struct PointFaces {
    pub(super) first: usize,
    pub(super) spot: bool,
    pub(super) shadow: Shadow,
}

/// Where each light's shadow maps sit among the layers, and the view-projection of each layer.
#[derive(Default)]
pub(super) struct ShadowPlan {
    pub(super) size: u32,
    pub(super) layers: Vec<Mat4>,
    pub(super) directional: Vec<Option<Cascades>>,
    pub(super) point: Vec<Option<PointFaces>>,
}

/// The camera a frame is drawn from.
#[derive(Clone, Copy)]
pub(super) struct Eye {
    pub(super) world: Mat4,
    pub(super) projection: Projection,
    pub(super) aspect: f32,
}

/// Lights past the layer budget, or past the shader's light limits, cast no shadow.
pub(super) fn plan(collected: &Collected, eye: Eye, max_size: u32) -> ShadowPlan {
    let directional = &collected.directional[..collected.directional.len().min(MAX_DIRECTIONAL)];
    let point = &collected.point[..collected.point.len().min(MAX_POINT)];
    let size = directional
        .iter()
        .map(|light| light.shadow)
        .chain(point.iter().map(|light| light.shadow))
        .flatten()
        .map(|shadow| shadow.map_size)
        .max()
        .unwrap_or(0)
        .clamp(1, max_size.max(1));
    let casters: Vec<(Vec3, f32)> = collected
        .draws
        .iter()
        .filter(|draw| draw.cast_shadow)
        .filter_map(world_sphere)
        .collect();
    let mut plan = ShadowPlan {
        size,
        ..ShadowPlan::default()
    };
    for light in directional {
        let cascades = light.shadow.and_then(|shadow| {
            let count = (shadow.cascades as usize).clamp(1, MAX_CASCADES);
            if plan.layers.len() + count > MAX_SHADOW_LAYERS {
                return None;
            }
            let (near, far) = depth_range(eye.projection);
            let reach = shadow.distance.clamp(near * 2.0, far);
            let splits = splits(near, reach, count);
            let mut texel = [0.0; MAX_CASCADES];
            let first = plan.layers.len();
            let mut start = near;
            for (cascade, end) in splits.iter().take(count).enumerate() {
                let (matrix, size) = cascade_matrix(eye, start, *end, light.toward, size, &casters);
                plan.layers.push(matrix);
                texel[cascade] = size;
                start = *end;
            }
            Some(Cascades {
                first,
                count,
                splits,
                texel,
                shadow,
            })
        });
        plan.directional.push(cascades);
    }
    for light in point {
        let faces = light.shadow.and_then(|shadow| {
            let needed = if light.cone.is_some() { 1 } else { FACES.len() };
            if plan.layers.len() + needed > MAX_SHADOW_LAYERS {
                return None;
            }
            let first = plan.layers.len();
            match light.cone {
                Some(cone) => plan
                    .layers
                    .push(spot_matrix(light.position, light.range, cone)),
                None => plan
                    .layers
                    .extend(face_matrices(light.position, light.range)),
            }
            Some(PointFaces {
                first,
                spot: light.cone.is_some(),
                shadow,
            })
        });
        plan.point.push(faces);
    }
    plan
}

fn depth_range(projection: Projection) -> (f32, f32) {
    match projection {
        Projection::Perspective { near, far, .. } | Projection::Orthographic { near, far, .. } => {
            (near.max(1e-4), far.max(near + 1e-3))
        }
    }
}

/// The view depth each of `count` cascades reaches, blending logarithmic and even spacing.
pub(super) fn splits(near: f32, far: f32, count: usize) -> [f32; MAX_CASCADES] {
    let mut splits = [far; MAX_CASCADES];
    for (index, split) in splits.iter_mut().enumerate().take(count) {
        let t = (index + 1) as f32 / count as f32;
        let logarithmic = near * (far / near).powf(t);
        let even = near + (far - near) * t;
        *split = SPLIT_LAMBDA * logarithmic + (1.0 - SPLIT_LAMBDA) * even;
    }
    splits
}

/// The corners, in view space, of the slice of the view between depths `near` and `far`.
fn slice_corners(eye: Eye, near: f32, far: f32) -> [Vec3; 8] {
    let half = |depth: f32| match eye.projection {
        Projection::Perspective { fov_y, .. } => {
            let y = depth * (fov_y / 2.0).tan();
            (y * eye.aspect, y)
        }
        Projection::Orthographic { height, .. } => (height / 2.0 * eye.aspect, height / 2.0),
    };
    let mut corners = [Vec3::ZERO; 8];
    for (slice, depth) in [near, far].into_iter().enumerate() {
        let (x, y) = half(depth);
        for (corner, (sx, sy)) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)]
            .into_iter()
            .enumerate()
        {
            corners[slice * 4 + corner] = Vec3::new(sx * x, sy * y, -depth);
        }
    }
    corners
}

fn light_up(direction: Vec3) -> Vec3 {
    if direction.y.abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    }
}

fn light_view(toward: Vec3) -> Mat4 {
    Mat4::look_to_rh(Vec3::ZERO, -toward, light_up(toward))
}

/// Fits one cascade around a view slice, returning its view-projection and texel size.
///
/// The fit is a sphere snapped to whole texels, so the map neither resizes as the camera turns nor
/// shifts by fractions of a texel as it moves, either of which makes shadow edges shimmer.
pub(super) fn cascade_matrix(
    eye: Eye,
    near: f32,
    far: f32,
    toward: Vec3,
    size: u32,
    casters: &[(Vec3, f32)],
) -> (Mat4, f32) {
    let corners = slice_corners(eye, near, far);
    let center = corners.iter().sum::<Vec3>() / 8.0;
    let radius = corners
        .iter()
        .map(|corner| corner.distance(center))
        .fold(0.0, f32::max);
    let radius = (radius * 16.0).ceil() / 16.0;
    let view = light_view(toward);
    let mut center = view.transform_point3(eye.world.transform_point3(center));
    let texel = 2.0 * radius / size as f32;
    center.x = (center.x / texel).floor() * texel;
    center.y = (center.y / texel).floor() * texel;
    // Casters between the slice and the light shadow it, so the near plane reaches back to them.
    let mut nearest = -center.z - radius;
    for (caster, caster_radius) in casters {
        let caster = view.transform_point3(*caster);
        let reach = radius + caster_radius;
        if (caster.x - center.x).abs() <= reach && (caster.y - center.y).abs() <= reach {
            nearest = nearest.min(-caster.z - caster_radius);
        }
    }
    let projection = Mat4::orthographic_rh(
        center.x - radius,
        center.x + radius,
        center.y - radius,
        center.y + radius,
        nearest,
        -center.z + radius,
    );
    (projection * view, texel)
}

fn local_near(range: f32) -> f32 {
    (range * 0.01).clamp(1e-3, 0.05)
}

fn face_matrices(position: Vec3, range: f32) -> [Mat4; 6] {
    let range = range.max(1e-3);
    let projection = Mat4::perspective_rh(FRAC_PI_2, 1.0, local_near(range), range);
    FACES.map(|(direction, up)| projection * Mat4::look_to_rh(position, direction, up))
}

pub(super) fn spot_matrix(position: Vec3, range: f32, cone: Cone) -> Mat4 {
    let range = range.max(1e-3);
    let fov = (cone.outer_angle * 2.0).clamp(0.01, 3.0);
    let projection = Mat4::perspective_rh(fov, 1.0, local_near(range), range);
    projection * Mat4::look_to_rh(position, cone.direction, light_up(cone.direction))
}

pub(super) struct ShadowMaps {
    pub(super) size: u32,
    pub(super) view: wgpu::TextureView,
    layer_views: Vec<wgpu::TextureView>,
}

impl ShadowMaps {
    pub(super) fn new(gpu: &Gpu, size: u32, layers: usize) -> ShadowMaps {
        // GLES makes a single-layer texture a plain 2D one and a square one with a multiple of six
        // layers a cube map, both of which an array binding rejects.
        let layers = match layers.max(2) {
            layers if layers % 6 == 0 => layers + 1,
            layers => layers,
        } as u32;
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("whisker-scene shadow maps"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SHADOW_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let layer_views = (0..layers)
            .map(|layer| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: layer,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        ShadowMaps {
            size,
            view,
            layer_views,
        }
    }

    pub(super) fn fits(&self, plan: &ShadowPlan) -> bool {
        plan.layers.is_empty()
            || (self.size == plan.size && self.layer_views.len() >= plan.layers.len())
    }
}

impl SceneRenderer {
    /// Renders every caster's depth into each layer of `plan`, culled against that layer.
    pub(super) fn encode_shadows(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        draws: &[Draw],
        plan: &ShadowPlan,
    ) {
        if plan.layers.is_empty() {
            return;
        }
        let stride = self.shadow_view_stride as usize;
        self.staging.clear();
        self.staging.resize(stride * plan.layers.len(), 0);
        for (layer, matrix) in plan.layers.iter().enumerate() {
            let columns = matrix.to_cols_array();
            let bytes: &[u8] = bytemuck::cast_slice(&columns);
            self.staging[layer * stride..layer * stride + bytes.len()].copy_from_slice(bytes);
        }
        gpu.queue
            .write_buffer(&self.shadow_view_buffer, 0, &self.staging);
        let shared = self.shared.clone();
        for (layer, matrix) in plan.layers.iter().enumerate() {
            let frustum = Frustum::new(*matrix);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("whisker-scene shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_maps.layer_views[layer],
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.shadow_view_bind_group, &[(layer * stride) as u32]);
            let mut bound = None;
            for (index, draw) in draws.iter().enumerate() {
                if !draw.cast_shadow || !visible(draw, &frustum) {
                    continue;
                }
                let Some((count, instances)) = bind_mesh(gpu, &shared, &mut pass, draw) else {
                    continue;
                };
                let tested = alpha_tested(&draw.material);
                if bound != Some(tested) {
                    pass.set_pipeline(shared.shadow_pipeline(gpu, tested));
                    bound = Some(tested);
                }
                let offset = (index as u64 * self.draw_stride) as u32;
                pass.set_bind_group(1, &self.draw_bind_group, &[offset]);
                pass.set_bind_group(
                    2,
                    self.materials.bind_group(gpu, &shared, &draw.material),
                    &[],
                );
                pass.draw_indexed(0..count, 0, 0..instances);
            }
        }
    }
}

#[cfg(test)]
mod tests;
