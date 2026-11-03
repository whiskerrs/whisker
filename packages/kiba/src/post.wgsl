override ENCODE_SRGB: bool = false;

@group(0) @binding(0) var scene: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(index & 2u) * 2 - 1);
    let y = f32(i32(index & 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let hdr = textureLoad(scene, vec2<i32>(position.xy), 0).rgb;
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
