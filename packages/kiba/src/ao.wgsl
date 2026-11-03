struct Parameters {
    // Maps a depth and screen position back to view space.
    inverse_projection: mat4x4<f32>,
    projection: mat4x4<f32>,
}

@group(0) @binding(0) var depth: texture_depth_2d;
@group(0) @binding(1) var<uniform> parameters: Parameters;

const SAMPLES: u32 = 8u;
// In world units, roughly the size of the creases worth darkening.
const RADIUS: f32 = 0.6;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32(index & 1u) * 2.0, f32(index & 2u));
    return vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}

// Takes whole texels, because a texel's coordinate sent through a fraction of an odd size can
// round onto its neighbor and collapse the normal into dark rows.
fn view_position(texel: vec2<i32>) -> vec3<f32> {
    let size = vec2<i32>(textureDimensions(depth));
    let clamped = clamp(texel, vec2<i32>(0), size - 1);
    let stored = textureLoad(depth, clamped, 0);
    let uv = (vec2<f32>(clamped) + 0.5) / vec2<f32>(size);
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, stored, 1.0);
    let view = parameters.inverse_projection * ndc;
    return view.xyz / view.w;
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(depth));
    // This pass draws at half the size of the depth it reads.
    let texel = vec2<i32>(position.xy * 2.0);
    // Zero is the far plane under reversed depth, where only the sky is, and it has no position.
    if textureLoad(depth, texel, 0) <= 0.0 {
        return vec4<f32>(1.0);
    }
    let here = view_position(texel);
    // The last row and column look back instead, since a clamped neighbor would be the texel itself.
    let inward = select(vec2<i32>(1), vec2<i32>(-1), texel + 1 >= vec2<i32>(size));
    let across = (view_position(texel + vec2<i32>(inward.x, 0)) - here) * f32(inward.x);
    let along = (view_position(texel + vec2<i32>(0, inward.y)) - here) * f32(inward.y);
    let normal = normalize(cross(along, across));
    // Interleaved gradient noise spreads the few samples into a pattern the blur can remove.
    let noise = fract(52.9829189 * fract(dot(position.xy, vec2<f32>(0.06711056, 0.00583715)))) * 6.2831853;
    var occluded = 0.0;
    for (var i = 0u; i < SAMPLES; i++) {
        let angle = noise + f32(i) * 2.39996323;
        let reach = RADIUS * sqrt((f32(i) + 0.5) / f32(SAMPLES));
        let clip = parameters.projection * vec4<f32>(here + vec3<f32>(cos(angle), sin(angle), 0.0) * reach, 1.0);
        let other = view_position(vec2<i32>(floor((vec2<f32>(clip.x, -clip.y) / clip.w * 0.5 + 0.5) * size)));
        let toward = other - here;
        let length_now = length(toward);
        if length_now > 1e-4 && length_now < RADIUS * 2.0 {
            occluded += max(dot(toward / length_now, normal) - 0.1, 0.0);
        }
    }
    return vec4<f32>(1.0 - saturate(occluded * 2.0 / f32(SAMPLES)));
}
