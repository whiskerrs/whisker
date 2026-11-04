override ENCODE_SRGB: bool = false;
const FOCUS_TAPS: u32 = 24u;
const SHAFT_TAPS: u32 = 32u;

struct Parameters {
    // Bloom strength, ambient occlusion strength, sharpening, then one for antialiasing.
    amounts: vec4<f32>,
    output_size: vec4<f32>,
    // The distance in focus, then the widest blur in scene pixels, zero for none.
    focus: vec4<f32>,
    // The sun's place on screen, then the strength of its shafts, zero for none.
    sun: vec4<f32>,
    // Linear RGB light of the sun, which colors its shafts.
    sun_color: vec4<f32>,
    inverse_projection: mat4x4<f32>,
}

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var bloom: texture_2d<f32>;
@group(0) @binding(3) var occlusion: texture_2d<f32>;
@group(0) @binding(4) var<uniform> parameters: Parameters;
@group(0) @binding(5) var depth: texture_depth_2d;

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
    let center = scene_at(uv, 0.0, 0.0, texel);
    var hdr = center;
    if parameters.amounts.w != 0.0 {
        hdr = antialiased(uv, center, texel);
    }

    // Sharpening recovers some of the crispness lost to a lower internal resolution.
    let sharpen = parameters.amounts.z;
    if sharpen > 0.0 {
        let around = scene_at(uv, -1.0, 0.0, texel) + scene_at(uv, 1.0, 0.0, texel) + scene_at(uv, 0.0, -1.0, texel) + scene_at(uv, 0.0, 1.0, texel);
        hdr = max(hdr + (center - around / 4.0) * sharpen, vec3<f32>(0.0));
    }

    if parameters.focus.y > 0.0 {
        hdr = focused(uv, hdr, position.xy);
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

    if parameters.sun.z > 0.0 {
        hdr += shafts(uv);
    }

    let mapped = tone_map(hdr);
    if ENCODE_SRGB {
        return vec4<f32>(linear_to_srgb(mapped), 1.0);
    }
    return vec4<f32>(mapped, 1.0);
}

fn luma(color: vec3<f32>) -> f32 {
    return dot(tone_map(color), vec3<f32>(0.299, 0.587, 0.114));
}

// FXAA: blends across an edge along the direction its brightness changes least, judged after tone mapping.
fn antialiased(uv: vec2<f32>, center: vec3<f32>, texel: vec2<f32>) -> vec3<f32> {
    let north_west = luma(scene_at(uv, -1.0, -1.0, texel));
    let north_east = luma(scene_at(uv, 1.0, -1.0, texel));
    let south_west = luma(scene_at(uv, -1.0, 1.0, texel));
    let south_east = luma(scene_at(uv, 1.0, 1.0, texel));
    let middle = luma(center);
    let lowest = min(middle, min(min(north_west, north_east), min(south_west, south_east)));
    let highest = max(middle, max(max(north_west, north_east), max(south_west, south_east)));
    if highest - lowest < max(0.0312, highest * 0.125) {
        return center;
    }
    var along = vec2<f32>(-((north_west + north_east) - (south_west + south_east)), (north_west + south_west) - (north_east + south_east));
    let reduce = max((north_west + north_east + south_west + south_east) * 0.03125, 1.0 / 128.0);
    along = clamp(along / (min(abs(along.x), abs(along.y)) + reduce), vec2<f32>(-8.0), vec2<f32>(8.0)) * texel;
    let near = 0.5 * (textureSampleLevel(scene, linear_sampler, uv - along / 6.0, 0.0).rgb + textureSampleLevel(scene, linear_sampler, uv + along / 6.0, 0.0).rgb);
    let far = near * 0.5 + 0.25 * (textureSampleLevel(scene, linear_sampler, uv - along * 0.5, 0.0).rgb + textureSampleLevel(scene, linear_sampler, uv + along * 0.5, 0.0).rgb);
    let far_luma = luma(far);
    // Reaching past the edge picks up a third shade, which shows as a halo.
    if far_luma < lowest || far_luma > highest {
        return near;
    }
    return far;
}

fn depth_size() -> vec2<i32> {
    return vec2<i32>(textureDimensions(depth));
}

fn distance_at(pixel: vec2<i32>) -> f32 {
    let size = depth_size();
    let at = clamp(pixel, vec2<i32>(0), size - 1);
    let ndc = (vec2<f32>(at) + 0.5) / vec2<f32>(size) * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0);
    let view = parameters.inverse_projection * vec4<f32>(ndc, textureLoad(depth, at, 0), 1.0);
    return length(view.xyz / view.w);
}

// The blur radius in scene pixels, as a thin lens would give it.
fn blur_at(pixel: vec2<i32>) -> f32 {
    let widest = parameters.focus.y;
    return min(widest * abs(1.0 - parameters.focus.x / max(distance_at(pixel), 1e-3)), widest);
}

// Gathers the blur discs that reach this pixel, so a blurred foreground spills over a sharp background.
fn focused(uv: vec2<f32>, sharp: vec3<f32>, screen: vec2<f32>) -> vec3<f32> {
    let size = vec2<f32>(depth_size());
    let pixel = vec2<i32>(uv * size);
    let own = blur_at(pixel);
    let own_distance = distance_at(pixel);
    let texel = 1.0 / size;
    let turn = fract(52.9829189 * fract(dot(screen, vec2<f32>(0.06711056, 0.00583715)))) * 6.2831853;
    var sum = sharp;
    var weight = 1.0;
    for (var i = 0u; i < FOCUS_TAPS; i++) {
        let radius = sqrt((f32(i) + 0.5) / f32(FOCUS_TAPS)) * parameters.focus.y;
        let angle = f32(i) * 2.39996323 + turn;
        let offset = vec2<f32>(cos(angle), sin(angle)) * radius;
        let tap = pixel + vec2<i32>(round(offset));
        // What lies behind this pixel cannot blur over it by more than this pixel's own blur.
        var reach = blur_at(tap);
        if distance_at(tap) > own_distance {
            reach = min(reach, own);
        }
        let counts = saturate(reach - radius + 1.0);
        sum += textureSampleLevel(scene, linear_sampler, uv + offset * texel, 0.0).rgb * counts;
        weight += counts;
    }
    return sum / weight;
}

// Glows around the sun as much as the way to it is clear, so its light streams past what stands in front of it.
// The glow takes the sun's color rather than the sky's, so a dim moon cannot wash a dark sky out.
fn shafts(uv: vec2<f32>) -> vec3<f32> {
    let sun = parameters.sun.xy;
    let size = vec2<f32>(depth_size());
    let step = (sun - uv) / f32(SHAFT_TAPS);
    var clear = 0.0;
    var at = uv;
    for (var i = 0u; i < SHAFT_TAPS; i++) {
        at += step;
        if all(at >= vec2<f32>(0.0)) && all(at <= vec2<f32>(1.0)) && textureLoad(depth, vec2<i32>(at * size), 0) <= 0.0 {
            clear += 1.0;
        }
    }
    let glow = exp(-length((uv - sun) * vec2<f32>(size.x / size.y, 1.0)) * 5.0);
    // Fades out as the sun leaves the screen, where its gaps can no longer be seen.
    let off_screen = length(max(abs(sun - 0.5) - 0.5, vec2<f32>(0.0)));
    return clear / f32(SHAFT_TAPS) * glow * parameters.sun_color.rgb * 2.0 * parameters.sun.z
        * saturate(1.0 - off_screen * 4.0);
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
