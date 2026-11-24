struct Post {
    projection: mat4x4<f32>,
    inverse_projection: mat4x4<f32>,
    // x threshold, y soft knee, z intensity, w upsampling spread in texels.
    bloom: vec4<f32>,
    // x radius, y intensity.
    occlusion: vec4<f32>,
    // x focus distance, y focus range, z widest blur radius in pixels, w 1 when enabled.
    focus: vec4<f32>,
    // x exposure, y tone curve, z lookup table size or 0, w 1 when bloom is enabled.
    tone: vec4<f32>,
    // x 1 when the composite writes sRGB-encoded values, y the same for FXAA; z 1 with occlusion.
    output: vec4<f32>,
};

@group(0) @binding(0) var<uniform> post: Post;
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var bloom_map: texture_2d<f32>;
@group(0) @binding(3) var occlusion_map: texture_2d<f32>;
@group(0) @binding(4) var lookup: texture_2d<f32>;
@group(0) @binding(5) var depth_map: texture_2d<f32>;
@group(0) @binding(6) var linear_sampler: sampler;

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) index: u32) -> Out {
    let corner = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    var out: Out;
    out.position = vec4(corner * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2(corner.x, 1.0 - corner.y);
    return out;
}

fn fetch(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(source, linear_sampler, uv, 0.0).rgb;
}

// Jimenez's 13-tap downsample, which keeps small bright spots from flickering as they move.
fn downsample(uv: vec2<f32>) -> vec3<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(source));
    let a = fetch(uv + t * vec2(-2.0, -2.0));
    let b = fetch(uv + t * vec2(0.0, -2.0));
    let c = fetch(uv + t * vec2(2.0, -2.0));
    let d = fetch(uv + t * vec2(-2.0, 0.0));
    let e = fetch(uv);
    let f = fetch(uv + t * vec2(2.0, 0.0));
    let g = fetch(uv + t * vec2(-2.0, 2.0));
    let h = fetch(uv + t * vec2(0.0, 2.0));
    let i = fetch(uv + t * vec2(2.0, 2.0));
    let j = fetch(uv + t * vec2(-1.0, -1.0));
    let k = fetch(uv + t * vec2(1.0, -1.0));
    let l = fetch(uv + t * vec2(-1.0, 1.0));
    let m = fetch(uv + t * vec2(1.0, 1.0));
    return e * 0.125 + (a + c + g + i) * 0.03125 + (b + d + f + h) * 0.0625 + (j + k + l + m) * 0.125;
}

@fragment
fn fs_bloom_prefilter(in: Out) -> @location(0) vec4<f32> {
    let color = downsample(in.uv);
    let brightness = max(color.r, max(color.g, color.b));
    let knee = post.bloom.y;
    var soft = clamp(brightness - post.bloom.x + knee, 0.0, 2.0 * knee);
    soft = soft * soft / (4.0 * knee + 1e-5);
    let kept = max(soft, brightness - post.bloom.x) / max(brightness, 1e-5);
    return vec4(color * kept, 1.0);
}

@fragment
fn fs_bloom_down(in: Out) -> @location(0) vec4<f32> {
    return vec4(downsample(in.uv), 1.0);
}

// A 3×3 tent, added onto the larger level below by blending.
@fragment
fn fs_bloom_up(in: Out) -> @location(0) vec4<f32> {
    let t = post.bloom.w / vec2<f32>(textureDimensions(source));
    var sum = fetch(uv_offset(in.uv, t, -1.0, -1.0)) + fetch(uv_offset(in.uv, t, 1.0, -1.0));
    sum += fetch(uv_offset(in.uv, t, -1.0, 1.0)) + fetch(uv_offset(in.uv, t, 1.0, 1.0));
    sum += 2.0 * (fetch(uv_offset(in.uv, t, 0.0, -1.0)) + fetch(uv_offset(in.uv, t, 0.0, 1.0)));
    sum += 2.0 * (fetch(uv_offset(in.uv, t, -1.0, 0.0)) + fetch(uv_offset(in.uv, t, 1.0, 0.0)));
    sum += 4.0 * fetch(in.uv);
    return vec4(sum / 16.0, 1.0);
}

fn uv_offset(uv: vec2<f32>, texel: vec2<f32>, x: f32, y: f32) -> vec2<f32> {
    return uv + texel * vec2(x, y);
}

fn depth_at(pixel: vec2<i32>) -> f32 {
    let size = vec2<i32>(textureDimensions(depth_map));
    return textureLoad(depth_map, clamp(pixel, vec2(0), size - 1), 0).r;
}

fn view_position(pixel: vec2<i32>, depth: f32) -> vec3<f32> {
    let size = vec2<f32>(textureDimensions(depth_map));
    let uv = (vec2<f32>(pixel) + 0.5) / size;
    let ndc = vec4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
    let view = post.inverse_projection * ndc;
    return view.xyz / view.w;
}

// A per-pixel angle that differs between neighbors, so the blur pass averages the samples' pattern away.
fn interleaved_noise(pixel: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(pixel, vec2(0.06711056, 0.00583715))));
}

const OCCLUSION_SAMPLES: i32 = 12;

// Scalable ambient obscurance (McGuire et al. 2012) over the depth prepass.
@fragment
fn fs_occlusion(in: Out) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(in.position.xy);
    let depth = depth_at(pixel);
    if depth >= 1.0 {
        return vec4(1.0);
    }
    let p = view_position(pixel, depth);
    let right = view_position(pixel + vec2(1, 0), depth_at(pixel + vec2(1, 0)));
    let down = view_position(pixel + vec2(0, 1), depth_at(pixel + vec2(0, 1)));
    var n = normalize(cross(right - p, down - p));
    if dot(n, p) > 0.0 {
        n = -n;
    }
    let radius = post.occlusion.x;
    let size = vec2<f32>(textureDimensions(depth_map));
    // Perspective projections scale offsets by distance; orthographic ones have a w column of 1.
    let w = select(-p.z, 1.0, post.projection[3][3] > 0.5);
    let reach = radius * post.projection[1][1] * 0.5 * size.y / max(w, 1e-4);
    let spin = interleaved_noise(in.position.xy) * 6.2831853;
    var sum = 0.0;
    for (var i = 0; i < OCCLUSION_SAMPLES; i++) {
        let t = (f32(i) + 0.5) / f32(OCCLUSION_SAMPLES);
        let angle = spin + f32(i) * 2.39996323;
        let offset = vec2(cos(angle), sin(angle)) * t * reach;
        let q_pixel = pixel + vec2<i32>(offset);
        let q = view_position(q_pixel, depth_at(q_pixel));
        let v = q - p;
        let vv = dot(v, v);
        let f = max(radius * radius - vv, 0.0);
        sum += f * f * f * max((dot(v, n) - 0.01 * radius) / (vv + 0.01), 0.0);
    }
    let r6 = pow(radius, 6.0);
    let occlusion = max(0.0, 1.0 - sum * post.occlusion.y * 5.0 / (r6 * f32(OCCLUSION_SAMPLES)));
    return vec4(occlusion, occlusion, occlusion, 1.0);
}

// A 4×4 box from four bilinear taps.
@fragment
fn fs_blur(in: Out) -> @location(0) vec4<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(source));
    let sum = fetch(in.uv + t * vec2(-1.0, -1.0)) + fetch(in.uv + t * vec2(1.0, -1.0))
        + fetch(in.uv + t * vec2(-1.0, 1.0)) + fetch(in.uv + t * vec2(1.0, 1.0));
    return vec4(sum / 4.0, 1.0);
}

// The blur radius in pixels at `pixel`, and its view depth.
fn circle_of_confusion(pixel: vec2<i32>) -> vec2<f32> {
    let depth = depth_at(pixel);
    let distance = -view_position(pixel, depth).z;
    let range = max(post.focus.y, 1e-4);
    let off = clamp((abs(distance - post.focus.x) - range * 0.5) / range, 0.0, 1.0);
    return vec2(off * post.focus.z, distance);
}

const FOCUS_SAMPLES: i32 = 24;

// A gather blur where nearer samples spread by their own blur, and farther ones only as far as
// this pixel's, so blurred backgrounds never bleed over sharp foregrounds.
fn depth_of_field(uv: vec2<f32>, pixel: vec2<i32>) -> vec3<f32> {
    let center = circle_of_confusion(pixel);
    let size = vec2<f32>(textureDimensions(source));
    var sum = fetch(uv);
    var weight = 1.0;
    for (var i = 0; i < FOCUS_SAMPLES; i++) {
        let r = sqrt((f32(i) + 0.5) / f32(FOCUS_SAMPLES)) * post.focus.z;
        let angle = f32(i) * 2.39996323;
        let offset = vec2(cos(angle), sin(angle)) * r;
        let sample_pixel = pixel + vec2<i32>(offset);
        let other = circle_of_confusion(sample_pixel);
        let reach = select(center.x, other.x, other.y < center.y);
        let w = clamp(reach - r + 1.0, 0.0, 1.0);
        sum += fetch((vec2<f32>(sample_pixel) + 0.5) / size) * w;
        weight += w;
    }
    return sum / weight;
}

fn grade(encoded: vec3<f32>) -> vec3<f32> {
    let n = post.tone.z;
    let c = clamp(encoded, vec3(0.0), vec3(1.0)) * (n - 1.0);
    let slice = floor(c.b);
    let t = c.b - slice;
    let width = n * n;
    let uv = vec2((c.r + 0.5) / width, (c.g + 0.5) / n);
    let low = textureSampleLevel(lookup, linear_sampler, uv + vec2(slice * n / width, 0.0), 0.0).rgb;
    let high = textureSampleLevel(lookup, linear_sampler, uv + vec2(min(slice + 1.0, n - 1.0) * n / width, 0.0), 0.0).rgb;
    return mix(low, high, t);
}

@fragment
fn fs_composite(in: Out) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(in.position.xy);
    var color = fetch(in.uv);
    if post.focus.w > 0.5 {
        color = depth_of_field(in.uv, pixel);
    }
    if post.tone.w > 0.5 {
        color += textureSampleLevel(bloom_map, linear_sampler, in.uv, 0.0).rgb * post.bloom.z;
    }
    if post.output.z > 0.5 {
        color *= textureSampleLevel(occlusion_map, linear_sampler, in.uv, 0.0).r;
    }
    color = clamp(tone_curve(color, post.tone.x, i32(post.tone.y)), vec3(0.0), vec3(1.0));
    var encoded = encode_srgb(color);
    if post.tone.z > 0.5 {
        encoded = grade(encoded);
    }
    if post.output.x > 0.5 {
        return vec4(encoded, 1.0);
    }
    return vec4(decode_srgb(encoded), 1.0);
}

fn luma(color: vec3<f32>) -> f32 {
    return dot(color, vec3(0.299, 0.587, 0.114));
}

// FXAA in its compact form, over sRGB-encoded input.
@fragment
fn fs_fxaa(in: Out) -> @location(0) vec4<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(source));
    let nw = luma(fetch(in.uv + t * vec2(-1.0, -1.0)));
    let ne = luma(fetch(in.uv + t * vec2(1.0, -1.0)));
    let sw = luma(fetch(in.uv + t * vec2(-1.0, 1.0)));
    let se = luma(fetch(in.uv + t * vec2(1.0, 1.0)));
    let middle = fetch(in.uv);
    let m = luma(middle);
    let lowest = min(m, min(min(nw, ne), min(sw, se)));
    let highest = max(m, max(max(nw, ne), max(sw, se)));
    var direction = vec2(-((nw + ne) - (sw + se)), (nw + sw) - (ne + se));
    let reduce = max((nw + ne + sw + se) * 0.25 * (1.0 / 8.0), 1.0 / 128.0);
    let scale = 1.0 / (min(abs(direction.x), abs(direction.y)) + reduce);
    direction = clamp(direction * scale, vec2(-8.0), vec2(8.0)) * t;
    let a = 0.5 * (fetch(in.uv + direction * (1.0 / 3.0 - 0.5)) + fetch(in.uv + direction * (2.0 / 3.0 - 0.5)));
    let b = a * 0.5 + 0.25 * (fetch(in.uv - direction * 0.5) + fetch(in.uv + direction * 0.5));
    let lb = luma(b);
    var color = b;
    if lb < lowest || lb > highest {
        color = a;
    }
    if (highest - lowest) < 1.0 / 32.0 {
        color = middle;
    }
    if post.output.y > 0.5 {
        return vec4(color, 1.0);
    }
    return vec4(decode_srgb(color), 1.0);
}
