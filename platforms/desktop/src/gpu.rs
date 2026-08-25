use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use glyphon::{Cache, Resolution, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport};
use wgpu::util::DeviceExt;
use wgpu::{
    BlendState, Buffer, BufferUsages, ColorTargetState, ColorWrites, CommandEncoderDescriptor,
    CompositeAlphaMode, Device, DeviceDescriptor, FragmentState, Instance, InstanceDescriptor,
    LoadOp, MultisampleState, Operations, PipelineCompilationOptions, PresentMode, PrimitiveState,
    Queue, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, RequestAdapterOptions, ShaderModuleDescriptor, ShaderSource, StoreOp,
    Surface, SurfaceConfiguration, SurfaceError, SurfaceTarget, TextureFormat, TextureUsages,
    TextureViewDescriptor, VertexAttribute, VertexBufferLayout, VertexFormat, VertexState,
    VertexStepMode,
};
use whisker_protocol::{
    GradientStop, LayoutRect, NodeId, PaintBox, PaintImage, ResourceId, Transform,
};

use crate::paint::box_paint::{
    BoxPrimitive, BoxPrimitiveKind, background_gradient_primitive, box_shadow_primitive, lower_box,
    resolve_box_geometry,
};
use crate::paint::color::{gpu_color, text_color};
use crate::scene::{LogicalClip, PaintCommand, ShapeClipStack};
use crate::text::NativeTextHost;

const BOX_SHADER: &str = r#"
struct Viewport {
    logical_size: vec2<f32>,
    physical_size: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> viewport: Viewport;

struct ShapeClipRecord {
    rect: vec4<f32>,
    radii_x: vec4<f32>,
    radii_y: vec4<f32>,
    inverse_transform: mat4x4<f32>,
    axes: vec4<u32>,
    path: vec4<u32>,
};

struct ShapeClipSegment {
    from_to: vec4<f32>,
};

struct ShapeClipSpan {
    offset: u32,
    count: u32,
    _padding: vec2<u32>,
};

@group(1) @binding(0)
var<storage, read> shape_clip_records: array<ShapeClipRecord>;

@group(1) @binding(1)
var<storage, read> shape_clip_spans: array<ShapeClipSpan>;

struct LinearGradientDraw {
    start_end: vec4<f32>,
    tile_rect: vec4<f32>,
    tile_stride: vec4<f32>,
    tile_domain: vec4<f32>,
    stop_offset: u32,
    stop_count: u32,
    kind: u32,
    geometry_flags: u32,
};

struct LinearGradientStop {
    position_and_padding: vec4<f32>,
    color: vec4<f32>,
};

@group(1) @binding(2)
var<storage, read> linear_gradient_draws: array<LinearGradientDraw>;

@group(1) @binding(3)
var<storage, read> linear_gradient_stops: array<LinearGradientStop>;

@group(1) @binding(4)
var<storage, read> shape_clip_segments: array<ShapeClipSegment>;

@group(2) @binding(0)
var background_image_texture: texture_2d<f32>;

@group(2) @binding(1)
var background_image_sampler: sampler;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) outer_rect: vec4<f32>,
    @location(3) outer_radii_x: vec4<f32>,
    @location(4) outer_radii_y: vec4<f32>,
    @location(5) inner_rect: vec4<f32>,
    @location(6) inner_radii_x: vec4<f32>,
    @location(7) inner_radii_y: vec4<f32>,
    @location(8) border_widths: vec4<f32>,
    @location(9) mode: f32,
    @location(10) border_top_color: vec4<f32>,
    @location(11) border_right_color: vec4<f32>,
    @location(12) border_bottom_color: vec4<f32>,
    @location(13) border_left_color: vec4<f32>,
    @location(14) border_styles: vec4<f32>,
    @location(15) transformed_position: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) logical_position: vec2<f32>,
    @location(2) outer_rect: vec4<f32>,
    @location(3) outer_radii_x: vec4<f32>,
    @location(4) outer_radii_y: vec4<f32>,
    @location(5) inner_rect: vec4<f32>,
    @location(6) inner_radii_x: vec4<f32>,
    @location(7) inner_radii_y: vec4<f32>,
    @location(8) border_widths: vec4<f32>,
    @location(9) @interpolate(flat) mode: f32,
    @location(10) border_top_color: vec4<f32>,
    @location(11) border_right_color: vec4<f32>,
    @location(12) border_bottom_color: vec4<f32>,
    @location(13) border_left_color: vec4<f32>,
    @location(14) @interpolate(flat) border_styles: vec4<f32>,
    @location(15) @interpolate(flat) draw_index: u32,
};

@vertex
fn vs_main(input: VertexInput, @builtin(instance_index) draw_index: u32) -> VertexOutput {
    let transformed = input.transformed_position;
    var output: VertexOutput;
    output.position = vec4<f32>(
        transformed.x / viewport.logical_size.x * 2.0 - transformed.w,
        transformed.w - transformed.y / viewport.logical_size.y * 2.0,
        transformed.z,
        transformed.w,
    );
    output.color = input.color;
    output.logical_position = input.position;
    output.outer_rect = input.outer_rect;
    output.outer_radii_x = input.outer_radii_x;
    output.outer_radii_y = input.outer_radii_y;
    output.inner_rect = input.inner_rect;
    output.inner_radii_x = input.inner_radii_x;
    output.inner_radii_y = input.inner_radii_y;
    output.border_widths = input.border_widths;
    output.mode = input.mode;
    output.border_top_color = input.border_top_color;
    output.border_right_color = input.border_right_color;
    output.border_bottom_color = input.border_bottom_color;
    output.border_left_color = input.border_left_color;
    output.border_styles = input.border_styles;
    output.draw_index = draw_index;
    return output;
}

fn ellipse_distance(
    position: vec2<f32>,
    center: vec2<f32>,
    radius: vec2<f32>,
) -> f32 {
    let normalized = (position - center) / radius;
    return (length(normalized) - 1.0) * min(radius.x, radius.y);
}

fn rounded_rect_distance(
    position: vec2<f32>,
    rect: vec4<f32>,
    radii_x: vec4<f32>,
    radii_y: vec4<f32>,
) -> f32 {
    let left = rect.x;
    let top = rect.y;
    let right = left + rect.z;
    let bottom = top + rect.w;
    let outside = max(vec2<f32>(left, top) - position, position - vec2<f32>(right, bottom));
    var distance = max(outside.x, outside.y);

    let top_left = vec2<f32>(radii_x.x, radii_y.x);
    if top_left.x > 0.0 && top_left.y > 0.0
        && position.x < left + top_left.x && position.y < top + top_left.y {
        distance = max(distance, ellipse_distance(
            position,
            vec2<f32>(left + top_left.x, top + top_left.y),
            top_left,
        ));
    }

    let top_right = vec2<f32>(radii_x.y, radii_y.y);
    if top_right.x > 0.0 && top_right.y > 0.0
        && position.x > right - top_right.x && position.y < top + top_right.y {
        distance = max(distance, ellipse_distance(
            position,
            vec2<f32>(right - top_right.x, top + top_right.y),
            top_right,
        ));
    }

    let bottom_right = vec2<f32>(radii_x.z, radii_y.z);
    if bottom_right.x > 0.0 && bottom_right.y > 0.0
        && position.x > right - bottom_right.x && position.y > bottom - bottom_right.y {
        distance = max(distance, ellipse_distance(
            position,
            vec2<f32>(right - bottom_right.x, bottom - bottom_right.y),
            bottom_right,
        ));
    }

    let bottom_left = vec2<f32>(radii_x.w, radii_y.w);
    if bottom_left.x > 0.0 && bottom_left.y > 0.0
        && position.x < left + bottom_left.x && position.y > bottom - bottom_left.y {
        distance = max(distance, ellipse_distance(
            position,
            vec2<f32>(left + bottom_left.x, bottom - bottom_left.y),
            bottom_left,
        ));
    }

    return distance;
}

fn erf_approx(value: f32) -> f32 {
    let sign = select(-1.0, 1.0, value >= 0.0);
    let x = abs(value);
    let t = 1.0 / (1.0 + 0.3275911 * x);
    let polynomial = (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t
        - 0.284496736) * t + 0.254829592) * t;
    return sign * (1.0 - polynomial * exp(-x * x));
}

fn shape_coverage(distance: f32) -> f32 {
    let smoothing = max(fwidth(distance), 0.0001);
    return clamp(0.5 - distance / smoothing, 0.0, 1.0);
}

fn path_contains(position: vec2<f32>, offset: u32, count: u32, even_odd: bool) -> bool {
    var winding = 0i;
    var parity = false;
    for (var index = 0u; index < count; index++) {
        let segment = shape_clip_segments[offset + index].from_to;
        let start_point = segment.xy;
        let end_point = segment.zw;
        let crosses = (start_point.y <= position.y && end_point.y > position.y)
            || (start_point.y > position.y && end_point.y <= position.y);
        if crosses {
            let intersection = start_point.x
                + (position.y - start_point.y) * (end_point.x - start_point.x)
                    / (end_point.y - start_point.y);
            if intersection > position.x {
                if even_odd {
                    parity = !parity;
                } else if end_point.y > start_point.y {
                    winding += 1;
                } else {
                    winding -= 1;
                }
            }
        }
    }
    return select(winding != 0, parity, even_odd);
}

fn border_side(position: vec2<f32>, rect: vec4<f32>, widths: vec4<f32>) -> vec2<f32> {
    let left = rect.x;
    let top = rect.y;
    let right = left + rect.z;
    let bottom = top + rect.w;
    var selected = -1.0;
    var score = 1e20;

    if widths.x > 0.0 {
        selected = 0.0;
        score = (position.y - top) / widths.x;
    }
    if widths.y > 0.0 {
        let candidate = (right - position.x) / widths.y;
        if candidate < score {
            selected = 1.0;
            score = candidate;
        }
    }
    if widths.z > 0.0 {
        let candidate = (bottom - position.y) / widths.z;
        if candidate < score {
            selected = 2.0;
            score = candidate;
        }
    }
    if widths.w > 0.0 {
        let candidate = (position.x - left) / widths.w;
        if candidate < score {
            selected = 3.0;
            score = candidate;
        }
    }
    return vec2<f32>(selected, clamp(score, 0.0, 1.0));
}

fn border_color(input: VertexOutput, side: f32) -> vec4<f32> {
    if side < 0.5 {
        return input.border_top_color;
    }
    if side < 1.5 {
        return input.border_right_color;
    }
    if side < 2.5 {
        return input.border_bottom_color;
    }
    return input.border_left_color;
}

fn border_style(input: VertexOutput, side: f32) -> f32 {
    if side < 0.5 {
        return input.border_styles.x;
    }
    if side < 1.5 {
        return input.border_styles.y;
    }
    if side < 2.5 {
        return input.border_styles.z;
    }
    return input.border_styles.w;
}

fn border_width(input: VertexOutput, side: f32) -> f32 {
    if side < 0.5 {
        return input.border_widths.x;
    }
    if side < 1.5 {
        return input.border_widths.y;
    }
    if side < 2.5 {
        return input.border_widths.z;
    }
    return input.border_widths.w;
}

fn border_path_position(position: vec2<f32>, rect: vec4<f32>, side: f32) -> f32 {
    let left = rect.x;
    let top = rect.y;
    let right = left + rect.z;
    let bottom = top + rect.w;
    if side < 0.5 {
        return position.x - left;
    }
    if side < 1.5 {
        return rect.z + position.y - top;
    }
    if side < 2.5 {
        return rect.z + rect.w + right - position.x;
    }
    return rect.z * 2.0 + rect.w + bottom - position.y;
}

fn patterned_coverage(
    style: f32,
    path_position: f32,
    width: f32,
    depth: f32,
) -> f32 {
    if style < 1.5 || style >= 3.5 {
        return 1.0;
    }
    if style < 2.5 {
        let period = max(width * 4.0, 0.0001);
        let phase = fract(path_position / period) * period;
        let distance = max(-phase, phase - width * 3.0);
        return shape_coverage(distance);
    }
    let period = max(width * 2.0, 0.0001);
    let along = abs(fract(path_position / period) * period - width);
    let across = abs(depth - 0.5) * width;
    let distance = length(vec2<f32>(along, across)) - width * 0.5;
    return shape_coverage(distance);
}

fn double_coverage(style: f32, depth: f32) -> f32 {
    if style < 3.5 || style >= 4.5 {
        return 1.0;
    }
    let distance = min(abs(depth - 1.0 / 6.0), abs(depth - 5.0 / 6.0)) - 1.0 / 6.0;
    return shape_coverage(distance);
}

fn shade(color: vec4<f32>, amount: f32) -> vec4<f32> {
    var rgb = color.rgb;
    if amount < 0.0 {
        rgb = rgb * (1.0 + amount);
    } else {
        rgb = rgb + (vec3<f32>(1.0) - rgb) * amount;
    }
    return vec4<f32>(rgb, color.a);
}

fn styled_color(color: vec4<f32>, style: f32, side: f32, depth: f32) -> vec4<f32> {
    let top_or_left = side < 0.5 || side > 2.5;
    let inset_amount = select(0.35, -0.35, top_or_left);
    if style >= 6.5 && style < 7.5 {
        return shade(color, inset_amount);
    }
    if style >= 7.5 {
        return shade(color, -inset_amount);
    }
    if style >= 4.5 && style < 5.5 {
        return shade(color, select(-inset_amount, inset_amount, depth < 0.5));
    }
    if style >= 5.5 && style < 6.5 {
        return shade(color, select(inset_amount, -inset_amount, depth < 0.5));
    }
    return color;
}

fn linear_gradient_color(draw_index: u32, position: vec2<f32>) -> vec4<f32> {
    let gradient = linear_gradient_draws[draw_index];
    let no_repeat_x = (gradient.geometry_flags & 1u) != 0u;
    let no_repeat_y = (gradient.geometry_flags & 2u) != 0u;
    let space_x = (gradient.geometry_flags & 4u) != 0u;
    let space_y = (gradient.geometry_flags & 8u) != 0u;
    let tile_end = gradient.tile_rect.xy + gradient.tile_rect.zw;
    if (no_repeat_x && (position.x < gradient.tile_rect.x || position.x >= tile_end.x))
        || (no_repeat_y && (position.y < gradient.tile_rect.y || position.y >= tile_end.y)) {
        return vec4<f32>(0.0);
    }
    let domain_end = gradient.tile_domain.xy + gradient.tile_domain.zw;
    if (space_x && (position.x < gradient.tile_domain.x || position.x >= domain_end.x))
        || (space_y && (position.y < gradient.tile_domain.y || position.y >= domain_end.y)) {
        return vec4<f32>(0.0);
    }
    var sample_position = position;
    if space_x {
        let phase = fract(
            (position.x - gradient.tile_rect.x) / max(gradient.tile_stride.x, 0.0001)
        ) * gradient.tile_stride.x;
        if phase >= gradient.tile_rect.z {
            return vec4<f32>(0.0);
        }
        sample_position.x = gradient.tile_rect.x + phase;
    } else if !no_repeat_x {
        sample_position.x = gradient.tile_rect.x
            + fract((position.x - gradient.tile_rect.x) / max(gradient.tile_rect.z, 0.0001))
                * gradient.tile_rect.z;
    }
    if space_y {
        let phase = fract(
            (position.y - gradient.tile_rect.y) / max(gradient.tile_stride.y, 0.0001)
        ) * gradient.tile_stride.y;
        if phase >= gradient.tile_rect.w {
            return vec4<f32>(0.0);
        }
        sample_position.y = gradient.tile_rect.y + phase;
    } else if !no_repeat_y {
        sample_position.y = gradient.tile_rect.y
            + fract((position.y - gradient.tile_rect.y) / max(gradient.tile_rect.w, 0.0001))
                * gradient.tile_rect.w;
    }
    if gradient.kind == 4u {
        let coordinates = clamp(
            (sample_position - gradient.tile_rect.xy)
                / max(gradient.tile_rect.zw, vec2<f32>(0.0001)),
            vec2<f32>(0.0),
            vec2<f32>(1.0),
        );
        let color = textureSample(background_image_texture, background_image_sampler, coordinates);
        return vec4<f32>(color.rgb, color.a * gradient.start_end.x);
    }
    if gradient.stop_count == 0u {
        return vec4<f32>(0.0);
    }
    let line = gradient.start_end.zw - gradient.start_end.xy;
    var progress = dot(sample_position - gradient.start_end.xy, line)
        / max(dot(line, line), 0.0001);
    if gradient.kind == 3u {
        let delta = sample_position - gradient.start_end.xy;
        let clockwise_turns = atan2(delta.x, -delta.y) / (2.0 * 3.141592653589793);
        progress = fract(clockwise_turns - gradient.start_end.z);
    } else if gradient.kind == 2u {
        let normalized = (sample_position - gradient.start_end.xy)
            / max(gradient.start_end.zw, vec2<f32>(0.0001));
        progress = length(normalized);
    } else if gradient.kind == 1u {
        progress = fract(progress);
    }
    let first = linear_gradient_stops[gradient.stop_offset];
    if progress <= first.position_and_padding.x {
        return first.color;
    }
    var previous = first;
    for (var index = 1u; index < gradient.stop_count; index++) {
        let current = linear_gradient_stops[gradient.stop_offset + index];
        if progress <= current.position_and_padding.x {
            let amount = clamp(
                (progress - previous.position_and_padding.x)
                    / max(current.position_and_padding.x - previous.position_and_padding.x, 0.0001),
                0.0,
                1.0,
            );
            let alpha = mix(previous.color.a, current.color.a, amount);
            let premultiplied = mix(
                previous.color.rgb * previous.color.a,
                current.color.rgb * current.color.a,
                amount,
            );
            let rgb = select(
                vec3<f32>(0.0),
                premultiplied / max(alpha, 0.0001),
                alpha > 0.0001,
            );
            return vec4<f32>(rgb, alpha);
        }
        previous = current;
    }
    return previous.color;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let span = shape_clip_spans[input.draw_index];
    var clip_coverage = 1.0;
    let world_position = input.position.xy * viewport.logical_size / viewport.physical_size;
    for (var index = 0u; index < span.count; index++) {
        let clip = shape_clip_records[span.offset + index];
        let local_h = clip.inverse_transform * vec4<f32>(world_position, 0.0, 1.0);
        let local = local_h.xy / local_h.w;
        if clip.path.z != 0u {
            clip_coverage *= select(
                0.0,
                1.0,
                path_contains(local, clip.path.x, clip.path.y, clip.path.w != 0u),
            );
            continue;
        }
        var distance = -1e20;
        if clip.axes.x != 0u && clip.axes.y != 0u {
            distance = rounded_rect_distance(local, clip.rect, clip.radii_x, clip.radii_y);
        } else {
            if clip.axes.x != 0u {
                distance = max(clip.rect.x - local.x, local.x - clip.rect.x - clip.rect.z);
            }
            if clip.axes.y != 0u {
                distance = max(
                    distance,
                    max(clip.rect.y - local.y, local.y - clip.rect.y - clip.rect.w),
                );
            }
        }
        clip_coverage *= shape_coverage(distance);
    }
    let outer_distance = rounded_rect_distance(
        input.logical_position,
        input.outer_rect,
        input.outer_radii_x,
        input.outer_radii_y,
    );
    if input.mode < -2.5 {
        let distance = rounded_rect_distance(
            input.logical_position,
            input.inner_rect,
            input.inner_radii_x,
            input.inner_radii_y,
        );
        let sigma = input.border_widths.x * 0.5;
        let coverage = 0.5 * (1.0 - erf_approx(distance / (1.41421356237 * sigma)));
        return vec4<f32>(input.color.rgb, input.color.a * coverage * clip_coverage);
    }
    let outer_coverage = shape_coverage(outer_distance);
    if input.mode < -1.5 {
        let color = linear_gradient_color(input.draw_index, input.logical_position);
        return vec4<f32>(color.rgb, color.a * outer_coverage * clip_coverage);
    }
    if input.mode < 0.0 {
        return vec4<f32>(input.color.rgb, input.color.a * outer_coverage * clip_coverage);
    }

    var inner_coverage = 0.0;
    if input.inner_rect.z > 0.0 && input.inner_rect.w > 0.0 {
        let inner_distance = rounded_rect_distance(
            input.logical_position,
            input.inner_rect,
            input.inner_radii_x,
            input.inner_radii_y,
        );
        if input.mode > 3.5 {
            let sigma = input.border_widths.x * 0.5;
            inner_coverage = 0.5 * (1.0 - erf_approx(
                inner_distance / (1.41421356237 * sigma),
            ));
        } else {
            inner_coverage = shape_coverage(inner_distance);
        }
    }
    if input.mode > 1.5 {
        let coverage = max(outer_coverage - inner_coverage, 0.0);
        if input.mode > 2.5 {
            return vec4<f32>(
                input.color.rgb,
                input.color.a * coverage * clip_coverage,
            );
        }
        let color = linear_gradient_color(input.draw_index, input.logical_position);
        return vec4<f32>(color.rgb, color.a * coverage * clip_coverage);
    }
    let side_and_depth = border_side(
        input.logical_position,
        input.outer_rect,
        input.border_widths,
    );
    let side = side_and_depth.x;
    let depth = side_and_depth.y;
    let style = border_style(input, side);
    let width = border_width(input, side);
    let path_position = border_path_position(input.logical_position, input.outer_rect, side);
    let color = styled_color(border_color(input, side), style, side, depth);
    let style_coverage = patterned_coverage(style, path_position, width, depth)
        * double_coverage(style, depth);
    let coverage = max(outer_coverage - inner_coverage, 0.0) * style_coverage;
    return vec4<f32>(color.rgb, color.a * coverage * clip_coverage);
}
"#;

const BACKDROP_SHADER: &str = r#"
struct BlurUniform {
    direction: vec2<f32>,
    radius: f32,
    _padding: f32,
};

@group(0) @binding(0) var source_texture: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> blur: BlurUniform;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var output: VertexOutput;
    output.position = vec4<f32>(positions[index], 0.0, 1.0);
    output.uv = positions[index] * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    if blur.radius <= 0.0 {
        return textureSample(source_texture, source_sampler, input.uv);
    }
    let dimensions = vec2<f32>(textureDimensions(source_texture));
    let step = blur.direction * max(blur.radius / 8.0, 0.5) / dimensions;
    var color = vec4<f32>(0.0);
    var weight_sum = 0.0;
    for (var offset = -8; offset <= 8; offset = offset + 1) {
        let value = f32(offset) / 4.0;
        let weight = exp(-0.5 * value * value);
        color += textureSample(source_texture, source_sampler, input.uv + step * f32(offset)) * weight;
        weight_sum += weight;
    }
    return color / weight_sum;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BackdropUniform {
    direction: [f32; 2],
    radius: f32,
    _padding: f32,
}

struct BackdropGpuPipeline {
    pipeline: RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl BackdropGpuPipeline {
    fn new(device: &Device, format: TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("whisker Desktop backdrop layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("whisker Desktop backdrop shader"),
            source: ShaderSource::Wgsl(BACKDROP_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("whisker Desktop backdrop pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("whisker Desktop backdrop pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("whisker Desktop backdrop sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            sampler,
        }
    }

    fn bind_group(
        &self,
        device: &Device,
        source: &wgpu::TextureView,
        uniform: BackdropUniform,
    ) -> (Buffer, wgpu::BindGroup) {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whisker Desktop backdrop uniform"),
            contents: bytemuck::bytes_of(&uniform),
            usage: BufferUsages::UNIFORM,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("whisker Desktop backdrop bind group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: buffer.as_entire_binding(),
                },
            ],
        });
        (buffer, bind_group)
    }
}

#[derive(Debug)]
pub(crate) struct GpuError(String);

impl fmt::Display for GpuError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for GpuError {}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BoxVertex {
    position: [f32; 2],
    color: [f32; 4],
    outer_rect: [f32; 4],
    outer_radii_x: [f32; 4],
    outer_radii_y: [f32; 4],
    inner_rect: [f32; 4],
    inner_radii_x: [f32; 4],
    inner_radii_y: [f32; 4],
    border_widths: [f32; 4],
    mode: f32,
    border_colors: [[f32; 4]; 4],
    border_styles: [f32; 4],
    transformed_position: [f32; 4],
}

impl BoxVertex {
    const ATTRIBUTES: [VertexAttribute; 16] = [
        VertexAttribute {
            format: VertexFormat::Float32x2,
            offset: 0,
            shader_location: 0,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 2]>() as u64,
            shader_location: 1,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 6]>() as u64,
            shader_location: 2,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 10]>() as u64,
            shader_location: 3,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 14]>() as u64,
            shader_location: 4,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 18]>() as u64,
            shader_location: 5,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 22]>() as u64,
            shader_location: 6,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 26]>() as u64,
            shader_location: 7,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 30]>() as u64,
            shader_location: 8,
        },
        VertexAttribute {
            format: VertexFormat::Float32,
            offset: std::mem::size_of::<[f32; 34]>() as u64,
            shader_location: 9,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 35]>() as u64,
            shader_location: 10,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 39]>() as u64,
            shader_location: 11,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 43]>() as u64,
            shader_location: 12,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 47]>() as u64,
            shader_location: 13,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 51]>() as u64,
            shader_location: 14,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 55]>() as u64,
            shader_location: 15,
        },
    ];

    fn layout() -> VertexBufferLayout<'static> {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewportUniform {
    logical_size: [f32; 2],
    physical_size: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ShapeClipRecordGpu {
    rect: [f32; 4],
    radii_x: [f32; 4],
    radii_y: [f32; 4],
    inverse_transform: [f32; 16],
    axes: [u32; 4],
    path: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ShapeClipSegmentGpu {
    from_to: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ShapeClipSpanGpu {
    offset: u32,
    count: u32,
    padding: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LinearGradientDrawGpu {
    start_end: [f32; 4],
    tile_rect: [f32; 4],
    tile_stride: [f32; 4],
    tile_domain: [f32; 4],
    stop_offset: u32,
    stop_count: u32,
    kind: u32,
    geometry_flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LinearGradientStopGpu {
    position: f32,
    padding: [f32; 3],
    color: [f32; 4],
}

#[derive(Clone)]
pub(crate) struct LinearGradientDraw {
    start_end: [f32; 4],
    tile_rect: [f32; 4],
    tile_stride: [f32; 4],
    tile_domain: [f32; 4],
    stops: Vec<LinearGradientStopGpu>,
    kind: u32,
    geometry_flags: u32,
}

#[cfg(all(test, feature = "host-conformance"))]
pub(crate) type ClippedBoxPrimitive = (
    BoxPrimitive,
    LogicalClip,
    Transform,
    ShapeClipStack,
    Option<LinearGradientDraw>,
    Option<ResourceId>,
    bool,
);

#[cfg(all(test, feature = "host-conformance"))]
pub(crate) type BackdropCheckpoint = (LayoutRect, f32, LogicalClip);

struct BoxGpuPipeline {
    pipeline: RenderPipeline,
    viewport_buffer: Buffer,
    viewport_bind_group: wgpu::BindGroup,
    shape_clip_layout: wgpu::BindGroupLayout,
    image_layout: wgpu::BindGroupLayout,
    fallback_image: GpuImageResource,
}

struct GpuImageResource {
    linear_bind_group: wgpu::BindGroup,
    nearest_bind_group: wgpu::BindGroup,
    _texture: wgpu::Texture,
    intrinsic_size: [f32; 2],
}

#[derive(Clone, Debug)]
pub(crate) struct RasterResource {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
}

impl RasterResource {
    pub(crate) fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, GpuError> {
        let expected = width
            .checked_mul(height)
            .and_then(|count| count.checked_mul(4))
            .map(|count| count as usize)
            .ok_or_else(|| GpuError("Desktop raster dimensions overflow".into()))?;
        if width == 0 || height == 0 || pixels.len() != expected {
            return Err(GpuError(format!(
                "Desktop raster has {} bytes, expected {expected} for {width}x{height} RGBA8",
                pixels.len()
            )));
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }
}

impl BoxGpuPipeline {
    fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        let viewport_uniform = ViewportUniform {
            logical_size: [1.0, 1.0],
            physical_size: [1.0, 1.0],
        };
        let viewport_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whisker Desktop logical viewport"),
            contents: bytemuck::bytes_of(&viewport_uniform),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });
        let viewport_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("whisker Desktop viewport layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let viewport_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("whisker Desktop viewport"),
            layout: &viewport_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport_buffer.as_entire_binding(),
            }],
        });
        let shape_clip_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("whisker Desktop shape clip layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let image_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("whisker Desktop background image layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let fallback_image = Self::create_image_resource(
            device,
            queue,
            &image_layout,
            "whisker Desktop fallback background image",
            &RasterResource {
                width: 1,
                height: 1,
                pixels: vec![0; 4],
            },
        );
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("whisker Desktop box shader"),
            source: ShaderSource::Wgsl(BOX_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("whisker Desktop box pipeline layout"),
            bind_group_layouts: &[&viewport_layout, &shape_clip_layout, &image_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("whisker Desktop box pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                buffers: &[BoxVertex::layout()],
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        Self {
            pipeline,
            viewport_buffer,
            viewport_bind_group,
            shape_clip_layout,
            image_layout,
            fallback_image,
        }
    }

    fn create_image_resource(
        device: &Device,
        queue: &Queue,
        layout: &wgpu::BindGroupLayout,
        label: &str,
        raster: &RasterResource,
    ) -> GpuImageResource {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: raster.width,
                height: raster.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &raster.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(raster.width * 4),
                rows_per_image: Some(raster.height),
            },
            wgpu::Extent3d {
                width: raster.width,
                height: raster.height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("whisker Desktop background image sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..wgpu::SamplerDescriptor::default()
        });
        let linear_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("whisker Desktop pixelated image sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..wgpu::SamplerDescriptor::default()
        });
        let nearest_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("whisker Desktop pixelated image bind group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&nearest_sampler),
                },
            ],
        });
        GpuImageResource {
            linear_bind_group,
            nearest_bind_group,
            _texture: texture,
            intrinsic_size: [raster.width as f32, raster.height as f32],
        }
    }

    fn upload_image(
        &self,
        device: &Device,
        queue: &Queue,
        raster: &RasterResource,
    ) -> GpuImageResource {
        Self::create_image_resource(
            device,
            queue,
            &self.image_layout,
            "whisker Desktop raster background image",
            raster,
        )
    }

    fn update_viewport(&self, queue: &Queue, logical_size: [f32; 2], physical_size: [f32; 2]) {
        queue.write_buffer(
            &self.viewport_buffer,
            0,
            bytemuck::bytes_of(&ViewportUniform {
                logical_size,
                physical_size,
            }),
        );
    }

    fn shape_clip_bind_group(&self, device: &Device, draws: &[DrawCommand]) -> wgpu::BindGroup {
        let mut records = Vec::new();
        let mut path_segments = Vec::new();
        let spans = draws
            .iter()
            .map(|draw| {
                let offset = records.len() as u32;
                if let DrawCommand::Quads { shape_clips, .. } = draw {
                    for clip in shape_clips.iter() {
                        let path_offset = path_segments.len() as u32;
                        if let Some(segments) = &clip.path {
                            path_segments.extend(segments.iter().map(|segment| {
                                ShapeClipSegmentGpu {
                                    from_to: [
                                        segment.from[0],
                                        segment.from[1],
                                        segment.to[0],
                                        segment.to[1],
                                    ],
                                }
                            }));
                        }
                        records.push(ShapeClipRecordGpu {
                            rect: [clip.rect.x, clip.rect.y, clip.rect.width, clip.rect.height],
                            radii_x: clip.radii.horizontal,
                            radii_y: clip.radii.vertical,
                            inverse_transform: clip.inverse_transform.0,
                            axes: [u32::from(clip.horizontal), u32::from(clip.vertical), 0, 0],
                            path: [
                                path_offset,
                                path_segments.len() as u32 - path_offset,
                                u32::from(clip.path.is_some()),
                                u32::from(clip.fill_rule == whisker_protocol::FillRule::EvenOdd),
                            ],
                        });
                    }
                }
                ShapeClipSpanGpu {
                    offset,
                    count: records.len() as u32 - offset,
                    padding: [0; 2],
                }
            })
            .collect::<Vec<_>>();
        if records.is_empty() {
            records.push(ShapeClipRecordGpu::zeroed());
        }
        if path_segments.is_empty() {
            path_segments.push(ShapeClipSegmentGpu::zeroed());
        }
        let spans = if spans.is_empty() {
            vec![ShapeClipSpanGpu::zeroed()]
        } else {
            spans
        };
        let mut gradient_stops = Vec::new();
        let gradient_draws = draws
            .iter()
            .map(|draw| {
                let Some(gradient) = draw.gradient() else {
                    return LinearGradientDrawGpu::zeroed();
                };
                let stop_offset = gradient_stops.len() as u32;
                gradient_stops.extend_from_slice(&gradient.stops);
                LinearGradientDrawGpu {
                    start_end: gradient.start_end,
                    tile_rect: gradient.tile_rect,
                    tile_stride: gradient.tile_stride,
                    tile_domain: gradient.tile_domain,
                    stop_offset,
                    stop_count: gradient.stops.len() as u32,
                    kind: gradient.kind,
                    geometry_flags: gradient.geometry_flags,
                }
            })
            .collect::<Vec<_>>();
        if gradient_stops.is_empty() {
            gradient_stops.push(LinearGradientStopGpu::zeroed());
        }
        let gradient_draws = if gradient_draws.is_empty() {
            vec![LinearGradientDrawGpu::zeroed()]
        } else {
            gradient_draws
        };
        let record_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whisker Desktop shape clip records"),
            contents: bytemuck::cast_slice(&records),
            usage: BufferUsages::STORAGE,
        });
        let span_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whisker Desktop shape clip spans"),
            contents: bytemuck::cast_slice(&spans),
            usage: BufferUsages::STORAGE,
        });
        let gradient_draw_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whisker Desktop linear gradient draws"),
            contents: bytemuck::cast_slice(&gradient_draws),
            usage: BufferUsages::STORAGE,
        });
        let gradient_stop_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whisker Desktop linear gradient stops"),
            contents: bytemuck::cast_slice(&gradient_stops),
            usage: BufferUsages::STORAGE,
        });
        let path_segment_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whisker Desktop shape clip path segments"),
            contents: bytemuck::cast_slice(&path_segments),
            usage: BufferUsages::STORAGE,
        });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("whisker Desktop shape clips"),
            layout: &self.shape_clip_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: record_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: span_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: gradient_draw_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: gradient_stop_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: path_segment_buffer.as_entire_binding(),
                },
            ],
        })
    }
}

enum DrawCommand {
    BackdropBlur {
        rect: LayoutRect,
        radius: f32,
        clip: LogicalClip,
    },
    Quads {
        vertices: Range<u32>,
        clip: LogicalClip,
        shape_clips: ShapeClipStack,
        gradient: Option<LinearGradientDraw>,
        resource: Option<ResourceId>,
        pixelated: bool,
    },
    Text {
        index: usize,
        node: NodeId,
    },
}

impl DrawCommand {
    fn gradient(&self) -> Option<&LinearGradientDraw> {
        match self {
            Self::Quads { gradient, .. } => gradient.as_ref(),
            Self::Text { .. } | Self::BackdropBlur { .. } => None,
        }
    }
}

pub(crate) fn linear_gradient_draw(
    positioning_rect: LayoutRect,
    angle_degrees: f32,
    repeating: bool,
    stops: &[GradientStop],
    opacity: f32,
) -> LinearGradientDraw {
    let angle = angle_degrees.to_radians();
    let direction = [angle.sin(), -angle.cos()];
    let center = [
        positioning_rect.x + positioning_rect.width * 0.5,
        positioning_rect.y + positioning_rect.height * 0.5,
    ];
    let half_length = direction[0].abs() * positioning_rect.width * 0.5
        + direction[1].abs() * positioning_rect.height * 0.5;
    let line_length = (half_length * 2.0).max(0.0001);
    LinearGradientDraw {
        start_end: [
            center[0] - direction[0] * half_length,
            center[1] - direction[1] * half_length,
            center[0] + direction[0] * half_length,
            center[1] + direction[1] * half_length,
        ],
        tile_rect: [
            positioning_rect.x,
            positioning_rect.y,
            positioning_rect.width,
            positioning_rect.height,
        ],
        tile_stride: [positioning_rect.width, positioning_rect.height, 0.0, 0.0],
        tile_domain: [
            positioning_rect.x,
            positioning_rect.y,
            positioning_rect.width,
            positioning_rect.height,
        ],
        stops: stops
            .iter()
            .map(|stop| LinearGradientStopGpu {
                position: stop.position.map_or(0.0, |position| {
                    position.fraction + position.length / line_length
                }),
                padding: [0.0; 3],
                color: gpu_color(&stop.color, opacity),
            })
            .collect(),
        kind: u32::from(repeating),
        geometry_flags: 0,
    }
}

pub(crate) fn radial_gradient_draw(
    positioning_rect: LayoutRect,
    center: whisker_protocol::PaintPosition,
    radii: (
        whisker_protocol::PaintLengthPercentage,
        whisker_protocol::PaintLengthPercentage,
    ),
    stops: &[GradientStop],
    opacity: f32,
) -> LinearGradientDraw {
    let center = [
        positioning_rect.x + center.x.length + center.x.fraction * positioning_rect.width,
        positioning_rect.y + center.y.length + center.y.fraction * positioning_rect.height,
    ];
    let radii = [
        radii.0.length + radii.0.fraction * positioning_rect.width,
        radii.1.length + radii.1.fraction * positioning_rect.height,
    ];
    LinearGradientDraw {
        start_end: [center[0], center[1], radii[0], radii[1]],
        tile_rect: [
            positioning_rect.x,
            positioning_rect.y,
            positioning_rect.width,
            positioning_rect.height,
        ],
        tile_stride: [positioning_rect.width, positioning_rect.height, 0.0, 0.0],
        tile_domain: [
            positioning_rect.x,
            positioning_rect.y,
            positioning_rect.width,
            positioning_rect.height,
        ],
        stops: stops
            .iter()
            .map(|stop| LinearGradientStopGpu {
                position: stop.position.map_or(0.0, |position| position.fraction),
                padding: [0.0; 3],
                color: gpu_color(&stop.color, opacity),
            })
            .collect(),
        kind: 2,
        geometry_flags: 0,
    }
}

pub(crate) fn conic_gradient_draw(
    positioning_rect: LayoutRect,
    from_degrees: f32,
    center: whisker_protocol::PaintPosition,
    stops: &[GradientStop],
    opacity: f32,
) -> LinearGradientDraw {
    let center = [
        positioning_rect.x + center.x.length + center.x.fraction * positioning_rect.width,
        positioning_rect.y + center.y.length + center.y.fraction * positioning_rect.height,
    ];
    LinearGradientDraw {
        start_end: [center[0], center[1], from_degrees / 360.0, 0.0],
        tile_rect: [
            positioning_rect.x,
            positioning_rect.y,
            positioning_rect.width,
            positioning_rect.height,
        ],
        tile_stride: [positioning_rect.width, positioning_rect.height, 0.0, 0.0],
        tile_domain: [
            positioning_rect.x,
            positioning_rect.y,
            positioning_rect.width,
            positioning_rect.height,
        ],
        stops: stops
            .iter()
            .map(|stop| LinearGradientStopGpu {
                position: stop.position.map_or(0.0, |position| position.fraction),
                padding: [0.0; 3],
                color: gpu_color(&stop.color, opacity),
            })
            .collect(),
        kind: 3,
        geometry_flags: 0,
    }
}

struct BackgroundTileGeometry {
    rect: LayoutRect,
    stride: [f32; 2],
    domain: LayoutRect,
    flags: u32,
}

struct BackgroundAxisGeometry {
    origin: f32,
    tile_size: f32,
    stride: f32,
    flags: u32,
}

fn background_axis_geometry(
    start: f32,
    area_size: f32,
    image_size: f32,
    position: whisker_protocol::PaintCoordinate,
    repeat: whisker_protocol::ImageRepeat,
    no_repeat_flag: u32,
    space_flag: u32,
) -> BackgroundAxisGeometry {
    use whisker_protocol::ImageRepeat;

    match repeat {
        ImageRepeat::Repeat | ImageRepeat::NoRepeat => {
            let origin = start + position.length + position.fraction * (area_size - image_size);
            BackgroundAxisGeometry {
                origin,
                tile_size: image_size,
                stride: image_size,
                flags: if repeat == ImageRepeat::NoRepeat {
                    no_repeat_flag
                } else {
                    0
                },
            }
        }
        ImageRepeat::Space => {
            let count = (area_size / image_size).floor();
            if count >= 2.0 {
                BackgroundAxisGeometry {
                    origin: start,
                    tile_size: image_size,
                    stride: (area_size - image_size) / (count - 1.0),
                    flags: space_flag,
                }
            } else {
                BackgroundAxisGeometry {
                    origin: start + position.length + position.fraction * (area_size - image_size),
                    tile_size: image_size,
                    stride: image_size,
                    flags: no_repeat_flag,
                }
            }
        }
        ImageRepeat::Round => {
            let count = (area_size / image_size).round().max(1.0);
            let tile_size = area_size / count;
            BackgroundAxisGeometry {
                origin: start + position.length + position.fraction * (area_size - tile_size),
                tile_size,
                stride: tile_size,
                flags: 0,
            }
        }
    }
}

fn background_tile_geometry(
    positioning_rect: LayoutRect,
    layer: &whisker_protocol::BackgroundLayer,
    intrinsic_size: Option<[f32; 2]>,
) -> Option<BackgroundTileGeometry> {
    use whisker_protocol::{BackgroundSize, ImageRepeat};

    let resolve = |value: whisker_protocol::PaintLengthPercentage, extent: f32| {
        value.length + value.fraction * extent
    };
    let [mut width, mut height] = match layer.size {
        BackgroundSize::Auto => {
            intrinsic_size.unwrap_or([positioning_rect.width, positioning_rect.height])
        }
        BackgroundSize::Cover | BackgroundSize::Contain => {
            let intrinsic = intrinsic_size?;
            if intrinsic[0] <= 0.0 || intrinsic[1] <= 0.0 {
                return None;
            }
            let width_scale = positioning_rect.width / intrinsic[0];
            let height_scale = positioning_rect.height / intrinsic[1];
            let scale = if layer.size == BackgroundSize::Cover {
                width_scale.max(height_scale)
            } else {
                width_scale.min(height_scale)
            };
            [intrinsic[0] * scale, intrinsic[1] * scale]
        }
        BackgroundSize::Explicit { width, height } => {
            let explicit_width = width.map(|value| resolve(value, positioning_rect.width));
            let explicit_height = height.map(|value| resolve(value, positioning_rect.height));
            match (explicit_width, explicit_height, intrinsic_size) {
                (Some(width), Some(height), _) => [width, height],
                (Some(width), None, Some(intrinsic)) if intrinsic[0] > 0.0 => {
                    [width, width * intrinsic[1] / intrinsic[0]]
                }
                (None, Some(height), Some(intrinsic)) if intrinsic[1] > 0.0 => {
                    [height * intrinsic[0] / intrinsic[1], height]
                }
                (None, None, Some(intrinsic)) => intrinsic,
                _ => return None,
            }
        }
    };
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let height_tracks_rounded_width = matches!(
        layer.size,
        BackgroundSize::Explicit {
            width: Some(_),
            height: None
        }
    ) && layer.repeat_x == ImageRepeat::Round
        && layer.repeat_y != ImageRepeat::Round;
    let width_tracks_rounded_height = matches!(
        layer.size,
        BackgroundSize::Explicit {
            width: None,
            height: Some(_)
        }
    ) && layer.repeat_y == ImageRepeat::Round
        && layer.repeat_x != ImageRepeat::Round;

    let mut x = background_axis_geometry(
        positioning_rect.x,
        positioning_rect.width,
        width,
        layer.position.x,
        layer.repeat_x,
        1,
        4,
    );
    if height_tracks_rounded_width {
        height *= x.tile_size / width;
    }
    let y = background_axis_geometry(
        positioning_rect.y,
        positioning_rect.height,
        height,
        layer.position.y,
        layer.repeat_y,
        2,
        8,
    );
    if width_tracks_rounded_height {
        width *= y.tile_size / height;
        x = background_axis_geometry(
            positioning_rect.x,
            positioning_rect.width,
            width,
            layer.position.x,
            layer.repeat_x,
            1,
            4,
        );
    }
    Some(BackgroundTileGeometry {
        rect: LayoutRect {
            x: x.origin,
            y: y.origin,
            width: x.tile_size,
            height: y.tile_size,
        },
        stride: [x.stride, y.stride],
        domain: positioning_rect,
        flags: x.flags | y.flags,
    })
}

pub(crate) fn background_gradient_draw(
    positioning_rect: LayoutRect,
    layer: &whisker_protocol::BackgroundLayer,
    opacity: f32,
) -> Option<LinearGradientDraw> {
    let tile = background_tile_geometry(positioning_rect, layer, None)?;
    let mut gradient = match &layer.image {
        PaintImage::LinearGradient {
            angle_degrees,
            repeating,
            stops,
        } => linear_gradient_draw(tile.rect, *angle_degrees, *repeating, stops, opacity),
        PaintImage::RadialGradient {
            center,
            radii: Some(radii),
            stops,
            ..
        } => radial_gradient_draw(tile.rect, *center, *radii, stops, opacity),
        PaintImage::ConicGradient {
            from_degrees,
            center,
            repeating: false,
            stops,
        } => conic_gradient_draw(tile.rect, *from_degrees, *center, stops, opacity),
        _ => return None,
    };
    gradient.tile_stride = [tile.stride[0], tile.stride[1], 0.0, 0.0];
    gradient.tile_domain = [
        tile.domain.x,
        tile.domain.y,
        tile.domain.width,
        tile.domain.height,
    ];
    gradient.geometry_flags = tile.flags;
    Some(gradient)
}

pub(crate) fn background_resource_draw(
    positioning_rect: LayoutRect,
    layer: &whisker_protocol::BackgroundLayer,
    intrinsic_size: [f32; 2],
    opacity: f32,
) -> Option<(LinearGradientDraw, ResourceId)> {
    let PaintImage::Resource(resource) = &layer.image else {
        return None;
    };
    let tile = background_tile_geometry(positioning_rect, layer, Some(intrinsic_size))?;
    Some((
        LinearGradientDraw {
            start_end: [opacity, 0.0, 0.0, 0.0],
            tile_rect: [tile.rect.x, tile.rect.y, tile.rect.width, tile.rect.height],
            tile_stride: [tile.stride[0], tile.stride[1], 0.0, 0.0],
            tile_domain: [
                tile.domain.x,
                tile.domain.y,
                tile.domain.width,
                tile.domain.height,
            ],
            stops: Vec::new(),
            kind: 4,
            geometry_flags: tile.flags,
        },
        *resource,
    ))
}

fn push_quad_draw(
    vertices: &mut Vec<BoxVertex>,
    draws: &mut Vec<DrawCommand>,
    primitive: BoxPrimitive,
    transform: Transform,
    clip: LogicalClip,
    shape_clips: &ShapeClipStack,
    background: (Option<LinearGradientDraw>, Option<ResourceId>, bool),
) {
    let (gradient, resource, pixelated) = background;
    let start = vertices.len() as u32;
    push_transformed_quad(vertices, primitive, transform);
    draws.push(DrawCommand::Quads {
        vertices: start..vertices.len() as u32,
        clip,
        shape_clips: shape_clips.clone(),
        gradient,
        resource,
        pixelated,
    });
}

pub(crate) struct GpuRenderer {
    surface: Surface<'static>,
    device: Device,
    queue: Queue,
    config: SurfaceConfiguration,
    box_gpu: BoxGpuPipeline,
    backdrop_gpu: BackdropGpuPipeline,
    text_viewport: Viewport,
    text_atlas: TextAtlas,
    text_renderers: HashMap<NodeId, TextRenderer>,
    image_resources: HashMap<ResourceId, GpuImageResource>,
}

impl GpuRenderer {
    pub(crate) async fn new(
        target: impl Into<SurfaceTarget<'static>>,
        physical_size: [u32; 2],
    ) -> Result<Self, GpuError> {
        let instance = Instance::new(&InstanceDescriptor::default());
        let surface = instance
            .create_surface(target)
            .map_err(|error| GpuError(format!("create Desktop GPU surface: {error}")))?;
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..RequestAdapterOptions::default()
            })
            .await
            .map_err(|error| GpuError(format!("select Desktop GPU adapter: {error}")))?;
        let (device, queue) = adapter
            .request_device(&DeviceDescriptor::default())
            .await
            .map_err(|error| GpuError(format!("create Desktop GPU device: {error}")))?;
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| !format.is_srgb())
            .ok_or_else(|| {
                GpuError(
                    "Desktop GPU surface exposes no non-sRGB format for CSS compositing".into(),
                )
            })?;
        let present_mode = capabilities
            .present_modes
            .iter()
            .copied()
            .find(|mode| *mode == PresentMode::Fifo)
            .unwrap_or(PresentMode::AutoVsync);
        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            width: physical_size[0].max(1),
            height: physical_size[1].max(1),
            present_mode,
            alpha_mode: CompositeAlphaMode::Opaque,
            view_formats: Vec::new(),
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let box_gpu = BoxGpuPipeline::new(&device, &queue, format);
        let backdrop_gpu = BackdropGpuPipeline::new(&device, format);

        let text_cache = Cache::new(&device);
        let text_viewport = Viewport::new(&device, &text_cache);
        let text_atlas = TextAtlas::new(&device, &queue, &text_cache, format);
        Ok(Self {
            surface,
            device,
            queue,
            config,
            box_gpu,
            backdrop_gpu,
            text_viewport,
            text_atlas,
            text_renderers: HashMap::new(),
            image_resources: HashMap::new(),
        })
    }

    pub(crate) fn register_raster_resource(
        &mut self,
        resource: ResourceId,
        raster: &RasterResource,
    ) {
        let image = self.box_gpu.upload_image(&self.device, &self.queue, raster);
        self.image_resources.insert(resource, image);
    }

    pub(crate) fn release_raster_resource(&mut self, resource: ResourceId) {
        self.image_resources.remove(&resource);
    }

    pub(crate) fn resize(&mut self, physical_size: [u32; 2]) {
        if physical_size[0] == 0 || physical_size[1] == 0 {
            return;
        }
        self.config.width = physical_size[0];
        self.config.height = physical_size[1];
        self.surface.configure(&self.device, &self.config);
    }

    pub(crate) fn render(
        &mut self,
        commands: &[PaintCommand<'_>],
        text: &mut NativeTextHost,
        logical_size: [f32; 2],
        scale: f32,
    ) -> Result<(), GpuError> {
        if self.config.width == 0 || self.config.height == 0 {
            return Ok(());
        }
        self.box_gpu.update_viewport(
            &self.queue,
            logical_size,
            [self.config.width as f32, self.config.height as f32],
        );
        self.text_viewport.update(
            &self.queue,
            Resolution {
                width: self.config.width,
                height: self.config.height,
            },
        );

        let mut vertices = Vec::new();
        let mut draws = Vec::new();
        for (index, command) in commands.iter().enumerate() {
            match command {
                PaintCommand::BackdropBlur { rect, radius, clip } => {
                    draws.push(DrawCommand::BackdropBlur {
                        rect: *rect,
                        radius: *radius,
                        clip: *clip,
                    });
                }
                PaintCommand::Box {
                    rect,
                    content_rect,
                    paint,
                    background_layers,
                    visual_effects,
                    clip,
                    shape_clips,
                    transform,
                    opacity,
                } => {
                    let default_paint = whisker_protocol::BoxPaint::default();
                    let paint = paint.unwrap_or(&default_paint);
                    for shadow in visual_effects
                        .box_shadows
                        .iter()
                        .rev()
                        .filter(|shadow| !shadow.inset)
                    {
                        if let Some(primitive) =
                            box_shadow_primitive(*rect, paint, shadow, *opacity)
                        {
                            push_quad_draw(
                                &mut vertices,
                                &mut draws,
                                primitive,
                                *transform,
                                *clip,
                                shape_clips,
                                (None, None, false),
                            );
                        }
                    }
                    let mut box_primitives = Vec::new();
                    lower_box(*rect, paint, *opacity, |primitive| {
                        box_primitives.push(primitive)
                    });
                    for primitive in box_primitives
                        .iter()
                        .copied()
                        .filter(|primitive| primitive.kind == BoxPrimitiveKind::Fill)
                    {
                        push_quad_draw(
                            &mut vertices,
                            &mut draws,
                            primitive,
                            *transform,
                            *clip,
                            shape_clips,
                            (None, None, false),
                        );
                    }
                    let box_geometry = resolve_box_geometry(*rect, paint);
                    for layer in background_layers.iter().rev() {
                        let positioning_rect = match layer.origin {
                            PaintBox::Border => box_geometry.outer_rect,
                            PaintBox::Padding => box_geometry.inner_rect,
                            PaintBox::Content => *content_rect,
                            _ => continue,
                        };
                        let (gradient, resource) = match &layer.image {
                            PaintImage::Resource(resource) => {
                                let Some(image) = self.image_resources.get(resource) else {
                                    continue;
                                };
                                let Some((draw, resource)) = background_resource_draw(
                                    positioning_rect,
                                    layer,
                                    image.intrinsic_size,
                                    *opacity,
                                ) else {
                                    continue;
                                };
                                (draw, Some(resource))
                            }
                            _ => {
                                let Some(draw) =
                                    background_gradient_draw(positioning_rect, layer, *opacity)
                                else {
                                    continue;
                                };
                                (draw, None)
                            }
                        };
                        push_quad_draw(
                            &mut vertices,
                            &mut draws,
                            background_gradient_primitive(*rect, *content_rect, paint, layer.clip),
                            *transform,
                            *clip,
                            shape_clips,
                            (
                                Some(gradient),
                                resource,
                                resource.is_some()
                                    && visual_effects.image_rendering
                                        == whisker_protocol::ImageRendering::Pixelated,
                            ),
                        );
                    }
                    for shadow in visual_effects
                        .box_shadows
                        .iter()
                        .rev()
                        .filter(|shadow| shadow.inset)
                    {
                        if let Some(primitive) =
                            box_shadow_primitive(*rect, paint, shadow, *opacity)
                        {
                            push_quad_draw(
                                &mut vertices,
                                &mut draws,
                                primitive,
                                *transform,
                                *clip,
                                shape_clips,
                                (None, None, false),
                            );
                        }
                    }
                    for primitive in box_primitives
                        .into_iter()
                        .filter(|primitive| primitive.kind == BoxPrimitiveKind::Border)
                    {
                        push_quad_draw(
                            &mut vertices,
                            &mut draws,
                            primitive,
                            *transform,
                            *clip,
                            shape_clips,
                            (None, None, false),
                        );
                    }
                }
                PaintCommand::Text {
                    node,
                    transform,
                    shape_clips,
                    ..
                } => {
                    // Glyph transforms and shape clips are implemented with the
                    // SetText paint slice; retain both states on the command now.
                    let _ = (transform, shape_clips);
                    draws.push(DrawCommand::Text { index, node: *node })
                }
            }
        }
        let live_text_nodes = draws
            .iter()
            .filter_map(|draw| match draw {
                DrawCommand::Text { node, .. } => Some(*node),
                DrawCommand::Quads { .. } | DrawCommand::BackdropBlur { .. } => None,
            })
            .collect::<HashSet<_>>();
        self.text_renderers
            .retain(|node, _| live_text_nodes.contains(node));
        let mut prepared_text_nodes = HashSet::new();
        for draw in &draws {
            let DrawCommand::Text { index, node } = draw else {
                continue;
            };
            let PaintCommand::Text {
                rect,
                content,
                clip,
                opacity,
                ..
            } = &commands[*index]
            else {
                unreachable!();
            };
            let Some(id) = content.prepared_content else {
                continue;
            };
            let Some(prepared) = text.prepared.get(&id) else {
                continue;
            };
            if !self.text_renderers.contains_key(node) {
                self.text_renderers.insert(
                    *node,
                    TextRenderer::new(
                        &mut self.text_atlas,
                        &self.device,
                        MultisampleState::default(),
                        None,
                    ),
                );
            }
            let bounds = text_bounds(*clip, self.config.width, self.config.height, scale);
            let color = text_color(&content.paint.foreground, *opacity);
            self.text_renderers
                .get_mut(node)
                .expect("renderer inserted for live text node")
                .prepare(
                    &self.device,
                    &self.queue,
                    &mut text.font_system,
                    &mut self.text_atlas,
                    &self.text_viewport,
                    [TextArea {
                        buffer: &prepared.buffer,
                        left: rect.x * scale,
                        top: rect.y * scale,
                        scale,
                        bounds,
                        default_color: color,
                        custom_glyphs: &[],
                    }],
                    &mut text.swash_cache,
                )
                .map_err(|error| GpuError(format!("prepare glyph atlas: {error}")))?;
            prepared_text_nodes.insert(*node);
        }
        let vertex_buffer = (!vertices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("whisker Desktop box vertices"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: BufferUsages::VERTEX,
                })
        });
        let shape_clip_bind_group = self.box_gpu.shape_clip_bind_group(&self.device, &draws);

        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(SurfaceError::Lost | SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                self.surface.get_current_texture().map_err(|error| {
                    GpuError(format!("acquire Desktop GPU frame after recovery: {error}"))
                })?
            }
            Err(SurfaceError::Timeout) => return Ok(()),
            Err(error) => return Err(GpuError(format!("acquire Desktop GPU frame: {error}"))),
        };
        let view = frame.texture.create_view(&TextureViewDescriptor::default());
        let scene_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("whisker Desktop composited scene"),
            size: wgpu::Extent3d {
                width: self.config.width,
                height: self.config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let scratch_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("whisker Desktop backdrop scratch"),
            size: wgpu::Extent3d {
                width: self.config.width,
                height: self.config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let scene_view = scene_texture.create_view(&TextureViewDescriptor::default());
        let scratch_view = scratch_texture.create_view(&TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("whisker Desktop frame"),
            });
        {
            let _clear = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("whisker Desktop clear"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &scene_view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }

        for (draw_index, draw) in draws.into_iter().enumerate() {
            match draw {
                DrawCommand::BackdropBlur { rect, radius, clip } => {
                    let horizontal = BackdropUniform {
                        direction: [1.0, 0.0],
                        radius: radius * scale,
                        _padding: 0.0,
                    };
                    let (_horizontal_buffer, horizontal_group) =
                        self.backdrop_gpu
                            .bind_group(&self.device, &scene_view, horizontal);
                    {
                        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                            label: Some("whisker Desktop backdrop horizontal blur"),
                            color_attachments: &[Some(RenderPassColorAttachment {
                                view: &scratch_view,
                                resolve_target: None,
                                ops: Operations {
                                    load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                    store: StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        pass.set_pipeline(&self.backdrop_gpu.pipeline);
                        pass.set_bind_group(0, &horizontal_group, &[]);
                        pass.draw(0..3, 0..1);
                    }
                    let vertical = BackdropUniform {
                        direction: [0.0, 1.0],
                        radius: radius * scale,
                        _padding: 0.0,
                    };
                    let (_vertical_buffer, vertical_group) =
                        self.backdrop_gpu
                            .bind_group(&self.device, &scratch_view, vertical);
                    let Some((x, y, width, height)) =
                        self.scissor(clip.intersect(rect, true, true), scale)
                    else {
                        continue;
                    };
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("whisker Desktop backdrop vertical blur"),
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: &scene_view,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Load,
                                store: StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    pass.set_scissor_rect(x, y, width, height);
                    pass.set_pipeline(&self.backdrop_gpu.pipeline);
                    pass.set_bind_group(0, &vertical_group, &[]);
                    pass.draw(0..3, 0..1);
                }
                DrawCommand::Quads {
                    vertices,
                    clip,
                    resource,
                    pixelated,
                    ..
                } => {
                    let Some((x, y, width, height)) = self.scissor(clip, scale) else {
                        continue;
                    };
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("whisker Desktop boxes"),
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: &scene_view,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Load,
                                store: StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    pass.set_scissor_rect(x, y, width, height);
                    pass.set_pipeline(&self.box_gpu.pipeline);
                    pass.set_bind_group(0, &self.box_gpu.viewport_bind_group, &[]);
                    pass.set_bind_group(1, &shape_clip_bind_group, &[]);
                    let image = resource
                        .and_then(|resource| self.image_resources.get(&resource))
                        .unwrap_or(&self.box_gpu.fallback_image);
                    pass.set_bind_group(
                        2,
                        if pixelated {
                            &image.nearest_bind_group
                        } else {
                            &image.linear_bind_group
                        },
                        &[],
                    );
                    pass.set_vertex_buffer(
                        0,
                        vertex_buffer
                            .as_ref()
                            .expect("quad draw has vertices")
                            .slice(..),
                    );
                    pass.draw(vertices, draw_index as u32..draw_index as u32 + 1);
                }
                DrawCommand::Text { node, .. } => {
                    if !prepared_text_nodes.contains(&node) {
                        continue;
                    }
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("whisker Desktop text"),
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: &scene_view,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Load,
                                store: StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    self.text_renderers
                        .get(&node)
                        .expect("prepared text renderer remains retained")
                        .render(&self.text_atlas, &self.text_viewport, &mut pass)
                        .map_err(|error| GpuError(format!("encode glyph draw: {error}")))?;
                }
            }
        }
        let present_uniform = BackdropUniform {
            direction: [0.0, 0.0],
            radius: 0.0,
            _padding: 0.0,
        };
        let (_present_buffer, present_group) =
            self.backdrop_gpu
                .bind_group(&self.device, &scene_view, present_uniform);
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("whisker Desktop present composited scene"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.backdrop_gpu.pipeline);
            pass.set_bind_group(0, &present_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        self.text_atlas.trim();
        Ok(())
    }

    fn scissor(&self, clip: LogicalClip, scale: f32) -> Option<(u32, u32, u32, u32)> {
        let left = (clip.left.unwrap_or(0.0) * scale).floor().max(0.0) as u32;
        let top = (clip.top.unwrap_or(0.0) * scale).floor().max(0.0) as u32;
        let right = (clip.right.unwrap_or(self.config.width as f32 / scale) * scale)
            .ceil()
            .clamp(0.0, self.config.width as f32) as u32;
        let bottom = (clip.bottom.unwrap_or(self.config.height as f32 / scale) * scale)
            .ceil()
            .clamp(0.0, self.config.height as f32) as u32;
        (right > left && bottom > top).then_some((left, top, right - left, bottom - top))
    }
}

#[cfg(test)]
fn push_quad(vertices: &mut Vec<BoxVertex>, primitive: BoxPrimitive) {
    push_transformed_quad(vertices, primitive, Transform::IDENTITY);
}

fn push_transformed_quad(
    vertices: &mut Vec<BoxVertex>,
    primitive: BoxPrimitive,
    transform: Transform,
) {
    let rect = primitive.outer_rect;
    let left = rect.x;
    let top = rect.y;
    let right = rect.x + rect.width;
    let bottom = rect.y + rect.height;
    for position in [
        [left, top],
        [right, top],
        [left, bottom],
        [left, bottom],
        [right, top],
        [right, bottom],
    ] {
        vertices.push(BoxVertex {
            position,
            color: primitive.color,
            outer_rect: [rect.x, rect.y, rect.width, rect.height],
            outer_radii_x: primitive.outer_radii_x,
            outer_radii_y: primitive.outer_radii_y,
            inner_rect: [
                primitive.inner_rect.x,
                primitive.inner_rect.y,
                primitive.inner_rect.width,
                primitive.inner_rect.height,
            ],
            inner_radii_x: primitive.inner_radii_x,
            inner_radii_y: primitive.inner_radii_y,
            border_widths: primitive.border_widths,
            mode: primitive.kind.shader_mode(),
            border_colors: primitive.border_colors,
            border_styles: primitive.border_styles,
            transformed_position: transform_point(transform, position),
        });
    }
}

fn transform_point(transform: Transform, [x, y]: [f32; 2]) -> [f32; 4] {
    [
        transform.0[0] * x + transform.0[4] * y + transform.0[12],
        transform.0[1] * x + transform.0[5] * y + transform.0[13],
        transform.0[2] * x + transform.0[6] * y + transform.0[14],
        transform.0[3] * x + transform.0[7] * y + transform.0[15],
    ]
}

fn text_bounds(clip: LogicalClip, width: u32, height: u32, scale: f32) -> TextBounds {
    TextBounds {
        left: (clip.left.unwrap_or(0.0) * scale).floor().max(0.0) as i32,
        top: (clip.top.unwrap_or(0.0) * scale).floor().max(0.0) as i32,
        right: (clip.right.unwrap_or(width as f32 / scale) * scale)
            .ceil()
            .clamp(0.0, width as f32) as i32,
        bottom: (clip.bottom.unwrap_or(height as f32 / scale) * scale)
            .ceil()
            .clamp(0.0, height as f32) as i32,
    }
}

#[cfg(all(test, feature = "host-conformance"))]
pub(crate) async fn render_box_primitives_offscreen(
    primitives: &[BoxPrimitive],
    logical_size: [u32; 2],
) -> Result<Vec<u8>, GpuError> {
    let clipped = primitives
        .iter()
        .copied()
        .map(|primitive| {
            (
                primitive,
                LogicalClip::default(),
                Transform::IDENTITY,
                ShapeClipStack::default(),
                None,
                None,
                false,
            )
        })
        .collect::<Vec<_>>();
    render_clipped_box_primitives_offscreen(&clipped, logical_size, &HashMap::new()).await
}

#[cfg(all(test, feature = "host-conformance"))]
pub(crate) async fn render_clipped_box_primitives_offscreen(
    primitives: &[ClippedBoxPrimitive],
    logical_size: [u32; 2],
    resources: &HashMap<ResourceId, RasterResource>,
) -> Result<Vec<u8>, GpuError> {
    render_clipped_box_primitives_with_backdrops_offscreen(primitives, &[], logical_size, resources)
        .await
}

#[cfg(all(test, feature = "host-conformance"))]
pub(crate) async fn render_clipped_box_primitives_with_backdrops_offscreen(
    primitives: &[ClippedBoxPrimitive],
    backdrops: &[BackdropCheckpoint],
    logical_size: [u32; 2],
    resources: &HashMap<ResourceId, RasterResource>,
) -> Result<Vec<u8>, GpuError> {
    let [width, height] = logical_size;
    if width == 0 || height == 0 {
        return Err(GpuError(
            "offscreen checkpoint has an empty viewport".into(),
        ));
    }

    let instance = Instance::new(&InstanceDescriptor::default());
    let adapter = instance
        .request_adapter(&RequestAdapterOptions::default())
        .await
        .map_err(|error| GpuError(format!("select offscreen Desktop GPU adapter: {error}")))?;
    let (device, queue) = adapter
        .request_device(&DeviceDescriptor::default())
        .await
        .map_err(|error| GpuError(format!("create offscreen Desktop GPU device: {error}")))?;
    let format = TextureFormat::Rgba8Unorm;
    let box_gpu = BoxGpuPipeline::new(&device, &queue, format);
    let backdrop_gpu = BackdropGpuPipeline::new(&device, format);
    box_gpu.update_viewport(
        &queue,
        [width as f32, height as f32],
        [width as f32, height as f32],
    );

    let mut vertices = Vec::new();
    let mut draws = Vec::new();
    for (primitive, clip, transform, shape_clips, gradient, resource, pixelated) in primitives {
        let start = vertices.len() as u32;
        push_transformed_quad(&mut vertices, *primitive, *transform);
        draws.push(DrawCommand::Quads {
            vertices: start..vertices.len() as u32,
            clip: *clip,
            shape_clips: shape_clips.clone(),
            gradient: gradient.clone(),
            resource: *resource,
            pixelated: *pixelated,
        });
    }
    let image_resources = resources
        .iter()
        .map(|(resource, raster)| (*resource, box_gpu.upload_image(&device, &queue, raster)))
        .collect::<HashMap<_, _>>();
    let vertex_buffer = (!vertices.is_empty()).then(|| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whisker Desktop conformance box vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: BufferUsages::VERTEX,
        })
    });
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("whisker Desktop conformance checkpoint"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT
            | TextureUsages::TEXTURE_BINDING
            | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor::default());
    let scratch = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("whisker Desktop conformance backdrop scratch"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let scratch_view = scratch.create_view(&TextureViewDescriptor::default());
    let unpadded_bytes_per_row = width * 4;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("whisker Desktop conformance readback"),
        size: u64::from(padded_bytes_per_row) * u64::from(height),
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("whisker Desktop conformance encoder"),
    });
    let shape_clip_bind_group = box_gpu.shape_clip_bind_group(&device, &draws);
    {
        encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("whisker Desktop conformance clear"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }
    if let Some(vertex_buffer) = &vertex_buffer {
        for (draw_index, draw) in draws.into_iter().enumerate() {
            let DrawCommand::Quads {
                vertices: range,
                clip,
                resource,
                pixelated,
                ..
            } = draw
            else {
                unreachable!();
            };
            let Some((x, y, clip_width, clip_height)) = offscreen_scissor(clip, width, height)
            else {
                continue;
            };
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("whisker Desktop conformance boxes"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Load,
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_scissor_rect(x, y, clip_width, clip_height);
            pass.set_pipeline(&box_gpu.pipeline);
            pass.set_bind_group(0, &box_gpu.viewport_bind_group, &[]);
            pass.set_bind_group(1, &shape_clip_bind_group, &[]);
            let image = resource
                .and_then(|resource| image_resources.get(&resource))
                .unwrap_or(&box_gpu.fallback_image);
            pass.set_bind_group(
                2,
                if pixelated {
                    &image.nearest_bind_group
                } else {
                    &image.linear_bind_group
                },
                &[],
            );
            pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            pass.draw(range, draw_index as u32..draw_index as u32 + 1);
        }
    }
    for (rect, radius, clip) in backdrops {
        let (_horizontal_buffer, horizontal_group) = backdrop_gpu.bind_group(
            &device,
            &view,
            BackdropUniform {
                direction: [1.0, 0.0],
                radius: *radius,
                _padding: 0.0,
            },
        );
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("whisker Desktop conformance horizontal backdrop blur"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &scratch_view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&backdrop_gpu.pipeline);
            pass.set_bind_group(0, &horizontal_group, &[]);
            pass.draw(0..3, 0..1);
        }
        let (_vertical_buffer, vertical_group) = backdrop_gpu.bind_group(
            &device,
            &scratch_view,
            BackdropUniform {
                direction: [0.0, 1.0],
                radius: *radius,
                _padding: 0.0,
            },
        );
        let Some((x, y, clip_width, clip_height)) =
            offscreen_scissor(clip.intersect(*rect, true, true), width, height)
        else {
            continue;
        };
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("whisker Desktop conformance vertical backdrop blur"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_scissor_rect(x, y, clip_width, clip_height);
        pass.set_pipeline(&backdrop_gpu.pipeline);
        pass.set_bind_group(0, &vertical_group, &[]);
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::wait())
        .map_err(|error| GpuError(format!("wait for Desktop GPU checkpoint: {error}")))?;
    receiver
        .recv()
        .map_err(|error| GpuError(format!("receive Desktop GPU checkpoint: {error}")))?
        .map_err(|error| GpuError(format!("map Desktop GPU checkpoint: {error}")))?;

    let mapped = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for row in mapped.chunks_exact(padded_bytes_per_row as usize) {
        pixels.extend_from_slice(&row[..unpadded_bytes_per_row as usize]);
    }
    drop(mapped);
    readback.unmap();
    Ok(pixels)
}

#[cfg(all(test, feature = "host-conformance"))]
fn offscreen_scissor(clip: LogicalClip, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    let left = clip.left.unwrap_or(0.0).floor().max(0.0) as u32;
    let top = clip.top.unwrap_or(0.0).floor().max(0.0) as u32;
    let right = clip
        .right
        .unwrap_or(width as f32)
        .ceil()
        .clamp(0.0, width as f32) as u32;
    let bottom = clip
        .bottom
        .unwrap_or(height as f32)
        .ceil()
        .clamp(0.0, height as f32) as u32;
    (right > left && bottom > top).then_some((left, top, right - left, bottom - top))
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyphon::Color as TextColor;
    use whisker_protocol::{
        BackgroundAttachment, BackgroundLayer, BackgroundSize, BlendMode, BorderLineStyle,
        BoxPaint, ImageRepeat, LayoutRect, PaintBox, PaintColor, PaintCoordinate,
        PaintCornerRadius, PaintCorners, PaintEdges, PaintImage, PaintLengthPercentage,
        PaintPosition, ResourceId,
    };

    use crate::paint::box_paint::{ResolvedRadii, resolve_box_geometry, resolve_radii};
    use crate::paint::color::srgba;

    fn radius(length: f32, fraction: f32) -> PaintCornerRadius {
        PaintCornerRadius::circular(PaintLengthPercentage { length, fraction })
    }

    fn lower_vertices(
        rect: LayoutRect,
        paint: &BoxPaint,
        opacity: f32,
        vertices: &mut Vec<BoxVertex>,
    ) {
        lower_box(rect, paint, opacity, |primitive| {
            push_quad(vertices, primitive);
        });
    }

    fn paint(background_color: PaintColor) -> BoxPaint {
        let zero = PaintLengthPercentage::default();
        BoxPaint {
            background_color,
            border_widths: PaintEdges {
                top: PaintLengthPercentage {
                    length: 1.0,
                    fraction: 0.0,
                },
                right: zero,
                bottom: zero,
                left: zero,
            },
            border_colors: PaintEdges {
                top: PaintColor::Named("blue".into()),
                right: PaintColor::default(),
                bottom: PaintColor::default(),
                left: PaintColor::default(),
            },
            border_styles: PaintEdges {
                top: BorderLineStyle::Solid,
                right: BorderLineStyle::None,
                bottom: BorderLineStyle::Hidden,
                left: BorderLineStyle::None,
            },
            border_radii: PaintCorners {
                top_left: PaintCornerRadius::default(),
                top_right: PaintCornerRadius::default(),
                bottom_right: PaintCornerRadius::default(),
                bottom_left: PaintCornerRadius::default(),
            },
        }
    }

    fn resource_layer(size: BackgroundSize) -> BackgroundLayer {
        BackgroundLayer {
            image: PaintImage::Resource(ResourceId::new(1).unwrap()),
            position: PaintPosition {
                x: PaintCoordinate {
                    length: 0.0,
                    fraction: 0.5,
                },
                y: PaintCoordinate {
                    length: 0.0,
                    fraction: 0.5,
                },
            },
            size,
            repeat_x: ImageRepeat::NoRepeat,
            repeat_y: ImageRepeat::NoRepeat,
            origin: PaintBox::Padding,
            clip: PaintBox::Border,
            attachment: BackgroundAttachment::Scroll,
            blend_mode: BlendMode::Normal,
        }
    }

    #[test]
    fn intrinsic_background_sizes_preserve_resource_aspect_ratio() {
        let area = LayoutRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 80.0,
        };
        let intrinsic = [4.0, 2.0];

        let auto =
            background_tile_geometry(area, &resource_layer(BackgroundSize::Auto), Some(intrinsic))
                .unwrap();
        assert_eq!(
            auto.rect,
            LayoutRect {
                x: 48.0,
                y: 39.0,
                width: 4.0,
                height: 2.0
            }
        );

        let contain = background_tile_geometry(
            area,
            &resource_layer(BackgroundSize::Contain),
            Some(intrinsic),
        )
        .unwrap();
        assert_eq!(
            contain.rect,
            LayoutRect {
                x: 0.0,
                y: 15.0,
                width: 100.0,
                height: 50.0
            }
        );

        let cover = background_tile_geometry(
            area,
            &resource_layer(BackgroundSize::Cover),
            Some(intrinsic),
        )
        .unwrap();
        assert_eq!(
            cover.rect,
            LayoutRect {
                x: -30.0,
                y: 0.0,
                width: 160.0,
                height: 80.0
            }
        );

        let width = background_tile_geometry(
            area,
            &resource_layer(BackgroundSize::Explicit {
                width: Some(PaintLengthPercentage {
                    length: 60.0,
                    fraction: 0.0,
                }),
                height: None,
            }),
            Some(intrinsic),
        )
        .unwrap();
        assert_eq!(
            width.rect,
            LayoutRect {
                x: 20.0,
                y: 25.0,
                width: 60.0,
                height: 30.0
            }
        );
    }

    #[test]
    fn one_axis_round_rescales_the_opposite_auto_axis() {
        let area = LayoutRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 80.0,
        };
        let intrinsic = [4.0, 2.0];

        let mut round_width = resource_layer(BackgroundSize::Explicit {
            width: Some(PaintLengthPercentage {
                length: 40.0,
                fraction: 0.0,
            }),
            height: None,
        });
        round_width.position = PaintPosition::default();
        round_width.repeat_x = ImageRepeat::Round;
        let horizontal = background_tile_geometry(area, &round_width, Some(intrinsic)).unwrap();
        assert!((horizontal.rect.width - 100.0 / 3.0).abs() < 0.001);
        assert!((horizontal.rect.height - 50.0 / 3.0).abs() < 0.001);

        let mut round_height = resource_layer(BackgroundSize::Explicit {
            width: None,
            height: Some(PaintLengthPercentage {
                length: 30.0,
                fraction: 0.0,
            }),
        });
        round_height.position = PaintPosition::default();
        round_height.repeat_y = ImageRepeat::Round;
        let vertical = background_tile_geometry(area, &round_height, Some(intrinsic)).unwrap();
        assert!((vertical.rect.width - 160.0 / 3.0).abs() < 0.001);
        assert!((vertical.rect.height - 80.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn box_lowering_emits_background_and_visible_borders() {
        let mut vertices = Vec::new();
        lower_vertices(
            LayoutRect {
                x: 2.0,
                y: 3.0,
                width: 20.0,
                height: 10.0,
            },
            &paint(PaintColor::Named("red".into())),
            0.5,
            &mut vertices,
        );
        assert_eq!(vertices.len(), 12);
        assert_eq!(vertices[0].position, [2.0, 3.0]);
        assert_eq!(vertices[0].outer_rect, [2.0, 3.0, 20.0, 10.0]);
        assert_eq!(vertices[0].outer_radii_x, [0.0; 4]);
        assert_eq!(vertices[0].outer_radii_y, [0.0; 4]);
        assert_eq!(vertices[0].mode, -1.0);
        assert!((vertices[0].color[3] - 0.5).abs() < f32::EPSILON);
        assert_eq!(vertices[6].position, [2.0, 3.0]);
        assert_eq!(vertices[8].position, [2.0, 13.0]);
        assert_eq!(vertices[6].inner_rect, [2.0, 4.0, 20.0, 9.0]);
        assert_eq!(vertices[6].border_widths, [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(vertices[6].mode, 1.0);
        assert!(vertices[6].border_colors[0][2] > 0.99);
        assert_eq!(vertices[6].border_colors[1], [0.0; 4]);

        vertices.clear();
        let mut transparent = paint(PaintColor::Named("transparent".into()));
        transparent.border_styles.top = BorderLineStyle::None;
        lower_vertices(LayoutRect::default(), &transparent, 1.0, &mut vertices);
        assert!(vertices.is_empty());
    }

    #[test]
    fn rounded_radii_resolve_percentages_and_scale_overlaps() {
        let resolved = resolve_radii(
            &PaintCorners {
                top_left: radius(30.0, 0.0),
                top_right: radius(0.0, 0.5),
                bottom_right: radius(30.0, 0.0),
                bottom_left: radius(30.0, 0.0),
            },
            LayoutRect {
                x: 5.0,
                y: 6.0,
                width: 100.0,
                height: 40.0,
            },
        );
        assert!((resolved.horizontal[0] - 20.0).abs() < f32::EPSILON);
        assert!((resolved.horizontal[1] - 100.0 / 3.0).abs() < 0.001);
        assert!((resolved.horizontal[2] - 20.0).abs() < f32::EPSILON);
        assert!((resolved.horizontal[3] - 20.0).abs() < f32::EPSILON);
        assert!((resolved.vertical[0] - 20.0).abs() < f32::EPSILON);
        assert!((resolved.vertical[1] - 40.0 / 3.0).abs() < 0.001);
        assert!((resolved.vertical[2] - 20.0).abs() < f32::EPSILON);
        assert!((resolved.vertical[3] - 20.0).abs() < f32::EPSILON);

        let mut rounded = paint(PaintColor::Named("red".into()));
        rounded.border_radii = PaintCorners {
            top_left: radius(8.0, 0.0),
            top_right: PaintCornerRadius::default(),
            bottom_right: PaintCornerRadius::default(),
            bottom_left: PaintCornerRadius::default(),
        };
        let mut vertices = Vec::new();
        lower_vertices(
            LayoutRect {
                x: 1.0,
                y: 2.0,
                width: 20.0,
                height: 10.0,
            },
            &rounded,
            1.0,
            &mut vertices,
        );
        assert_eq!(vertices[0].outer_radii_x, [8.0, 0.0, 0.0, 0.0]);
        assert_eq!(vertices[0].outer_radii_y, [8.0, 0.0, 0.0, 0.0]);
        assert_eq!(vertices[6].inner_radii_x, [8.0, 0.0, 0.0, 0.0]);
        assert_eq!(vertices[6].inner_radii_y, [7.0, 0.0, 0.0, 0.0]);
        assert_eq!(
            resolve_radii(&rounded.border_radii, LayoutRect::default()),
            ResolvedRadii {
                horizontal: [0.0; 4],
                vertical: [0.0; 4],
            }
        );
    }

    #[test]
    fn rounded_border_geometry_preserves_corner_arc_centers() {
        let three = PaintLengthPercentage {
            length: 3.0,
            fraction: 0.0,
        };
        let mut bordered = paint(PaintColor::Named("green".into()));
        bordered.border_widths = PaintEdges {
            top: three,
            right: three,
            bottom: three,
            left: three,
        };
        bordered.border_styles = PaintEdges {
            top: BorderLineStyle::Solid,
            right: BorderLineStyle::Solid,
            bottom: BorderLineStyle::Solid,
            left: BorderLineStyle::Solid,
        };
        bordered.border_colors = PaintEdges {
            top: PaintColor::Named("yellow".into()),
            right: PaintColor::Named("yellow".into()),
            bottom: PaintColor::Named("yellow".into()),
            left: PaintColor::Named("yellow".into()),
        };
        bordered.border_radii = PaintCorners {
            top_left: radius(40.0, 0.0),
            top_right: radius(8.0, 0.0),
            bottom_right: radius(40.0, 0.0),
            bottom_left: radius(8.0, 0.0),
        };
        let geometry = resolve_box_geometry(
            LayoutRect {
                x: 10.0,
                y: 20.0,
                width: 200.0,
                height: 88.0,
            },
            &bordered,
        );

        assert_eq!(geometry.inner_rect.x, 13.0);
        assert_eq!(geometry.inner_rect.y, 23.0);
        assert_eq!(geometry.inner_rect.width, 194.0);
        assert_eq!(geometry.inner_rect.height, 82.0);
        assert_eq!(geometry.outer_radii.horizontal, [40.0, 8.0, 40.0, 8.0]);
        assert_eq!(geometry.outer_radii.vertical, [40.0, 8.0, 40.0, 8.0]);
        assert_eq!(geometry.inner_radii.horizontal, [37.0, 5.0, 37.0, 5.0]);
        assert_eq!(geometry.inner_radii.vertical, [37.0, 5.0, 37.0, 5.0]);
        assert_eq!(
            geometry.outer_rect.x + geometry.outer_radii.horizontal[0],
            geometry.inner_rect.x + geometry.inner_radii.horizontal[0]
        );
        assert_eq!(
            geometry.outer_rect.y + geometry.outer_radii.vertical[0],
            geometry.inner_rect.y + geometry.inner_radii.vertical[0]
        );

        let mut vertices = Vec::new();
        lower_vertices(geometry.outer_rect, &bordered, 1.0, &mut vertices);
        assert_eq!(vertices.len(), 12);
        assert_eq!(vertices[6].mode, 1.0);
        assert!(
            vertices[6]
                .border_colors
                .windows(2)
                .all(|colors| colors[0] == colors[1])
        );
    }

    #[test]
    fn protocol_colors_convert_to_gpu_and_glyph_colors() {
        let hsl = PaintColor::Hsla {
            hue_degrees: 120.0,
            saturation: 100.0,
            lightness: 50.0,
            alpha: 0.8,
        };
        let [red, green, blue, alpha] = srgba(&hsl, 0.5);
        assert!(red.abs() < f32::EPSILON);
        assert!((green - 1.0).abs() < f32::EPSILON);
        assert!(blue.abs() < f32::EPSILON);
        assert!((alpha - 0.4).abs() < f32::EPSILON);
        assert_eq!(text_color(&hsl, 0.5), TextColor::rgba(0, 255, 0, 102));

        assert_eq!(
            srgba(&PaintColor::Named("not-a-css-color".into()), 1.0),
            [0.0; 4]
        );
    }

    #[test]
    fn text_bounds_scale_and_clamp_logical_clips() {
        let bounds = text_bounds(
            LogicalClip {
                left: Some(-2.0),
                top: Some(3.0),
                right: Some(70.0),
                bottom: None,
            },
            100,
            80,
            2.0,
        );
        assert_eq!(
            bounds,
            TextBounds {
                left: 0,
                top: 6,
                right: 100,
                bottom: 80
            }
        );
    }
}
