struct View {
    view_projection: mat4x4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;

struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) offset: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn vs_main(
    @builtin(vertex_index) index: u32,
    @location(0) center: vec3<f32>,
    @location(1) size: f32,
    @location(2) color: vec4<f32>,
) -> Output {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let offset = corners[index];
    let world = center + (view.right.xyz * offset.x + view.up.xyz * offset.y) * size;
    return Output(view.view_projection * vec4<f32>(world, 1.0), offset, color);
}

@fragment
fn fs_main(input: Output) -> @location(0) vec4<f32> {
    let falloff = saturate(1.0 - dot(input.offset, input.offset));
    return vec4<f32>(input.color.rgb * input.color.a * falloff * falloff, 0.0);
}
