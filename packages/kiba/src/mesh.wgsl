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
@group(1) @binding(0) var base_color_texture: texture_2d<f32>;
@group(1) @binding(1) var metallic_roughness_texture: texture_2d<f32>;
@group(1) @binding(2) var normal_texture: texture_2d<f32>;
@group(1) @binding(3) var material_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) base_color: vec3<f32>,
    @location(3) metallic: f32,
    @location(4) roughness: f32,
    @location(5) uv: vec2<f32>,
}

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) column0: vec4<f32>,
    @location(5) column1: vec4<f32>,
    @location(6) column2: vec4<f32>,
    @location(7) column3: vec4<f32>,
    @location(8) base_color: vec3<f32>,
    @location(9) metallic: f32,
    @location(10) roughness: f32,
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
    output.uv = uv;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let normal = mapped_normal(normalize(input.normal), input.world_position, input.uv);
    let metal_rough = textureSample(metallic_roughness_texture, material_sampler, input.uv);
    let metallic = input.metallic * metal_rough.b;
    let base_color = input.base_color * textureSample(base_color_texture, material_sampler, input.uv).rgb;
    let to_camera = normalize(frame.camera_position.xyz - input.world_position);
    let to_sun = frame.to_sun.xyz;
    let halfway = normalize(to_camera + to_sun);
    let n_dot_l = max(dot(normal, to_sun), 0.0);
    let n_dot_v = max(dot(normal, to_camera), 1e-4);
    let n_dot_h = max(dot(normal, halfway), 0.0);
    let v_dot_h = max(dot(to_camera, halfway), 0.0);

    let roughness = clamp(input.roughness * metal_rough.g, MIN_ROUGHNESS, 1.0);
    let alpha = roughness * roughness;
    let reflectance = mix(vec3<f32>(DIELECTRIC_REFLECTANCE), base_color, metallic);
    let diffuse = base_color * (1.0 - metallic);

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

// Derives the tangent frame from screen derivatives, so meshes need no tangent attribute.
fn mapped_normal(normal: vec3<f32>, position: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let dp_x = dpdx(position);
    let dp_y = dpdy(position);
    let duv_x = dpdx(uv);
    let duv_y = dpdy(uv);
    let across_y = cross(dp_y, normal);
    let across_x = cross(normal, dp_x);
    // Framebuffer Y points down, which negates this frame relative to the usual y-up derivation.
    let along_u = -(across_y * duv_x.x + across_x * duv_y.x);
    let along_v = -(across_y * duv_x.y + across_x * duv_y.y);
    // The floor keeps a mesh without texture coordinates from dividing by zero.
    let scale = inverseSqrt(max(max(dot(along_u, along_u), dot(along_v, along_v)), 1e-20));
    let mapped = textureSample(normal_texture, material_sampler, uv).xyz * 2.0 - 1.0;
    return normalize(mapped.x * along_u * scale - mapped.y * along_v * scale + mapped.z * normal);
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
