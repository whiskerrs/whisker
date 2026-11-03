override ENCODE_SRGB: bool = false;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec3<f32>,
}

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) column0: vec4<f32>,
    @location(3) column1: vec4<f32>,
    @location(4) column2: vec4<f32>,
    @location(5) column3: vec4<f32>,
) -> VertexOutput {
    let transform = mat4x4<f32>(column0, column1, column2, column3);
    var output: VertexOutput;
    output.position = transform * vec4<f32>(position, 1.0);
    output.color = color;
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
