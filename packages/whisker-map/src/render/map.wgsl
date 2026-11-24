struct Frame {
    // x, y viewport in logical pixels; z pixels per logical pixel; w 1 when the target encodes sRGB.
    viewport: vec4<f32>,
    // xyz direction toward the light, w intensity.
    light: vec4<f32>,
    light_color: vec4<f32>,
    // Takes Earth-fixed unit vectors to clip space.
    globe: mat4x4<f32>,
    globe_inverse: mat4x4<f32>,
    // x how far the map has turned into a globe, y the horizon's dot product with camera_dir,
    // z cosine of the center's latitude, w the globe's radius in world pixels.
    globe_params: vec4<f32>,
    camera_dir: vec4<f32>,
    // Premultiplied colors of the atmosphere at the limb and of space beyond it.
    halo: vec4<f32>,
    space: vec4<f32>,
};

struct Draw {
    matrix: mat4x4<f32>,
    // Premultiplied color, used unless the vertex carries its own.
    color: vec4<f32>,
    // x 1 to use the vertex color, y opacity, z tile units per logical pixel, w width or radius.
    params: vec4<f32>,
    // Line: x gap width, y offset, z blur, w dash count. Circle: x stroke width, y blur.
    extra: vec4<f32>,
    // Circle stroke color, premultiplied.
    color2: vec4<f32>,
    dash0: vec4<f32>,
    dash1: vec4<f32>,
    // The tile's Mercator origin, 0 to 1 across the world, and the size of one tile unit.
    tile: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var<uniform> draw: Draw;
@group(2) @binding(0) var tex: texture_2d<f32>;
@group(2) @binding(1) var tex_sampler: sampler;

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3(2.4));
    return select(high, low, c <= vec3(0.04045));
}

// Colors are premultiplied sRGB; a target that encodes sRGB itself needs linear values.
fn finish(c: vec4<f32>) -> vec4<f32> {
    if frame.viewport.w < 0.5 || c.a <= 0.0 {
        return c;
    }
    return vec4(to_linear(c.rgb / c.a) * c.a, c.a);
}

fn base_color(vertex_color: vec4<f32>) -> vec4<f32> {
    let c = select(draw.color, vertex_color, draw.params.x > 0.5);
    return c * draw.params.y;
}

// ---- Projection ----

const PI: f32 = 3.141592653589793;

fn tile_latitude(pos: vec2<f32>) -> f32 {
    let y = draw.tile.y + pos.y * draw.tile.z;
    if y <= 0.0 {
        return PI / 2.0;
    }
    if y >= 1.0 {
        return -PI / 2.0;
    }
    return atan(sinh(PI * (1.0 - 2.0 * y)));
}

fn globe_unit(pos: vec2<f32>) -> vec3<f32> {
    let lng = (draw.tile.x + pos.x * draw.tile.z) * 2.0 * PI - PI;
    let lat = tile_latitude(pos);
    return vec3(cos(lat) * cos(lng), cos(lat) * sin(lng), sin(lat));
}

// Depth runs from the point under the camera to the horizon, so the far side is clipped.
fn globe_clip(unit: vec3<f32>) -> vec4<f32> {
    var clip = frame.globe * vec4(unit, 1.0);
    let horizon = frame.globe_params.y;
    let facing = (dot(unit, frame.camera_dir.xyz) - horizon) / max(1.0 - horizon, 1e-6);
    clip.z = (1.0 - facing) * clip.w;
    return clip;
}

// Mercator and the globe, blended by how far the map has turned into a globe.
fn tile_clip(pos: vec2<f32>) -> vec4<f32> {
    let plane = draw.matrix * vec4(pos, 0.0, 1.0);
    let t = frame.globe_params.x;
    if t <= 0.0 {
        return plane;
    }
    return mix(plane, globe_clip(globe_unit(pos)), t);
}

// Tile units shrink on the globe away from the center's latitude, as Mercator stretches them.
fn globe_stretch(pos: vec2<f32>) -> f32 {
    let t = frame.globe_params.x;
    if t <= 0.0 {
        return 1.0;
    }
    return mix(1.0, frame.globe_params.z / max(cos(tile_latitude(pos)), 0.01), t);
}

// ---- Stencil masks and background ----

struct PlainOut {
    // Masks and the draws they clip must land on the same pixels.
    @invariant @builtin(position) clip: vec4<f32>,
};

@vertex
fn vs_mask(@location(0) pos: vec2<f32>) -> PlainOut {
    var out: PlainOut;
    out.clip = tile_clip(pos);
    return out;
}

@vertex
fn vs_screen(@builtin(vertex_index) index: u32) -> PlainOut {
    let x = f32(index & 1u) * 4.0 - 1.0;
    let y = f32(index >> 1u) * 4.0 - 1.0;
    var out: PlainOut;
    out.clip = vec4(x, y, 0.0, 1.0);
    return out;
}

@fragment
fn fs_solid() -> @location(0) vec4<f32> {
    return finish(draw.color * draw.params.y);
}

// ---- Fill ----

struct FillOut {
    @invariant @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_fill(@location(0) pos: vec2<f32>, @location(1) color: vec4<f32>) -> FillOut {
    var out: FillOut;
    out.clip = tile_clip(pos);
    out.color = base_color(color);
    return out;
}

@fragment
fn fs_fill(@location(0) color: vec4<f32>) -> @location(0) vec4<f32> {
    return finish(color);
}

// ---- Line ----

struct LineOut {
    @invariant @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    // x distance along the line in widths, y across it in pixels from the center, z half width.
    @location(1) data: vec3<f32>,
};

@vertex
fn vs_line(
    @location(0) pos: vec2<f32>,
    @location(1) extrude: vec2<f32>,
    @location(2) data: vec4<f32>,
    @location(3) color: vec4<f32>,
) -> LineOut {
    let units = draw.params.z;
    let width = select(draw.params.w, data.z, data.z > 0.0);
    let gap = draw.extra.x;
    // With a gap, the line becomes two strokes on either side of it.
    let half = select(width / 2.0, gap / 2.0 + width, gap > 0.0);
    // One device pixel more on each side leaves room for the antialiased edge.
    let outset = half + 1.0 / frame.viewport.z;
    let p = pos + extrude * outset * units * globe_stretch(pos);
    var out: LineOut;
    out.clip = tile_clip(p);
    out.color = base_color(color);
    out.data = vec3(data.x / max(width * units, 1e-6), data.y * outset, half);
    return out;
}

fn dash_at(i: u32) -> f32 {
    if i < 4u {
        return draw.dash0[i];
    }
    return draw.dash1[i - 4u];
}

@fragment
fn fs_line(@location(0) color: vec4<f32>, @location(1) data: vec3<f32>) -> @location(0) vec4<f32> {
    var alpha = 1.0;
    let across = abs(data.y);
    let gap = draw.extra.x;
    // Edges fade over one device pixel, widened by the style's blur, centered on the true edge.
    let blur = max(draw.extra.z, 1.0 / frame.viewport.z);
    if gap > 0.0 {
        alpha = clamp((across - gap / 2.0) / blur + 0.5, 0.0, 1.0);
    }
    alpha *= clamp((data.z - across) / blur + 0.5, 0.0, 1.0);
    let count = u32(draw.extra.w);
    if count > 1u {
        var total = 0.0;
        for (var i = 0u; i < count; i++) {
            total += dash_at(i);
        }
        if total > 0.0 {
            let t = data.x - floor(data.x / total) * total;
            var start = 0.0;
            var on = false;
            for (var i = 0u; i < count; i++) {
                let end = start + dash_at(i);
                if t >= start && t < end {
                    on = i % 2u == 0u;
                }
                start = end;
            }
            if !on {
                discard;
            }
        }
    }
    if alpha <= 0.0 {
        discard;
    }
    return finish(color * alpha);
}

// ---- Circle ----

struct CircleOut {
    @invariant @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    // xy position within the quad in pixels, z radius, w stroke width.
    @location(1) data: vec4<f32>,
};

@vertex
fn vs_circle(
    @location(0) pos: vec2<f32>,
    @location(1) corner: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) data: vec4<f32>,
) -> CircleOut {
    let radius = select(draw.params.w, data.x, data.x > 0.0);
    let stroke = draw.extra.x;
    let extent = radius + stroke + 1.0;
    var clip = tile_clip(pos);
    clip = vec4(clip.xy + corner * extent * 2.0 / frame.viewport.xy * clip.w, clip.zw);
    var out: CircleOut;
    out.clip = clip;
    out.color = base_color(color);
    out.data = vec4(corner * extent, radius, stroke);
    return out;
}

@fragment
fn fs_circle(@location(0) color: vec4<f32>, @location(1) data: vec4<f32>) -> @location(0) vec4<f32> {
    let d = length(data.xy);
    let radius = data.z;
    let stroke = data.w;
    let aa = 1.0 / frame.viewport.z;
    let blur = max(draw.extra.y * radius, aa);
    let outer = clamp((radius + stroke - d) / blur + 0.5, 0.0, 1.0);
    if outer <= 0.0 {
        discard;
    }
    let inside = clamp((radius - d) / aa + 0.5, 0.0, 1.0);
    let stroke_color = draw.color2 * draw.params.y;
    let c = select(color, mix(stroke_color, color, inside), stroke > 0.0);
    return finish(c * outer);
}

// ---- Raster ----

struct RasterOut {
    @invariant @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_raster(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>) -> RasterOut {
    var out: RasterOut;
    out.clip = tile_clip(pos);
    out.uv = uv;
    return out;
}

@fragment
fn fs_raster(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    var c = textureSample(tex, tex_sampler, uv);
    // x brightness min, y brightness max, z saturation, w contrast.
    let adjust = draw.extra;
    var rgb = c.rgb;
    let gray = (rgb.r + rgb.g + rgb.b) / 3.0;
    if adjust.z > 0.0 {
        rgb = rgb + (gray - rgb) * (1.0 - 1.0 / (1.001 - adjust.z));
    } else {
        rgb = rgb + (gray - rgb) * -adjust.z;
    }
    let contrast = select(1.0 / (1.0 - adjust.w), 1.0 + adjust.w, adjust.w <= 0.0);
    rgb = (rgb - 0.5) * contrast + 0.5;
    rgb = mix(vec3(adjust.x), vec3(adjust.y), rgb);
    let a = c.a * draw.params.y;
    return finish(vec4(clamp(rgb, vec3(0.0), vec3(1.0)) * a, a));
}

// ---- Fill extrusion ----

struct ExtrusionOut {
    // The depth prepass and the color pass must compute identical depths for the equal test.
    @invariant @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_extrusion(
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
) -> ExtrusionOut {
    // params.z is world pixels per meter, the unit the tile matrix leaves z in.
    let p = vec3(pos.xy, pos.z * draw.params.z);
    var out: ExtrusionOut;
    out.clip = draw.matrix * vec4(p, 1.0);
    let t = frame.globe_params.x;
    if t > 0.0 {
        let lift = 1.0 + p.z / frame.globe_params.w;
        out.clip = mix(out.clip, frame.globe * vec4(globe_unit(p.xy) * lift, 1.0), t);
    }
    let c = select(draw.color, color, draw.params.x > 0.5);
    // MapLibre's fill-extrusion lighting, with a wall's base taken as 0 m unless it is raised.
    let roof = normal.z > 0.5;
    let n = select(vec3(normal.xy, 0.0), vec3(0.0, 0.0, 1.0), roof);
    let intensity = frame.light.w;
    let luminance = dot(c.rgb, vec3(0.2126, 0.7152, 0.0722));
    var shade = mix(
        1.0 - intensity,
        max(1.0 - luminance + intensity, 1.0),
        clamp(dot(n, frame.light.xyz), 0.0, 1.0),
    );
    if !roof && draw.extra.x > 0.5 {
        let top = normal.z > 0.1;
        let raised = select(0.0, 1.0, pos.z > 0.0);
        let gradient = select(raised, sqrt(pos.z / 150.0), top);
        shade *= clamp(gradient, mix(0.7, 0.98, 1.0 - intensity), 1.0);
    }
    let floor = mix(vec3(0.0), vec3(0.3), 1.0 - frame.light_color.rgb);
    let lit = clamp(c.rgb * shade * frame.light_color.rgb, floor, vec3(1.0));
    let a = c.a * draw.params.y;
    out.color = vec4(lit * a, a);
    return out;
}

@fragment
fn fs_extrusion(@location(0) color: vec4<f32>) -> @location(0) vec4<f32> {
    return finish(color);
}

// ---- Atmosphere ----

// How far the ray through a fragment passes from the Earth's center, in Earth radii.
fn ray_miss(position: vec4<f32>) -> f32 {
    let size = frame.viewport.xy * frame.viewport.z;
    let ndc = vec2(position.x / size.x * 2.0 - 1.0, 1.0 - position.y / size.y * 2.0);
    let near = frame.globe_inverse * vec4(ndc, 0.0, 1.0);
    let far = frame.globe_inverse * vec4(ndc, 1.0, 1.0);
    let origin = near.xyz / near.w;
    let dir = normalize(far.xyz / far.w - origin);
    return length(origin - dir * dot(origin, dir));
}

@fragment
fn fs_atmosphere(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let glow = exp(-max(ray_miss(position) - 1.0, 0.0) * 14.0);
    return finish(mix(frame.space, frame.halo, glow) * frame.globe_params.x);
}

// The background covers the globe; once it starts flattening, it is larger than the screen.
@fragment
fn fs_globe_background(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    if frame.globe_params.x >= 1.0 && ray_miss(position) > 1.0 {
        discard;
    }
    return finish(draw.color * draw.params.y);
}
