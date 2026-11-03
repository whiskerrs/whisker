//! The cooked file format: a header, then tagged sections. Readers skip sections they do not use.

#[cfg(any(test, feature = "cook"))]
use std::collections::HashMap;

use bytemuck::Pod;
use glam::{Mat4, Quat, Vec3};

use crate::animation::{Channel, Clip, Keys, MAX_JOINTS, Pose, Rig, RigNode, Skin};
use crate::mesh::SkinVertex;
use crate::model::{Lod, ModelData, ModelPart};
use crate::{AlphaMode, Material, Mesh, Texture, Vertex};

const MAGIC: &[u8; 4] = b"KIBA";
const VERSION: u32 = 3;
pub(crate) const MODEL_SECTION: &[u8; 4] = b"MODL";
pub(crate) const PROBE_SECTION: &[u8; 4] = b"PROB";
pub(crate) const WORLD_SECTION: &[u8; 4] = b"WRLD";

pub(crate) fn is_pack(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

#[cfg(any(test, feature = "cook"))]
pub(crate) fn write_pack(sections: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(MAGIC);
    out.extend(VERSION.to_le_bytes());
    for (tag, payload) in sections {
        out.extend(*tag);
        out.extend((payload.len() as u64).to_le_bytes());
        out.extend(payload);
    }
    out
}

pub(crate) fn read_section<'a>(bytes: &'a [u8], tag: &[u8; 4]) -> Result<&'a [u8], String> {
    if !is_pack(bytes) {
        return Err("the file is not a cooked pack".into());
    }
    let mut reader = Reader { bytes, at: 4 };
    let version = reader.u32()?;
    if version != VERSION {
        return Err(format!(
            "pack version {version}, and this engine reads {VERSION}"
        ));
    }
    while reader.at < bytes.len() {
        let found = reader.take(4)?;
        let length = reader.u64()? as usize;
        let payload = reader.take(length)?;
        if found == tag {
            return Ok(payload);
        }
    }
    Err(format!(
        "the pack has no {} section",
        String::from_utf8_lossy(tag)
    ))
}

#[cfg(any(test, feature = "cook"))]
#[derive(Default)]
pub(crate) struct Writer(pub(crate) Vec<u8>);

#[cfg(any(test, feature = "cook"))]
impl Writer {
    pub(crate) fn u32(&mut self, value: u32) {
        self.0.extend(value.to_le_bytes());
    }

    pub(crate) fn f32(&mut self, value: f32) {
        self.0.extend(value.to_le_bytes());
    }

    fn option(&mut self, value: Option<usize>) {
        self.u32(value.map_or(u32::MAX, |value| value as u32));
    }

    pub(crate) fn vec3(&mut self, value: Vec3) {
        for part in value.to_array() {
            self.f32(part);
        }
    }

    fn quat(&mut self, value: Quat) {
        for part in value.to_array() {
            self.f32(part);
        }
    }

    fn mat4(&mut self, value: &Mat4) {
        for part in value.to_cols_array() {
            self.f32(part);
        }
    }

    pub(crate) fn text(&mut self, value: &str) {
        self.u32(value.len() as u32);
        self.0.extend(value.as_bytes());
    }

    pub(crate) fn slice<T: Pod>(&mut self, values: &[T]) {
        self.u32(values.len() as u32);
        self.0.extend_from_slice(bytemuck::cast_slice(values));
    }
}

pub(crate) struct Reader<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) at: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(length)
            .ok_or("a section's length overflows")?;
        let taken = self
            .bytes
            .get(self.at..end)
            .ok_or("the pack ends too early")?;
        self.at = end;
        Ok(taken)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        Ok(self.take(N)?.try_into().expect("took exactly N bytes"))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    pub(crate) fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    fn option(&mut self) -> Result<Option<usize>, String> {
        let value = self.u32()?;
        Ok((value != u32::MAX).then_some(value as usize))
    }

    pub(crate) fn vec3(&mut self) -> Result<Vec3, String> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }

    fn quat(&mut self) -> Result<Quat, String> {
        Ok(Quat::from_xyzw(
            self.f32()?,
            self.f32()?,
            self.f32()?,
            self.f32()?,
        ))
    }

    fn mat4(&mut self) -> Result<Mat4, String> {
        let mut columns = [0.0; 16];
        for part in &mut columns {
            *part = self.f32()?;
        }
        Ok(Mat4::from_cols_array(&columns))
    }

    pub(crate) fn text(&mut self) -> Result<String, String> {
        let length = self.u32()? as usize;
        String::from_utf8(self.take(length)?.to_vec()).map_err(|error| error.to_string())
    }

    pub(crate) fn slice<T: Pod>(&mut self) -> Result<Vec<T>, String> {
        let count = self.u32()? as usize;
        let length = count
            .checked_mul(size_of::<T>())
            .ok_or("a section's length overflows")?;
        Ok(bytemuck::pod_collect_to_vec(self.take(length)?))
    }

    fn list<T>(
        &mut self,
        mut read: impl FnMut(&mut Self) -> Result<T, String>,
    ) -> Result<Vec<T>, String> {
        let count = self.u32()? as usize;
        // Every item takes at least a byte, so a corrupt count cannot demand a huge allocation.
        if count > self.bytes.len() - self.at {
            return Err("the pack ends too early".into());
        }
        (0..count).map(|_| read(self)).collect()
    }
}

#[cfg(any(test, feature = "cook"))]
pub(crate) fn write_model(data: &ModelData) -> Vec<u8> {
    let mut textures = Vec::new();
    let mut texture_slots = HashMap::new();
    let mut meshes = Vec::new();
    let mut mesh_slots = HashMap::new();
    for part in &data.parts {
        for mesh in std::iter::once(&part.mesh).chain(part.lods.iter().map(|lod| &lod.mesh)) {
            mesh_slots.entry(mesh.key()).or_insert_with(|| {
                meshes.push(mesh.clone());
                meshes.len() - 1
            });
        }
        let material = &part.material;
        for texture in [
            &material.base_color_texture,
            &material.metallic_roughness_texture,
            &material.normal_texture,
            &material.emissive_texture,
        ]
        .into_iter()
        .flatten()
        {
            texture_slots.entry(texture.key()).or_insert_with(|| {
                textures.push(texture.clone());
                textures.len() - 1
            });
        }
    }

    let mut out = Writer::default();
    out.u32(textures.len() as u32);
    for texture in &textures {
        let data = texture.data();
        out.u32(data.width);
        out.u32(data.height);
        out.slice(&data.pixels);
    }
    out.u32(meshes.len() as u32);
    for mesh in &meshes {
        let data = mesh.data();
        out.slice(&data.vertices);
        out.slice(&data.indices);
        out.slice(data.skin.as_deref().unwrap_or(&[]));
    }
    out.u32(data.parts.len() as u32);
    for part in &data.parts {
        out.u32(mesh_slots[&part.mesh.key()] as u32);
        let material = &part.material;
        out.vec3(material.base_color);
        out.f32(material.metallic);
        out.f32(material.roughness);
        out.vec3(material.emissive);
        out.f32(material.opacity);
        let (mode, cutoff) = match material.alpha_mode {
            AlphaMode::Opaque => (0, 0.0),
            AlphaMode::Mask { cutoff } => (1, cutoff),
            AlphaMode::Blend => (2, 0.0),
        };
        out.u32(mode);
        out.f32(cutoff);
        out.u32(u32::from(material.double_sided));
        for texture in [
            &material.base_color_texture,
            &material.metallic_roughness_texture,
            &material.normal_texture,
            &material.emissive_texture,
        ] {
            out.option(
                texture
                    .as_ref()
                    .map(|texture| texture_slots[&texture.key()]),
            );
        }
        out.mat4(&part.transform);
        out.option(part.node);
        out.option(part.skin);
        out.u32(part.lods.len() as u32);
        for lod in &part.lods {
            out.f32(lod.distance);
            out.u32(mesh_slots[&lod.mesh.key()] as u32);
        }
    }
    write_rig(&mut out, data.rig.as_ref());
    out.0
}

#[cfg(any(test, feature = "cook"))]
fn write_rig(out: &mut Writer, rig: Option<&Rig>) {
    let Some(rig) = rig else {
        out.u32(0);
        return;
    };
    out.u32(1);
    out.u32(rig.nodes.len() as u32);
    for node in &rig.nodes {
        out.option(node.parent);
        out.vec3(node.rest.translation);
        out.quat(node.rest.rotation);
        out.vec3(node.rest.scale);
    }
    out.u32(rig.skins.len() as u32);
    for skin in &rig.skins {
        out.u32(skin.joints.len() as u32);
        for (joint, inverse_bind) in skin.joints.iter().zip(&skin.inverse_binds) {
            out.u32(*joint as u32);
            out.mat4(inverse_bind);
        }
    }
    out.u32(rig.clips.len() as u32);
    for (name, clip) in &rig.clips {
        out.text(name);
        out.f32(clip.duration);
        out.u32(clip.channels.len() as u32);
        for channel in &clip.channels {
            out.u32(channel.node as u32);
            out.u32(u32::from(channel.step));
            out.slice(&channel.times);
            match &channel.keys {
                Keys::Translation(keys) => {
                    out.u32(0);
                    out.slice(keys);
                }
                Keys::Rotation(keys) => {
                    out.u32(1);
                    out.slice(keys);
                }
                Keys::Scale(keys) => {
                    out.u32(2);
                    out.slice(keys);
                }
            }
        }
    }
}

pub(crate) fn read_model(bytes: &[u8]) -> Result<ModelData, String> {
    let mut input = Reader::new(bytes);
    let textures = input.list(|input| {
        let (width, height) = (input.u32()?, input.u32()?);
        let pixels: Vec<u8> = input.slice()?;
        if width == 0
            || height == 0
            || pixels.len() as u64 != u64::from(width) * u64::from(height) * 4
        {
            return Err("a texture's size does not match its pixels".to_string());
        }
        Ok(Texture::from_rgba8(width, height, pixels))
    })?;
    let meshes = input.list(|input| {
        let vertices: Vec<Vertex> = input.slice()?;
        let indices: Vec<u32> = input.slice()?;
        let skin: Vec<SkinVertex> = input.slice()?;
        if !indices.len().is_multiple_of(3) || indices.iter().any(|&i| i as usize >= vertices.len())
        {
            return Err("a mesh's indices are invalid".to_string());
        }
        if skin.is_empty() {
            return Ok(Mesh::new(vertices, indices));
        }
        if skin.len() != vertices.len() {
            return Err("a mesh's skin does not match its vertices".to_string());
        }
        Ok(Mesh::skinned(vertices, indices, skin))
    })?;
    let checked = |index: Option<usize>, length: usize, what: &str| match index {
        Some(index) if index >= length => Err(format!("a {what} index is out of range")),
        _ => Ok(index),
    };
    let parts = input.list(|input| {
        let mesh = meshes
            .get(input.u32()? as usize)
            .ok_or("a part's mesh is out of range")?
            .clone();
        let (base_color, metallic, roughness) = (input.vec3()?, input.f32()?, input.f32()?);
        let (emissive, opacity) = (input.vec3()?, input.f32()?);
        let (mode, cutoff) = (input.u32()?, input.f32()?);
        let alpha_mode = match mode {
            0 => AlphaMode::Opaque,
            1 => AlphaMode::Mask { cutoff },
            2 => AlphaMode::Blend,
            _ => return Err("a material's alpha mode is unknown".into()),
        };
        let double_sided = input.u32()? != 0;
        let texture = |input: &mut Reader<'_>| -> Result<Option<Texture>, String> {
            let slot = checked(input.option()?, textures.len(), "texture")?;
            Ok(slot.map(|slot| textures[slot].clone()))
        };
        let material = Material {
            base_color,
            metallic,
            roughness,
            emissive,
            opacity,
            alpha_mode,
            double_sided,
            base_color_texture: texture(input)?,
            metallic_roughness_texture: texture(input)?,
            normal_texture: texture(input)?,
            emissive_texture: texture(input)?,
        };
        let transform = input.mat4()?;
        let node = input.option()?;
        let skin = input.option()?;
        let lods = input.list(|input| {
            let distance = input.f32()?;
            let mesh = meshes
                .get(input.u32()? as usize)
                .ok_or("a part's mesh is out of range")?
                .clone();
            Ok(Lod { distance, mesh })
        })?;
        Ok(ModelPart {
            mesh,
            material,
            transform,
            node,
            skin,
            lods,
        })
    })?;
    let rig = read_rig(&mut input)?;
    let (nodes, skins) = rig
        .as_ref()
        .map_or((0, 0), |rig| (rig.nodes.len(), rig.skins.len()));
    for part in &parts {
        checked(part.node, nodes, "node")?;
        checked(part.skin, skins, "skin")?;
    }
    Ok(ModelData { parts, rig })
}

fn read_rig(input: &mut Reader<'_>) -> Result<Option<Rig>, String> {
    if input.u32()? == 0 {
        return Ok(None);
    }
    let mut at = 0;
    let nodes = input.list(|input| {
        let parent = input.option()?;
        if parent.is_some_and(|parent| parent >= at) {
            return Err("a node comes before its parent".to_string());
        }
        at += 1;
        Ok(RigNode {
            parent,
            rest: Pose {
                translation: input.vec3()?,
                rotation: input.quat()?,
                scale: input.vec3()?,
            },
        })
    })?;
    let count = nodes.len();
    let check_node = |node: usize| {
        (node < count)
            .then_some(node)
            .ok_or("a joint's node is out of range".to_string())
    };
    let skins = input.list(|input| {
        let pairs = input.list(|input| Ok((check_node(input.u32()? as usize)?, input.mat4()?)))?;
        if pairs.len() > MAX_JOINTS {
            return Err("a skin has too many joints".to_string());
        }
        let (joints, inverse_binds) = pairs.into_iter().unzip();
        Ok(Skin {
            joints,
            inverse_binds,
        })
    })?;
    let clips = input.list(|input| {
        let name = input.text()?;
        let duration = input.f32()?;
        let channels = input.list(|input| {
            let node = check_node(input.u32()? as usize)?;
            let step = input.u32()? != 0;
            let times: Vec<f32> = input.slice()?;
            let (keys, length) = match input.u32()? {
                0 => {
                    let keys: Vec<Vec3> = input.slice()?;
                    let length = keys.len();
                    (Keys::Translation(keys), length)
                }
                1 => {
                    let keys: Vec<Quat> = input.slice()?;
                    let length = keys.len();
                    (Keys::Rotation(keys), length)
                }
                2 => {
                    let keys: Vec<Vec3> = input.slice()?;
                    let length = keys.len();
                    (Keys::Scale(keys), length)
                }
                other => return Err(format!("unknown channel kind {other}")),
            };
            if times.is_empty() || length != times.len() {
                return Err("an animation channel has mismatched keys".to_string());
            }
            Ok(Channel {
                node,
                times,
                keys,
                step,
            })
        })?;
        Ok((name, Clip { duration, channels }))
    })?;
    Ok(Some(Rig {
        nodes,
        skins,
        clips: clips.into_iter().collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> ModelData {
        let texture = Texture::from_rgba8(1, 1, vec![1, 2, 3, 4]);
        let mesh = Mesh::cube(1.0);
        let part = |transform| ModelPart {
            mesh: mesh.clone(),
            material: Material {
                emissive: Vec3::new(0.5, 0.25, 0.0),
                opacity: 0.75,
                alpha_mode: AlphaMode::Mask { cutoff: 0.3 },
                double_sided: true,
                base_color_texture: Some(texture.clone()),
                emissive_texture: Some(texture.clone()),
                ..Material::default()
            },
            transform,
            node: Some(0),
            skin: None,
            lods: vec![Lod {
                distance: 10.0,
                mesh: Mesh::plane(1.0),
            }],
        };
        ModelData {
            parts: vec![part(Mat4::IDENTITY), part(Mat4::from_translation(Vec3::X))],
            rig: Some(Rig {
                nodes: vec![RigNode {
                    parent: None,
                    rest: Pose {
                        translation: Vec3::Y,
                        rotation: Quat::IDENTITY,
                        scale: Vec3::ONE,
                    },
                }],
                skins: Vec::new(),
                clips: HashMap::from([(
                    "wave".to_string(),
                    Clip {
                        duration: 1.0,
                        channels: vec![Channel {
                            node: 0,
                            times: vec![0.0, 1.0],
                            keys: Keys::Rotation(vec![Quat::IDENTITY, Quat::from_rotation_y(1.0)]),
                            step: false,
                        }],
                    },
                )]),
            }),
        }
    }

    #[test]
    fn a_model_survives_a_round_trip_and_keeps_sharing_its_data() {
        let read = read_model(&write_model(&model())).expect("valid");
        assert_eq!(read.parts.len(), 2);
        assert_eq!(read.parts[0].mesh.key(), read.parts[1].mesh.key());
        assert_eq!(read.parts[0].lods[0].distance, 10.0);
        assert_eq!(read.parts[0].lods[0].mesh.data().indices.len(), 6);
        assert_eq!(read.parts[0].mesh.data().indices.len(), 36);
        assert_eq!(read.parts[1].transform, Mat4::from_translation(Vec3::X));
        assert_eq!(
            read.parts[0].material.base_color_texture,
            read.parts[1].material.base_color_texture
        );
        let material = &read.parts[0].material;
        assert_eq!(material.emissive, Vec3::new(0.5, 0.25, 0.0));
        assert_eq!(material.opacity, 0.75);
        assert_eq!(material.alpha_mode, AlphaMode::Mask { cutoff: 0.3 });
        assert!(material.double_sided);
        assert_eq!(material.emissive_texture, material.base_color_texture);
        let clip = &read.rig.as_ref().unwrap().clips["wave"];
        assert_eq!(clip.channels[0].times, vec![0.0, 1.0]);
    }

    #[test]
    fn sections_are_found_by_tag_and_others_are_skipped() {
        let pack = write_pack(&[(b"XTRA", vec![1, 2]), (MODEL_SECTION, vec![3])]);
        assert_eq!(read_section(&pack, MODEL_SECTION).unwrap(), &[3]);
        assert!(read_section(&pack, PROBE_SECTION).is_err());
    }

    #[test]
    fn every_truncation_of_a_model_is_an_error_rather_than_a_panic() {
        let bytes = write_model(&model());
        for length in 0..bytes.len() {
            assert!(read_model(&bytes[..length]).is_err(), "length {length}");
        }
    }

    #[test]
    fn out_of_range_references_are_errors() {
        let mut broken = model();
        broken.parts[0].node = Some(9);
        assert!(read_model(&write_model(&broken)).is_err());
    }
}
