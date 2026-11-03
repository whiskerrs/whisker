//! Work done ahead of time on a development machine.

use glam::{UVec3, Vec3};

use crate::Model;
use crate::pack::{MODEL_SECTION, write_model, write_pack};
use crate::probes::{ProbeData, ProbeVolume};

const RAYS: usize = 256;
// Light from beyond this is treated as open sky, which keeps far walls from darkening whole valleys.
const RAY_LENGTH: f32 = 30.0;
const LEAF_SIZE: usize = 8;
const MOST_PROBES: u64 = 1 << 22;

#[derive(Clone, Copy)]
struct Triangle {
    corners: [Vec3; 3],
}

impl Triangle {
    fn center(&self) -> Vec3 {
        (self.corners[0] + self.corners[1] + self.corners[2]) / 3.0
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

    /// The nearest thing a ray strikes, and whether it was a back face.
    fn trace(&self, origin: Vec3, direction: Vec3) -> Option<bool> {
        let inverse = direction.recip();
        let mut nearest = RAY_LENGTH;
        let mut back = None;
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
                    back = Some(is_back);
                }
            }
        }
        back
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

/// Calculates how much sky each point around `models` can see, one probe every `spacing` units.
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
            for corner in data.indices.chunks_exact(3) {
                triangles.push(Triangle {
                    corners: [0, 1, 2].map(|i| {
                        part.transform
                            .transform_point3(data.vertices[corner[i] as usize].position.into())
                    }),
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
    }))
}

/// Writes `model` as a cooked pack, in the form a game loads with
/// [`Model::load`](crate::Model::load).
pub fn cook_model(model: &Model) -> Result<Vec<u8>, String> {
    let data = model.data().ok_or("the model is not loaded")?;
    Ok(write_pack(&[(MODEL_SECTION, write_model(data))]))
}

/// [`bake_probes`], in the form a game loads with [`ProbeVolume::load`].
pub fn bake_probes_to_bytes(models: &[Model], spacing: f32) -> Result<Vec<u8>, String> {
    let volume = bake_probes(models, spacing)?;
    Ok(volume.data().expect("just baked").to_bytes())
}

/// The mean sky visibility and the mean direction it comes from, or nothing for a probe inside a wall.
type Probe = Option<(f32, Vec3)>;

fn trace_probe(tree: &Tree, position: Vec3, directions: &[Vec3]) -> Probe {
    let (mut open, mut toward_open, mut backs) = (0.0, Vec3::ZERO, 0);
    for &direction in directions {
        match tree.trace(position, direction) {
            None => {
                open += 1.0;
                toward_open += direction;
            }
            Some(true) => backs += 1,
            Some(false) => {}
        }
    }
    // A probe that sees mostly back faces is inside something, and its darkness must not leak out.
    if backs * 4 > directions.len() {
        return None;
    }
    let rays = directions.len() as f32;
    Some((open / rays, toward_open * 3.0 / rays))
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
                let mut sum = (0.0, Vec3::ZERO);
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
                    if let Some((mean, direction)) =
                        probes[index(near[0] as u32, near[1] as u32, near[2] as u32)]
                    {
                        sum = (sum.0 + mean, sum.1 + direction);
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

fn encode((mean, direction): (f32, Vec3)) -> [u8; 4] {
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let [x, y, z] = direction.to_array();
    [
        byte(x * 0.5 + 0.5),
        byte(y * 0.5 + 0.5),
        byte(z * 0.5 + 0.5),
        byte(mean),
    ]
}

#[cfg(test)]
mod tests {
    use glam::Mat4;

    use super::*;
    use crate::{Material, Mesh};

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
    fn a_probe_inside_a_solid_takes_its_light_from_outside() {
        let cube = model(Mesh::cube(1.0), Mat4::IDENTITY);
        let volume = bake_probes(&[cube], 1.0).expect("bakes");
        let inside = at(&volume, volume.data().unwrap().origin + Vec3::splat(1.0));
        assert!(inside[3] > 64, "{inside:?}");
    }

    #[test]
    fn bad_input_is_an_error() {
        assert!(bake_probes(&[], 1.0).is_err());
        assert!(bake_probes(&[model(Mesh::cube(1.0), Mat4::IDENTITY)], 0.0).is_err());
        assert!(bake_probes(&[model(Mesh::cube(1000.0), Mat4::IDENTITY)], 0.5).is_err());
    }
}
