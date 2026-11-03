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
    cascades: array<mat4x4<f32>, 4>,
    // Where each cascade ends.
    shadow_ends: vec4<f32>,
    // Each cascade's texel width and depth range, in world units.
    shadow_scales: array<vec4<f32>, 4>,
    // One for soft shadows, then the tangent of the sun's angular radius.
    shadow_shape: vec4<f32>,
    // The minimum corner of the probe box, with W as the inverse of the probe spacing.
    probe_min: vec4<f32>,
    // The inverse size of the probe box, with W at zero when there is none.
    probe_scale: vec4<f32>,
    // The cascade count, then one when shadows are on, then one when a sky lights the scene, then the light count.
    shadow_params: vec4<f32>,
    // The color of the air, with W as its density.
    fog: vec4<f32>,
    lights: array<Light, 16>,
    // The reflection probe's box, with W at one when there is one.
    reflection_min: vec4<f32>,
    reflection_max: vec4<f32>,
    // Where the probe captured from, with W as the width over which it fades in at the box's edge.
    reflection_center: vec4<f32>,
}

struct Light {
    // Position, then range.
    position: vec4<f32>,
    // Color times intensity, then the cosine of the outer angle, below -1 for a point light.
    color: vec4<f32>,
    // The axis of a spot, then the cosine of its inner angle.
    direction: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var shadow_map: texture_depth_2d_array;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;
@group(0) @binding(3) var sky_radiance: texture_cube<f32>;
@group(0) @binding(4) var sky_irradiance: texture_cube<f32>;
@group(0) @binding(5) var sky_sampler: sampler;
@group(0) @binding(6) var reflection: texture_cube<f32>;
@group(1) @binding(0) var base_color_texture: texture_2d<f32>;
@group(1) @binding(1) var metallic_roughness_texture: texture_2d<f32>;
@group(1) @binding(2) var normal_texture: texture_2d<f32>;
@group(1) @binding(3) var material_sampler: sampler;
@group(1) @binding(4) var emissive_texture: texture_2d<f32>;

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
    @location(6) emissive: vec3<f32>,
    // Opacity, the cutoff below which a masked surface is absent, and transmission.
    @location(7) opacity: vec3<f32>,
}

struct InstanceInput {
    @location(4) column0: vec4<f32>,
    @location(5) column1: vec4<f32>,
    @location(6) column2: vec4<f32>,
    @location(7) column3: vec4<f32>,
    @location(8) base_color: vec3<f32>,
    @location(9) metallic: f32,
    @location(10) roughness: f32,
    @location(13) emissive: vec3<f32>,
    @location(14) opacity: vec3<f32>,
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
    output.emissive = instance.emissive;
    output.opacity = instance.opacity;
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
fn fs_main(input: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return vec4<f32>(lit(input, front, 1.0), 1.0);
}

@fragment
fn fs_masked(input: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if opacity_of(input) < input.opacity.y {
        discard;
    }
    return vec4<f32>(lit(input, front, 1.0), 1.0);
}

// Premultiplied, so that what the surface reflects keeps its strength however clear the surface is.
@fragment
fn fs_blended(input: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let opacity = opacity_of(input);
    return vec4<f32>(lit(input, front, opacity), opacity);
}

fn opacity_of(input: VertexOutput) -> f32 {
    return textureSample(base_color_texture, material_sampler, input.uv).a * input.opacity.x;
}

// Scales only what the surface itself sends back by `opacity`, and fades it into the fog.
fn lit(input: VertexOutput, front: bool, opacity: f32) -> vec3<f32> {
    // A back face seen through a double-sided material is lit as the side facing the viewer.
    let facing = select(-1.0, 1.0, front);
    let geometric = normalize(input.normal) * facing;
    let normal = mapped_normal(geometric, input.world_position, input.uv);
    let metal_rough = textureSample(metallic_roughness_texture, material_sampler, input.uv);
    let metallic = input.metallic * metal_rough.b;
    let base_color = input.base_color * textureSample(base_color_texture, material_sampler, input.uv).rgb;
    let to_camera = normalize(frame.camera_position.xyz - input.world_position);
    let n_dot_v = max(dot(normal, to_camera), 1e-4);

    let roughness = clamp(input.roughness * metal_rough.g, MIN_ROUGHNESS, 1.0);
    let alpha = roughness * roughness;
    let reflectance = mix(vec3<f32>(DIELECTRIC_REFLECTANCE), base_color, metallic);
    let diffuse = base_color * (1.0 - metallic) * opacity;
    let surface = Surface(normal, to_camera, n_dot_v, alpha, diffuse, reflectance);

    let shadowed = sun_visibility(input.world_position, geometric, input.position.xy);
    var direct = shade(surface, frame.to_sun.xyz, frame.sun_color.rgb * shadowed);
    let transmission = input.opacity.z;
    if transmission > 0.0 {
        let behind = max(dot(-geometric, frame.to_sun.xyz), 0.0);
        if behind > 0.0 {
            let through = sun_visibility(input.world_position, -geometric, input.position.xy);
            direct += diffuse * frame.sun_color.rgb * (transmission * behind * through);
        }
    }
    for (var i = 0u; i < u32(frame.shadow_params.w); i++) {
        direct += local_light(surface, frame.lights[i], input.world_position);
    }
    let mirrored = reflect(-to_camera, normal);
    var ambient_diffuse = frame.ambient.rgb;
    var ambient_reflected = frame.ambient.rgb;
    if frame.shadow_params.z != 0.0 {
        ambient_diffuse = textureSampleLevel(sky_irradiance, sky_sampler, normal, 0.0).rgb;
        ambient_reflected = textureSampleLevel(sky_radiance, sky_sampler, mirrored, roughness * SKY_LAST_MIP).rgb;
    }
    let sky_reflected = ambient_reflected * sky_visibility(input.world_position, mirrored);
    let reflected = probe_reflection(input.world_position, mirrored, roughness, sky_reflected);
    let ambient = diffuse * (ambient_diffuse * sky_visibility(input.world_position, normal) + bounce_light(input.world_position, normal))
        + ambient_specular(reflectance, roughness, n_dot_v) * reflected;
    let glow = input.emissive * textureSample(emissive_texture, material_sampler, input.uv).rgb * opacity;
    let air = 1.0 - exp(-length(frame.camera_position.xyz - input.world_position) * frame.fog.w);
    return mix(direct + ambient + glow, frame.fog.rgb * opacity, air) * frame.camera_position.w;
}

struct Surface {
    normal: vec3<f32>,
    to_camera: vec3<f32>,
    n_dot_v: f32,
    alpha: f32,
    diffuse: vec3<f32>,
    reflectance: vec3<f32>,
}

// Light colors are the light leaving a white diffuse surface, so diffuse carries no 1/PI and specular gains PI.
fn shade(surface: Surface, to_light: vec3<f32>, light: vec3<f32>) -> vec3<f32> {
    let halfway = normalize(surface.to_camera + to_light);
    let n_dot_l = max(dot(surface.normal, to_light), 0.0);
    let n_dot_h = max(dot(surface.normal, halfway), 0.0);
    let v_dot_h = max(dot(surface.to_camera, halfway), 0.0);
    let specular = ggx(n_dot_h, surface.alpha) * visibility(n_dot_l, surface.n_dot_v, surface.alpha) * fresnel(surface.reflectance, v_dot_h);
    return (surface.diffuse + specular * PI) * light * n_dot_l;
}

fn local_light(surface: Surface, light: Light, position: vec3<f32>) -> vec3<f32> {
    let offset = light.position.xyz - position;
    let distance_squared = max(dot(offset, offset), 1e-4);
    let to_light = offset * inverseSqrt(distance_squared);
    // Karis's window brings the inverse square to exactly zero at the range.
    let reach = saturate(1.0 - pow(distance_squared / (light.position.w * light.position.w), 2.0));
    var strength = reach * reach / distance_squared;
    if light.color.w >= -1.0 {
        let along = dot(-to_light, light.direction.xyz);
        strength *= smoothstep(light.color.w, light.direction.w, along);
    }
    return shade(surface, to_light, light.color.rgb * strength);
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

// Points spread evenly over a unit disk, for soft shadows.
const DISK = array<vec2<f32>, 16>(
    vec2<f32>(-0.942, -0.399), vec2<f32>(0.946, -0.769), vec2<f32>(-0.094, -0.929), vec2<f32>(0.345, 0.294),
    vec2<f32>(-0.916, 0.458), vec2<f32>(-0.815, -0.879), vec2<f32>(-0.383, 0.277), vec2<f32>(0.975, 0.756),
    vec2<f32>(0.443, -0.975), vec2<f32>(0.537, -0.474), vec2<f32>(-0.265, -0.419), vec2<f32>(0.792, 0.191),
    vec2<f32>(-0.242, 0.997), vec2<f32>(-0.814, 0.914), vec2<f32>(0.200, 0.786), vec2<f32>(0.144, -0.141),
);
// In texels, so a penumbra stays within what the blocker search can see.
const LARGEST_PENUMBRA: f32 = 8.0;
// The share of each cascade over which it hands over to the next, so the seam does not show.
const CASCADE_BLEND: f32 = 0.15;

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

// Inside the probe's box, the surroundings it captured replace `outside`.
fn probe_reflection(position: vec3<f32>, mirrored: vec3<f32>, roughness: f32, outside: vec3<f32>) -> vec3<f32> {
    let low = frame.reflection_min.xyz;
    let high = frame.reflection_max.xyz;
    if frame.reflection_min.w == 0.0 || any(position < low) || any(position > high) {
        return outside;
    }
    // Looking up where the ray leaves the box, rather than its direction alone, keeps near walls in place.
    let leave = max((high - position) / mirrored, (low - position) / mirrored);
    let reach = min(leave.x, min(leave.y, leave.z));
    let toward = position + mirrored * reach - frame.reflection_center.xyz;
    let captured = textureSampleLevel(reflection, sky_sampler, toward, roughness * SKY_LAST_MIP).rgb;
    let inward = min(position - low, high - position);
    let fade = saturate(min(inward.x, min(inward.y, inward.z)) / frame.reflection_center.w);
    return mix(outside, captured, fade);
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

fn sun_visibility(position: vec3<f32>, normal: vec3<f32>, pixel: vec2<f32>) -> f32 {
    if frame.shadow_params.y == 0.0 {
        return 1.0;
    }
    let count = i32(frame.shadow_params.x);
    let distance = length(position - frame.camera_position.xyz);
    var cascade = 0;
    while cascade < count - 1 && distance > frame.shadow_ends[cascade] {
        cascade++;
    }
    let end = frame.shadow_ends[cascade];
    if distance > end {
        return 1.0;
    }
    // Interleaved gradient noise turns each pixel's filter differently, trading banding for grain.
    let turn = fract(52.9829189 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715)))) * 6.2831853;
    var lit = cascade_visibility(cascade, position, normal, turn);
    let start = select(0.0, frame.shadow_ends[max(cascade - 1, 0)], cascade > 0);
    let blend_from = end - (end - start) * CASCADE_BLEND;
    if distance > blend_from {
        let toward_next = (distance - blend_from) / (end - blend_from);
        // The last cascade fades out instead, so shadows do not stop at a visible line.
        var next = 1.0;
        if cascade < count - 1 {
            next = cascade_visibility(cascade + 1, position, normal, turn);
        }
        lit = mix(lit, next, toward_next);
    }
    return lit;
}

fn cascade_visibility(cascade: i32, position: vec3<f32>, normal: vec3<f32>, turn: f32) -> f32 {
    let scale = frame.shadow_scales[cascade];
    // Pushing the lookup off the surface by more than a texel removes self-shadowing without detaching contact shadows.
    let offset = normalize(normal) * scale.x * 1.5;
    let clip = frame.cascades[cascade] * vec4<f32>(position + offset, 1.0);
    let uv = clip.xy * vec2<f32>(0.5, -0.5) + 0.5;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) || clip.z > 1.0 {
        return 1.0;
    }
    let size = vec2<f32>(textureDimensions(shadow_map));
    let texel = 1.0 / size;
    if frame.shadow_shape.x == 0.0 {
        var lit = 0.0;
        for (var i = 0; i < 4; i++) {
            lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + SHADOW_TAPS[i] * texel, cascade, clip.z);
        }
        return lit / 4.0;
    }
    let rotation = mat2x2<f32>(cos(turn), sin(turn), -sin(turn), cos(turn));
    // Averaging what lies between the surface and the sun sizes the penumbra, which widens with that gap.
    var blockers = 0.0;
    var found = 0.0;
    for (var i = 0; i < 8; i++) {
        let at = vec2<i32>((uv + rotation * DISK[i * 2] * LARGEST_PENUMBRA * texel) * size);
        let depth = textureLoad(shadow_map, clamp(at, vec2<i32>(0), vec2<i32>(size) - 1), cascade, 0);
        if depth < clip.z {
            blockers += depth;
            found += 1.0;
        }
    }
    if found == 0.0 {
        return 1.0;
    }
    let gap = (clip.z - blockers / found) * scale.y;
    let radius = clamp(gap * frame.shadow_shape.y * 2.0 / scale.x, 1.0, LARGEST_PENUMBRA);
    var lit = 0.0;
    for (var i = 0; i < 16; i++) {
        lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + rotation * DISK[i] * radius * texel, cascade, clip.z);
    }
    return lit / 16.0;
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
