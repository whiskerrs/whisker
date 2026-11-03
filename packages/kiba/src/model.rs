use std::sync::{Arc, OnceLock};

use glam::Mat4;

use crate::{Material, Mesh};

#[derive(Clone, Debug)]
pub(crate) struct ModelPart {
    pub(crate) mesh: Mesh,
    pub(crate) material: Material,
    pub(crate) transform: Mat4,
}

/// Meshes placed relative to an entity, used as a component next to a [`Transform`](crate::Transform).
///
/// Clones share their meshes, so entities using one model draw as instances.
#[derive(Clone, Debug)]
pub struct Model(Arc<OnceLock<Result<Vec<ModelPart>, String>>>);

/// How far a [`Model::load`] has come.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadState {
    /// Still loading. The model draws nothing until it is ready.
    Loading,
    /// Loaded and drawing.
    Ready,
    /// Failed for the given reason. The model draws nothing.
    Failed(String),
}

impl Model {
    /// Creates a model from meshes, each with its material and its transform within the model.
    pub fn new(parts: impl IntoIterator<Item = (Mesh, Material, Mat4)>) -> Self {
        let parts = parts
            .into_iter()
            .map(|(mesh, material, transform)| ModelPart {
                mesh,
                material,
                transform,
            })
            .collect();
        Self(Arc::new(OnceLock::from(Ok(parts))))
    }

    /// Reports how far loading has come.
    pub fn load_state(&self) -> LoadState {
        match self.0.get() {
            None => LoadState::Loading,
            Some(Ok(_)) => LoadState::Ready,
            Some(Err(reason)) => LoadState::Failed(reason.clone()),
        }
    }

    pub(crate) fn parts(&self) -> &[ModelPart] {
        match self.0.get() {
            Some(Ok(parts)) => parts,
            _ => &[],
        }
    }
}

#[cfg(feature = "gltf")]
pub use import::ModelError;

#[cfg(feature = "gltf")]
mod import {
    use std::collections::HashMap;
    use std::fmt;
    use std::sync::{Arc, OnceLock};

    use glam::{Mat4, Vec3};

    use super::Model;
    use crate::renderer::log;
    use crate::{Material, Mesh, Texture, Vertex};

    /// Why a file could not become a [`Model`].
    #[derive(Debug)]
    pub struct ModelError(String);

    impl fmt::Display for ModelError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.0)
        }
    }

    impl std::error::Error for ModelError {}

    fn error(message: impl Into<String>) -> ModelError {
        ModelError(message.into())
    }

    impl Model {
        /// Starts loading a binary glTF file and returns at once.
        ///
        /// `location` is a URL on the Web and a path elsewhere, as `asset!` gives. Check
        /// [`Model::load_state`] to learn about progress and failures, which are also logged.
        pub fn load(location: impl Into<String>) -> Self {
            let state = Arc::new(OnceLock::new());
            let finish = {
                let state = state.clone();
                move |result: Result<Vec<u8>, String>| {
                    let parsed = result.and_then(|bytes| {
                        Model::from_glb(&bytes)
                            .map(|model| model.parts().to_vec())
                            .map_err(|error| error.to_string())
                    });
                    if let Err(reason) = &parsed {
                        log(&format!("failed to load a model: {reason}"));
                    }
                    let _ = state.set(parsed);
                }
            };
            read_file(location.into(), finish);
            Model(state)
        }

        /// Reads the default scene of a binary glTF file.
        ///
        /// Takes geometry, vertex colors, and base color, metallic-roughness, and normal
        /// materials. Other textures and non-triangle primitives are left out with a log line.
        /// Decoding images blocks the caller, and textures ignore their glTF sampler.
        pub fn from_glb(bytes: &[u8]) -> Result<Self, ModelError> {
            let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| error(e.to_string()))?;
            let mut scenes = gltf.scenes();
            let scene = gltf
                .default_scene()
                .or_else(|| scenes.next())
                .ok_or_else(|| error("the file has no scene"))?;
            let mut importer = Importer {
                blob: gltf.blob.as_deref(),
                meshes: HashMap::new(),
                images: HashMap::new(),
                parts: Vec::new(),
                unsupported_textures: false,
            };
            for node in scene.nodes() {
                importer.visit(&node, Mat4::IDENTITY)?;
            }
            if importer.unsupported_textures {
                log("ignored glTF occlusion or emissive textures, or a second UV set");
            }
            Ok(Model::new(importer.parts))
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn read_file(url: String, finish: impl FnOnce(Result<Vec<u8>, String>) + 'static) {
        use wasm_bindgen::JsCast;

        wasm_bindgen_futures::spawn_local(async move {
            let fetched = async {
                let window = web_sys::window().ok_or("no window")?;
                let response = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(&url))
                    .await
                    .map_err(|error| format!("{url}: {error:?}"))?
                    .unchecked_into::<web_sys::Response>();
                if !response.ok() {
                    return Err(format!("{url}: HTTP {}", response.status()));
                }
                let buffer = response.array_buffer().map_err(|e| format!("{e:?}"))?;
                let buffer = wasm_bindgen_futures::JsFuture::from(buffer)
                    .await
                    .map_err(|e| format!("{e:?}"))?;
                Ok::<_, String>(js_sys::Uint8Array::new(&buffer).to_vec())
            };
            finish(fetched.await);
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn read_file(path: String, finish: impl FnOnce(Result<Vec<u8>, String>) + Send + 'static) {
        std::thread::spawn(move || {
            finish(std::fs::read(&path).map_err(|error| format!("{path}: {error}")));
        });
    }

    struct Importer<'a> {
        blob: Option<&'a [u8]>,
        meshes: HashMap<(usize, usize, bool), Mesh>,
        images: HashMap<usize, Texture>,
        parts: Vec<(Mesh, Material, Mat4)>,
        unsupported_textures: bool,
    }

    impl Importer<'_> {
        fn visit(&mut self, node: &gltf::Node<'_>, parent: Mat4) -> Result<(), ModelError> {
            let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
            if let Some(mesh) = node.mesh() {
                // A mirrored transform turns triangles inside out, which glTF requires undoing.
                let mirrored = transform.determinant() < 0.0;
                for primitive in mesh.primitives() {
                    if primitive.mode() != gltf::mesh::Mode::Triangles {
                        log(&format!("skipped a {:?} primitive", primitive.mode()));
                        continue;
                    }
                    let key = (mesh.index(), primitive.index(), mirrored);
                    let part_mesh = match self.meshes.get(&key) {
                        Some(mesh) => mesh.clone(),
                        None => {
                            let read = read_mesh(&primitive, self.blob, mirrored)?;
                            self.meshes.insert(key, read.clone());
                            read
                        }
                    };
                    let material = self.read_material(&primitive.material())?;
                    self.parts.push((part_mesh, material, transform));
                }
            }
            for child in node.children() {
                self.visit(&child, transform)?;
            }
            Ok(())
        }

        fn read_material(&mut self, material: &gltf::Material<'_>) -> Result<Material, ModelError> {
            let pbr = material.pbr_metallic_roughness();
            let base = pbr.base_color_texture();
            let metal_rough = pbr.metallic_roughness_texture();
            let normal = material.normal_texture();
            self.unsupported_textures |= material.occlusion_texture().is_some()
                || material.emissive_texture().is_some()
                || base.as_ref().is_some_and(|t| t.tex_coord() != 0)
                || metal_rough.as_ref().is_some_and(|t| t.tex_coord() != 0)
                || normal.as_ref().is_some_and(|t| t.tex_coord() != 0);
            let [r, g, b, _] = pbr.base_color_factor();
            Ok(Material {
                base_color: Vec3::new(r, g, b),
                metallic: pbr.metallic_factor(),
                roughness: pbr.roughness_factor(),
                base_color_texture: base.map(|t| self.read_image(&t.texture())).transpose()?,
                metallic_roughness_texture: metal_rough
                    .map(|t| self.read_image(&t.texture()))
                    .transpose()?,
                normal_texture: normal.map(|t| self.read_image(&t.texture())).transpose()?,
            })
        }

        fn read_image(&mut self, texture: &gltf::Texture<'_>) -> Result<Texture, ModelError> {
            let source = texture.source();
            if let Some(image) = self.images.get(&source.index()) {
                return Ok(image.clone());
            }
            let gltf::image::Source::View { view, .. } = source.source() else {
                return Err(error("external images are not supported"));
            };
            let blob = self
                .blob
                .ok_or_else(|| error("the file has no binary chunk"))?;
            let bytes = blob
                .get(view.offset()..view.offset() + view.length())
                .ok_or_else(|| error("an image is outside the binary chunk"))?;
            let decoded = image::load_from_memory(bytes)
                .map_err(|e| error(e.to_string()))?
                .into_rgba8();
            let (width, height) = decoded.dimensions();
            let image = Texture::from_rgba8(width, height, decoded.into_raw());
            self.images.insert(source.index(), image.clone());
            Ok(image)
        }
    }

    fn read_mesh(
        primitive: &gltf::Primitive<'_>,
        blob: Option<&[u8]>,
        mirrored: bool,
    ) -> Result<Mesh, ModelError> {
        let reader = primitive.reader(|buffer| match buffer.source() {
            gltf::buffer::Source::Bin => blob,
            gltf::buffer::Source::Uri(_) => None,
        });
        let positions: Vec<[f32; 3]> = reader
            .read_positions()
            .ok_or_else(|| error("a primitive has no readable positions"))?
            .collect();
        let normals: Option<Vec<[f32; 3]>> = reader.read_normals().map(Iterator::collect);
        let colors: Option<Vec<[f32; 3]>> = reader
            .read_colors(0)
            .map(|colors| colors.into_rgb_f32().collect());
        let indices: Vec<u32> = match reader.read_indices() {
            Some(indices) => indices.into_u32().collect(),
            None => (0..positions.len() as u32).collect(),
        };
        if !indices.len().is_multiple_of(3) {
            return Err(error("a primitive's indices do not form triangles"));
        }
        if indices.iter().any(|&i| i as usize >= positions.len()) {
            return Err(error("a primitive's index is out of range"));
        }
        let uvs: Option<Vec<[f32; 2]>> = reader
            .read_tex_coords(0)
            .map(|uvs| uvs.into_f32().collect());
        let uv_at = |i: usize| uvs.as_ref().and_then(|u| u.get(i)).map_or([0.0; 2], |u| *u);
        let color_at = |i: usize| {
            colors
                .as_ref()
                .and_then(|c| c.get(i))
                .map_or([1.0; 3], |c| *c)
        };

        let (vertices, mut indices) = match normals {
            Some(normals) if normals.len() == positions.len() => {
                let vertices = positions
                    .iter()
                    .zip(&normals)
                    .enumerate()
                    .map(|(i, (position, normal))| Vertex {
                        position: *position,
                        normal: *normal,
                        color: color_at(i),
                        uv: uv_at(i),
                    })
                    .collect();
                (vertices, indices)
            }
            // glTF asks for flat shading here, which needs one vertex per triangle corner.
            _ => {
                let mut vertices = Vec::with_capacity(indices.len());
                for triangle in indices.chunks_exact(3) {
                    let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(positions[triangle[i] as usize]));
                    let normal = (b - a).cross(c - a).normalize_or_zero().to_array();
                    vertices.extend(triangle.iter().map(|&i| Vertex {
                        position: positions[i as usize],
                        normal,
                        color: color_at(i as usize),
                        uv: uv_at(i as usize),
                    }));
                }
                let count = vertices.len() as u32;
                (vertices, (0..count).collect())
            }
        };
        if mirrored {
            for triangle in indices.chunks_exact_mut(3) {
                triangle.swap(1, 2);
            }
        }
        Ok(Mesh::new(vertices, indices))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Builds a GLB holding one triangle, with `extra` merged into its JSON's mesh setup.
        fn glb(json: &str, bin: &[u8]) -> Vec<u8> {
            let mut json = json.as_bytes().to_vec();
            json.resize(json.len().next_multiple_of(4), b' ');
            let mut bin = bin.to_vec();
            bin.resize(bin.len().next_multiple_of(4), 0);
            let total = 12 + 8 + json.len() + 8 + bin.len();
            let mut out = Vec::new();
            out.extend(b"glTF");
            out.extend(2u32.to_le_bytes());
            out.extend((total as u32).to_le_bytes());
            out.extend((json.len() as u32).to_le_bytes());
            out.extend(b"JSON");
            out.extend(json);
            out.extend((bin.len() as u32).to_le_bytes());
            out.extend(b"BIN\0");
            out.extend(bin);
            out
        }

        fn triangle_bin(indices: [u16; 3]) -> Vec<u8> {
            let mut bin = Vec::new();
            for value in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
                bin.extend(value.to_le_bytes());
            }
            for value in [0.0f32, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0] {
                bin.extend(value.to_le_bytes());
            }
            for index in indices {
                bin.extend(index.to_le_bytes());
            }
            bin
        }

        /// One triangle mesh, used by the given `nodes`, with or without its normals.
        fn triangle_json(nodes: &str, scene_nodes: &str, normals: bool, length: usize) -> String {
            let normal = if normals { r#","NORMAL":1"# } else { "" };
            format!(
                r#"{{"asset":{{"version":"2.0"}},"scene":0,
                "scenes":[{{"nodes":[{scene_nodes}]}}],"nodes":[{nodes}],
                "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0{normal}}},"indices":2,"material":0}}]}}],
                "materials":[{{"pbrMetallicRoughness":{{"baseColorFactor":[0.5,0.25,1,1],"metallicFactor":0.75,"roughnessFactor":0.2}}}}],
                "buffers":[{{"byteLength":{length}}}],
                "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":36}},{{"buffer":0,"byteOffset":72,"byteLength":6}}],
                "accessors":[
                  {{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}},
                  {{"bufferView":1,"componentType":5126,"count":3,"type":"VEC3"}},
                  {{"bufferView":2,"componentType":5123,"count":3,"type":"SCALAR"}}]}}"#
            )
        }

        fn load(nodes: &str, scene_nodes: &str, normals: bool) -> Model {
            let bin = triangle_bin([0, 1, 2]);
            let json = triangle_json(nodes, scene_nodes, normals, bin.len());
            Model::from_glb(&glb(&json, &bin)).expect("valid file")
        }

        #[test]
        fn material_factors_are_read() {
            let model = load(r#"{"mesh":0}"#, "0", true);
            let material = &model.parts()[0].material;
            assert_eq!(material.base_color, Vec3::new(0.5, 0.25, 1.0));
            assert_eq!(material.metallic, 0.75);
            assert_eq!(material.roughness, 0.2);
        }

        #[test]
        fn child_nodes_inherit_their_parents_transform() {
            let model = load(
                r#"{"translation":[0,2,0],"children":[1]},{"mesh":0,"translation":[1,0,0]}"#,
                "0",
                true,
            );
            let origin = model.parts()[0].transform.transform_point3(Vec3::ZERO);
            assert_eq!(origin, Vec3::new(1.0, 2.0, 0.0));
        }

        #[test]
        fn nodes_reusing_a_mesh_share_one_copy() {
            let model = load(
                r#"{"mesh":0},{"mesh":0,"translation":[1,0,0]}"#,
                "0,1",
                true,
            );
            assert_eq!(model.parts().len(), 2);
            assert_eq!(model.parts()[0].mesh.key(), model.parts()[1].mesh.key());
        }

        #[test]
        fn mirrored_nodes_keep_triangles_facing_out() {
            let model = load(r#"{"mesh":0},{"mesh":0,"scale":[-1,1,1]}"#, "0,1", true);
            let front = |part: &crate::model::ModelPart| {
                let data = part.mesh.data();
                let [a, b, c] = [0, 1, 2].map(|i| {
                    part.transform
                        .transform_point3(data.vertices[data.indices[i] as usize].position.into())
                });
                (b - a).cross(c - a).z
            };
            assert!(front(&model.parts()[0]) > 0.0);
            assert!(front(&model.parts()[1]) > 0.0);
        }

        #[test]
        fn missing_normals_become_flat_ones() {
            let model = load(r#"{"mesh":0}"#, "0", false);
            for vertex in &model.parts()[0].mesh.data().vertices {
                assert_eq!(vertex.normal, [0.0, 0.0, 1.0]);
            }
        }

        #[test]
        fn loading_reports_progress_and_failure_without_panicking() {
            let model = Model::load("/nonexistent/model.glb");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while model.load_state() == crate::LoadState::Loading {
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert!(matches!(model.load_state(), crate::LoadState::Failed(_)));
            assert!(model.parts().is_empty());
        }

        #[test]
        fn broken_files_are_errors_rather_than_panics() {
            assert!(Model::from_glb(b"not a glb").is_err());

            let bin = triangle_bin([0, 1, 9]);
            let json = triangle_json(r#"{"mesh":0}"#, "0", true, bin.len());
            let message = Model::from_glb(&glb(&json, &bin)).unwrap_err().to_string();
            assert!(message.contains("out of range"), "{message}");
        }
    }
}
