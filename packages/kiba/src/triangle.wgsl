override ENCODE_SRGB: bool = false;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec3<f32>,
}

@vertex
fn vs_main(
    @builtin(vertex_index) index: u32,
    @location(0) column0: vec4<f32>,
    @location(1) column1: vec4<f32>,
    @location(2) column2: vec4<f32>,
    @location(3) column3: vec4<f32>,
) -> VertexOutput {
    let transform = mat4x4<f32>(column0, column1, column2, column3);
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.6),
        vec2<f32>(-0.6, -0.6),
        vec2<f32>(0.6, -0.6),
    );
    var colors = array<vec3<f32>, 3>(
        vec3<f32>(1.0, 0.25, 0.25),
        vec3<f32>(0.25, 1.0, 0.25),
        vec3<f32>(0.25, 0.25, 1.0),
    );
    var output: VertexOutput;
    output.position = transform * vec4<f32>(positions[index], 0.0, 1.0);
    output.color = colors[index];
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    if ENCODE_SRGB {
        return vec4<f32>(linear_to_srgb(input.color), 1.0);
    }
    return vec4<f32>(input.color, 1.0);
}

fn linear_to_srgb(linear: vec3<f32>) -> vec3<f32> {
    let low = linear * 12.92;
    let high = 1.055 * pow(linear, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, linear <= vec3<f32>(0.0031308));
}
