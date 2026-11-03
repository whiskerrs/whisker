override ENCODE_SRGB: bool = false;

struct Frame {
    view_projection: mat4x4<f32>,
    to_sun: vec4<f32>,
    sun_color: vec4<f32>,
    ambient: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
}

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) column0: vec4<f32>,
    @location(4) column1: vec4<f32>,
    @location(5) column2: vec4<f32>,
    @location(6) column3: vec4<f32>,
) -> VertexOutput {
    let transform = mat4x4<f32>(column0, column1, column2, column3);
    let basis = mat3x3<f32>(column0.xyz, column1.xyz, column2.xyz);
    let squared_scale = vec3<f32>(
        dot(column0.xyz, column0.xyz),
        dot(column1.xyz, column1.xyz),
        dot(column2.xyz, column2.xyz),
    );
    var output: VertexOutput;
    output.position = frame.view_projection * transform * vec4<f32>(position, 1.0);
    // Equals the inverse transpose for a matrix without shear, which every Transform is, and WGSL has no inverse.
    output.normal = basis * (normal / squared_scale);
    output.color = color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let facing = max(dot(normalize(input.normal), frame.to_sun.xyz), 0.0);
    let lit = input.color * (frame.ambient.rgb + frame.sun_color.rgb * facing);
    if ENCODE_SRGB {
        return vec4<f32>(linear_to_srgb(lit), 1.0);
    }
    return vec4<f32>(lit, 1.0);
}

fn linear_to_srgb(linear: vec3<f32>) -> vec3<f32> {
    let low = linear * 12.92;
    let high = 1.055 * pow(linear, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, linear <= vec3<f32>(0.0031308));
}
