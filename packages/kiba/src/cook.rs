//! Work done ahead of time on a development machine.

use std::collections::{BTreeMap, HashMap};

use glam::{IVec2, Mat4, UVec3, Vec2, Vec3, Vec4};

use crate::model::{Lod, ModelData, ModelPart};
use crate::pack::{MODEL_SECTION, PROBE_SECTION, WORLD_SECTION, Writer, write_model, write_pack};
use crate::probes::{ProbeData, ProbeVolume};
use crate::{Material, Mesh, Model, Texture, Vertex};

const RAYS: usize = 256;
// Light from beyond this is treated as open sky, which keeps far walls from darkening whole valleys.
const RAY_LENGTH: f32 = 30.0;
const LEAF_SIZE: usize = 8;
const MOST_PROBES: u64 = 1 << 22;

#[derive(Clone, Copy)]
struct Triangle {
    corners: [Vec3; 3],
    /// Linear RGB of the light the face sends back.
    albedo: Vec3,
}

impl Triangle {
    fn center(&self) -> Vec3 {
        (self.corners[0] + self.corners[1] + self.corners[2]) / 3.0
    }

    fn normal(&self) -> Vec3 {
        let [a, b, c] = self.corners;
        (b - a).cross(c - a).normalize_or_zero()
    }

    /// The distance along the ray, and whether the ray struck the back face.
    fn hit(&self, origin: Vec3, direction: Vec3, longest: f32) -> Option<(f32, bool)> {
        let [a, b, c] = self.corners;
        let (edge1, edge2) = (b - a, c - a);
        let cross = direction.cross(edge2);
        let determinant = edge1.dot(cross);
        if determinant.abs() < 1e-8 {
            return None;
        }
        let to_origin = origin - a;
        let u = to_origin.dot(cross) / determinant;
        let turned = to_origin.cross(edge1);
        let v = direction.dot(turned) / determinant;
        if u < 0.0 || v < 0.0 || u + v > 1.0 {
            return None;
        }
        let distance = edge2.dot(turned) / determinant;
        (distance > 1e-4 && distance < longest).then_some((distance, determinant < 0.0))
    }
}

struct Node {
    min: Vec3,
    max: Vec3,
    /// A leaf's first triangle, or a branch's second child when `count` is zero.
    first: usize,
    count: usize,
}

/// A bounding volume hierarchy, because a scene has far more triangles than a linear scan can bear.
struct Tree {
    nodes: Vec<Node>,
    triangles: Vec<Triangle>,
}

impl Tree {
    fn new(triangles: Vec<Triangle>) -> Self {
        let mut tree = Self {
            nodes: Vec::new(),
            triangles,
        };
        let count = tree.triangles.len();
        tree.split(0, count);
        tree
    }

    fn split(&mut self, first: usize, count: usize) -> usize {
        let (mut min, mut max) = (Vec3::MAX, Vec3::MIN);
        for triangle in &self.triangles[first..first + count] {
            for corner in triangle.corners {
                min = min.min(corner);
                max = max.max(corner);
            }
        }
        let at = self.nodes.len();
        self.nodes.push(Node {
            min,
            max,
            first,
            count,
        });
        if count <= LEAF_SIZE {
            return at;
        }
        let axis = (max - min).max_position();
        self.triangles[first..first + count]
            .sort_unstable_by(|a, b| a.center()[axis].total_cmp(&b.center()[axis]));
        let half = count / 2;
        self.split(first, half);
        let second = self.split(first + half, count - half);
        self.nodes[at].first = second;
        self.nodes[at].count = 0;
        at
    }

    /// The nearest triangle a ray strikes, and whether it struck its back face.
    fn trace(&self, origin: Vec3, direction: Vec3) -> Option<(&Triangle, bool)> {
        let inverse = direction.recip();
        let mut nearest = RAY_LENGTH;
        let mut struck = None;
        let mut stack = vec![0usize];
        while let Some(at) = stack.pop() {
            let Some(node) = self.nodes.get(at) else {
                continue;
            };
            let (a, b) = ((node.min - origin) * inverse, (node.max - origin) * inverse);
            let enter = a.min(b).max_element();
            let leave = a.max(b).min_element();
            if leave < enter.max(0.0) || enter > nearest {
                continue;
            }
            if node.count == 0 {
                stack.extend([at + 1, node.first]);
                continue;
            }
            for triangle in &self.triangles[node.first..node.first + node.count] {
                if let Some((distance, is_back)) = triangle.hit(origin, direction, nearest) {
                    nearest = distance;
                    struck = Some((triangle, is_back));
                }
            }
        }
        struck
    }
}

fn sphere_directions() -> Vec<Vec3> {
    let golden_angle = std::f32::consts::PI * (3.0 - 5.0f32.sqrt());
    (0..RAYS)
        .map(|i| {
            let y = 1.0 - 2.0 * (i as f32 + 0.5) / RAYS as f32;
            let radius = (1.0 - y * y).sqrt();
            let angle = golden_angle * i as f32;
            Vec3::new(radius * angle.cos(), y, radius * angle.sin())
        })
        .collect()
}

/// Calculates how much sky each point around `models` can see, and the albedo of what hides the rest,
/// one probe every `spacing` units.
///
/// Models stay at their file's position, because cooking sees only what does not move.
pub fn bake_probes(models: &[Model], spacing: f32) -> Result<ProbeVolume, String> {
    if spacing <= 0.0 {
        return Err("the spacing must be greater than zero".into());
    }
    let mut triangles = Vec::new();
    for model in models {
        for part in model.parts() {
            let data = part.mesh.data();
            let tint = part.material.base_color
                * part
                    .material
                    .base_color_texture
                    .as_ref()
                    .map_or(Vec3::ONE, mean_color);
            for corner in data.indices.chunks_exact(3) {
                let vertices = [0, 1, 2].map(|i| data.vertices[corner[i] as usize]);
                let color = vertices
                    .iter()
                    .map(|vertex| Vec3::from(vertex.color))
                    .sum::<Vec3>()
                    / 3.0;
                triangles.push(Triangle {
                    corners: vertices
                        .map(|vertex| part.transform.transform_point3(vertex.position.into())),
                    albedo: tint * color,
                });
            }
        }
    }
    if triangles.is_empty() {
        return Err("the models hold no triangles".into());
    }
    let tree = Tree::new(triangles);
    let root = &tree.nodes[0];
    let origin = root.min - Vec3::splat(spacing);
    let dimensions = ((root.max - root.min) / spacing).ceil().as_uvec3() + UVec3::splat(3);
    let count = dimensions.as_u64vec3().element_product();
    if count > MOST_PROBES {
        return Err(format!(
            "{count} probes would be too many, so raise the spacing"
        ));
    }

    let directions = sphere_directions();
    let mut probes = Vec::with_capacity(count as usize);
    for z in 0..dimensions.z {
        for y in 0..dimensions.y {
            for x in 0..dimensions.x {
                let position = origin + UVec3::new(x, y, z).as_vec3() * spacing;
                probes.push(trace_probe(&tree, position, &directions));
            }
        }
    }
    fill_buried(&mut probes, dimensions);
    Ok(ProbeVolume::from_data(ProbeData {
        dimensions,
        origin,
        spacing,
        visibility: probes
            .iter()
            .map(|probe| probe.map_or([128, 128, 128, 255], encode))
            .collect(),
        bounce: probes
            .iter()
            .map(|probe| probe.map_or([0; 4], encode_bounce))
            .collect(),
    }))
}

/// The linear mean of an sRGB image.
fn mean_color(texture: &Texture) -> Vec3 {
    let data = texture.data();
    let to_linear = |byte: u8| {
        let value = f32::from(byte) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let pixels = data.pixels.chunks_exact(4);
    let count = pixels.len().max(1) as f32;
    pixels
        .map(|pixel| {
            Vec3::new(
                to_linear(pixel[0]),
                to_linear(pixel[1]),
                to_linear(pixel[2]),
            )
        })
        .sum::<Vec3>()
        / count
}

/// Writes `model` as a cooked pack, in the form a game loads with
/// [`Model::load`](crate::Model::load).
pub fn cook_model(model: &Model) -> Result<Vec<u8>, String> {
    let data = model.data().ok_or("the model is not loaded")?;
    Ok(write_pack(&[(MODEL_SECTION, write_model(data))]))
}

// A simpler mesh takes over at these multiples of a part's radius, when it covers little of the screen.
const LOD_STEPS: [(f32, f32); 2] = [(10.0, 0.4), (30.0, 0.15)];
const FAR_RATIO: f32 = 0.1;
const SMALLEST_SIMPLIFIED_INDICES: usize = 300;

fn simplify(mesh: &Mesh, ratio: f32) -> Option<Mesh> {
    let data = mesh.data();
    if data.indices.len() < SMALLEST_SIMPLIFIED_INDICES || data.skin.is_some() {
        return None;
    }
    let adapter = meshopt::VertexDataAdapter::new(
        bytemuck::cast_slice(&data.vertices),
        size_of::<Vertex>(),
        0,
    )
    .ok()?;
    let target = ((data.indices.len() as f32 * ratio) as usize / 3 * 3).max(3);
    let kept = meshopt::simplify(
        &data.indices,
        &adapter,
        target,
        // Moving the surface by up to 5% of its size is invisible at the distances that use this.
        0.05,
        meshopt::SimplifyOptions::None,
        None,
    );
    if kept.is_empty() || kept.len() >= data.indices.len() {
        return None;
    }
    let mut remap = HashMap::new();
    let mut vertices = Vec::new();
    let indices = kept
        .iter()
        .map(|&index| {
            *remap.entry(index).or_insert_with(|| {
                vertices.push(data.vertices[index as usize]);
                vertices.len() as u32 - 1
            })
        })
        .collect();
    Some(Mesh::new(vertices, indices))
}

/// A single untextured mesh standing in for everything in a cell from far away.
fn far_stand_in(parts: &[ModelPart]) -> Option<ModelPart> {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for part in parts {
        let tint = part.material.base_color * average_color(&part.material);
        let normal_matrix = part.transform.inverse().transpose();
        let first = vertices.len() as u32;
        let data = part.mesh.data();
        vertices.extend(data.vertices.iter().map(|vertex| {
            Vertex {
                position: part
                    .transform
                    .transform_point3(vertex.position.into())
                    .to_array(),
                normal: normal_matrix
                    .transform_vector3(vertex.normal.into())
                    .normalize_or_zero()
                    .to_array(),
                color: (Vec3::from(vertex.color) * tint).to_array(),
                uv: vertex.uv,
            }
        }));
        indices.extend(data.indices.iter().map(|index| index + first));
    }
    if indices.is_empty() {
        return None;
    }
    let merged = Mesh::new(vertices, indices);
    Some(ModelPart {
        mesh: simplify(&merged, FAR_RATIO).unwrap_or(merged),
        material: Material {
            roughness: 0.8,
            ..Material::default()
        },
        transform: Mat4::IDENTITY,
        node: None,
        skin: None,
        lods: Vec::new(),
    })
}

fn average_color(material: &Material) -> Vec3 {
    let Some(texture) = &material.base_color_texture else {
        return Vec3::ONE;
    };
    let pixels = &texture.data().pixels;
    let mut sum = Vec3::ZERO;
    for pixel in pixels.chunks_exact(4) {
        let srgb = Vec3::new(pixel[0] as f32, pixel[1] as f32, pixel[2] as f32) / 255.0;
        sum += srgb.powf(2.2);
    }
    sum / (pixels.len() / 4).max(1) as f32
}

/// Splits `models` into the files of a [`WorldStream`](crate::WorldStream), as (name, contents).
///
/// A part belongs to the cell holding its center, whatever its size. Every cell gets simpler meshes
/// for distance, a far stand-in, and with a positive `probe_spacing` its own sky visibility.
pub fn cook_world(
    models: &[Model],
    cell_size: f32,
    probe_spacing: f32,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    if cell_size <= 0.0 {
        return Err("the cell size must be greater than zero".into());
    }
    let mut cells: BTreeMap<(i32, i32), Vec<ModelPart>> = BTreeMap::new();
    for model in models {
        for part in model.parts() {
            let center = part
                .transform
                .transform_point3(part.mesh.data().bounds_center);
            let cell = IVec2::new(
                (center.x / cell_size).floor() as i32,
                (center.z / cell_size).floor() as i32,
            );
            let mut part = part.clone();
            let radius = part.mesh.data().bounds_radius;
            let scale = part
                .transform
                .to_scale_rotation_translation()
                .0
                .max_element();
            part.lods = LOD_STEPS
                .iter()
                .filter_map(|&(step, ratio)| {
                    Some(Lod {
                        distance: radius * scale * step,
                        mesh: simplify(&part.mesh, ratio)?,
                    })
                })
                .collect();
            cells.entry((cell.x, cell.y)).or_default().push(part);
        }
    }
    if cells.is_empty() {
        return Err("the models hold no triangles".into());
    }

    let mut files = Vec::new();
    let mut index = Writer::default();
    index.f32(cell_size);
    let positions: Vec<[i32; 2]> = cells.keys().map(|&(x, z)| [x, z]).collect();
    index.slice(&positions);
    files.push((
        "index.kiba".to_owned(),
        write_pack(&[(WORLD_SECTION, index.0)]),
    ));
    for ((x, z), parts) in cells {
        let far = far_stand_in(&parts);
        let data = ModelData { parts, rig: None };
        let mut sections = vec![(MODEL_SECTION, write_model(&data))];
        if probe_spacing > 0.0 {
            let volume = bake_probes(&[Model::from_data(data)], probe_spacing)?;
            sections.push((
                PROBE_SECTION,
                volume.data().expect("just baked").to_section(),
            ));
        }
        files.push((format!("cell_{x}_{z}.kiba"), write_pack(&sections)));
        let far_data = ModelData {
            parts: far.into_iter().collect(),
            rig: None,
        };
        files.push((
            format!("cell_{x}_{z}.far.kiba"),
            write_pack(&[(MODEL_SECTION, write_model(&far_data))]),
        ));
    }
    Ok(files)
}

fn terrain_height(at: Vec2) -> f32 {
    (at.x * 0.013).sin() * 9.0
        + (at.y * 0.017).cos() * 7.0
        + (at.dot(Vec2::new(0.04, 0.03))).sin() * 1.5
}

fn terrain(cell: IVec2, cell_size: f32) -> Mesh {
    const SIDE: u32 = 64;
    let step = cell_size / SIDE as f32;
    let origin = cell.as_vec2() * cell_size;
    let mut vertices = Vec::new();
    for z in 0..=SIDE {
        for x in 0..=SIDE {
            let at = origin + Vec2::new(x as f32, z as f32) * step;
            let slope = Vec2::new(
                terrain_height(at + Vec2::X) - terrain_height(at - Vec2::X),
                terrain_height(at + Vec2::Y) - terrain_height(at - Vec2::Y),
            ) / 2.0;
            vertices.push(Vertex {
                position: [at.x, terrain_height(at), at.y],
                normal: Vec3::new(-slope.x, 1.0, -slope.y).normalize().to_array(),
                color: [1.0; 3],
                uv: [x as f32 / 8.0, z as f32 / 8.0],
            });
        }
    }
    let mut indices = Vec::new();
    for z in 0..SIDE {
        for x in 0..SIDE {
            let a = z * (SIDE + 1) + x;
            let (b, c, d) = (a + 1, a + SIDE + 1, a + SIDE + 2);
            indices.extend([a, c, b, b, c, d]);
        }
    }
    Mesh::new(vertices, indices)
}

/// Makes a procedural landscape for measuring streaming, because no authored world exists yet.
pub fn synthetic_world(cells_per_side: i32, cell_size: f32) -> Model {
    let rock = Mesh::sphere(1.0);
    let mut parts = Vec::new();
    for z in 0..cells_per_side {
        for x in 0..cells_per_side {
            let cell = IVec2::new(x, z);
            let shade = 80 + ((x * 31 + z * 17) % 60) as u8;
            let pixels = (0..128u32 * 128)
                .flat_map(|i| {
                    let grain = (i.wrapping_mul(2654435761) >> 28) as u8 * 3;
                    [shade + grain, shade + grain / 2, shade / 2, 255]
                })
                .collect();
            parts.push((
                terrain(cell, cell_size),
                Material {
                    base_color_texture: Some(Texture::from_rgba8(128, 128, pixels)),
                    roughness: 0.9,
                    ..Material::default()
                },
                Mat4::IDENTITY,
            ));
            for i in 0..40u32 {
                let seed = i
                    .wrapping_add((x * 1000 + z) as u32)
                    .wrapping_mul(2654435761);
                let fraction = |shift: u32| ((seed >> shift) & 0xff) as f32 / 255.0;
                let at = (cell.as_vec2() + Vec2::new(fraction(0), fraction(8))) * cell_size;
                let size = 0.5 + fraction(16) * 4.0;
                let position = Vec3::new(at.x, terrain_height(at), at.y);
                parts.push((
                    rock.clone(),
                    Material {
                        base_color: Vec3::splat(0.3 + fraction(24) * 0.3),
                        roughness: 0.8,
                        ..Material::default()
                    },
                    Mat4::from_scale_rotation_translation(
                        Vec3::splat(size),
                        glam::Quat::IDENTITY,
                        position,
                    ),
                ));
            }
        }
    }
    Model::new(parts)
}

/// [`bake_probes`], in the form a game loads with [`ProbeVolume::load`].
pub fn bake_probes_to_bytes(models: &[Model], spacing: f32) -> Result<Vec<u8>, String> {
    let volume = bake_probes(models, spacing)?;
    Ok(volume.data().expect("just baked").to_bytes())
}

/// The mean direction toward open sky in XYZ with the mean sky visibility in W, then the mean albedo
/// of the surfaces the probe sees in XYZ with how much they face up in W, or nothing inside a wall.
type Probe = Option<(Vec4, Vec4)>;

fn trace_probe(tree: &Tree, position: Vec3, directions: &[Vec3]) -> Probe {
    let (mut open, mut toward_open, mut backs) = (0.0, Vec3::ZERO, 0);
    let (mut albedo, mut upward, mut fronts) = (Vec3::ZERO, 0.0, 0.0);
    for &direction in directions {
        match tree.trace(position, direction) {
            None => {
                open += 1.0;
                toward_open += direction;
            }
            Some((_, true)) => backs += 1,
            Some((triangle, false)) => {
                albedo += triangle.albedo;
                upward += triangle.normal().y.max(0.0);
                fronts += 1.0;
            }
        }
    }
    // A probe that sees mostly back faces is inside something, and its darkness must not leak out.
    if backs * 4 > directions.len() {
        return None;
    }
    let rays = directions.len() as f32;
    let fronts = f32::max(fronts, 1.0);
    Some((
        (toward_open * 3.0 / rays).extend(open / rays),
        (albedo / fronts).extend(upward / fronts),
    ))
}

fn fill_buried(probes: &mut [Probe], dimensions: UVec3) {
    let index =
        |x: u32, y: u32, z: u32| (x + y * dimensions.x + z * dimensions.x * dimensions.y) as usize;
    for z in 0..dimensions.z {
        for y in 0..dimensions.y {
            for x in 0..dimensions.x {
                if probes[index(x, y, z)].is_some() {
                    continue;
                }
                let mut sum = (Vec4::ZERO, Vec4::ZERO);
                let mut found = 0.0;
                for (dx, dy, dz) in [
                    (-1, 0, 0),
                    (1, 0, 0),
                    (0, -1, 0),
                    (0, 1, 0),
                    (0, 0, -1),
                    (0, 0, 1),
                ] {
                    let near = [x as i32 + dx, y as i32 + dy, z as i32 + dz];
                    if near.iter().any(|&v| v < 0)
                        || near[0] as u32 >= dimensions.x
                        || near[1] as u32 >= dimensions.y
                        || near[2] as u32 >= dimensions.z
                    {
                        continue;
                    }
                    if let Some((visibility, bounce)) =
                        probes[index(near[0] as u32, near[1] as u32, near[2] as u32)]
                    {
                        sum = (sum.0 + visibility, sum.1 + bounce);
                        found += 1.0;
                    }
                }
                if found > 0.0 {
                    probes[index(x, y, z)] = Some((sum.0 / found, sum.1 / found));
                }
            }
        }
    }
}

fn byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn encode((visibility, _): (Vec4, Vec4)) -> [u8; 4] {
    let [x, y, z, mean] = visibility.to_array();
    [
        byte(x * 0.5 + 0.5),
        byte(y * 0.5 + 0.5),
        byte(z * 0.5 + 0.5),
        byte(mean),
    ]
}

// Square roots spend the bytes on dark albedo, where the eye sees steps.
fn encode_bounce((_, bounce): (Vec4, Vec4)) -> [u8; 4] {
    let [r, g, b, upward] = bounce.to_array();
    [byte(r.sqrt()), byte(g.sqrt()), byte(b.sqrt()), byte(upward)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(volume: &ProbeVolume, position: Vec3) -> [u8; 4] {
        let data = volume.data().expect("cooked");
        let grid = ((position - data.origin) / data.spacing).round().as_uvec3();
        let dimensions = data.dimensions;
        data.visibility
            [(grid.x + grid.y * dimensions.x + grid.z * dimensions.x * dimensions.y) as usize]
    }

    fn model(mesh: Mesh, transform: Mat4) -> Model {
        Model::new([(mesh, Material::default(), transform)])
    }

    #[test]
    fn a_probe_under_a_roof_sees_less_sky_and_finds_it_sideways() {
        let roof = model(
            Mesh::plane(8.0),
            Mat4::from_translation(Vec3::Y * 1.0) * Mat4::from_rotation_x(std::f32::consts::PI),
        );
        let ground = model(Mesh::plane(8.0), Mat4::IDENTITY);
        let volume = bake_probes(&[roof, ground], 0.5).expect("bakes");

        let between = at(&volume, Vec3::new(0.0, 0.5, 0.0));
        let beside = at(&volume, Vec3::new(4.5, 0.5, 0.0));
        assert!(between[3] < 60, "{between:?}");
        assert!(beside[3] > between[3] * 2, "{beside:?} {between:?}");
    }

    #[test]
    fn a_probe_records_the_color_and_facing_of_what_hides_the_sky() {
        let red = Material {
            base_color: Vec3::new(0.8, 0.1, 0.1),
            ..Material::default()
        };
        let ground = Model::new([(Mesh::plane(8.0), red, Mat4::IDENTITY)]);
        let volume = bake_probes(&[ground], 0.5).expect("bakes");
        let data = volume.data().expect("cooked");
        let grid = ((Vec3::new(0.0, 0.5, 0.0) - data.origin) / data.spacing)
            .round()
            .as_uvec3();
        let dimensions = data.dimensions;
        let [r, g, b, upward] = data.bounce
            [(grid.x + grid.y * dimensions.x + grid.z * dimensions.x * dimensions.y) as usize];
        assert!(r > 200 && g < 100 && b < 100, "{r} {g} {b}");
        assert!(upward > 240, "{upward}");
    }

    #[test]
    fn a_probe_inside_a_solid_takes_its_light_from_outside() {
        let cube = model(Mesh::cube(1.0), Mat4::IDENTITY);
        let volume = bake_probes(&[cube], 1.0).expect("bakes");
        let inside = at(&volume, volume.data().unwrap().origin + Vec3::splat(1.0));
        assert!(inside[3] > 64, "{inside:?}");
    }

    #[test]
    fn a_world_splits_into_cells_with_simpler_meshes_and_stand_ins() {
        let far_cube = Mat4::from_translation(Vec3::new(300.0, 0.0, 50.0));
        let world = Model::new([
            (Mesh::sphere(1.0), Material::default(), Mat4::IDENTITY),
            (Mesh::sphere(1.0), Material::default(), far_cube),
        ]);
        let files = cook_world(&[world], 100.0, 0.0).expect("cooks");
        let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
        assert!(names.contains(&"index.kiba"));
        assert!(names.contains(&"cell_0_0.kiba"));
        assert!(names.contains(&"cell_3_0.far.kiba"));

        let (_, bytes) = files
            .iter()
            .find(|(name, _)| name == "cell_0_0.kiba")
            .unwrap();
        let model = Model::from_pack(bytes).expect("valid");
        let part = &model.parts()[0];
        assert_eq!(part.lods.len(), 2);
        let (full, simple) = (
            part.mesh.data().indices.len(),
            part.lods[1].mesh.data().indices.len(),
        );
        assert!(simple < full / 2, "{simple} of {full}");

        let (_, bytes) = files
            .iter()
            .find(|(name, _)| name == "cell_0_0.far.kiba")
            .unwrap();
        let far = Model::from_pack(bytes).expect("valid");
        assert!(far.parts()[0].mesh.data().indices.len() < full);
    }

    #[test]
    fn nearby_parts_pick_full_detail_and_far_ones_a_simpler_mesh() {
        let part = ModelPart {
            mesh: Mesh::cube(1.0),
            material: Material::default(),
            transform: Mat4::IDENTITY,
            node: None,
            skin: None,
            lods: vec![
                Lod {
                    distance: 10.0,
                    mesh: Mesh::plane(1.0),
                },
                Lod {
                    distance: 30.0,
                    mesh: Mesh::triangle(),
                },
            ],
        };
        assert_eq!(part.mesh_at(1.0).key(), part.mesh.key());
        assert_eq!(part.mesh_at(15.0).key(), part.lods[0].mesh.key());
        assert_eq!(part.mesh_at(40.0).key(), part.lods[1].mesh.key());
    }

    #[test]
    fn bad_input_is_an_error() {
        assert!(bake_probes(&[], 1.0).is_err());
        assert!(bake_probes(&[model(Mesh::cube(1.0), Mat4::IDENTITY)], 0.0).is_err());
        assert!(bake_probes(&[model(Mesh::cube(1000.0), Mat4::IDENTITY)], 0.5).is_err());
    }
}
