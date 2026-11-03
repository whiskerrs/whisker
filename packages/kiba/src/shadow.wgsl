struct Joints {
    matrices: array<mat4x4<f32>, 128>,
}

@group(0) @binding(0) var<uniform> light: mat4x4<f32>;
@group(1) @binding(0) var<uniform> joints: Joints;
@group(2) @binding(0) var base_color_texture: texture_2d<f32>;
@group(2) @binding(3) var material_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(4) column0: vec4<f32>,
    @location(5) column1: vec4<f32>,
    @location(6) column2: vec4<f32>,
    @location(7) column3: vec4<f32>,
}

struct SkinInput {
    @location(11) joint_indices: vec4<u32>,
    @location(12) weights: vec4<f32>,
}

struct MaskInput {
    @location(3) uv: vec2<f32>,
    // Opacity, then the cutoff below which the surface is absent.
    @location(14) opacity: vec3<f32>,
}

struct MaskOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) opacity: vec2<f32>,
}

fn transform_of(input: VertexInput) -> mat4x4<f32> {
    return mat4x4<f32>(input.column0, input.column1, input.column2, input.column3);
}

fn skin_of(skin: SkinInput) -> mat4x4<f32> {
    return joints.matrices[skin.joint_indices.x] * skin.weights.x
        + joints.matrices[skin.joint_indices.y] * skin.weights.y
        + joints.matrices[skin.joint_indices.z] * skin.weights.z
        + joints.matrices[skin.joint_indices.w] * skin.weights.w;
}

@vertex
fn vs_main(input: VertexInput) -> @builtin(position) vec4<f32> {
    return light * transform_of(input) * vec4<f32>(input.position, 1.0);
}

@vertex
fn vs_skinned(input: VertexInput, skin: SkinInput) -> @builtin(position) vec4<f32> {
    return light * transform_of(input) * skin_of(skin) * vec4<f32>(input.position, 1.0);
}

@vertex
fn vs_masked(input: VertexInput, mask: MaskInput) -> MaskOutput {
    return MaskOutput(light * transform_of(input) * vec4<f32>(input.position, 1.0), mask.uv, mask.opacity.xy);
}

@vertex
fn vs_masked_skinned(input: VertexInput, skin: SkinInput, mask: MaskInput) -> MaskOutput {
    let position = light * transform_of(input) * skin_of(skin) * vec4<f32>(input.position, 1.0);
    return MaskOutput(position, mask.uv, mask.opacity.xy);
}

@fragment
fn fs_masked(input: MaskOutput) {
    let alpha = textureSample(base_color_texture, material_sampler, input.uv).a * input.opacity.x;
    if alpha < input.opacity.y {
        discard;
    }
}
