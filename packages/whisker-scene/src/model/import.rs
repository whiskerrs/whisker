use base64::Engine;
use glam::{Mat4, Quat, Vec2, Vec3};
use gltf::khr_lights_punctual::Kind;
use whisker::css::Color;

use super::animation::{Channel, Clip, Interpolation, Property};
use super::{ModelData, NodeData, Primitive, SkinData};
use crate::core::{
    AlphaMode, Geometry, Light, Material, MorphTarget, StandardMaterial, Texture, TextureTransform,
    Transform, Vertex, generate_tangents,
};

pub(super) fn import(bytes: &[u8]) -> Result<ModelData, String> {
    let gltf::Gltf { document, blob } = gltf::Gltf::from_slice(bytes).map_err(|e| e.to_string())?;
    for extension in document.extensions_required() {
        if !SUPPORTED_EXTENSIONS.contains(&extension) {
            return Err(format!(
                "the required extension {extension} is not supported"
            ));
        }
    }
    let buffers = document
        .buffers()
        .map(|buffer| match buffer.source() {
            gltf::buffer::Source::Bin => blob
                .clone()
                .ok_or_else(|| "the GLB binary chunk is missing".to_string()),
            gltf::buffer::Source::Uri(uri) => data_uri(uri),
        })
        .collect::<Result<Vec<Vec<u8>>, String>>()?;
    let textures = document
        .images()
        .map(|image| {
            let bytes = match image.source() {
                gltf::image::Source::View { view, .. } => {
                    let buffer = &buffers[view.buffer().index()];
                    buffer
                        .get(view.offset()..view.offset() + view.length())
                        .ok_or_else(|| "an image view is out of range".to_string())?
                        .to_vec()
                }
                gltf::image::Source::Uri { uri, .. } => data_uri(uri)?,
            };
            Texture::decode(&bytes).map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<Texture>, String>>()?;
    let texture = |info: Option<gltf::texture::Texture<'_>>| {
        info.map(|texture| textures[texture.source().index()].clone())
    };
    let materials: Vec<Material> = document
        .materials()
        .map(|material| {
            let pbr = material.pbr_metallic_roughness();
            let base_color = linear_color(pbr.base_color_factor());
            if material.unlit() {
                return Material::Unlit { color: base_color };
            }
            let base_info = pbr.base_color_texture();
            let texture_transform = base_info
                .as_ref()
                .and_then(|info| info.texture_transform())
                .map(|transform| TextureTransform {
                    offset: Vec2::from(transform.offset()),
                    rotation: transform.rotation(),
                    scale: Vec2::from(transform.scale()),
                })
                .unwrap_or_default();
            let [er, eg, eb] = material.emissive_factor();
            Material::Standard(StandardMaterial {
                base_color,
                metallic: pbr.metallic_factor(),
                roughness: pbr.roughness_factor(),
                emissive: linear_color([er, eg, eb, 1.0]),
                emissive_intensity: material.emissive_strength().unwrap_or(1.0),
                base_color_texture: texture(base_info.map(|info| info.texture())),
                metallic_roughness_texture: texture(
                    pbr.metallic_roughness_texture().map(|info| info.texture()),
                ),
                normal_texture: texture(material.normal_texture().map(|info| info.texture())),
                emissive_texture: texture(material.emissive_texture().map(|info| info.texture())),
                occlusion_texture: texture(material.occlusion_texture().map(|info| info.texture())),
                texture_transform,
                alpha_mode: match material.alpha_mode() {
                    gltf::material::AlphaMode::Opaque => AlphaMode::Opaque,
                    gltf::material::AlphaMode::Mask => {
                        AlphaMode::Mask(material.alpha_cutoff().unwrap_or(0.5))
                    }
                    gltf::material::AlphaMode::Blend => AlphaMode::Blend,
                },
                double_sided: material.double_sided(),
            })
        })
        .collect();
    let default_material = Material::Standard(StandardMaterial {
        alpha_mode: AlphaMode::Opaque,
        roughness: 1.0,
        metallic: 1.0,
        ..Default::default()
    });
    let read = |buffer: gltf::Buffer<'_>| buffers.get(buffer.index()).map(Vec::as_slice);
    let meshes = document
        .meshes()
        .map(|mesh| {
            mesh.primitives()
                .filter_map(|primitive| {
                    let geometry = primitive_geometry(&primitive, &read)?;
                    let material = primitive
                        .material()
                        .index()
                        .map_or_else(|| default_material.clone(), |i| materials[i].clone());
                    Some(Primitive { geometry, material })
                })
                .collect()
        })
        .collect();
    let skins = document
        .skins()
        .map(|skin| {
            let joints: Vec<usize> = skin.joints().map(|joint| joint.index()).collect();
            let inverse_bind = skin
                .reader(read)
                .read_inverse_bind_matrices()
                .map(|matrices| matrices.map(|m| Mat4::from_cols_array_2d(&m)).collect())
                .unwrap_or_else(|| vec![Mat4::IDENTITY; joints.len()]);
            SkinData {
                joints,
                inverse_bind,
            }
        })
        .collect();
    let nodes = document
        .nodes()
        .map(|node| {
            let (translation, rotation, scale) = node.transform().decomposed();
            let weights = node
                .weights()
                .or_else(|| node.mesh().and_then(|mesh| mesh.weights()))
                .map(<[f32]>::to_vec)
                .unwrap_or_else(|| {
                    let targets = node.mesh().map_or(0, |mesh| {
                        mesh.primitives()
                            .map(|p| p.morph_targets().len())
                            .max()
                            .unwrap_or(0)
                    });
                    vec![0.0; targets]
                });
            NodeData {
                name: node.name().map(str::to_owned),
                transform: Transform {
                    translation: Vec3::from(translation),
                    rotation: Quat::from_array(rotation),
                    scale: Vec3::from(scale),
                },
                mesh: node.mesh().map(|mesh| mesh.index()),
                skin: node.skin().map(|skin| skin.index()),
                light: node.light().map(light),
                weights,
                children: node.children().map(|child| child.index()).collect(),
            }
        })
        .collect();
    let roots = match document
        .default_scene()
        .or_else(|| document.scenes().next())
    {
        Some(scene) => scene.nodes().map(|node| node.index()).collect(),
        None => {
            let mut child = vec![false; document.nodes().len()];
            for node in document.nodes() {
                for c in node.children() {
                    child[c.index()] = true;
                }
            }
            (0..child.len()).filter(|&i| !child[i]).collect()
        }
    };
    let clips = document
        .animations()
        .map(|animation| clip(&animation, &read))
        .collect::<Result<Vec<Clip>, String>>()?;
    Ok(ModelData {
        nodes,
        meshes,
        skins,
        roots,
        clips,
    })
}

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "KHR_lights_punctual",
    "KHR_materials_emissive_strength",
    "KHR_materials_unlit",
    "KHR_texture_transform",
];

fn data_uri(uri: &str) -> Result<Vec<u8>, String> {
    let rest = uri
        .strip_prefix("data:")
        .ok_or_else(|| format!("external file {uri:?} is not supported; embed it or use .glb"))?;
    let (_, payload) = rest
        .split_once(";base64,")
        .ok_or_else(|| "a data URI is not base64".to_string())?;
    base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|e| e.to_string())
}

/// A linear-light factor as the sRGB color that the material API takes.
fn linear_color([r, g, b, a]: [f32; 4]) -> Color {
    let encode = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        let srgb = if c <= 0.003_130_8 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        (srgb * 255.0).round() as u8
    };
    Color::Rgba(encode(r), encode(g), encode(b), a.clamp(0.0, 1.0))
}

fn light(light: gltf::khr_lights_punctual::Light<'_>) -> Light {
    let [r, g, b] = light.color();
    let color = linear_color([r, g, b, 1.0]);
    let intensity = light.intensity();
    match light.kind() {
        Kind::Directional => Light::Directional { color, intensity },
        // Spot cones are not modeled; their light spreads like a point light's.
        Kind::Point | Kind::Spot { .. } => Light::Point {
            color,
            intensity,
            // Without a range, the falloff ends where the light drops to a negligible 1/2000.
            range: light
                .range()
                .unwrap_or_else(|| (intensity * 2000.0).sqrt().max(1.0)),
        },
    }
}

fn primitive_geometry<'a, 's>(
    primitive: &'a gltf::Primitive<'a>,
    read: &(impl Clone + Fn(gltf::Buffer<'a>) -> Option<&'s [u8]>),
) -> Option<Geometry> {
    use gltf::mesh::Mode;
    let reader = primitive.reader(read.clone());
    let positions: Vec<Vec3> = reader.read_positions()?.map(Vec3::from).collect();
    let count = positions.len();
    let raw: Vec<u32> = match reader.read_indices() {
        Some(indices) => indices.into_u32().collect(),
        None => (0..count as u32).collect(),
    };
    let indices = match primitive.mode() {
        Mode::Triangles => raw,
        Mode::TriangleStrip => (2..raw.len())
            .flat_map(|i| {
                if i % 2 == 0 {
                    [raw[i - 2], raw[i - 1], raw[i]]
                } else {
                    [raw[i - 1], raw[i - 2], raw[i]]
                }
            })
            .collect(),
        Mode::TriangleFan => (2..raw.len())
            .flat_map(|i| [raw[0], raw[i - 1], raw[i]])
            .collect(),
        _ => return None,
    };
    let indices: Vec<u32> = indices
        .chunks_exact(3)
        .filter(|t| t.iter().all(|&i| (i as usize) < count))
        .flatten()
        .copied()
        .collect();
    let normals: Option<Vec<Vec3>> = reader.read_normals().map(|n| n.map(Vec3::from).collect());
    let uvs: Vec<Vec2> = reader
        .read_tex_coords(0)
        .map(|uvs| uvs.into_f32().map(Vec2::from).collect())
        .unwrap_or_else(|| vec![Vec2::ZERO; count]);
    let colors: Option<Vec<[f32; 4]>> = reader
        .read_colors(0)
        .map(|colors| colors.into_rgba_f32().collect());
    let tangents: Option<Vec<[f32; 4]>> = reader.read_tangents().map(Iterator::collect);
    let vertices: Vec<Vertex> = (0..count)
        .map(|i| {
            let mut vertex = Vertex::new(
                positions[i],
                normals.as_ref().map_or(Vec3::ZERO, |n| n[i]),
                uvs.get(i).copied().unwrap_or(Vec2::ZERO),
            );
            if let Some(colors) = &colors {
                vertex.color = colors[i];
            }
            vertex
        })
        .collect();
    // Without normals, each corner becomes a vertex of its own; `source` maps it back.
    let source: Option<Vec<u32>> = normals.is_none().then(|| indices.clone());
    let (mut vertices, indices) = match &source {
        Some(_) => flat_shaded(&vertices, &indices),
        None => (vertices, indices),
    };
    match (&tangents, &source) {
        (Some(tangents), None) => {
            for (vertex, tangent) in vertices.iter_mut().zip(tangents) {
                vertex.tangent = *tangent;
            }
        }
        _ => generate_tangents(&mut vertices, &indices),
    }
    let remap = |values: Vec<Vec3>| match &source {
        Some(source) if !values.is_empty() => source.iter().map(|&i| values[i as usize]).collect(),
        _ => values,
    };
    let joints = match (reader.read_joints(0), reader.read_weights(0)) {
        (Some(joints), Some(weights)) => {
            let influences: Vec<([u16; 4], [f32; 4])> = joints
                .into_u16()
                .zip(weights.into_f32())
                .map(|(joints, weights)| {
                    let sum: f32 = weights.iter().sum();
                    let weights = if sum > 0.0 {
                        weights.map(|w| w / sum)
                    } else {
                        weights
                    };
                    (joints, weights)
                })
                .collect();
            Some(match &source {
                Some(source) => source.iter().map(|&i| influences[i as usize]).collect(),
                None => influences,
            })
        }
        _ => None,
    };
    let morph_targets = reader
        .read_morph_targets()
        .map(|(positions, normals, tangents)| MorphTarget {
            positions: remap(positions.map_or_else(Vec::new, |p| p.map(Vec3::from).collect())),
            normals: remap(normals.map_or_else(Vec::new, |n| n.map(Vec3::from).collect())),
            tangents: remap(tangents.map_or_else(Vec::new, |t| t.map(Vec3::from).collect())),
        })
        .collect();
    Some(Geometry::deformable(
        vertices,
        indices,
        joints,
        morph_targets,
    ))
}

/// Unshares vertices so each triangle carries its own face normal.
fn flat_shaded(vertices: &[Vertex], indices: &[u32]) -> (Vec<Vertex>, Vec<u32>) {
    let mut out = Vec::with_capacity(indices.len());
    for triangle in indices.chunks_exact(3) {
        let [a, b, c] = [0, 1, 2].map(|i| vertices[triangle[i] as usize]);
        let normal = (Vec3::from(b.position) - Vec3::from(a.position))
            .cross(Vec3::from(c.position) - Vec3::from(a.position))
            .normalize_or_zero()
            .to_array();
        for mut vertex in [a, b, c] {
            vertex.normal = normal;
            out.push(vertex);
        }
    }
    let indices = (0..out.len() as u32).collect();
    (out, indices)
}

fn clip<'a, 's>(
    animation: &gltf::Animation<'a>,
    read: &(impl Clone + Fn(gltf::Buffer<'a>) -> Option<&'s [u8]>),
) -> Result<Clip, String> {
    use gltf::animation::util::ReadOutputs;
    let mut channels = Vec::new();
    for channel in animation.channels() {
        let reader = channel.reader(read.clone());
        let Some(times) = reader.read_inputs() else {
            continue;
        };
        let times: Vec<f32> = times.collect();
        let Some(outputs) = reader.read_outputs() else {
            continue;
        };
        let (property, values): (Property, Vec<f32>) = match outputs {
            ReadOutputs::Translations(values) => {
                (Property::Translation, values.flatten().collect())
            }
            ReadOutputs::Rotations(values) => {
                (Property::Rotation, values.into_f32().flatten().collect())
            }
            ReadOutputs::Scales(values) => (Property::Scale, values.flatten().collect()),
            ReadOutputs::MorphTargetWeights(values) => {
                (Property::Weights, values.into_f32().collect())
            }
        };
        let interpolation = match channel.sampler().interpolation() {
            gltf::animation::Interpolation::Linear => Interpolation::Linear,
            gltf::animation::Interpolation::Step => Interpolation::Step,
            gltf::animation::Interpolation::CubicSpline => Interpolation::CubicSpline,
        };
        if times.is_empty() {
            continue;
        }
        channels.push(Channel::new(
            channel.target().node().index(),
            property,
            interpolation,
            times,
            values,
        ));
    }
    let name = animation
        .name()
        .map(str::to_owned)
        .unwrap_or_else(|| animation.index().to_string());
    Ok(Clip::new(name, channels))
}
