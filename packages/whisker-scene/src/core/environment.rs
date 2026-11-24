//! Image-based lighting: a panorama of the light arriving from every direction, prefiltered on the
//! CPU into what the shader samples.

use std::cell::OnceCell;
use std::f32::consts::PI;
use std::rc::Rc;

use glam::{Vec2, Vec3};
use whisker::css::Color;

use super::TextureError;
use super::color::linear_rgba;

/// Width and height of the sharpest prefiltered cube face.
pub(crate) const CUBE_SIZE: u32 = 128;
/// Cube mip levels, from mirror-smooth at level 0 to fully rough at the last.
pub(crate) const CUBE_LEVELS: u32 = 6;
const SAMPLES: u32 = 64;

/// Light arriving from every direction, given as an equirectangular panorama.
///
/// The panorama's center faces -Z and its top +Y.
#[derive(Clone)]
pub struct Environment {
    data: Rc<EnvironmentData>,
}

pub(crate) struct EnvironmentData {
    width: u32,
    height: u32,
    /// Linear radiance, row by row from the top.
    pixels: Vec<Vec3>,
    baked: OnceCell<Baked>,
    pub(crate) gpu: OnceCell<crate::render::GpuTexture>,
}

/// What the shader samples, derived once from the panorama.
pub(crate) struct Baked {
    /// Irradiance over π as nine spherical-harmonic coefficients, ready to evaluate.
    pub(crate) irradiance: [Vec3; 9],
    /// Each level's six faces, in the +X, -X, +Y, -Y, +Z, -Z order, row by row.
    pub(crate) levels: Vec<Vec<Vec3>>,
}

impl PartialEq for Environment {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.data, &other.data)
    }
}

impl std::fmt::Debug for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Environment")
            .field("width", &self.data.width)
            .field("height", &self.data.height)
            .finish()
    }
}

impl Environment {
    /// Decodes a Radiance HDR file, or a PNG, JPEG or WebP read as sRGB.
    pub fn decode(bytes: &[u8]) -> Result<Environment, TextureError> {
        let image = image::load_from_memory(bytes).map_err(TextureError)?;
        let hdr = matches!(
            image.color(),
            image::ColorType::Rgb32F | image::ColorType::Rgba32F
        );
        let image = image.into_rgb32f();
        let (width, height) = image.dimensions();
        let decode = |c: f32| {
            if hdr {
                c
            } else if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let pixels = image
            .pixels()
            .map(|p| Vec3::new(decode(p[0]), decode(p[1]), decode(p[2])))
            .collect();
        Ok(Environment::from_radiance(width, height, pixels))
    }

    /// A panorama of linear radiance, row by row from the top.
    ///
    /// # Panics
    ///
    /// If `pixels` is not `width * height` long, or either side is zero.
    pub fn from_radiance(width: u32, height: u32, pixels: Vec<Vec3>) -> Environment {
        assert!(
            width > 0 && height > 0,
            "an environment needs at least one pixel"
        );
        assert_eq!(
            pixels.len(),
            width as usize * height as usize,
            "pixels must be width * height values"
        );
        Environment {
            data: Rc::new(EnvironmentData {
                width,
                height,
                pixels,
                baked: OnceCell::new(),
                gpu: OnceCell::new(),
            }),
        }
    }

    /// A sky fading from `sky` overhead to `horizon`, over `ground` below.
    pub fn gradient(sky: Color, horizon: Color, ground: Color) -> Environment {
        let rgb = |color| {
            let [r, g, b, _] = linear_rgba(color);
            Vec3::new(r, g, b)
        };
        let (sky, horizon, ground) = (rgb(sky), rgb(horizon), rgb(ground));
        let (width, height) = (64, 32);
        let pixels = (0..height)
            .flat_map(|y| {
                let up = ((y as f32 + 0.5) / height as f32 * PI).cos();
                let color = if up >= 0.0 {
                    horizon.lerp(sky, up.sqrt())
                } else {
                    horizon.lerp(ground, (-up * 8.0).min(1.0))
                };
                (0..width).map(move |_| color)
            })
            .collect();
        Environment::from_radiance(width, height, pixels)
    }

    pub(crate) fn data(&self) -> &EnvironmentData {
        &self.data
    }
}

impl EnvironmentData {
    pub(crate) fn baked(&self) -> &Baked {
        self.baked.get_or_init(|| {
            let pyramid = pyramid(self.width, self.height, &self.pixels);
            Baked {
                irradiance: irradiance(&pyramid),
                levels: (0..CUBE_LEVELS)
                    .map(|level| prefilter(&pyramid, level))
                    .collect(),
            }
        })
    }
}

/// One level of a panorama.
struct Level {
    width: u32,
    height: u32,
    pixels: Vec<Vec3>,
}

impl Level {
    fn texel(&self, x: i64, y: i64) -> Vec3 {
        let x = x.rem_euclid(i64::from(self.width)) as u32;
        let y = y.clamp(0, i64::from(self.height) - 1) as u32;
        self.pixels[(y * self.width + x) as usize]
    }

    fn bilinear(&self, uv: Vec2) -> Vec3 {
        let x = uv.x * self.width as f32 - 0.5;
        let y = uv.y * self.height as f32 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let top = self.texel(x0, y0).lerp(self.texel(x0 + 1, y0), fx);
        let bottom = self.texel(x0, y0 + 1).lerp(self.texel(x0 + 1, y0 + 1), fx);
        top.lerp(bottom, fy)
    }
}

/// The panorama and each half-size level below it, averaged 2×2, down to one row.
fn pyramid(width: u32, height: u32, pixels: &[Vec3]) -> Vec<Level> {
    let mut levels = vec![Level {
        width,
        height,
        pixels: pixels.to_vec(),
    }];
    while let Some(last) = levels.last()
        && last.height > 1
    {
        let (width, height) = ((last.width / 2).max(1), last.height / 2);
        let pixels = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .map(|(x, y)| {
                let (x, y) = (i64::from(x) * 2, i64::from(y) * 2);
                (last.texel(x, y)
                    + last.texel(x + 1, y)
                    + last.texel(x, y + 1)
                    + last.texel(x + 1, y + 1))
                    / 4.0
            })
            .collect();
        levels.push(Level {
            width,
            height,
            pixels,
        });
    }
    levels
}

/// Where `direction` lands on the panorama.
pub(crate) fn equirect_uv(direction: Vec3) -> Vec2 {
    let d = direction.normalize_or(Vec3::NEG_Z);
    Vec2::new(
        0.5 + d.x.atan2(-d.z) / (2.0 * PI),
        d.y.clamp(-1.0, 1.0).acos() / PI,
    )
}

fn sample(pyramid: &[Level], direction: Vec3, lod: f32) -> Vec3 {
    let uv = equirect_uv(direction);
    let lod = lod.clamp(0.0, (pyramid.len() - 1) as f32);
    let (low, t) = (lod.floor() as usize, lod.fract());
    let a = pyramid[low].bilinear(uv);
    if t == 0.0 || low + 1 >= pyramid.len() {
        return a;
    }
    a.lerp(pyramid[low + 1].bilinear(uv), t)
}

/// The direction through texel (`x`, `y`) of cube `face` at `size`.
pub(crate) fn cube_direction(face: usize, x: u32, y: u32, size: u32) -> Vec3 {
    let u = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
    let v = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
    match face {
        0 => Vec3::new(1.0, -v, -u),
        1 => Vec3::new(-1.0, -v, u),
        2 => Vec3::new(u, 1.0, v),
        3 => Vec3::new(u, -1.0, -v),
        4 => Vec3::new(u, -v, 1.0),
        _ => Vec3::new(-u, -v, -1.0),
    }
    .normalize()
}

fn hammersley(index: u32, count: u32) -> Vec2 {
    Vec2::new(
        index as f32 / count as f32,
        index.reverse_bits() as f32 * 2.328_306_4e-10,
    )
}

/// One cube level, GGX-filtered for the roughness the level stands for.
///
/// Each sample reads the pyramid level whose texels match the sample's share of the lobe, so few
/// samples give a smooth result.
fn prefilter(pyramid: &[Level], level: u32) -> Vec<Vec3> {
    let size = (CUBE_SIZE >> level).max(1);
    let roughness = level as f32 / (CUBE_LEVELS - 1) as f32;
    let source = &pyramid[0];
    let texel_angle = 4.0 * PI / (source.width * source.height) as f32;
    let mut out = Vec::with_capacity((size * size * 6) as usize);
    for face in 0..6 {
        for y in 0..size {
            for x in 0..size {
                let n = cube_direction(face, x, y, size);
                if level == 0 {
                    let lod = 0.5 * (texel_angle_at(size) / texel_angle).log2();
                    out.push(sample(pyramid, n, lod));
                    continue;
                }
                out.push(ggx_filter(pyramid, n, roughness, texel_angle));
            }
        }
    }
    out
}

/// The solid angle of a texel of a cube face at `size`, near the face's center.
fn texel_angle_at(size: u32) -> f32 {
    4.0 * PI / (6 * size * size) as f32
}

fn ggx_filter(pyramid: &[Level], n: Vec3, roughness: f32, texel_angle: f32) -> Vec3 {
    let alpha = (roughness * roughness).max(1e-3);
    let up = if n.y.abs() < 0.999 { Vec3::Y } else { Vec3::X };
    let tangent = up.cross(n).normalize();
    let bitangent = n.cross(tangent);
    let (mut sum, mut weight) = (Vec3::ZERO, 0.0);
    for index in 0..SAMPLES {
        let xi = hammersley(index, SAMPLES);
        let phi = 2.0 * PI * xi.x;
        let cos_theta = ((1.0 - xi.y) / (1.0 + (alpha * alpha - 1.0) * xi.y)).sqrt();
        let sin_theta = (1.0 - cos_theta * cos_theta).sqrt();
        let h =
            tangent * (sin_theta * phi.cos()) + bitangent * (sin_theta * phi.sin()) + n * cos_theta;
        let l = 2.0 * n.dot(h) * h - n;
        let n_dot_l = n.dot(l);
        if n_dot_l <= 0.0 {
            continue;
        }
        // With the view along the normal, the pdf of l is D(h) / 4.
        let a2 = alpha * alpha;
        let d = cos_theta * cos_theta * (a2 - 1.0) + 1.0;
        let pdf = a2 / (PI * d * d) / 4.0;
        let sample_angle = 1.0 / (SAMPLES as f32 * pdf);
        let lod = 0.5 * (sample_angle / texel_angle).log2() + 1.0;
        sum += sample(pyramid, l, lod) * n_dot_l;
        weight += n_dot_l;
    }
    sum / weight.max(1e-6)
}

/// Irradiance over π, so that an even environment of radiance L evaluates to L.
fn irradiance(pyramid: &[Level]) -> [Vec3; 9] {
    // A coarse level holds the low frequencies nine coefficients can represent.
    let level = pyramid
        .iter()
        .find(|level| level.width <= 64)
        .unwrap_or(&pyramid[pyramid.len() - 1]);
    let mut projected = [Vec3::ZERO; 9];
    for y in 0..level.height {
        let theta = (y as f32 + 0.5) / level.height as f32 * PI;
        let (top, bottom) = (
            (y as f32 / level.height as f32 * PI).cos(),
            ((y + 1) as f32 / level.height as f32 * PI).cos(),
        );
        let angle = (2.0 * PI / level.width as f32) * (top - bottom);
        for x in 0..level.width {
            let u = (x as f32 + 0.5) / level.width as f32;
            let phi = (u - 0.5) * 2.0 * PI;
            let d = Vec3::new(
                theta.sin() * phi.sin(),
                theta.cos(),
                -theta.sin() * phi.cos(),
            );
            let radiance = level.pixels[(y * level.width + x) as usize] * angle;
            for (coefficient, basis) in projected.iter_mut().zip(basis(d)) {
                *coefficient += radiance * basis;
            }
        }
    }
    // Convolving with the clamped cosine scales each band by π, 2π/3 and π/4, here over π.
    const BAND: [f32; 9] = [
        1.0,
        2.0 / 3.0,
        2.0 / 3.0,
        2.0 / 3.0,
        0.25,
        0.25,
        0.25,
        0.25,
        0.25,
    ];
    let mut out = [Vec3::ZERO; 9];
    for (index, slot) in out.iter_mut().enumerate() {
        *slot = projected[index] * BAND[index] * BASIS_SCALE[index];
    }
    out
}

/// The real spherical-harmonic basis up to the second band, without its constant factors.
fn basis(d: Vec3) -> [f32; 9] {
    let raw = [
        1.0,
        d.y,
        d.z,
        d.x,
        d.x * d.y,
        d.y * d.z,
        3.0 * d.z * d.z - 1.0,
        d.x * d.z,
        d.x * d.x - d.y * d.y,
    ];
    let mut out = [0.0; 9];
    for (index, value) in raw.into_iter().enumerate() {
        out[index] = value * BASIS_SCALE[index];
    }
    out
}

const BASIS_SCALE: [f32; 9] = [
    0.282_095, 0.488_603, 0.488_603, 0.488_603, 1.092_548, 1.092_548, 0.315_392, 1.092_548,
    0.546_274,
];

/// Evaluates baked irradiance the way the shader does.
#[cfg(test)]
pub(crate) fn evaluate_irradiance(irradiance: &[Vec3; 9], d: Vec3) -> Vec3 {
    let raw = [
        1.0,
        d.y,
        d.z,
        d.x,
        d.x * d.y,
        d.y * d.z,
        3.0 * d.z * d.z - 1.0,
        d.x * d.z,
        d.x * d.x - d.y * d.y,
    ];
    irradiance
        .iter()
        .zip(raw)
        .map(|(coefficient, value)| *coefficient * value)
        .sum()
}

#[cfg(test)]
mod tests;
