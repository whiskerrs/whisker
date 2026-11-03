struct Joints {
    matrices: array<mat4x4<f32>, 128>,
}

@group(0) @binding(0) var<uniform> light: mat4x4<f32>;
@group(1) @binding(0) var<uniform> joints: Joints;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(4) column0: vec4<f32>,
    @location(5) column1: vec4<f32>,
    @location(6) column2: vec4<f32>,
    @location(7) column3: vec4<f32>,
}

@vertex
fn vs_main(input: VertexInput) -> @builtin(position) vec4<f32> {
    let transform = mat4x4<f32>(input.column0, input.column1, input.column2, input.column3);
    return light * transform * vec4<f32>(input.position, 1.0);
}

@vertex
fn vs_skinned(
    input: VertexInput,
    @location(11) joint_indices: vec4<u32>,
    @location(12) weights: vec4<f32>,
) -> @builtin(position) vec4<f32> {
    let transform = mat4x4<f32>(input.column0, input.column1, input.column2, input.column3);
    let skin = joints.matrices[joint_indices.x] * weights.x
        + joints.matrices[joint_indices.y] * weights.y
        + joints.matrices[joint_indices.z] * weights.z
        + joints.matrices[joint_indices.w] * weights.w;
    return light * transform * skin * vec4<f32>(input.position, 1.0);
}
