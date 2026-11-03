use glam::{Vec3, Vec4};

use crate::Transform;

/// The most point and spot lights a frame shades with, chosen nearest the camera first.
pub(crate) const MAX_LIGHTS: usize = 16;

/// Light from one point in every direction, used as a component next to a [`Transform`].
///
/// It casts no shadows. `color * intensity` is the light on a white surface facing it from one
/// unit away, in the same scale as [`Sun::color`](crate::Sun::color), and it falls with the square of
/// the distance, reaching nothing at `range`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointLight {
    /// Linear RGB.
    pub color: Vec3,
    pub intensity: f32,
    /// How far the light reaches, in world units.
    pub range: f32,
}

impl Default for PointLight {
    /// A warm bulb that lights a small room.
    fn default() -> Self {
        Self {
            color: Vec3::new(1.0, 0.85, 0.65),
            intensity: 4.0,
            range: 10.0,
        }
    }
}

/// Light from one point in a cone along its [`Transform`]'s -Z, used as a component next to it.
///
/// It casts no shadows, and its brightness follows [`PointLight`]'s rule. Inside `inner_angle` from
/// the axis it is full, and it fades to nothing at `outer_angle`, both in radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpotLight {
    /// Linear RGB.
    pub color: Vec3,
    pub intensity: f32,
    /// How far the light reaches, in world units.
    pub range: f32,
    pub inner_angle: f32,
    pub outer_angle: f32,
}

impl Default for SpotLight {
    /// A white flashlight.
    fn default() -> Self {
        Self {
            color: Vec3::ONE,
            intensity: 20.0,
            range: 20.0,
            inner_angle: 0.3,
            outer_angle: 0.5,
        }
    }
}

/// One light as the shader reads it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LightUniform {
    /// Position, then range.
    position: Vec4,
    /// Color times intensity, then the cosine of the outer angle, below -1 for a point light.
    color: Vec4,
    /// The axis a spot points along, then the cosine of the inner angle.
    direction: Vec4,
}

impl LightUniform {
    pub(crate) fn point(transform: &Transform, light: &PointLight) -> Self {
        Self {
            position: transform.translation.extend(light.range),
            color: (light.color * light.intensity).extend(-2.0),
            direction: Vec4::ZERO,
        }
    }

    pub(crate) fn spot(transform: &Transform, light: &SpotLight) -> Self {
        let outer = light.outer_angle.max(1e-3);
        Self {
            position: transform.translation.extend(light.range),
            color: (light.color * light.intensity).extend(outer.cos()),
            direction: (transform.rotation * Vec3::NEG_Z)
                .extend(light.inner_angle.min(outer - 1e-3).cos()),
        }
    }

    pub(crate) fn center(&self) -> Vec3 {
        self.position.truncate()
    }

    pub(crate) fn range(&self) -> f32 {
        self.position.w
    }
}
