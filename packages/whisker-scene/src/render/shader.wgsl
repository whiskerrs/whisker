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
    // x seconds since the renderer's first frame, y 1 when transmission_map holds the scene behind.
    time: vec4<f32>,
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
    // x clearcoat, y clearcoat roughness, z transmission, w index of refraction.
    layers: vec4<f32>,
    // rgb sheen color, w sheen roughness.
    sheen: vec4<f32>,
    // x anisotropy, y its rotation, z thickness.
    anisotropy: vec4<f32>,
    // A custom material's own parameters.
    custom: array<vec4<f32>, 4>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var shadow_maps: texture_depth_2d_array;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;
@group(0) @binding(3) var environment_map: texture_cube<f32>;
@group(0) @binding(4) var environment_sampler: sampler;
// One point light per row, its fields in the row's four texels.
@group(0) @binding(5) var point_lights: texture_2d<f32>;
@group(1) @binding(0) var<uniform> draw: Draw;
// A standard material binds base color, metallic-roughness, normal, emissive and occlusion maps.
@group(2) @binding(0) var texture0: texture_2d<f32>;
@group(2) @binding(1) var texture1: texture_2d<f32>;
@group(2) @binding(2) var texture2: texture_2d<f32>;
@group(2) @binding(3) var texture3: texture_2d<f32>;
@group(2) @binding(4) var texture4: texture_2d<f32>;
@group(2) @binding(5) var material_sampler: sampler;
// The opaque scene in linear color, which transmissive surfaces show through.
@group(3) @binding(0) var transmission_map: texture_2d<f32>;
@group(3) @binding(1) var transmission_sampler: sampler;

struct SurfaceInput {
    uv: vec2<f32>,
    world: vec3<f32>,
    // The interpolated vertex normal, facing the viewer.
    normal: vec3<f32>,
    color: vec4<f32>,
};

struct Surface {
    // Linear color and alpha.
    base_color: vec4<f32>,
    emissive: vec3<f32>,
    metallic: f32,
    roughness: f32,
    // In tangent space, where z runs along the vertex normal.
    normal: vec3<f32>,
    occlusion: f32,
    clearcoat: f32,
    clearcoat_roughness: f32,
    sheen_color: vec3<f32>,
    sheen_roughness: f32,
    transmission: f32,
    ior: f32,
    thickness: f32,
    anisotropy: f32,
    // In tangent space.
    anisotropy_direction: vec2<f32>,
};

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
    let view_term = n_dot_l * sqrt(n_dot_v * n_dot_v * (1.0 - a2) + a2);
    let light_term = n_dot_v * sqrt(n_dot_l * n_dot_l * (1.0 - a2) + a2);
    return 0.5 / max(view_term + light_term, 1e-5);
}

// A surface's lighting inputs in world space.
struct Shading {
    n: vec3<f32>,
    v: vec3<f32>,
    // The clearcoat's normal, which the normal map does not bend.
    coat: vec3<f32>,
    // Along and across the anisotropy direction.
    t: vec3<f32>,
    b: vec3<f32>,
    base: vec3<f32>,
    metallic: f32,
    roughness: f32,
    clearcoat: f32,
    clearcoat_roughness: f32,
    sheen_color: vec3<f32>,
    sheen_roughness: f32,
    transmission: f32,
    anisotropy: f32,
};

fn anisotropic_distribution(t_dot_h: f32, b_dot_h: f32, n_dot_h: f32, at: f32, ab: f32) -> f32 {
    let d = vec3(ab * t_dot_h, at * b_dot_h, at * ab * n_dot_h);
    let w = at * ab / dot(d, d);
    return at * ab * w * w / PI;
}

fn anisotropic_visibility(sh: Shading, l: vec3<f32>, n_dot_v: f32, n_dot_l: f32, at: f32, ab: f32) -> f32 {
    let view_term = n_dot_l * length(vec3(at * dot(sh.t, sh.v), ab * dot(sh.b, sh.v), n_dot_v));
    let light_term = n_dot_v * length(vec3(at * dot(sh.t, l), ab * dot(sh.b, l), n_dot_l));
    return 0.5 / max(view_term + light_term, 1e-5);
}

// Estevez and Kulla's "Charlie" sheen distribution.
fn sheen_distribution(n_dot_h: f32, roughness: f32) -> f32 {
    let inverse = 1.0 / max(roughness * roughness, 1e-3);
    let sin2 = max(1.0 - n_dot_h * n_dot_h, 0.0);
    return (2.0 + inverse) * pow(sin2, inverse * 0.5) / (2.0 * PI);
}

// How much a sheen dims the layers under it, from the brightest channel of its color.
fn sheen_scaling(sheen: vec3<f32>) -> f32 {
    return 1.0 - max(sheen.r, max(sheen.g, sheen.b)) * 0.157;
}

fn shade(sh: Shading, l: vec3<f32>, radiance: vec3<f32>) -> vec3<f32> {
    var result = vec3(0.0);
    let n_dot_l = dot(sh.n, l);
    let h = normalize(sh.v + l);
    if n_dot_l > 0.0 {
        let n_dot_v = max(dot(sh.n, sh.v), 1e-4);
        let n_dot_h = max(dot(sh.n, h), 0.0);
        let v_dot_h = max(dot(sh.v, h), 0.0);
        let alpha = max(sh.roughness * sh.roughness, 0.002);
        let f0 = mix(vec3(0.04), sh.base, sh.metallic);
        let fresnel = f0 + (vec3(1.0) - f0) * pow(1.0 - v_dot_h, 5.0);
        var lobe: f32;
        if abs(sh.anisotropy) > 1e-3 {
            let at = max(alpha * (1.0 + sh.anisotropy), 0.002);
            let ab = max(alpha * (1.0 - sh.anisotropy), 0.002);
            lobe = anisotropic_distribution(dot(sh.t, h), dot(sh.b, h), n_dot_h, at, ab)
                * anisotropic_visibility(sh, l, n_dot_v, n_dot_l, at, ab);
        } else {
            lobe = distribution(n_dot_h, alpha) * visibility(n_dot_v, n_dot_l, alpha);
        }
        let diffuse = (vec3(1.0) - fresnel) * (1.0 - sh.metallic) * (1.0 - sh.transmission) * sh.base / PI;
        let sheen = sh.sheen_color * sheen_distribution(n_dot_h, sh.sheen_roughness)
            / (4.0 * (n_dot_l + n_dot_v - n_dot_l * n_dot_v));
        // Scaled by π so an intensity of 1 lights a white matte surface facing the light fully.
        result = ((diffuse + fresnel * lobe) * sheen_scaling(sh.sheen_color) + sheen) * (PI * n_dot_l);
    }
    let coat_n_dot_l = dot(sh.coat, l);
    if sh.clearcoat > 0.0 && coat_n_dot_l > 0.0 {
        let coat_fresnel = (0.04 + 0.96 * pow(1.0 - max(dot(sh.v, h), 0.0), 5.0)) * sh.clearcoat;
        let alpha = max(sh.clearcoat_roughness * sh.clearcoat_roughness, 0.002);
        // Kelemen's visibility, cheap and close for a smooth layer.
        let coat = distribution(max(dot(sh.coat, h), 0.0), alpha) * 0.25 / max(dot(l, h) * dot(l, h), 1e-4);
        result = result * (1.0 - coat_fresnel) + coat_fresnel * coat * PI * coat_n_dot_l;
    }
    return result * radiance;
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

fn environment_light(sh: Shading) -> vec3<f32> {
    let n_dot_v = max(dot(sh.n, sh.v), 1e-4);
    var reflect_normal = sh.n;
    if abs(sh.anisotropy) > 1e-3 {
        // Bends the reflection toward the stretch, as Filament approximates it.
        let along = select(sh.t, sh.b, sh.anisotropy >= 0.0);
        let bent = cross(cross(along, sh.v), along);
        let stretch = abs(sh.anisotropy) * clamp(5.0 * sh.roughness, 0.0, 1.0);
        reflect_normal = normalize(mix(sh.n, bent, stretch));
    }
    let brdf = environment_brdf(sh.roughness, n_dot_v);
    let specular = mix(vec3(0.04), sh.base, sh.metallic) * brdf.x + brdf.y;
    let lod = sh.roughness * frame.environment.z;
    let reflected = textureSampleLevel(environment_map, environment_sampler, reflect(-sh.v, reflect_normal), lod).rgb;
    let irradiance = environment_irradiance(sh.n);
    let diffuse = irradiance * sh.base * (1.0 - sh.metallic) * (1.0 - sh.transmission) * (vec3(1.0) - specular);
    var color = (diffuse + reflected * frame.environment.x * specular) * sheen_scaling(sh.sheen_color)
        + sh.sheen_color * irradiance * 0.157;
    if sh.clearcoat > 0.0 {
        let coat_n_dot_v = max(dot(sh.coat, sh.v), 1e-4);
        let coat_fresnel = (0.04 + 0.96 * pow(1.0 - coat_n_dot_v, 5.0)) * sh.clearcoat;
        let coat_lod = sh.clearcoat_roughness * frame.environment.z;
        let coat = textureSampleLevel(environment_map, environment_sampler, reflect(-sh.v, sh.coat), coat_lod).rgb;
        color = color * (1.0 - coat_fresnel) + coat * frame.environment.x * coat_fresnel;
    }
    return color;
}

// What a transmissive surface shows of the opaque scene behind it, bent by refraction.
fn transmitted(sh: Shading, world: vec3<f32>, ior: f32, thickness: f32) -> vec3<f32> {
    if frame.time.y < 0.5 {
        return vec3(0.0);
    }
    let refracted = refract(-sh.v, sh.n, 1.0 / max(ior, 1.0));
    let exit = world + refracted * thickness;
    let clip = frame.view_proj * vec4(exit, 1.0);
    let uv = clip.xy / clip.w * vec2(0.5, -0.5) + 0.5;
    let spread = sh.roughness * sh.roughness * 0.05;
    var behind = vec3(0.0);
    for (var i = 0; i < 4; i++) {
        let angle = f32(i) * 1.5707963 + 0.785398;
        behind += textureSampleLevel(transmission_map, transmission_sampler, uv + vec2(cos(angle), sin(angle)) * spread, 0.0).rgb;
    }
    let n_dot_v = max(dot(sh.n, sh.v), 1e-4);
    let fresnel = 0.04 + 0.96 * pow(1.0 - n_dot_v, 5.0);
    return behind * 0.25 * sh.base * sh.transmission * (1.0 - fresnel);
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
    var geometric = normalize(in.normal);
    var t = in.tangent.xyz - geometric * dot(geometric, in.tangent.xyz);
    var b = cross(geometric, t) * in.tangent.w;
    if !front {
        geometric = -geometric;
        t = -t;
        b = -b;
    }
    // Called before any branch, since sampling textures needs uniform control flow.
    let s = surface(SurfaceInput(in.uv, in.world, geometric, in.color));
    let base = s.base_color;
    var alpha = base.a;
    if draw.alpha.x < 0.5 || s.transmission > 0.0 {
        alpha = 1.0;
    } else if draw.alpha.x < 1.5 {
        if base.a < draw.alpha.y {
            discard;
        }
        alpha = 1.0;
    }
    var color = base.rgb;
    if draw.params.z < 0.5 {
        if dot(t, t) > 1e-8 {
            t = normalize(t);
            b = normalize(b);
        } else {
            t = normalize(cross(select(vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), abs(geometric.y) < 0.99), geometric));
            b = cross(geometric, t);
        }
        let n = normalize(mat3x3(t, b, geometric) * s.normal);
        let along = t * s.anisotropy_direction.x + b * s.anisotropy_direction.y;
        let across = normalize(cross(n, along));
        let sh = Shading(
            n,
            normalize(frame.camera_position.xyz - in.world),
            geometric,
            cross(across, n),
            across,
            base.rgb,
            clamp(s.metallic, 0.0, 1.0),
            clamp(s.roughness, 0.0, 1.0),
            clamp(s.clearcoat, 0.0, 1.0),
            clamp(s.clearcoat_roughness, 0.0, 1.0),
            s.sheen_color,
            clamp(s.sheen_roughness, 0.0, 1.0),
            clamp(s.transmission, 0.0, 1.0),
            clamp(s.anisotropy, -1.0, 1.0),
        );
        let ambient = frame.ambient.rgb + mix(frame.ground.rgb, frame.sky.rgb, n.y * 0.5 + 0.5);
        color = ambient * base.rgb * s.occlusion * (1.0 - sh.transmission) + s.emissive;
        if frame.environment.y > 0.5 {
            color += environment_light(sh) * s.occlusion;
        }
        if sh.transmission > 0.0 {
            color += transmitted(sh, in.world, s.ior, s.thickness);
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
            color += shade(sh, l, radiance * lit);
        }
    }
    return vec4(finish(color, fog_amount(in.world)), alpha);
}
