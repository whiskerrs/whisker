struct Draw {
    model: mat4x4<f32>,
    normal: mat4x4<f32>,
    base_color: vec4<f32>,
    emissive: vec4<f32>,
    params: vec4<f32>,
    uv_x: vec4<f32>,
    uv_y: vec4<f32>,
    // x 0 opaque, 1 mask, 2 blend; y mask cutoff.
    alpha: vec4<f32>,
};

@group(0) @binding(0) var<uniform> light_view_proj: mat4x4<f32>;
@group(1) @binding(0) var<uniform> draw: Draw;
@group(2) @binding(0) var base_texture: texture_2d<f32>;
@group(2) @binding(5) var material_sampler: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(4) color: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
};

@vertex
fn vs(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = light_view_proj * draw.model * vec4(in.position, 1.0);
    let uv = vec3(in.uv, 1.0);
    out.uv = vec2(dot(draw.uv_x.xyz, uv), dot(draw.uv_y.xyz, uv));
    out.alpha = draw.base_color.a * in.color.a;
    return out;
}

@fragment
fn fs(in: VertexOut) {
    let alpha = in.alpha * textureSample(base_texture, material_sampler, in.uv).a;
    // Blended surfaces cast where they are mostly opaque, as if masked at one half.
    let cutoff = select(0.5, draw.alpha.y, draw.alpha.x < 1.5);
    if draw.alpha.x > 0.5 && alpha < cutoff {
        discard;
    }
}
