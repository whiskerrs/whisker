const SAMPLES: u32 = 128;
const PI: f32 = 3.14159265;
// The sun's angular radius in the sky, about a quarter of a degree.
const SUN_DISK_COS: f32 = 0.99999;

struct Sky {
    inverse_view_projection: mat4x4<f32>,
    camera_position: vec4<f32>,
    to_sun: vec4<f32>,
    sun_color: vec4<f32>,
    zenith: vec4<f32>,
    horizon: vec4<f32>,
    ground: vec4<f32>,
    // Cube face, then roughness or -1 for irradiance.
    bake: vec4<f32>,
}

@group(0) @binding(0) var<uniform> sky: Sky;

struct Clip {
    @builtin(position) position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> Clip {
    let ndc = vec2<f32>(f32(index & 1u) * 4.0 - 1.0, f32(index & 2u) * 2.0 - 1.0);
    // Zero is the far plane under reversed depth, so anything drawn earlier stays in front.
    return Clip(vec4<f32>(ndc, 0.0, 1.0), ndc);
}

fn gradient(direction: vec3<f32>) -> vec3<f32> {
    let up = direction.y;
    let above = mix(sky.horizon.rgb, sky.zenith.rgb, 1.0 - pow(1.0 - max(up, 0.0), 4.0));
    return mix(sky.ground.rgb, above, smoothstep(-0.02, 0.0, up));
}

@fragment
fn fs_sky(input: Clip) -> @location(0) vec4<f32> {
    let far = sky.inverse_view_projection * vec4<f32>(input.ndc, 0.5, 1.0);
    let direction = normalize(far.xyz / far.w - sky.camera_position.xyz);
    let to_sun = dot(direction, sky.to_sun.xyz);
    let glow = pow(max(to_sun, 0.0), 512.0) * 0.5 + pow(max(to_sun, 0.0), 16.0) * 0.08;
    let disk = smoothstep(SUN_DISK_COS - 1e-5, SUN_DISK_COS, to_sun) * 40.0;
    let color = gradient(direction) + sky.sun_color.rgb * (glow + disk) * step(0.0, direction.y);
    return vec4<f32>(color * sky.camera_position.w, 1.0);
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

fn hammersley(i: u32) -> vec2<f32> {
    return vec2<f32>(f32(i) / f32(SAMPLES), f32(reverseBits(i)) * 2.3283064365386963e-10);
}

// The sun is left out, because surfaces already receive it directly.
@fragment
fn fs_bake(input: Clip) -> @location(0) vec4<f32> {
    let normal = face_direction(u32(sky.bake.x), input.ndc);
    let up = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(normal.y) > 0.9);
    let tangent = normalize(cross(up, normal));
    let bitangent = cross(normal, tangent);
    let roughness = sky.bake.y;

    var sum = vec3<f32>(0.0);
    var weight = 0.0;
    for (var i = 0u; i < SAMPLES; i++) {
        let xi = hammersley(i);
        let phi = 2.0 * PI * xi.x;
        if roughness < 0.0 {
            // Cosine-distributed samples average to what a white diffuse surface receives.
            let radius = sqrt(xi.y);
            let local = vec3<f32>(radius * cos(phi), radius * sin(phi), sqrt(1.0 - xi.y));
            sum += gradient(tangent * local.x + bitangent * local.y + normal * local.z);
            weight += 1.0;
        } else {
            let alpha = roughness * roughness;
            let cos_theta = sqrt((1.0 - xi.y) / (1.0 + (alpha * alpha - 1.0) * xi.y));
            let sin_theta = sqrt(1.0 - cos_theta * cos_theta);
            let halfway = tangent * sin_theta * cos(phi) + bitangent * sin_theta * sin(phi) + normal * cos_theta;
            let light = reflect(-normal, halfway);
            let n_dot_l = dot(normal, light);
            if n_dot_l > 0.0 {
                sum += gradient(light) * n_dot_l;
                weight += n_dot_l;
            }
        }
    }
    return vec4<f32>(sum / max(weight, 1e-4), 1.0);
}
