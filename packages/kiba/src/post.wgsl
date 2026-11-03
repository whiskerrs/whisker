override ENCODE_SRGB: bool = false;

struct Parameters {
    // Bloom strength, ambient occlusion strength, and sharpening, then the output size.
    amounts: vec4<f32>,
    output_size: vec4<f32>,
}

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var bloom: texture_2d<f32>;
@group(0) @binding(3) var occlusion: texture_2d<f32>;
@group(0) @binding(4) var<uniform> parameters: Parameters;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(index & 2u) * 2 - 1);
    let y = f32(i32(index & 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn scene_at(uv: vec2<f32>, x: f32, y: f32, texel: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(scene, linear_sampler, uv + vec2<f32>(x, y) * texel, 0.0).rgb;
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = position.xy / parameters.output_size.xy;
    let texel = 1.0 / vec2<f32>(textureDimensions(scene));
    var hdr = scene_at(uv, 0.0, 0.0, texel);

    // Sharpening recovers some of the crispness lost to a lower internal resolution.
    let sharpen = parameters.amounts.z;
    if sharpen > 0.0 {
        let around = scene_at(uv, -1.0, 0.0, texel) + scene_at(uv, 1.0, 0.0, texel) + scene_at(uv, 0.0, -1.0, texel) + scene_at(uv, 0.0, 1.0, texel);
        hdr = max(hdr + (hdr - around / 4.0) * sharpen, vec3<f32>(0.0));
    }

    let strength = parameters.amounts.y;
    if strength > 0.0 {
        let step = 1.0 / vec2<f32>(textureDimensions(occlusion));
        var open = 0.0;
        for (var y = -1.0; y <= 1.0; y += 2.0) {
            for (var x = -1.0; x <= 1.0; x += 2.0) {
                open += textureSampleLevel(occlusion, linear_sampler, uv + vec2<f32>(x, y) * step, 0.0).r;
            }
        }
        hdr *= mix(1.0, open / 4.0, strength);
    }

    let glow = parameters.amounts.x;
    if glow > 0.0 {
        hdr += textureSampleLevel(bloom, linear_sampler, uv, 0.0).rgb * glow;
    }

    let mapped = tone_map(hdr);
    if ENCODE_SRGB {
        return vec4<f32>(linear_to_srgb(mapped), 1.0);
    }
    return vec4<f32>(mapped, 1.0);
}

// Narkowicz's fit of the ACES curve, cheap enough for the lowest tier.
fn tone_map(color: vec3<f32>) -> vec3<f32> {
    let x = color * 0.6;
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn linear_to_srgb(linear: vec3<f32>) -> vec3<f32> {
    let low = linear * 12.92;
    let high = 1.055 * pow(linear, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, linear <= vec3<f32>(0.0031308));
}
