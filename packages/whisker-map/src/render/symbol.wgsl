struct Frame {
    viewport: vec4<f32>,
    light: vec4<f32>,
    light_color: vec4<f32>,
};

struct Draw {
    matrix: mat4x4<f32>,
    color: vec4<f32>,
    // x and y the texture's size in texels.
    params: vec4<f32>,
    extra: vec4<f32>,
    color2: vec4<f32>,
    dash0: vec4<f32>,
    dash1: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var<uniform> draw: Draw;
@group(2) @binding(0) var atlas: texture_2d<f32>;
@group(2) @binding(1) var atlas_sampler: sampler;

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3(2.4));
    return select(high, low, c <= vec3(0.04045));
}

fn finish(c: vec4<f32>) -> vec4<f32> {
    if frame.viewport.w < 0.5 || c.a <= 0.0 {
        return c;
    }
    return vec4(to_linear(c.rgb / c.a) * c.a, c.a);
}

struct SymbolOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) halo: vec4<f32>,
    @location(3) params: vec4<f32>,
    @location(4) extra: vec2<f32>,
};

@vertex
fn vs_symbol(
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) halo: vec4<f32>,
    @location(4) params: vec4<f32>,
    @location(5) extra: vec2<f32>,
) -> SymbolOut {
    var out: SymbolOut;
    let ndc = pos / frame.viewport.xy * vec2(2.0, -2.0) + vec2(-1.0, 1.0);
    out.clip = vec4(ndc, 0.0, 1.0);
    out.uv = uv / draw.params.xy;
    out.color = color;
    out.halo = halo;
    out.params = params;
    out.extra = extra;
    return out;
}

// MapLibre's symbol_sdf shader: the glyph edge sits at 0.75 and SDF_PX is 8.
@fragment
fn fs_symbol(in: SymbolOut) -> @location(0) vec4<f32> {
    let sample = textureSample(atlas, atlas_sampler, in.uv);
    let opacity = in.params.w;
    if in.params.x < 0.5 {
        return finish(vec4(sample.rgb * sample.a, sample.a) * opacity);
    }
    let dist = sample.r;
    let font_scale = in.params.y;
    let halo_width = in.params.z;
    let halo_blur = in.extra.x;
    let edge_gamma = 0.105 / frame.viewport.z;
    let inner_edge = 0.75;
    let gamma = edge_gamma / font_scale;
    let fill = smoothstep(inner_edge - gamma, inner_edge + gamma, dist);
    var c = in.color * fill;
    if in.halo.a > 0.0 && halo_width > 0.0 {
        let halo_gamma = (halo_blur * 1.19 / 8.0 + edge_gamma) / font_scale;
        let halo_inner = inner_edge + halo_gamma;
        let inside = smoothstep(halo_inner - halo_gamma, halo_inner + halo_gamma, dist);
        let halo_edge = (6.0 - halo_width / font_scale) / 8.0;
        let halo = min(smoothstep(halo_edge - halo_gamma, halo_edge + halo_gamma, dist), 1.0 - inside);
        c = c + in.halo * halo * (1.0 - fill);
    }
    return finish(c * opacity);
}
