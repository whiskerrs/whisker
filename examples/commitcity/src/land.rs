//! The countryside around the town: river, paddies, cedar hills, mountain ranges and a far peak.

use std::f32::consts::TAU;

use whisker_scene::math::{Mat4, Quat, Vec2, Vec3};
use whisker_scene::{CustomMaterial, Geometry, Material, MaterialShader, Texture};

/// Ground level of the plain, just under the street so the two never z-fight.
pub const PLAIN: f32 = -0.02;
pub const WATER: f32 = -0.3;

const EXTENT_X: f32 = 230.0;
const NEAR_Z: f32 = 110.0;
const FAR_Z: f32 = -240.0;
const STEP: f32 = 2.0;
const MAP: u32 = 512;

/// The volcano on the horizon, centre and height.
const PEAK: (f32, f32, f32) = (55.0, -330.0, 120.0);

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn hash(x: i32, z: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (z as u32).wrapping_mul(0xd816_3841)
        ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 16_777_216.0
}

fn noise(x: f32, z: f32, seed: u32) -> f32 {
    let (x0, z0) = (x.floor(), z.floor());
    let (fx, fz) = (x - x0, z - z0);
    let (sx, sz) = (fx * fx * (3.0 - 2.0 * fx), fz * fz * (3.0 - 2.0 * fz));
    let (ix, iz) = (x0 as i32, z0 as i32);
    let a = hash(ix, iz, seed) + (hash(ix + 1, iz, seed) - hash(ix, iz, seed)) * sx;
    let b = hash(ix, iz + 1, seed) + (hash(ix + 1, iz + 1, seed) - hash(ix, iz + 1, seed)) * sx;
    a + (b - a) * sz
}

fn fbm(x: f32, z: f32, octaves: u32) -> f32 {
    let (mut sum, mut amplitude, mut frequency, mut total) = (0.0, 0.5, 1.0, 0.0);
    for octave in 0..octaves {
        sum += noise(x * frequency, z * frequency, 7 + octave) * amplitude;
        total += amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }
    sum / total
}

/// Folded noise, sharp along ridgelines.
fn ridged(x: f32, z: f32) -> f32 {
    let (mut sum, mut amplitude, mut frequency) = (0.0, 0.5, 1.0);
    for octave in 0..5 {
        let n = 1.0 - (noise(x * frequency, z * frequency, 31 + octave) * 2.0 - 1.0).abs();
        sum += n * n * amplitude;
        amplitude *= 0.5;
        frequency *= 2.05;
    }
    sum
}

pub fn river_z(x: f32) -> f32 {
    12.0 + 2.2 * (x / 19.0).sin() + 0.9 * (x / 7.5 + 1.0).sin()
}

/// How far `(x, z)` lies inside the cedar woods, 0 outside.
pub fn woods(x: f32, z: f32) -> f32 {
    let slope = smoothstep(-24.0, -34.0, z).max(smoothstep(85.0, 110.0, x.abs()));
    let patches = smoothstep(0.38, 0.55, fbm(x / 16.0, z / 16.0, 3));
    slope * patches * smoothstep(-150.0, -120.0, z)
}

pub fn height(x: f32, z: f32) -> f32 {
    let bank = smoothstep(1.6, 3.4, (z - river_z(x)).abs());
    let bed = bank - 1.0;
    let back = smoothstep(-22.0, -70.0, z);
    let sides = smoothstep(80.0, 150.0, x.abs());
    let front = smoothstep(55.0, 100.0, z);
    let hills = (0.25 + fbm(x / 26.0, z / 26.0, 4)) * 14.0 * back.max(sides).max(front * 0.6);
    let range = smoothstep(-90.0, -170.0, z) * (10.0 + 70.0 * ridged(x / 70.0, z / 70.0));
    PLAIN + bed + hills + range
}

fn normal(x: f32, z: f32) -> Vec3 {
    let e = 0.6;
    let dx = height(x + e, z) - height(x - e, z);
    let dz = height(x, z + e) - height(x, z - e);
    Vec3::new(-dx, 2.0 * e, -dz).normalize()
}

fn map_uv(x: f32, z: f32) -> Vec2 {
    Vec2::new(
        (x + EXTENT_X) / (2.0 * EXTENT_X),
        (z - FAR_Z) / (NEAR_Z - FAR_Z),
    )
}

pub fn terrain() -> Geometry {
    let columns = (2.0 * EXTENT_X / STEP) as u32 + 1;
    let rows = ((NEAR_Z - FAR_Z) / STEP) as u32 + 1;
    let (mut positions, mut normals, mut uvs) = (Vec::new(), Vec::new(), Vec::new());
    for r in 0..rows {
        for c in 0..columns {
            let (x, z) = (-EXTENT_X + c as f32 * STEP, FAR_Z + r as f32 * STEP);
            positions.push(Vec3::new(x, height(x, z), z));
            normals.push(normal(x, z));
            uvs.push(map_uv(x, z));
        }
    }
    let mut indices = Vec::new();
    for r in 0..rows - 1 {
        for c in 0..columns - 1 {
            let i = r * columns + c;
            indices.extend([i, i + columns, i + 1, i + 1, i + columns, i + columns + 1]);
        }
    }
    Geometry::from_vertices(positions, normals, Some(uvs), indices)
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

fn rgb(hex: u32) -> [f32; 3] {
    [16, 8, 0].map(|shift| ((hex >> shift) & 0xff) as f32 / 255.0)
}

/// The ground's color over the whole terrain, in sRGB.
pub fn color_map() -> Texture {
    let (grass, dry, forest, rock, earth) = (
        rgb(0x5d7a33),
        rgb(0x8c8a48),
        rgb(0x2f4128),
        rgb(0x6f6a5c),
        rgb(0x8a7556),
    );
    let mut pixels = Vec::with_capacity((MAP * MAP * 4) as usize);
    for row in 0..MAP {
        for column in 0..MAP {
            let x = -EXTENT_X + (column as f32 + 0.5) / MAP as f32 * 2.0 * EXTENT_X;
            let z = FAR_Z + (row as f32 + 0.5) / MAP as f32 * (NEAR_Z - FAR_Z);
            let h = height(x, z);
            let n = normal(x, z);
            let mut color = mix(grass, dry, smoothstep(0.35, 0.75, fbm(x / 9.0, z / 9.0, 3)));
            color = mix(
                color,
                forest,
                woods(x, z).max(smoothstep(8.0, 22.0, h) * 0.8),
            );
            color = mix(color, rock, smoothstep(0.82, 0.62, n.y));
            color = mix(
                color,
                earth,
                1.0 - smoothstep(1.7, 2.6, (z - river_z(x)).abs()),
            );
            let shade = 0.88 + 0.24 * fbm(x / 2.5, z / 2.5, 2);
            pixels.extend(color.map(|c| ((c * shade).clamp(0.0, 1.0) * 255.0) as u8));
            pixels.push(255);
        }
    }
    Texture::from_rgba8(MAP, MAP, pixels)
}

/// A tiling grey texture of blades and pebbles that breaks up the color map up close.
pub fn detail_map() -> Texture {
    const SIZE: u32 = 128;
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let u = x as f32 / SIZE as f32 * 16.0;
            let v = y as f32 / SIZE as f32 * 16.0;
            let wrap = |u: f32, v: f32| {
                let (a, b) = (noise(u, v, 91), noise(u - 16.0, v, 91));
                let (c, d) = (noise(u, v - 16.0, 91), noise(u - 16.0, v - 16.0, 91));
                let (s, t) = (u / 16.0, v / 16.0);
                (a * (1.0 - s) + b * s) * (1.0 - t) + (c * (1.0 - s) + d * s) * t
            };
            let fine = wrap(u * 1.0, v * 1.0) * 0.5 + wrap(u * 2.0 % 16.0, v * 2.0 % 16.0) * 0.5;
            let value = (0.75 + 0.5 * fine).clamp(0.0, 1.0);
            let byte = (value * 200.0) as u8;
            pixels.extend([byte, byte, byte, 255]);
        }
    }
    Texture::from_rgba8(SIZE, SIZE, pixels)
}

const GROUND_SHADER: &str = "
fn surface(input: SurfaceInput) -> Surface {
    var s = standard_surface(input);
    let base = pow(textureSample(texture0, material_sampler, input.uv).rgb, vec3(2.2));
    let near = textureSample(texture1, material_sampler, input.world.xz * draw.custom[0].x).r;
    let far = textureSample(texture1, material_sampler, input.world.xz * draw.custom[0].y).r;
    s.base_color = vec4(base * mix(near, far, 0.5) * 1.1, 1.0);
    s.roughness = 0.95;
    s.metallic = 0.0;
    return s;
}
";

pub fn ground_material() -> Material {
    let shader = MaterialShader::new(GROUND_SHADER).expect("ground shader");
    let mut material = CustomMaterial::new(shader);
    material.params[0].x = 0.45;
    material.params[0].y = 0.07;
    material.textures = vec![color_map(), detail_map()];
    Material::Custom(material)
}

const WATER_SHADER: &str = "
fn surface(input: SurfaceInput) -> Surface {
    var s = standard_surface(input);
    let t = frame.time.x;
    let p = input.world.xz;
    let a = sin(p.x * 1.3 + p.y * 0.4 + t * 1.1) + sin(p.x * 0.5 - p.y * 1.7 + t * 0.7);
    let b = cos(p.y * 1.9 + t * 1.3) + cos(p.x * 2.3 + p.y * 0.9 - t * 0.9);
    s.normal = normalize(vec3(a * draw.custom[1].x, b * draw.custom[1].x, 1.0));
    s.base_color = vec4(draw.custom[0].rgb, 1.0);
    s.roughness = draw.custom[1].y;
    s.metallic = 0.0;
    return s;
}
";

/// Rippling water tinted `color`, waves `strength` deep, at `roughness`.
pub fn water_material(color: [f32; 3], strength: f32, roughness: f32) -> Material {
    let shader = MaterialShader::new(WATER_SHADER).expect("water shader");
    let mut material = CustomMaterial::new(shader);
    material.params[0] = whisker_scene::math::Vec4::new(color[0], color[1], color[2], 1.0);
    material.params[1].x = strength;
    material.params[1].y = roughness;
    Material::Custom(material)
}

pub fn river() -> (Geometry, Vec3) {
    (
        Geometry::plane(Vec2::new(2.0 * EXTENT_X, 12.0)),
        Vec3::new(0.0, WATER, 12.0),
    )
}

/// The volcano on the horizon, snow above its shoulders.
pub fn peak() -> (Geometry, Geometry) {
    const RINGS: u32 = 40;
    const SEGMENTS: u32 = 96;
    let (cx, cz, top) = PEAK;
    let radius = 260.0;
    let profile = |t: f32| top * (1.0 - t).powf(1.7) * (1.0 - 0.04 * (t * 9.0).sin());
    let mut parts: [(Vec<Vec3>, Vec<Vec3>, Vec<u32>); 2] = Default::default();
    let point = |ring: u32, segment: u32| {
        let t = 0.03 + ring as f32 / RINGS as f32 * 0.97;
        let angle = segment as f32 / SEGMENTS as f32 * TAU;
        let wobble = 1.0 + 0.06 * fbm(angle.cos() * 3.0 + 5.0, angle.sin() * 3.0, 3);
        let r = t * radius * wobble;
        Vec3::new(cx + angle.cos() * r, profile(t) - 8.0, cz + angle.sin() * r)
    };
    for ring in 0..RINGS {
        for segment in 0..SEGMENTS {
            let quad = [
                point(ring, segment),
                point(ring, segment + 1),
                point(ring + 1, segment + 1),
                point(ring + 1, segment),
            ];
            let gully = noise(segment as f32 * 0.5, 3.0, 17);
            let snowline = top * (0.62 - 0.12 * gully);
            let snowy = (quad[0].y + quad[3].y) / 2.0 > snowline;
            let (positions, normals, indices) = &mut parts[usize::from(!snowy)];
            for triangle in [[quad[0], quad[2], quad[1]], [quad[0], quad[3], quad[2]]] {
                let n = (triangle[1] - triangle[0])
                    .cross(triangle[2] - triangle[0])
                    .normalize();
                for p in triangle {
                    indices.push(positions.len() as u32);
                    positions.push(p);
                    normals.push(n);
                }
            }
        }
    }
    let [snow, rock] = parts.map(|(positions, normals, indices)| {
        Geometry::from_vertices(positions, normals, None, indices)
    });
    (snow, rock)
}

fn in_town(x: f32, z: f32) -> bool {
    x.abs() < 46.0 && z > -24.0 && z < 7.5
}

fn near_river(x: f32, z: f32, margin: f32) -> bool {
    (z - river_z(x)).abs() < 3.4 + margin
}

fn on_plain(x: f32, z: f32) -> bool {
    (height(x, z) - PLAIN).abs() < 0.02
}

/// Paddies on the plain, split into young and ripening rice, and the earth bunds between them.
pub fn paddies() -> ([Vec<Mat4>; 2], Vec<Mat4>) {
    let (width, depth, bund) = (4.2, 2.8, 0.16);
    let (mut fields, mut bunds) = (<[Vec<Mat4>; 2]>::default(), Vec::new());
    let mut z = -20.0;
    let mut row = 0;
    while z < 60.0 {
        let shift = hash(row, 0, 5) * width;
        let mut x = -120.0 + shift;
        while x < 120.0 {
            let (cx, cz) = (x + width / 2.0, z + depth / 2.0);
            let corners = [
                (x, z),
                (x + width, z),
                (x, z + depth),
                (x + width, z + depth),
            ];
            if !in_town(cx, cz)
                && !near_river(cx, cz, 1.2)
                && corners.iter().all(|&(px, pz)| on_plain(px, pz))
                && hash((x * 3.0) as i32, (z * 3.0) as i32, 9) > 0.12
            {
                let crop = usize::from(hash((x * 7.0) as i32, (z * 7.0) as i32, 13) > 0.7);
                fields[crop].push(Mat4::from_scale_rotation_translation(
                    Vec3::new(width - bund, 1.0, depth - bund),
                    Quat::IDENTITY,
                    Vec3::new(cx, PLAIN + 0.01, cz),
                ));
                for (size, at) in [
                    (Vec3::new(width, 0.07, bund), Vec3::new(cx, PLAIN, z)),
                    (Vec3::new(bund, 0.07, depth), Vec3::new(x, PLAIN, cz)),
                ] {
                    bunds.push(Mat4::from_scale_rotation_translation(
                        size,
                        Quat::IDENTITY,
                        at,
                    ));
                }
            }
            x += width;
        }
        z += depth;
        row += 1;
    }
    (fields, bunds)
}

/// Cedars over the wooded hills, as instance transforms for a model one unit across.
pub fn cedars() -> Vec<Mat4> {
    let mut transforms = Vec::new();
    for i in 0..22_000 {
        let x = (hash(i, 1, 21) * 2.0 - 1.0) * EXTENT_X;
        let z = FAR_Z * 0.62 + hash(i, 2, 21) * (NEAR_Z - FAR_Z * 0.62);
        let density = woods(x, z);
        if density < 0.05 || hash(i, 4, 21) > density || in_town(x, z) {
            continue;
        }
        let size = 2.0 + hash(i, 3, 21) * 1.6;
        let yaw = hash(i, 5, 21) * TAU;
        transforms.push(Mat4::from_scale_rotation_translation(
            Vec3::new(size * 0.9, size * 1.3, size * 0.9),
            Quat::from_rotation_y(yaw),
            Vec3::new(x, height(x, z) - 0.15, z),
        ));
    }
    transforms
}

/// Willows along the riverbanks outside the town.
pub fn willows() -> Vec<Mat4> {
    let mut transforms = Vec::new();
    for i in 0..70 {
        let x = -110.0 + i as f32 * 3.2 + hash(i, 1, 31) * 1.6;
        let side = if hash(i, 6, 31) > 0.5 { -1.0 } else { 1.0 };
        let z = river_z(x) + side * (3.9 + hash(i, 2, 31) * 0.7);
        if in_town(x, z) || hash(i, 3, 31) < 0.45 {
            continue;
        }
        let size = 1.8 + hash(i, 4, 31) * 0.6;
        transforms.push(Mat4::from_scale_rotation_translation(
            Vec3::splat(size),
            Quat::from_rotation_y(hash(i, 5, 31) * TAU),
            Vec3::new(x, height(x, z).max(PLAIN), z),
        ));
    }
    transforms
}

/// Where the bridges cross the river, at the town's two ends and its middle.
pub fn bridges() -> Vec<Mat4> {
    [-38.0f32, 0.0, 38.0]
        .into_iter()
        .map(|x| {
            let z = river_z(x);
            let slope = (river_z(x + 0.5) - river_z(x - 0.5)).atan();
            Mat4::from_scale_rotation_translation(
                Vec3::splat(8.5),
                Quat::from_rotation_y(-slope),
                Vec3::new(x, WATER - 0.2, z),
            )
        })
        .collect()
}
