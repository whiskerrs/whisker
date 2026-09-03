//! Desktop Host implementation for `whisker-svg`.

use std::sync::Mutex;

use base64::Engine;
use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Stroke};
use whisker_desktop::{
    DesktopRaster, DesktopViewDefinition, ModuleDefinition, WhiskerModule, WhiskerValue,
};
use whisker_svg::{Color, Transform, Visitor, replay};

#[derive(Debug)]
struct SvgDesktopView {
    display_list: Vec<u8>,
    tint: Color,
    generation: u64,
    cache: Mutex<Option<CachedRaster>>,
}

#[derive(Debug)]
struct CachedRaster {
    generation: u64,
    width: u32,
    height: u32,
    raster: DesktopRaster,
}

impl Default for SvgDesktopView {
    fn default() -> Self {
        Self {
            display_list: Vec::new(),
            tint: Color::BLACK,
            generation: 0,
            cache: Mutex::new(None),
        }
    }
}

impl SvgDesktopView {
    fn set_display_list(&mut self, encoded: &str) {
        self.display_list = if encoded.is_empty() {
            Vec::new()
        } else {
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap_or_default()
        };
        self.invalidate();
    }

    fn set_color(&mut self, value: &str) {
        self.tint = parse_color(value);
        self.invalidate();
    }

    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        *self
            .cache
            .get_mut()
            .expect("SVG raster cache lock poisoned") = None;
    }

    fn rasterize(&self, width: u32, height: u32) -> Option<DesktopRaster> {
        if self.display_list.is_empty() || width == 0 || height == 0 {
            return None;
        }
        let mut cache = self.cache.lock().expect("SVG raster cache lock poisoned");
        if let Some(cached) = cache.as_ref()
            && cached.generation == self.generation
            && cached.width == width
            && cached.height == height
        {
            return Some(cached.raster.clone());
        }
        let raster = render_raster(
            &self.display_list,
            self.tint,
            self.generation,
            width,
            height,
        )?;
        *cache = Some(CachedRaster {
            generation: self.generation,
            width,
            height,
            raster: raster.clone(),
        });
        Some(raster)
    }
}

struct SvgModule;

#[WhiskerModule]
impl WhiskerModule for SvgModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new().name("whisker-svg:Svg").view(
            DesktopViewDefinition::new("whisker-svg:Svg", |_| SvgDesktopView::default())
                .prop(
                    "display-list",
                    |view, value| {
                        let WhiskerValue::String(value) = value else {
                            unreachable!("Desktop Host validates SVG property shapes")
                        };
                        view.set_display_list(value);
                    },
                    |view| view.set_display_list(""),
                )
                .prop(
                    "color",
                    |view, value| {
                        let WhiskerValue::String(value) = value else {
                            unreachable!("Desktop Host validates SVG property shapes")
                        };
                        view.set_color(value);
                    },
                    |view| view.set_color(""),
                )
                .raster(SvgDesktopView::rasterize),
        )
    }
}

#[derive(Clone, Copy)]
enum PaintSource {
    Literal(Color),
    Tint,
}

#[derive(Clone, Copy)]
struct State {
    transform: tiny_skia::Transform,
    fill: Option<PaintSource>,
    stroke: Option<PaintSource>,
    stroke_width: f32,
    opacity: f32,
}

impl Default for State {
    fn default() -> Self {
        Self {
            transform: tiny_skia::Transform::identity(),
            fill: Some(PaintSource::Literal(Color::BLACK)),
            stroke: None,
            stroke_width: 1.0,
            opacity: 1.0,
        }
    }
}

struct RasterVisitor {
    pixmap: Pixmap,
    size: [f32; 2],
    tint: Color,
    state: State,
    stack: Vec<State>,
    builder: Option<PathBuilder>,
    path: Option<Path>,
}

impl RasterVisitor {
    fn new(width: u32, height: u32, tint: Color) -> Option<Self> {
        Some(Self {
            pixmap: Pixmap::new(width, height)?,
            size: [width as f32, height as f32],
            tint,
            state: State::default(),
            stack: Vec::new(),
            builder: None,
            path: None,
        })
    }

    fn builder(&mut self) -> &mut PathBuilder {
        self.path = None;
        self.builder.get_or_insert_with(PathBuilder::new)
    }

    fn finish_path(&mut self) -> Option<&Path> {
        if self.path.is_none() {
            self.path = self.builder.take().and_then(PathBuilder::finish);
        }
        self.path.as_ref()
    }

    fn paint(&self, source: PaintSource) -> Paint<'static> {
        let color = match source {
            PaintSource::Literal(color) => color,
            PaintSource::Tint => self.tint,
        };
        let alpha =
            ((color.a as f32 * self.state.opacity.clamp(0.0, 1.0)).round()).clamp(0.0, 255.0) as u8;
        let mut paint = Paint::default();
        paint.set_color_rgba8(color.r, color.g, color.b, alpha);
        paint
    }

    fn draw_fill(&mut self) {
        let Some(source) = self.state.fill else {
            return;
        };
        let paint = self.paint(source);
        let transform = self.state.transform;
        let Some(path) = self.finish_path().cloned() else {
            return;
        };
        self.pixmap
            .fill_path(&path, &paint, FillRule::Winding, transform, None);
    }

    fn draw_stroke(&mut self) {
        let Some(source) = self.state.stroke else {
            return;
        };
        let paint = self.paint(source);
        let transform = self.state.transform;
        let stroke = Stroke {
            width: self.state.stroke_width.max(0.0),
            ..Stroke::default()
        };
        let Some(path) = self.finish_path().cloned() else {
            return;
        };
        self.pixmap
            .stroke_path(&path, &paint, &stroke, transform, None);
    }

    fn into_straight_rgba(mut self) -> Vec<u8> {
        for pixel in self.pixmap.data_mut().chunks_exact_mut(4) {
            let alpha = pixel[3];
            if alpha == 0 {
                pixel[..3].fill(0);
            } else if alpha < 255 {
                for channel in &mut pixel[..3] {
                    *channel =
                        ((*channel as u32 * 255 + alpha as u32 / 2) / alpha as u32).min(255) as u8;
                }
            }
        }
        self.pixmap.take()
    }
}

impl Visitor for RasterVisitor {
    fn save(&mut self) {
        self.stack.push(self.state);
    }

    fn restore(&mut self) {
        if let Some(state) = self.stack.pop() {
            self.state = state;
        }
    }

    fn concat(&mut self, transform: &Transform) {
        self.state.transform = self.state.transform.pre_concat(to_skia(*transform));
    }

    fn viewport(&mut self, min_x: f32, min_y: f32, width: f32, height: f32) {
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let scale = (self.size[0] / width).min(self.size[1] / height);
        let tx = (self.size[0] - width * scale) * 0.5 - min_x * scale;
        let ty = (self.size[1] - height * scale) * 0.5 - min_y * scale;
        self.state.transform = self
            .state
            .transform
            .pre_concat(tiny_skia::Transform::from_row(
                scale, 0.0, 0.0, scale, tx, ty,
            ));
    }

    fn fill_color(&mut self, color: Color) {
        self.state.fill = Some(PaintSource::Literal(color));
    }

    fn stroke_color(&mut self, color: Color) {
        self.state.stroke = Some(PaintSource::Literal(color));
    }

    fn stroke_width(&mut self, width: f32) {
        self.state.stroke_width = width;
    }

    fn opacity(&mut self, alpha: f32) {
        self.state.opacity = alpha.clamp(0.0, 1.0);
    }

    fn fill_tint(&mut self) {
        self.state.fill = Some(PaintSource::Tint);
    }

    fn stroke_tint(&mut self) {
        self.state.stroke = Some(PaintSource::Tint);
    }

    fn path_begin(&mut self) {
        self.builder = Some(PathBuilder::new());
        self.path = None;
    }

    fn move_to(&mut self, x: f32, y: f32) {
        self.builder().move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.builder().line_to(x, y);
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.builder().quad_to(cx, cy, x, y);
    }

    fn cubic_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.builder().cubic_to(c1x, c1y, c2x, c2y, x, y);
    }

    fn close(&mut self) {
        self.builder().close();
    }

    fn fill(&mut self) {
        self.draw_fill();
    }

    fn stroke(&mut self) {
        self.draw_stroke();
    }

    fn fill_and_stroke(&mut self) {
        self.draw_fill();
        self.draw_stroke();
    }
}

fn render_raster(
    bytes: &[u8],
    tint: Color,
    generation: u64,
    width: u32,
    height: u32,
) -> Option<DesktopRaster> {
    let mut visitor = RasterVisitor::new(width, height, tint)?;
    replay(bytes, &mut visitor).ok()?;
    DesktopRaster::new(generation, width, height, visitor.into_straight_rgba()).ok()
}

fn to_skia(transform: Transform) -> tiny_skia::Transform {
    tiny_skia::Transform::from_row(
        transform.a,
        transform.b,
        transform.c,
        transform.d,
        transform.tx,
        transform.ty,
    )
}

fn parse_color(value: &str) -> Color {
    value
        .parse::<csscolorparser::Color>()
        .map(|color| {
            let [r, g, b, a] = color.to_rgba8();
            Color::rgba(r, g, b, a)
        })
        .unwrap_or(Color::BLACK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_color_rasterizes_with_transparent_background() {
        let compiled = whisker_svg::compile(
            r#"<svg viewBox="0 0 10 10"><rect width="10" height="10" fill="currentColor"/></svg>"#,
        )
        .unwrap();
        let raster =
            render_raster(&compiled.bytes, Color::rgba(12, 34, 56, 255), 7, 20, 10).unwrap();
        assert_eq!(raster.generation(), 7);
        assert_eq!([raster.width(), raster.height()], [20, 10]);
        assert_eq!(&raster.pixels()[0..4], &[0, 0, 0, 0]);
        let center = ((5 * 20 + 10) * 4) as usize;
        assert_eq!(&raster.pixels()[center..center + 4], &[12, 34, 56, 255]);
    }

    #[test]
    fn raster_cache_reuses_generation_and_size() {
        let compiled =
            whisker_svg::compile(r#"<svg viewBox="0 0 1 1"><rect width="1" height="1"/></svg>"#)
                .unwrap();
        let view = SvgDesktopView {
            display_list: compiled.bytes,
            ..SvgDesktopView::default()
        };
        let first = view.rasterize(8, 8).unwrap();
        let second = view.rasterize(8, 8).unwrap();
        assert_eq!(first.generation(), second.generation());
        assert_eq!(first.pixels(), second.pixels());
    }
}
