const MAX_DIRECTIONAL: u32 = 4u;
const MAX_POINT: u32 = 256u;
const MAX_SHADOW_LAYERS: u32 = 16u;
const PI: f32 = 3.14159265;

struct DirectionalLight {
    // Unit vector from the surface toward the light.
    direction: vec4<f32>,
    color: vec4<f32>,
    // x first shadow layer or -1, y cascade count, z bias, w softness.
    shadow: vec4<f32>,
    // The view depth each cascade reaches.
    splits: vec4<f32>,
    // World size of a texel in each cascade.
    texel: vec4<f32>,
};

// A point light, or a spot light when its cone narrows it.
struct PointLight {
    // xyz position, w range.
    position: vec4<f32>,
    // rgb radiance, w cone offset.
    color: vec4<f32>,
    // x first shadow layer or -1, y 1 for a spot light's single layer, z bias, w softness.
    shadow: vec4<f32>,
    // xyz the direction a spot light shines, w cone scale; the cone fades as
    // clamp(cos θ · scale + offset, 0, 1)², which a scale of 0 and offset of 1 make a point light.
    direction: vec4<f32>,
};

struct Frame {
    view_proj: mat4x4<f32>,
    inverse_view_proj: mat4x4<f32>,
    camera_position: vec4<f32>,
    camera_forward: vec4<f32>,
    ambient: vec4<f32>,
    // Hemisphere light from above and below.
    sky: vec4<f32>,
    ground: vec4<f32>,
    // x directional count, y point count, z 1 when the target needs sRGB-encoded output.
    counts: vec4<u32>,
    // x 1 / shadow map size.
    shadow: vec4<f32>,
    // x exposure, y tone mapping: 0 none, 1 Reinhard, 2 ACES, 3 AgX; z 1 to write unmapped
    // linear color for post-processing to finish.
    tone: vec4<f32>,
    fog_color: vec4<f32>,
    // x 0 none, 1 linear from y to z, 2 exponential with density y.
    fog: vec4<f32>,
    // x intensity, y 1 when there is an environment, z its last mip level.
    environment: vec4<f32>,
    // The environment's irradiance over π as spherical harmonics, already scaled by its intensity.
    irradiance: array<vec4<f32>, 9>,
    directional: array<DirectionalLight, 4>,
    shadow_matrices: array<mat4x4<f32>, MAX_SHADOW_LAYERS>,
};

struct Draw {
    model: mat4x4<f32>,
    normal: mat4x4<f32>,
    base_color: vec4<f32>,
    emissive: vec4<f32>,
    // x metallic, y roughness, z 1 when unlit.
    params: vec4<f32>,
    // Rows of the 2×3 texture-coordinate transform.
    uv_x: vec4<f32>,
    uv_y: vec4<f32>,
    // x 0 opaque, 1 mask, 2 blend; y mask cutoff; z 1 when shadows darken it.
    alpha: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var shadow_maps: texture_depth_2d_array;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;
@group(0) @binding(3) var environment_map: texture_cube<f32>;
@group(0) @binding(4) var environment_sampler: sampler;
// One point light per row, its fields in the row's four texels.
@group(0) @binding(5) var point_lights: texture_2d<f32>;
@group(1) @binding(0) var<uniform> draw: Draw;
@group(2) @binding(0) var base_texture: texture_2d<f32>;
@group(2) @binding(1) var metallic_roughness_texture: texture_2d<f32>;
@group(2) @binding(2) var normal_texture: texture_2d<f32>;
@group(2) @binding(3) var emissive_texture: texture_2d<f32>;
@group(2) @binding(4) var occlusion_texture: texture_2d<f32>;
@group(2) @binding(5) var material_sampler: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tangent: vec4<f32>,
    @location(4) color: vec4<f32>,
    @location(5) instance_x: vec4<f32>,
    @location(6) instance_y: vec4<f32>,
    @location(7) instance_z: vec4<f32>,
    @location(8) instance_w: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tangent: vec4<f32>,
    @location(4) color: vec4<f32>,
};

// The cofactor matrix, which transforms normals like the inverse transpose up to a positive scale.
fn normal_matrix(m: mat3x3<f32>) -> mat3x3<f32> {
    let cofactor = mat3x3(cross(m[1], m[2]), cross(m[2], m[0]), cross(m[0], m[1]));
    return cofactor * sign(determinant(m));
}

@vertex
fn vs(in: VertexIn) -> VertexOut {
    let instance = mat4x4(in.instance_x, in.instance_y, in.instance_z, in.instance_w);
    let model = draw.model * instance;
    let world = model * vec4(in.position, 1.0);
    var out: VertexOut;
    out.clip = frame.view_proj * world;
    out.world = world.xyz;
    let instance_normal = normal_matrix(mat3x3(instance[0].xyz, instance[1].xyz, instance[2].xyz)) * in.normal;
    out.normal = (draw.normal * vec4(instance_normal, 0.0)).xyz;
    let uv = vec3(in.uv, 1.0);
    out.uv = vec2(dot(draw.uv_x.xyz, uv), dot(draw.uv_y.xyz, uv));
    out.tangent = vec4((model * vec4(in.tangent.xyz, 0.0)).xyz, in.tangent.w);
    out.color = in.color;
    return out;
}

fn distribution(n_dot_h: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

// Height-correlated Smith visibility, including the 1 / (4 n·l n·v) term.
fn visibility(n_dot_v: f32, n_dot_l: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    let gv = n_dot_l * sqrt(n_dot_v * n_dot_v * (1.0 - a2) + a2);
    let gl = n_dot_v * sqrt(n_dot_l * n_dot_l * (1.0 - a2) + a2);
    return 0.5 / max(gv + gl, 1e-5);
}

fn shade(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, radiance: vec3<f32>, base: vec3<f32>, metallic: f32, roughness: f32) -> vec3<f32> {
    let n_dot_l = dot(n, l);
    if n_dot_l <= 0.0 {
        return vec3(0.0);
    }
    let h = normalize(v + l);
    let n_dot_v = max(dot(n, v), 1e-4);
    let n_dot_h = max(dot(n, h), 0.0);
    let v_dot_h = max(dot(v, h), 0.0);
    let alpha = max(roughness * roughness, 0.002);
    let f0 = mix(vec3(0.04), base, metallic);
    let fresnel = f0 + (vec3(1.0) - f0) * pow(1.0 - v_dot_h, 5.0);
    let specular = fresnel * distribution(n_dot_h, alpha) * visibility(n_dot_v, n_dot_l, alpha);
    let diffuse = (vec3(1.0) - fresnel) * (1.0 - metallic) * base / PI;
    // Scaled by π so an intensity of 1 lights a white matte surface facing the light fully.
    return (diffuse + specular) * radiance * (PI * n_dot_l);
}

// The lit fraction of a 3×3 neighborhood, each tap itself a filtered 2×2 comparison.
fn filtered_shadow(layer: i32, clip: vec3<f32>, softness: f32) -> f32 {
    let uv = vec2(clip.x * 0.5 + 0.5, 0.5 - clip.y * 0.5);
    if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) || clip.z > 1.0 {
        return 1.0;
    }
    let step = frame.shadow.x * softness;
    var lit = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let offset = vec2(f32(x), f32(y)) * step;
            lit += textureSampleCompareLevel(shadow_maps, shadow_sampler, uv + offset, layer, clip.z);
        }
    }
    return lit / 9.0;
}

fn directional_shadow(light: DirectionalLight, world: vec3<f32>, normal: vec3<f32>) -> f32 {
    let count = i32(light.shadow.y);
    if light.shadow.x < 0.0 || count == 0 {
        return 1.0;
    }
    let depth = dot(world - frame.camera_position.xyz, frame.camera_forward.xyz);
    var cascade = count;
    for (var c = 0; c < count; c++) {
        if depth < light.splits[c] {
            cascade = c;
            break;
        }
    }
    if cascade == count {
        return 1.0;
    }
    let offset = normal * light.texel[cascade] * light.shadow.z;
    let layer = i32(light.shadow.x) + cascade;
    let clip = frame.shadow_matrices[layer] * vec4(world + offset, 1.0);
    let lit = filtered_shadow(layer, clip.xyz, light.shadow.w);
    let reach = light.splits[count - 1];
    let fade = clamp((reach - depth) / (reach * 0.1), 0.0, 1.0);
    return mix(1.0, lit, fade);
}

fn point_shadow(light: PointLight, world: vec3<f32>, normal: vec3<f32>) -> f32 {
    if light.shadow.x < 0.0 {
        return 1.0;
    }
    let distance = length(world - light.position.xyz);
    // A face spans 90°, so a texel at this distance is 2·distance / size wide.
    let offset = normal * 2.0 * distance * frame.shadow.x * light.shadow.z;
    let v = world + offset - light.position.xyz;
    let a = abs(v);
    var face: i32;
    if a.x >= a.y && a.x >= a.z {
        face = select(1, 0, v.x > 0.0);
    } else if a.y >= a.z {
        face = select(3, 2, v.y > 0.0);
    } else {
        face = select(5, 4, v.z > 0.0);
    }
    if light.shadow.y > 0.5 {
        face = 0;
    }
    let layer = i32(light.shadow.x) + face;
    let clip = frame.shadow_matrices[layer] * vec4(world + offset, 1.0);
    return filtered_shadow(layer, clip.xyz / clip.w, light.shadow.w);
}



fn tone_map(color: vec3<f32>) -> vec3<f32> {
    return tone_curve(color, frame.tone.x, i32(frame.tone.y));
}

fn fog_amount(world: vec3<f32>) -> f32 {
    let distance = length(world - frame.camera_position.xyz);
    if frame.fog.x > 1.5 {
        let d = frame.fog.y * distance;
        return 1.0 - exp(-d * d);
    } else if frame.fog.x > 0.5 {
        return clamp((distance - frame.fog.y) / max(frame.fog.z - frame.fog.y, 1e-4), 0.0, 1.0);
    }
    return 0.0;
}

fn environment_irradiance(n: vec3<f32>) -> vec3<f32> {
    let c = frame.irradiance;
    return c[0].rgb + c[1].rgb * n.y + c[2].rgb * n.z + c[3].rgb * n.x
        + c[4].rgb * (n.x * n.y) + c[5].rgb * (n.y * n.z) + c[6].rgb * (3.0 * n.z * n.z - 1.0)
        + c[7].rgb * (n.x * n.z) + c[8].rgb * (n.x * n.x - n.y * n.y);
}

// Karis' analytic fit of the split-sum BRDF integral: the scale and bias applied to F0.
fn environment_brdf(roughness: f32, n_dot_v: f32) -> vec2<f32> {
    let r = roughness * vec4(-1.0, -0.0275, -0.572, 0.022) + vec4(1.0, 0.0425, 1.04, -0.04);
    let a004 = min(r.x * r.x, exp2(-9.28 * n_dot_v)) * r.x + r.y;
    return vec2(-1.04, 1.04) * a004 + r.zw;
}

fn environment_light(n: vec3<f32>, v: vec3<f32>, base: vec3<f32>, metallic: f32, roughness: f32) -> vec3<f32> {
    let brdf = environment_brdf(roughness, max(dot(n, v), 1e-4));
    let specular = mix(vec3(0.04), base, metallic) * brdf.x + brdf.y;
    let lod = roughness * frame.environment.z;
    let reflected = textureSampleLevel(environment_map, environment_sampler, reflect(-v, n), lod).rgb;
    let diffuse = environment_irradiance(n) * base * (1.0 - metallic) * (vec3(1.0) - specular);
    return diffuse + reflected * frame.environment.x * specular;
}

// Fogs, tone maps, clamps and encodes a final linear color for the target.
//
// Fog follows tone mapping so it meets the untouched background, except under post-processing,
// which tone maps the background too.
fn finish(linear: vec3<f32>, fog: f32) -> vec3<f32> {
    if frame.tone.z > 0.5 {
        return mix(linear, frame.fog_color.rgb, fog);
    }
    let color = clamp(mix(tone_map(linear), frame.fog_color.rgb, fog), vec3(0.0), vec3(1.0));
    if frame.counts.z == 1u {
        return encode_srgb(color);
    }
    return color;
}

struct SkyOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_sky(@builtin(vertex_index) index: u32) -> SkyOut {
    let ndc = vec2(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0;
    var out: SkyOut;
    out.clip = vec4(ndc, 1.0, 1.0);
    out.ndc = ndc;
    return out;
}

@fragment
fn fs_sky(in: SkyOut) -> @location(0) vec4<f32> {
    let near = frame.inverse_view_proj * vec4(in.ndc, 0.0, 1.0);
    let far = frame.inverse_view_proj * vec4(in.ndc, 1.0, 1.0);
    let direction = far.xyz / far.w - near.xyz / near.w;
    let radiance = textureSampleLevel(environment_map, environment_sampler, direction, 0.0).rgb;
    return vec4(finish(radiance * frame.environment.x, 0.0), 1.0);
}


@fragment
fn fs(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    // Sampled before any branch, since sampling needs uniform control flow.
    let texel = textureSample(base_texture, material_sampler, in.uv);
    let metal_rough = textureSample(metallic_roughness_texture, material_sampler, in.uv);
    let mapped = textureSample(normal_texture, material_sampler, in.uv).xyz * 2.0 - 1.0;
    let glow = textureSample(emissive_texture, material_sampler, in.uv).rgb;
    let occlusion = textureSample(occlusion_texture, material_sampler, in.uv).r;
    let base = draw.base_color * texel * in.color;
    var alpha = base.a;
    if draw.alpha.x < 0.5 {
        alpha = 1.0;
    } else if draw.alpha.x < 1.5 {
        if base.a < draw.alpha.y {
            discard;
        }
        alpha = 1.0;
    }
    var color = base.rgb;
    if draw.params.z < 0.5 {
        var geometric = normalize(in.normal);
        var t = in.tangent.xyz - geometric * dot(geometric, in.tangent.xyz);
        var b = cross(geometric, t) * in.tangent.w;
        if !front {
            geometric = -geometric;
            t = -t;
            b = -b;
        }
        var n = geometric;
        if dot(t, t) > 1e-8 {
            n = normalize(mat3x3(normalize(t), normalize(b), geometric) * mapped);
        }
        let v = normalize(frame.camera_position.xyz - in.world);
        let metallic = clamp(draw.params.x * metal_rough.b, 0.0, 1.0);
        let roughness = clamp(draw.params.y * metal_rough.g, 0.0, 1.0);
        let ambient = frame.ambient.rgb + mix(frame.ground.rgb, frame.sky.rgb, n.y * 0.5 + 0.5);
        color = ambient * base.rgb * occlusion + draw.emissive.rgb * glow;
        if frame.environment.y > 0.5 {
            color += environment_light(n, v, base.rgb, metallic, roughness) * occlusion;
        }
        let receive = draw.alpha.z > 0.5;
        let directional = min(frame.counts.x, MAX_DIRECTIONAL);
        let lights = directional + min(frame.counts.y, MAX_POINT);
        // Calling shade() from two loops loses the device on WARP, Windows' software adapter.
        for (var i = 0u; i < lights; i++) {
            var l: vec3<f32>;
            var radiance: vec3<f32>;
            var lit = 1.0;
            if i < directional {
                let light = frame.directional[i];
                l = light.direction.xyz;
                radiance = light.color.rgb;
                if receive {
                    lit = directional_shadow(light, in.world, geometric);
                }
            } else {
                let row = i32(i - directional);
                let position = textureLoad(point_lights, vec2(0, row), 0);
                let to_light = position.xyz - in.world;
                let distance2 = max(dot(to_light, to_light), 1e-4);
                let range = max(position.w, 1e-4);
                if distance2 >= range * range {
                    continue;
                }
                let light = PointLight(
                    position,
                    textureLoad(point_lights, vec2(1, row), 0),
                    textureLoad(point_lights, vec2(2, row), 0),
                    textureLoad(point_lights, vec2(3, row), 0),
                );
                let window = clamp(1.0 - pow(distance2 / (range * range), 2.0), 0.0, 1.0);
                l = normalize(to_light);
                let cone = clamp(dot(-l, light.direction.xyz) * light.direction.w + light.color.w, 0.0, 1.0);
                radiance = light.color.rgb * window * window * cone * cone / max(distance2, 0.01);
                if receive {
                    lit = point_shadow(light, in.world, geometric);
                }
            }
            color += shade(n, v, l, radiance * lit, base.rgb, metallic, roughness);
        }
    }
    return vec4(finish(color, fog_amount(in.world)), alpha);
}
