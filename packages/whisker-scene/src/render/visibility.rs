//! Which draws a view sees, and the order they are drawn in.

use glam::{Mat4, Vec3, Vec4, Vec4Swizzles};

use super::{Draw, translucent};

/// The six planes bounding a view-projection's clip volume, normals pointing inward.
pub(crate) struct Frustum {
    planes: [Vec4; 6],
}

impl Frustum {
    /// Expects wgpu's clip space, where depth runs from 0 to 1.
    pub(crate) fn new(view_proj: Mat4) -> Frustum {
        let row = |i| view_proj.row(i);
        let planes = [
            row(3) + row(0),
            row(3) - row(0),
            row(3) + row(1),
            row(3) - row(1),
            row(2),
            row(3) - row(2),
        ]
        .map(|plane| plane / plane.xyz().length().max(f32::MIN_POSITIVE));
        Frustum { planes }
    }

    pub(crate) fn intersects_sphere(&self, center: Vec3, radius: f32) -> bool {
        self.planes
            .iter()
            .all(|plane| plane.xyz().dot(center) + plane.w >= -radius)
    }
}

/// A sphere around the draw in world space, or `None` when it cannot be bounded.
pub(crate) fn world_sphere(draw: &Draw) -> Option<(Vec3, f32)> {
    // Skinned and morphed vertices move away from the rest pose the bounds describe.
    if draw.deform.is_some() {
        return None;
    }
    let mut sphere = draw.geometry.bounds();
    if let Some(instances) = &draw.instances {
        let key = draw.geometry.data() as *const _ as usize;
        sphere = instances.bounds(key, sphere);
    }
    let (center, radius) = sphere;
    let scale = [draw.model.x_axis, draw.model.y_axis, draw.model.z_axis]
        .iter()
        .map(|axis| axis.xyz().length())
        .fold(0.0, f32::max);
    Some((draw.model.transform_point3(center), radius * scale))
}

pub(crate) fn visible(draw: &Draw, frustum: &Frustum) -> bool {
    world_sphere(draw).is_none_or(|(center, radius)| frustum.intersects_sphere(center, radius))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Ordered {
    pub(crate) index: usize,
    pub(crate) blended: bool,
}

/// The draws `frustum` can see: opaque ones first, then translucent ones farthest first.
pub(crate) fn draw_order(draws: &[Draw], view: Mat4, frustum: &Frustum) -> Vec<Ordered> {
    let depth = |index: usize| {
        let center = draws[index].model.transform_point3(Vec3::ZERO);
        (view * center.extend(1.0)).z
    };
    let mut order: Vec<Ordered> = draws
        .iter()
        .enumerate()
        .filter(|(_, draw)| visible(draw, frustum))
        .map(|(index, draw)| Ordered {
            index,
            blended: translucent(&draw.material),
        })
        .collect();
    order.sort_by(|a, b| {
        a.blended.cmp(&b.blended).then_with(|| match a.blended {
            true => depth(a.index).total_cmp(&depth(b.index)),
            false => std::cmp::Ordering::Equal,
        })
    });
    order
}

#[cfg(test)]
mod tests;
