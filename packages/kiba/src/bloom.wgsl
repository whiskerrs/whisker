@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;

struct Clip {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> Clip {
    let uv = vec2<f32>(f32(index & 1u) * 2.0, f32(index & 2u));
    return Clip(vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0), uv);
}

// Below this, light stays out of bloom, which keeps ordinary sunlit surfaces from glowing.
const THRESHOLD: f32 = 1.0;

fn tap(uv: vec2<f32>, x: f32, y: f32, texel: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(source, linear_sampler, uv + vec2<f32>(x, y) * texel, 0.0).rgb;
}

// Four bilinear taps, which is the lightest filter that does not shimmer as highlights move.
fn down(uv: vec2<f32>) -> vec3<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(source));
    return (tap(uv, -1.0, -1.0, texel) + tap(uv, 1.0, -1.0, texel) + tap(uv, -1.0, 1.0, texel) + tap(uv, 1.0, 1.0, texel)) / 4.0;
}

@fragment
fn fs_first(input: Clip) -> @location(0) vec4<f32> {
    let color = min(down(input.uv), vec3<f32>(64.0));
    let brightness = max(color.r, max(color.g, color.b));
    return vec4<f32>(color * max(brightness - THRESHOLD, 0.0) / max(brightness, 1e-4), 1.0);
}

@fragment
fn fs_down(input: Clip) -> @location(0) vec4<f32> {
    return vec4<f32>(down(input.uv), 1.0);
}

@fragment
fn fs_up(input: Clip) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(source));
    let sum = tap(input.uv, -1.0, 0.0, texel) + tap(input.uv, 1.0, 0.0, texel) + tap(input.uv, 0.0, -1.0, texel) + tap(input.uv, 0.0, 1.0, texel);
    return vec4<f32>(sum / 4.0, 1.0);
}
