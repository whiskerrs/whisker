pub(super) const BOX_SHADER: &str = r#"
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
        0.0,
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

pub(super) const BACKDROP_SHADER: &str = r#"
struct BlurUniform {
    direction: vec2<f32>,
    radius: f32,
    opacity: f32,
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
        return textureSample(source_texture, source_sampler, input.uv) * blur.opacity;
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
    return color / weight_sum * blur.opacity;
}
"#;
