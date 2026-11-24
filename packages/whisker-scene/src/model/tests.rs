use glam::{Quat, Vec3};
use serde_json::{Value, json};
use std::time::Duration;

use whisker::css::Color;
use whisker_surface::FrameInfo;

use super::*;
use crate::core::{Material, Node, Scene};

/// Builds a `.glb` from a JSON document and the accessors its test needs.
#[derive(Default)]
pub(crate) struct Glb {
    bin: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Glb {
    /// Appends `values` as an accessor of `count` elements of `kind` (`"VEC3"`, `"SCALAR"`, …).
    pub(crate) fn floats(&mut self, values: &[f32], kind: &str) -> usize {
        let width = match kind {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            "MAT4" => 16,
            _ => unreachable!(),
        };
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut accessor =
            json!({ "componentType": 5126, "count": values.len() / width, "type": kind });
        if kind == "VEC3" {
            let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
            for v in values.chunks(3) {
                for i in 0..3 {
                    min[i] = min[i].min(v[i]);
                    max[i] = max[i].max(v[i]);
                }
            }
            accessor["min"] = json!(min);
            accessor["max"] = json!(max);
        }
        if kind == "SCALAR" {
            let min = values.iter().copied().fold(f32::MAX, f32::min);
            let max = values.iter().copied().fold(f32::MIN, f32::max);
            accessor["min"] = json!([min]);
            accessor["max"] = json!([max]);
        }
        self.push(bytes, accessor)
    }

    pub(crate) fn indices(&mut self, values: &[u16]) -> usize {
        let bytes = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.push(
            bytes,
            json!({ "componentType": 5123, "count": values.len(), "type": "SCALAR" }),
        )
    }

    pub(crate) fn joints(&mut self, values: &[[u16; 4]]) -> usize {
        let bytes = values
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        self.push(
            bytes,
            json!({ "componentType": 5123, "count": values.len(), "type": "VEC4" }),
        )
    }

    fn push(&mut self, mut bytes: Vec<u8>, mut accessor: Value) -> usize {
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
        self.views
            .push(json!({ "buffer": 0, "byteOffset": self.bin.len(), "byteLength": bytes.len() }));
        self.bin.extend(bytes);
        accessor["bufferView"] = json!(self.views.len() - 1);
        self.accessors.push(accessor);
        self.accessors.len() - 1
    }

    /// The file, with `document` given everything but `asset`, buffers, views and accessors.
    pub(crate) fn finish(self, mut document: Value) -> Vec<u8> {
        document["asset"] = json!({ "version": "2.0" });
        document["buffers"] = json!([{ "byteLength": self.bin.len() }]);
        document["bufferViews"] = Value::Array(self.views);
        document["accessors"] = Value::Array(self.accessors);
        let mut text = serde_json::to_vec(&document).unwrap();
        while !text.len().is_multiple_of(4) {
            text.push(b' ');
        }
        let total = 12 + 8 + text.len() + 8 + self.bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend(b"glTF");
        out.extend(2u32.to_le_bytes());
        out.extend((total as u32).to_le_bytes());
        out.extend((text.len() as u32).to_le_bytes());
        out.extend(b"JSON");
        out.extend(text);
        out.extend((self.bin.len() as u32).to_le_bytes());
        out.extend(b"BIN\0");
        out.extend(self.bin);
        out
    }
}

/// One triangle in the XY plane, facing +Z.
pub(crate) fn triangle(glb: &mut Glb) -> Value {
    let position = glb.floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], "VEC3");
    let normal = glb.floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0], "VEC3");
    let indices = glb.indices(&[0, 1, 2]);
    json!({ "attributes": { "POSITION": position, "NORMAL": normal }, "indices": indices })
}

#[test]
fn instantiates_named_nodes_with_their_transforms_meshes_and_materials() {
    let mut glb = Glb::default();
    let mut primitive = triangle(&mut glb);
    primitive["material"] = json!(0);
    let bytes = glb.finish(json!({
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [
            { "name": "Body", "translation": [1.0, 2.0, 3.0], "children": [1] },
            { "name": "Wheel", "mesh": 0, "scale": [2.0, 2.0, 2.0] },
        ],
        "meshes": [{ "primitives": [primitive] }],
        "materials": [{
            "pbrMetallicRoughness": { "baseColorFactor": [1.0, 0.0, 0.0, 1.0], "metallicFactor": 0.25 },
            "alphaMode": "MASK",
            "alphaCutoff": 0.3,
            "doubleSided": true,
        }],
    }));
    let model = Model::from_slice(&bytes).unwrap();
    let root = model.instantiate();
    let body = root.find("Body").unwrap();
    assert_eq!(body.transform().translation, Vec3::new(1.0, 2.0, 3.0));
    let wheel = root.find("Wheel").unwrap();
    assert_eq!(wheel.parent(), Some(body));
    assert_eq!(wheel.transform().scale, Vec3::splat(2.0));
    let (_, material) = wheel.mesh().unwrap();
    let Material::Standard(material) = material else {
        panic!("{material:?}");
    };
    assert_eq!(material.base_color, Color::rgb(255, 0, 0));
    assert_eq!(material.metallic, 0.25);
    assert_eq!(material.alpha_mode, crate::core::AlphaMode::Mask(0.3));
    assert!(material.double_sided);
    let second = model.instantiate();
    assert_ne!(second.find("Wheel"), Some(wheel.clone()));
    assert_eq!(
        second.find("Wheel").unwrap().mesh().unwrap().0,
        wheel.mesh().unwrap().0
    );
}

#[test]
fn rejects_external_files_and_unknown_required_extensions() {
    let external = json!({
        "asset": { "version": "2.0" },
        "buffers": [{ "byteLength": 4, "uri": "mesh.bin" }],
    });
    let error = Model::from_slice(&serde_json::to_vec(&external).unwrap()).unwrap_err();
    assert!(error.to_string().contains("mesh.bin"), "{error}");
    let draco = json!({
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_draco_mesh_compression"],
        "extensionsRequired": ["KHR_draco_mesh_compression"],
    });
    let error = Model::from_slice(&serde_json::to_vec(&draco).unwrap()).unwrap_err();
    assert!(error.to_string().contains("KHR_draco"), "{error}");
}

#[test]
fn reads_buffers_embedded_as_data_uris() {
    use base64::Engine;
    let positions: Vec<u8> = [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let uri = format!(
        "data:application/octet-stream;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&positions)
    );
    let document = json!({
        "asset": { "version": "2.0" },
        "buffers": [{ "byteLength": positions.len(), "uri": uri }],
        "bufferViews": [{ "buffer": 0, "byteLength": positions.len() }],
        "accessors": [{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                        "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] }],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 } }] }],
        "nodes": [{ "mesh": 0 }],
    });
    let model = Model::from_slice(&serde_json::to_vec(&document).unwrap()).unwrap();
    let root = model.instantiate();
    let (geometry, _) = root.children()[0].mesh().unwrap();
    // Without normals each triangle is lit by its own face normal.
    assert_eq!(geometry.data().vertices[0].normal, [0.0, 0.0, 1.0]);
}

/// Two joints along +Y with the second rotating a quarter turn around Z over one second, and a
/// vertex bound to each.
fn skinned() -> Model {
    let mut glb = Glb::default();
    let position = glb.floats(&[0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.1, 2.0, 0.0], "VEC3");
    let normal = glb.floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0], "VEC3");
    let joints = glb.joints(&[[0, 0, 0, 0], [1, 0, 0, 0], [1, 0, 0, 0]]);
    let weights = glb.floats(
        &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
        "VEC4",
    );
    let indices = glb.indices(&[0, 1, 2]);
    let inverse_bind = glb.floats(
        &[
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, //
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 1.0,
        ],
        "MAT4",
    );
    let times = glb.floats(&[0.0, 1.0], "SCALAR");
    let turn = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    let rotations = glb.floats(
        &[0.0, 0.0, 0.0, 1.0, turn.x, turn.y, turn.z, turn.w],
        "VEC4",
    );
    let bytes = glb.finish(json!({
        "scenes": [{ "nodes": [0, 1] }],
        "nodes": [
            { "name": "Skin", "mesh": 0, "skin": 0 },
            { "name": "Hip", "children": [2] },
            { "name": "Knee", "translation": [0.0, 1.0, 0.0] },
        ],
        "meshes": [{ "primitives": [{
            "attributes": { "POSITION": position, "NORMAL": normal, "JOINTS_0": joints, "WEIGHTS_0": weights },
            "indices": indices,
        }] }],
        "skins": [{ "joints": [1, 2], "inverseBindMatrices": inverse_bind }],
        "animations": [{
            "name": "Bend",
            "samplers": [{ "input": times, "output": rotations }],
            "channels": [{ "sampler": 0, "target": { "node": 2, "path": "rotation" } }],
        }],
    }));
    Model::from_slice(&bytes).unwrap()
}

fn deformed_positions(node: &Node) -> Vec<Vec3> {
    let deform = node.deform().unwrap();
    deform.update(&node.mesh().unwrap().0);
    deform.with_vertices(|vertices| vertices.iter().map(|v| Vec3::from(v.position)).collect())
}

#[test]
fn skinned_vertices_follow_their_joints_as_a_clip_plays() {
    let model = skinned();
    assert_eq!(model.animations(), ["Bend"]);
    let scene = Scene::new();
    let root = model.instantiate();
    scene.root().add_child(&root);
    let skin = root.find("Skin").unwrap();
    let rest = deformed_positions(&skin);
    assert!(
        rest[1].abs_diff_eq(Vec3::new(0.0, 2.0, 0.0), 1e-5),
        "{rest:?}"
    );

    let player = AnimationPlayer::new(&root).unwrap();
    assert!(player.play(
        "Bend",
        AnimationOptions {
            fade: 0.0,
            looping: false,
            speed: 1.0
        }
    ));
    player.update(1.0);
    let bent = deformed_positions(&skin);
    assert!(bent[0].abs_diff_eq(Vec3::ZERO, 1e-5), "{bent:?}");
    // The knee at y = 1 turned a quarter around Z, swinging the vertex above it to -X.
    assert!(
        bent[1].abs_diff_eq(Vec3::new(-1.0, 1.0, 0.0), 1e-5),
        "{bent:?}"
    );

    player.stop(0.0);
    player.update(0.1);
    assert!(!player.is_playing());
    assert_eq!(
        root.find("Knee").unwrap().transform().rotation,
        Quat::IDENTITY
    );
}

#[test]
fn cross_fading_blends_toward_the_new_clip() {
    let model = skinned();
    let root = model.instantiate();
    let player = AnimationPlayer::new(&root).unwrap();
    let options = AnimationOptions {
        fade: 1.0,
        looping: false,
        speed: 0.0,
    };
    player.play("Bend", options);
    player.seek("Bend", 1.0);
    player.update(0.5);
    let knee = root.find("Knee").unwrap();
    let (axis, angle) = knee.transform().rotation.to_axis_angle();
    // Half faded in from the rest pose: half of the quarter turn.
    assert!(
        (angle - std::f32::consts::FRAC_PI_4).abs() < 1e-4,
        "{axis} {angle}"
    );
    assert!(!player.play("Missing", options));
}

#[test]
fn morph_weights_animate_the_targets() {
    let mut glb = Glb::default();
    let mut primitive = triangle(&mut glb);
    let offsets = glb.floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0], "VEC3");
    primitive["targets"] = json!([{ "POSITION": offsets }]);
    let times = glb.floats(&[0.0, 1.0], "SCALAR");
    let weights = glb.floats(&[0.0, 1.0], "SCALAR");
    let bytes = glb.finish(json!({
        "nodes": [{ "name": "Face", "mesh": 0 }],
        "meshes": [{ "primitives": [primitive], "weights": [0.0] }],
        "animations": [{
            "samplers": [{ "input": times, "output": weights, "interpolation": "STEP" }],
            "channels": [{ "sampler": 0, "target": { "node": 0, "path": "weights" } }],
        }],
    }));
    let model = Model::from_slice(&bytes).unwrap();
    assert_eq!(model.animations(), ["0"]);
    let root = model.instantiate();
    let face = root.find("Face").unwrap();
    let player = AnimationPlayer::new(&root).unwrap();
    player.play(
        "0",
        AnimationOptions {
            fade: 0.0,
            looping: false,
            speed: 1.0,
        },
    );
    player.update(0.5);
    assert_eq!(deformed_positions(&face)[0].z, 0.0);
    player.update(0.6);
    assert_eq!(deformed_positions(&face)[0].z, 1.0);
}

#[test]
fn players_only_attach_to_model_roots() {
    assert!(AnimationPlayer::new(&Node::new()).is_none());
}

#[test]
fn instances_are_freed_even_when_a_mesh_sits_under_its_own_joints() {
    let root = skinned().instantiate();
    let hip = root.find("Hip").unwrap();
    hip.add_child(&root.find("Skin").unwrap());
    let knee = root.find("Knee").unwrap().downgrade();
    drop(hip);
    drop(root);
    assert!(knee.upgrade().is_none());
}

#[test]
fn dropping_the_driver_stops_a_looping_clip() {
    let scene = Scene::new();
    let root = skinned().instantiate();
    scene.root().add_child(&root);
    let player = AnimationPlayer::new(&root).unwrap();
    player.play(
        "Bend",
        AnimationOptions {
            fade: 0.0,
            ..Default::default()
        },
    );
    let driver = crate::components::AnimationDriver::default();
    driver.run(&scene, player);
    let knee = root.find("Knee").unwrap();
    let frame = |ms| FrameInfo {
        timestamp: Duration::from_millis(ms),
    };
    scene.inner().run_frame_callbacks(&frame(0));
    scene.inner().run_frame_callbacks(&frame(100));
    let moved = knee.transform().rotation;
    assert_ne!(moved, Quat::IDENTITY);
    drop(driver);
    scene.inner().run_frame_callbacks(&frame(200));
    assert_eq!(knee.transform().rotation, moved);
}
