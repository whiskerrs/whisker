override ENCODE_SRGB: bool = false;

const PI: f32 = 3.14159265;
const DIELECTRIC_REFLECTANCE: f32 = 0.04;
// Below this, the highlight of a directional light shrinks under one pixel and flickers.
const MIN_ROUGHNESS: f32 = 0.045;

struct Frame {
    view_projection: mat4x4<f32>,
    camera_position: vec4<f32>,
    to_sun: vec4<f32>,
    sun_color: vec4<f32>,
    ambient: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) base_color: vec3<f32>,
    @location(3) metallic: f32,
    @location(4) roughness: f32,
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
    @location(7) base_color: vec3<f32>,
    @location(8) metallic: f32,
    @location(9) roughness: f32,
) -> VertexOutput {
    let transform = mat4x4<f32>(column0, column1, column2, column3);
    let basis = mat3x3<f32>(column0.xyz, column1.xyz, column2.xyz);
    let squared_scale = vec3<f32>(
        dot(column0.xyz, column0.xyz),
        dot(column1.xyz, column1.xyz),
        dot(column2.xyz, column2.xyz),
    );
    var output: VertexOutput;
    let world_position = transform * vec4<f32>(position, 1.0);
    output.position = frame.view_projection * world_position;
    output.world_position = world_position.xyz;
    // Equals the inverse transpose for a matrix without shear, which every Transform is, and WGSL has no inverse.
    output.normal = basis * (normal / squared_scale);
    output.base_color = color * base_color;
    output.metallic = metallic;
    output.roughness = roughness;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let normal = normalize(input.normal);
    let to_camera = normalize(frame.camera_position.xyz - input.world_position);
    let to_sun = frame.to_sun.xyz;
    let halfway = normalize(to_camera + to_sun);
    let n_dot_l = max(dot(normal, to_sun), 0.0);
    let n_dot_v = max(dot(normal, to_camera), 1e-4);
    let n_dot_h = max(dot(normal, halfway), 0.0);
    let v_dot_h = max(dot(to_camera, halfway), 0.0);

    let roughness = clamp(input.roughness, MIN_ROUGHNESS, 1.0);
    let alpha = roughness * roughness;
    let reflectance = mix(vec3<f32>(DIELECTRIC_REFLECTANCE), input.base_color, input.metallic);
    let diffuse = input.base_color * (1.0 - input.metallic);

    let specular = ggx(n_dot_h, alpha) * visibility(n_dot_l, n_dot_v, alpha) * fresnel(reflectance, v_dot_h);
    // Sun.color is the light leaving a white diffuse surface, so diffuse carries no 1/PI and specular gains PI.
    let sun = (diffuse + specular * PI) * frame.sun_color.rgb * n_dot_l;
    let ambient = (diffuse + ambient_specular(reflectance, roughness, n_dot_v)) * frame.ambient.rgb;
    let lit = sun + ambient;
    if ENCODE_SRGB {
        return vec4<f32>(linear_to_srgb(lit), 1.0);
    }
    return vec4<f32>(lit, 1.0);
}

fn ggx(n_dot_h: f32, alpha: f32) -> f32 {
    let alpha_squared = alpha * alpha;
    let denominator = n_dot_h * n_dot_h * (alpha_squared - 1.0) + 1.0;
    return alpha_squared / (PI * denominator * denominator);
}

fn visibility(n_dot_l: f32, n_dot_v: f32, alpha: f32) -> f32 {
    return 0.5 / max(mix(2.0 * n_dot_l * n_dot_v, n_dot_l + n_dot_v, alpha), 1e-4);
}

fn fresnel(reflectance: vec3<f32>, v_dot_h: f32) -> vec3<f32> {
    return reflectance + (vec3<f32>(1.0) - reflectance) * pow(1.0 - v_dot_h, 5.0);
}

// Karis's analytic fit of the split-sum BRDF, which keeps metals from going black under ambient light.
fn ambient_specular(reflectance: vec3<f32>, roughness: f32, n_dot_v: f32) -> vec3<f32> {
    let r = roughness * vec4<f32>(-1.0, -0.0275, -0.572, 0.022) + vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let a004 = min(r.x * r.x, pow(2.0, -9.28 * n_dot_v)) * r.x + r.y;
    let ab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return reflectance * ab.x + vec3<f32>(ab.y);
}

fn linear_to_srgb(linear: vec3<f32>) -> vec3<f32> {
    let low = linear * 12.92;
    let high = 1.055 * pow(linear, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, linear <= vec3<f32>(0.0031308));
}
