const MAX_DIRECTIONAL: u32 = 4u;
const MAX_POINT: u32 = 16u;
const PI: f32 = 3.14159265;

struct DirectionalLight {
    // Unit vector from the surface toward the light.
    direction: vec4<f32>,
    color: vec4<f32>,
};

struct PointLight {
    // xyz position, w range.
    position: vec4<f32>,
    color: vec4<f32>,
};

struct Frame {
    view_proj: mat4x4<f32>,
    camera_position: vec4<f32>,
    ambient: vec4<f32>,
    // x directional count, y point count, z 1 when the target needs sRGB-encoded output.
    counts: vec4<u32>,
    directional: array<DirectionalLight, 4>,
    point: array<PointLight, 16>,
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
    // x 0 opaque, 1 mask, 2 blend; y mask cutoff.
    alpha: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
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
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tangent: vec4<f32>,
    @location(4) color: vec4<f32>,
};

@vertex
fn vs(in: VertexIn) -> VertexOut {
    let world = draw.model * vec4(in.position, 1.0);
    var out: VertexOut;
    out.clip = frame.view_proj * world;
    out.world = world.xyz;
    out.normal = (draw.normal * vec4(in.normal, 0.0)).xyz;
    let uv = vec3(in.uv, 1.0);
    out.uv = vec2(dot(draw.uv_x.xyz, uv), dot(draw.uv_y.xyz, uv));
    out.tangent = vec4((draw.model * vec4(in.tangent.xyz, 0.0)).xyz, in.tangent.w);
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

fn encode_srgb(linear: vec3<f32>) -> vec3<f32> {
    let low = linear * 12.92;
    let high = 1.055 * pow(linear, vec3(1.0 / 2.4)) - 0.055;
    return select(high, low, linear <= vec3(0.0031308));
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
        color = frame.ambient.rgb * base.rgb * occlusion + draw.emissive.rgb * glow;
        let directional = min(frame.counts.x, MAX_DIRECTIONAL);
        let lights = directional + min(frame.counts.y, MAX_POINT);
        // Calling shade() from two loops loses the device on WARP, Windows' software adapter.
        for (var i = 0u; i < lights; i++) {
            var l: vec3<f32>;
            var radiance: vec3<f32>;
            if i < directional {
                let light = frame.directional[i];
                l = light.direction.xyz;
                radiance = light.color.rgb;
            } else {
                let light = frame.point[i - directional];
                let to_light = light.position.xyz - in.world;
                let distance2 = max(dot(to_light, to_light), 1e-4);
                let range = max(light.position.w, 1e-4);
                let window = clamp(1.0 - pow(distance2 / (range * range), 2.0), 0.0, 1.0);
                l = normalize(to_light);
                radiance = light.color.rgb * window * window / max(distance2, 0.01);
            }
            color += shade(n, v, l, radiance, base.rgb, metallic, roughness);
        }
    }
    color = clamp(color, vec3(0.0), vec3(1.0));
    if frame.counts.z == 1u {
        color = encode_srgb(color);
    }
    return vec4(color, alpha);
}
