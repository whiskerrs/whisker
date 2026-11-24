use std::cell::{Cell, RefCell};

use glam::{Mat3, Mat4, Vec3, Vec4};

use super::{Geometry, Vertex, WeakNode};

/// Skinning and morph state of one mesh node, deformed on the CPU into vertices of its own.
pub(crate) struct Deform {
    /// Joint nodes and the inverse bind matrix of each; weak because a mesh may sit under its own joints.
    pub(crate) skin: Option<(Vec<WeakNode>, Vec<Mat4>)>,
    pub(crate) weights: RefCell<Vec<f32>>,
    output: RefCell<Output>,
    pub(crate) version: Cell<u64>,
    pub(crate) gpu: RefCell<Option<(crate::render::DeformBuffer, u64)>>,
}

#[derive(Default)]
struct Output {
    inputs: Vec<f32>,
    vertices: Vec<Vertex>,
}

impl Deform {
    pub(crate) fn new(skin: Option<(Vec<WeakNode>, Vec<Mat4>)>, weights: Vec<f32>) -> Deform {
        Deform {
            skin,
            weights: RefCell::new(weights),
            output: RefCell::new(Output::default()),
            version: Cell::new(0),
            gpu: RefCell::new(None),
        }
    }

    /// Whether the deformed vertices are already in world space instead of the node's space.
    pub(crate) fn is_skinned(&self) -> bool {
        self.skin.is_some()
    }

    /// Recomputes the vertices when the joints or weights moved since the last call.
    pub(crate) fn update(&self, geometry: &Geometry) {
        let joints: Vec<Mat4> = match &self.skin {
            Some((nodes, inverse_bind)) => nodes
                .iter()
                .zip(inverse_bind)
                .map(|(node, inverse)| {
                    node.upgrade()
                        .map_or(Mat4::IDENTITY, |node| node.world_matrix())
                        * *inverse
                })
                .collect(),
            None => Vec::new(),
        };
        let weights = self.weights.borrow();
        let mut inputs: Vec<f32> = Vec::with_capacity(joints.len() * 16 + weights.len());
        for joint in &joints {
            inputs.extend_from_slice(&joint.to_cols_array());
        }
        inputs.extend_from_slice(&weights);
        let mut output = self.output.borrow_mut();
        if output.inputs == inputs && !output.vertices.is_empty() {
            return;
        }
        output.vertices = deform(geometry, &joints, &weights);
        output.inputs = inputs;
        self.version.set(self.version.get() + 1);
    }

    pub(crate) fn with_vertices<R>(&self, f: impl FnOnce(&[Vertex]) -> R) -> R {
        f(&self.output.borrow().vertices)
    }
}

fn deform(geometry: &Geometry, joints: &[Mat4], weights: &[f32]) -> Vec<Vertex> {
    let data = geometry.data();
    let mut vertices = data.vertices.clone();
    for (target, &weight) in data.morph_targets.iter().zip(weights) {
        if weight == 0.0 {
            continue;
        }
        for (i, vertex) in vertices.iter_mut().enumerate() {
            if let Some(offset) = target.positions.get(i) {
                vertex.position = (Vec3::from(vertex.position) + *offset * weight).to_array();
            }
            if let Some(offset) = target.normals.get(i) {
                vertex.normal = (Vec3::from(vertex.normal) + *offset * weight).to_array();
            }
            if let Some(offset) = target.tangents.get(i) {
                let tangent = Vec4::from(vertex.tangent);
                vertex.tangent = (tangent.truncate() + *offset * weight)
                    .extend(tangent.w)
                    .to_array();
            }
        }
    }
    if let Some(influences) = &data.joints
        && !joints.is_empty()
    {
        for (vertex, (indices, amounts)) in vertices.iter_mut().zip(influences) {
            let mut matrix = Mat4::ZERO;
            for (&joint, &amount) in indices.iter().zip(amounts) {
                if amount != 0.0
                    && let Some(joint) = joints.get(usize::from(joint))
                {
                    matrix += *joint * amount;
                }
            }
            let linear = Mat3::from_mat4(matrix);
            vertex.position = matrix.transform_point3(vertex.position.into()).to_array();
            vertex.normal = (linear * Vec3::from(vertex.normal))
                .normalize_or_zero()
                .to_array();
            let tangent = Vec4::from(vertex.tangent);
            vertex.tangent = (linear * tangent.truncate())
                .normalize_or_zero()
                .extend(tangent.w)
                .to_array();
        }
    }
    vertices
}
