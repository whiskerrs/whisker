const PI: f32 = 3.14159265;
const SAMPLES: u32 = 64u;

struct Params {
    // Each capture layer's view and projection, without the probe's position.
    faces: array<mat4x4<f32>, 6>,
    // The layer or face drawn, then roughness, then one when a sky fills what the capture missed.
    pass_info: vec4<f32>,
    // Light where nothing was captured and there is no sky.
    ambient: vec4<f32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var source: texture_2d_array<f32>;
@group(0) @binding(2) var linear_sampler: sampler;
@group(0) @binding(3) var sky: texture_cube<f32>;

struct Clip {
    @builtin(position) position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> Clip {
    let ndc = vec2<f32>(f32(index & 1u) * 4.0 - 1.0, f32(index & 2u) * 2.0 - 1.0);
    return Clip(vec4<f32>(ndc, 0.0, 1.0), ndc);
}

// Reading between four source texels at once averages them, which halves the layer.
@fragment
fn fs_downsample(input: Clip) -> @location(0) vec4<f32> {
    let uv = input.ndc * vec2<f32>(0.5, -0.5) + 0.5;
    return textureSampleLevel(source, linear_sampler, uv, i32(params.pass_info.x), 0.0);
}

fn face_direction(face: u32, ndc: vec2<f32>) -> vec3<f32> {
    // The texel at (s, t), with t growing down the target, as the cube map addressing rule defines.
    let s = ndc.x;
    let t = -ndc.y;
    switch face {
        case 0u: { return normalize(vec3<f32>(1.0, -t, -s)); }
        case 1u: { return normalize(vec3<f32>(-1.0, -t, s)); }
        case 2u: { return normalize(vec3<f32>(s, 1.0, t)); }
        case 3u: { return normalize(vec3<f32>(s, -1.0, -t)); }
        case 4u: { return normalize(vec3<f32>(s, -t, 1.0)); }
        default: { return normalize(vec3<f32>(-s, -t, -1.0)); }
    }
}

// Layers are drawn by ordinary cameras, which a cube map's mirrored faces cannot be, so lookups project by hand.
fn captured(direction: vec3<f32>, lod: f32) -> vec3<f32> {
    let a = abs(direction);
    var layer = 0u;
    if a.x >= a.y && a.x >= a.z {
        layer = select(1u, 0u, direction.x > 0.0);
    } else if a.y >= a.z {
        layer = select(3u, 2u, direction.y > 0.0);
    } else {
        layer = select(5u, 4u, direction.z > 0.0);
    }
    let clip = params.faces[layer] * vec4<f32>(direction, 1.0);
    let uv = clip.xy / clip.w * vec2<f32>(0.5, -0.5) + 0.5;
    let texel = textureSampleLevel(source, linear_sampler, uv, layer, lod);
    var behind = params.ambient.rgb;
    if params.pass_info.z != 0.0 {
        behind = textureSampleLevel(sky, linear_sampler, direction, 0.0).rgb;
    }
    // Geometry wrote an alpha of one, and shrinking kept color premultiplied by it.
    return texel.rgb + (1.0 - texel.a) * behind;
}

fn hammersley(i: u32) -> vec2<f32> {
    return vec2<f32>(f32(i) / f32(SAMPLES), f32(reverseBits(i)) * 2.3283064365386963e-10);
}

@fragment
fn fs_prefilter(input: Clip) -> @location(0) vec4<f32> {
    let normal = face_direction(u32(params.pass_info.x), input.ndc);
    let roughness = params.pass_info.y;
    if roughness == 0.0 {
        return vec4<f32>(captured(normal, 0.0), 1.0);
    }
    let up = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(normal.y) > 0.9);
    let tangent = normalize(cross(up, normal));
    let bitangent = cross(normal, tangent);
    let alpha = roughness * roughness;
    let side = f32(textureDimensions(source).x);
    let texel_angle = 4.0 * PI / (6.0 * side * side);
    let last_lod = f32(textureNumLevels(source) - 1);

    var sum = vec3<f32>(0.0);
    var weight = 0.0;
    for (var i = 0u; i < SAMPLES; i++) {
        let xi = hammersley(i);
        let phi = 2.0 * PI * xi.x;
        let cos_theta = sqrt((1.0 - xi.y) / (1.0 + (alpha * alpha - 1.0) * xi.y));
        let sin_theta = sqrt(1.0 - cos_theta * cos_theta);
        let halfway = tangent * sin_theta * cos(phi) + bitangent * sin_theta * sin(phi) + normal * cos_theta;
        let light = reflect(-normal, halfway);
        let n_dot_l = dot(normal, light);
        if n_dot_l > 0.0 {
            // Each sample reads a mip as wide as the share of the sphere it stands for, so few samples do not speckle.
            let denominator = cos_theta * cos_theta * (alpha * alpha - 1.0) + 1.0;
            let pdf = alpha * alpha / (PI * denominator * denominator) / 4.0;
            let sample_angle = 1.0 / (f32(SAMPLES) * pdf);
            let lod = clamp(0.5 * log2(sample_angle / texel_angle) + 1.0, 0.0, last_lod);
            sum += captured(light, lod) * n_dot_l;
            weight += n_dot_l;
        }
    }
    return vec4<f32>(sum / max(weight, 1e-4), 1.0);
}
