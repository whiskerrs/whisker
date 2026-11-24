use std::cell::{Cell, OnceCell};
use std::fmt;
use std::rc::Rc;

use glam::{Mat4, Vec3, Vec4Swizzles};

/// Transforms that draw one mesh many times in a single draw call, each relative to its node.
#[derive(Clone)]
pub struct Instances {
    data: Rc<InstancesData>,
}

pub(crate) struct InstancesData {
    pub(crate) transforms: Vec<Mat4>,
    pub(crate) gpu: OnceCell<crate::render::InstanceBuffer>,
    /// The last geometry sphere bounded, by address, and the sphere around every copy of it.
    bounds: Cell<Option<(usize, Vec3, f32)>>,
}

impl Instances {
    pub fn new(transforms: Vec<Mat4>) -> Instances {
        Instances {
            data: Rc::new(InstancesData {
                transforms,
                gpu: OnceCell::new(),
                bounds: Cell::new(None),
            }),
        }
    }

    pub fn transforms(&self) -> &[Mat4] {
        &self.data.transforms
    }

    pub(crate) fn data(&self) -> &InstancesData {
        &self.data
    }

    /// A sphere around every instance of the sphere `(center, radius)` keyed by `key`.
    pub(crate) fn bounds(&self, key: usize, (center, radius): (Vec3, f32)) -> (Vec3, f32) {
        if let Some((cached, center, radius)) = self.data.bounds.get()
            && cached == key
        {
            return (center, radius);
        }
        let spheres = self.data.transforms.iter().map(|transform| {
            let scale = [transform.x_axis, transform.y_axis, transform.z_axis]
                .iter()
                .map(|axis| axis.xyz().length())
                .fold(0.0, f32::max);
            (transform.transform_point3(center), radius * scale)
        });
        let (min, max) = spheres.clone().fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(min, max), (c, r)| (min.min(c - r), max.max(c + r)),
        );
        let bounds = if self.data.transforms.is_empty() {
            (Vec3::ZERO, 0.0)
        } else {
            let middle = (min + max) / 2.0;
            let reach = spheres
                .map(|(c, r)| c.distance(middle) + r)
                .fold(0.0, f32::max);
            (middle, reach)
        };
        self.data.bounds.set(Some((key, bounds.0, bounds.1)));
        bounds
    }
}

impl PartialEq for Instances {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.data, &other.data)
    }
}

impl fmt::Debug for Instances {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Instances")
            .field(&self.data.transforms.len())
            .finish()
    }
}
