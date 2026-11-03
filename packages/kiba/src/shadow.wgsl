@group(0) @binding(0) var<uniform> light: mat4x4<f32>;

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(4) column0: vec4<f32>,
    @location(5) column1: vec4<f32>,
    @location(6) column2: vec4<f32>,
    @location(7) column3: vec4<f32>,
) -> @builtin(position) vec4<f32> {
    return light * mat4x4<f32>(column0, column1, column2, column3) * vec4<f32>(position, 1.0);
}
