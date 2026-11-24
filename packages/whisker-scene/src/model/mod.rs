//! glTF 2.0 models.

mod animation;
mod import;

use std::rc::Rc;

use glam::Mat4;

use crate::core::{Deform, Geometry, Light, Material, Node, Transform};

pub(crate) use animation::Clip;
pub use animation::{AnimationOptions, AnimationPlayer};

/// A glTF 2.0 model, parsed once and placed any number of times with [`Model::instantiate`].
///
/// Instances share geometry and textures.
#[derive(Clone)]
pub struct Model {
    data: Rc<ModelData>,
}

impl PartialEq for Model {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.data, &other.data)
    }
}

impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Model")
            .field("nodes", &self.data.nodes.len())
            .field("animations", &self.animations())
            .finish()
    }
}

/// Why [`Model::from_slice`] failed.
#[derive(Debug)]
pub struct ModelError(String);

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the glTF model could not be loaded: {}", self.0)
    }
}

impl std::error::Error for ModelError {}

pub(crate) struct ModelData {
    pub(crate) nodes: Vec<NodeData>,
    pub(crate) meshes: Vec<Vec<Primitive>>,
    pub(crate) skins: Vec<SkinData>,
    pub(crate) roots: Vec<usize>,
    pub(crate) clips: Vec<Clip>,
}

pub(crate) struct NodeData {
    pub(crate) name: Option<String>,
    pub(crate) transform: Transform,
    pub(crate) mesh: Option<usize>,
    pub(crate) skin: Option<usize>,
    pub(crate) light: Option<Light>,
    pub(crate) weights: Vec<f32>,
    pub(crate) children: Vec<usize>,
}

pub(crate) struct Primitive {
    pub(crate) geometry: Geometry,
    pub(crate) material: Material,
}

pub(crate) struct SkinData {
    pub(crate) joints: Vec<usize>,
    pub(crate) inverse_bind: Vec<Mat4>,
}

/// One placed copy of a model, remembered on its root node.
pub(crate) struct Instance {
    pub(crate) model: Model,
    /// The node made for each glTF node.
    pub(crate) nodes: Vec<Node>,
    /// The nodes drawing each glTF node's mesh primitives, whose morph weights it animates.
    pub(crate) primitives: Vec<Vec<Node>>,
}

impl Model {
    /// Parses a binary `.glb` file, or a `.gltf` file whose buffers and images are embedded as data
    /// URIs.
    ///
    /// Texture images may be PNG, JPEG or WebP.
    pub fn from_slice(bytes: &[u8]) -> Result<Model, ModelError> {
        Ok(Model {
            data: Rc::new(import::import(bytes).map_err(ModelError)?),
        })
    }

    /// The names of the model's animation clips, in file order; unnamed clips are `"0"`, `"1"`, …
    /// by index.
    pub fn animations(&self) -> Vec<String> {
        self.data
            .clips
            .iter()
            .map(|clip| clip.name.clone())
            .collect()
    }

    /// A new node tree for the model's default scene, under one root node.
    ///
    /// Pass the root to [`AnimationPlayer::new`] to animate it.
    pub fn instantiate(&self) -> Node {
        let data = &self.data;
        let nodes: Vec<Node> = data
            .nodes
            .iter()
            .map(|definition| {
                let node = Node::new();
                node.set_name(definition.name.clone());
                node.set_transform(definition.transform);
                node.set_light(definition.light);
                node
            })
            .collect();
        for (definition, node) in data.nodes.iter().zip(&nodes) {
            for &child in &definition.children {
                node.add_child(&nodes[child]);
            }
        }
        let mut primitives = vec![Vec::new(); nodes.len()];
        for (index, definition) in data.nodes.iter().enumerate() {
            let Some(mesh) = definition.mesh else {
                continue;
            };
            let skin = definition.skin.map(|skin| {
                let skin = &data.skins[skin];
                (
                    skin.joints
                        .iter()
                        .map(|&joint| nodes[joint].downgrade())
                        .collect(),
                    skin.inverse_bind.clone(),
                )
            });
            let parts = &data.meshes[mesh];
            for (i, primitive) in parts.iter().enumerate() {
                let target = if parts.len() == 1 {
                    nodes[index].clone()
                } else {
                    let child = Node::new();
                    nodes[index].insert_child(i, &child);
                    child
                };
                target.set_mesh(Some((
                    primitive.geometry.clone(),
                    primitive.material.clone(),
                )));
                let geometry = primitive.geometry.data();
                let skinned = skin.is_some() && geometry.joints.is_some();
                if skinned || !geometry.morph_targets.is_empty() {
                    target.set_deform(Some(Rc::new(Deform::new(
                        skin.clone().filter(|_| skinned),
                        definition.weights.clone(),
                    ))));
                }
                primitives[index].push(target);
            }
        }
        let root = Node::new();
        for &index in &data.roots {
            root.add_child(&nodes[index]);
        }
        root.set_instance(Some(Rc::new(Instance {
            model: self.clone(),
            nodes,
            primitives,
        })));
        root
    }

    pub(crate) fn data(&self) -> &ModelData {
        &self.data
    }
}

#[cfg(test)]
mod tests;
