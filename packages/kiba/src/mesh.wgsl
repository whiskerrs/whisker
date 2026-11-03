const PI: f32 = 3.14159265;
const DIELECTRIC_REFLECTANCE: f32 = 0.04;
// Below this, the highlight of a directional light shrinks under one pixel and flickers.
const MIN_ROUGHNESS: f32 = 0.045;
const SKY_LAST_MIP: f32 = 4.0;

struct Frame {
    view_projection: mat4x4<f32>,
    camera_position: vec4<f32>,
    to_sun: vec4<f32>,
    sun_color: vec4<f32>,
    ambient: vec4<f32>,
    cascades: array<mat4x4<f32>, 2>,
    // The minimum corner of the probe box, with W as the inverse of the probe spacing.
    probe_min: vec4<f32>,
    // The inverse size of the probe box, with W at zero when there is none.
    probe_scale: vec4<f32>,
    // Split distance, then one when shadows are on, then one when a sky lights the scene.
    shadow_params: vec4<f32>,
    // The color of the air, with W as its density.
    fog: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var shadow_map: texture_depth_2d_array;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;
@group(0) @binding(3) var sky_radiance: texture_cube<f32>;
@group(0) @binding(4) var sky_irradiance: texture_cube<f32>;
@group(0) @binding(5) var sky_sampler: sampler;
@group(1) @binding(0) var base_color_texture: texture_2d<f32>;
@group(1) @binding(1) var metallic_roughness_texture: texture_2d<f32>;
@group(1) @binding(2) var normal_texture: texture_2d<f32>;
@group(1) @binding(3) var material_sampler: sampler;

struct Joints {
    matrices: array<mat4x4<f32>, 128>,
}

@group(2) @binding(0) var<uniform> joints: Joints;
@group(3) @binding(0) var probes: texture_3d<f32>;
@group(3) @binding(1) var probe_bounce: texture_3d<f32>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) base_color: vec3<f32>,
    @location(3) metallic: f32,
    @location(4) roughness: f32,
    @location(5) uv: vec2<f32>,
}

struct InstanceInput {
    @location(4) column0: vec4<f32>,
    @location(5) column1: vec4<f32>,
    @location(6) column2: vec4<f32>,
    @location(7) column3: vec4<f32>,
    @location(8) base_color: vec3<f32>,
    @location(9) metallic: f32,
    @location(10) roughness: f32,
}

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) uv: vec2<f32>,
}

fn place_vertex(vertex: VertexInput, instance: InstanceInput, transform: mat4x4<f32>) -> VertexOutput {
    let basis = mat3x3<f32>(transform[0].xyz, transform[1].xyz, transform[2].xyz);
    let squared_scale = vec3<f32>(dot(basis[0], basis[0]), dot(basis[1], basis[1]), dot(basis[2], basis[2]));
    var output: VertexOutput;
    let world_position = transform * vec4<f32>(vertex.position, 1.0);
    output.position = frame.view_projection * world_position;
    output.world_position = world_position.xyz;
    // Equals the inverse transpose for a matrix without shear, which every Transform is, and WGSL has no inverse.
    output.normal = basis * (vertex.normal / squared_scale);
    output.base_color = vertex.color * instance.base_color;
    output.metallic = instance.metallic;
    output.roughness = instance.roughness;
    output.uv = vertex.uv;
    return output;
}

@vertex
fn vs_main(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    let transform = mat4x4<f32>(instance.column0, instance.column1, instance.column2, instance.column3);
    return place_vertex(vertex, instance, transform);
}

@vertex
fn vs_skinned(
    vertex: VertexInput,
    instance: InstanceInput,
    @location(11) joint_indices: vec4<u32>,
    @location(12) weights: vec4<f32>,
) -> VertexOutput {
    let transform = mat4x4<f32>(instance.column0, instance.column1, instance.column2, instance.column3);
    let skin = joints.matrices[joint_indices.x] * weights.x
        + joints.matrices[joint_indices.y] * weights.y
        + joints.matrices[joint_indices.z] * weights.z
        + joints.matrices[joint_indices.w] * weights.w;
    return place_vertex(vertex, instance, transform * skin);
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
    let lit = sun_visibility(input.world_position, input.normal);
    let sun = (diffuse + specular * PI) * frame.sun_color.rgb * n_dot_l * lit;
    var ambient_diffuse = frame.ambient.rgb;
    var ambient_reflected = frame.ambient.rgb;
    if frame.shadow_params.z != 0.0 {
        ambient_diffuse = textureSampleLevel(sky_irradiance, sky_sampler, normal, 0.0).rgb;
        let mirrored = reflect(-to_camera, normal);
        ambient_reflected = textureSampleLevel(sky_radiance, sky_sampler, mirrored, roughness * SKY_LAST_MIP).rgb;
    }
    let ambient = diffuse * (ambient_diffuse * sky_visibility(input.world_position, normal) + bounce_light(input.world_position, normal))
        + ambient_specular(reflectance, roughness, n_dot_v) * ambient_reflected * sky_visibility(input.world_position, reflect(-to_camera, normal));
    let air = 1.0 - exp(-length(frame.camera_position.xyz - input.world_position) * frame.fog.w);
    return vec4<f32>(mix(sun + ambient, frame.fog.rgb, air) * frame.camera_position.w, 1.0);
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

// A rotated grid hides the stair steps of the map better than an aligned one.
const SHADOW_TAPS = array<vec2<f32>, 4>(
    vec2<f32>(-0.75, -0.25),
    vec2<f32>(0.25, -0.75),
    vec2<f32>(0.75, 0.25),
    vec2<f32>(-0.25, 0.75),
);

fn sky_visibility(position: vec3<f32>, direction: vec3<f32>) -> f32 {
    if frame.probe_scale.w == 0.0 {
        return 1.0;
    }
    let uvw = (position - frame.probe_min.xyz) * frame.probe_scale.xyz;
    if any(uvw < vec3<f32>(0.0)) || any(uvw > vec3<f32>(1.0)) {
        return 1.0;
    }
    let probe = textureSampleLevel(probes, sky_sampler, uvw, 0.0);
    let visible = saturate(probe.a + dot(probe.rgb * 2.0 - 1.0, direction));
    // Fading out over the outermost probes keeps the edge of the box from showing on surfaces.
    let inward = min(uvw, 1.0 - uvw) / frame.probe_scale.xyz;
    return mix(1.0, visible, saturate(min(inward.x, min(inward.y, inward.z)) * frame.probe_min.w));
}

// Lights what blocks the sky from above, by the sun and sky, and returns it in that surface's color,
// so the sun's height can change without a second cook.
fn bounce_light(position: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    if frame.probe_scale.w == 0.0 {
        return vec3<f32>(0.0);
    }
    let uvw = (position - frame.probe_min.xyz) * frame.probe_scale.xyz;
    if any(uvw < vec3<f32>(0.0)) || any(uvw > vec3<f32>(1.0)) {
        return vec3<f32>(0.0);
    }
    let bounce = textureSampleLevel(probe_bounce, sky_sampler, uvw, 0.0);
    var from_above = frame.ambient.rgb;
    if frame.shadow_params.z != 0.0 {
        from_above = textureSampleLevel(sky_irradiance, sky_sampler, vec3<f32>(0.0, 1.0, 0.0), 0.0).rgb;
    }
    let arriving = (frame.sun_color.rgb * max(frame.to_sun.y, 0.0) + from_above) * bounce.a;
    return bounce.rgb * bounce.rgb * arriving * (1.0 - sky_visibility(position, normal));
}

fn sun_visibility(position: vec3<f32>, normal: vec3<f32>) -> f32 {
    if frame.shadow_params.y == 0.0 {
        return 1.0;
    }
    let distance = length(position - frame.camera_position.xyz);
    let cascade = select(1i, 0i, distance < frame.shadow_params.x);
    // Pushing the lookup off the surface removes self-shadowing without detaching contact shadows.
    let offset = normalize(normal) * 0.02 * (1.0 + f32(cascade) * 3.0);
    let clip = frame.cascades[cascade] * vec4<f32>(position + offset, 1.0);
    let uv = clip.xy * vec2<f32>(0.5, -0.5) + 0.5;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) || clip.z > 1.0 {
        return 1.0;
    }
    let texel = 1.0 / vec2<f32>(textureDimensions(shadow_map));
    var lit = 0.0;
    for (var i = 0; i < 4; i++) {
        lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + SHADOW_TAPS[i] * texel, cascade, clip.z);
    }
    return lit / 4.0;
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
