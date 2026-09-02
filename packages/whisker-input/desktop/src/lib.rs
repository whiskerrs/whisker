//! Native Desktop Host implementation for `whisker-input`.
//!
//! The value, selection, marked text, and glyph raster live in this package.
//! `whisker-desktop` only routes window focus, keyboard, clipboard, and IME
//! messages through its generic editable-text seam.

use std::fmt;
use std::sync::{Mutex, OnceLock};

use glyphon::{
    Attrs, Buffer, Color as GlyphColor, Family, FontSystem, Metrics, Shaping, Style, SwashCache,
    Weight, Wrap, cosmic_text::Cursor,
};
use unicode_segmentation::UnicodeSegmentation;
use whisker_desktop::{
    DesktopEventEmitter, DesktopNativeEvent, DesktopRaster, DesktopTextInputEvent,
    DesktopTextInputKey, DesktopViewDefinition, ModuleDefinition, WhiskerModule, WhiskerTextStyle,
    WhiskerValue,
};
use whisker_protocol::{MeasureFontFamily, MeasureFontStyle, MeasureLineHeight, PaintColor};

const MODULE_NAME: &str = "whisker-input:Input";

struct RasterState {
    cached: Option<CachedRaster>,
}

struct TextRasterizer {
    font_system: FontSystem,
    swash_cache: SwashCache,
}

struct CachedRaster {
    generation: u64,
    width: u32,
    height: u32,
    scale_bits: u32,
    raster: DesktopRaster,
}

struct InputDesktopView {
    emitter: DesktopEventEmitter,
    value: String,
    placeholder: String,
    placeholder_color: String,
    caret_color: String,
    selection_color: String,
    multiline: bool,
    secure: bool,
    editable: bool,
    max_length: usize,
    focused: bool,
    selection: (usize, usize),
    composition: Option<(usize, usize)>,
    text_style: Option<WhiskerTextStyle>,
    generation: u64,
    raster: Mutex<RasterState>,
}

impl fmt::Debug for InputDesktopView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InputDesktopView")
            .field("value", &self.value)
            .field("focused", &self.focused)
            .field("selection", &self.selection)
            .finish_non_exhaustive()
    }
}

impl InputDesktopView {
    fn new(emitter: DesktopEventEmitter) -> Self {
        Self {
            emitter,
            value: String::new(),
            placeholder: String::new(),
            placeholder_color: String::new(),
            caret_color: String::new(),
            selection_color: String::new(),
            multiline: false,
            secure: false,
            editable: true,
            max_length: 0,
            focused: false,
            selection: (0, 0),
            composition: None,
            text_style: None,
            generation: 1,
            raster: Mutex::new(RasterState { cached: None }),
        }
    }

    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1).max(1);
        self.raster
            .lock()
            .expect("Input raster lock poisoned")
            .cached = None;
    }

    fn set_value(&mut self, value: &str) {
        if self.value == value {
            return;
        }
        self.value.clear();
        self.value.push_str(value);
        let end = self.value.len();
        self.selection = (end, end);
        self.composition = None;
        self.invalidate();
    }

    fn set_focus(&mut self, focused: bool) {
        if self.focused == focused || (focused && !self.editable) {
            return;
        }
        self.focused = focused;
        self.composition = None;
        self.invalidate();
        self.emitter.emit(DesktopNativeEvent {
            event: if focused { "focus" } else { "blur" }.into(),
            detail: WhiskerValue::Null,
        });
        if !focused {
            self.emit_value("change");
        }
    }

    fn emit_value(&self, event: &str) {
        self.emitter.emit(DesktopNativeEvent {
            event: event.into(),
            detail: WhiskerValue::map([("value", WhiskerValue::String(self.value.clone()))]),
        });
    }

    fn replace_selection(&mut self, inserted: &str) {
        if !self.editable {
            return;
        }
        let (start, end) = ordered(self.selection);
        let available = if self.max_length == 0 {
            usize::MAX
        } else {
            self.max_length.saturating_sub(
                self.value[..start].chars().count() + self.value[end..].chars().count(),
            )
        };
        let inserted = inserted.chars().take(available).collect::<String>();
        self.value.replace_range(start..end, &inserted);
        let caret = start + inserted.len();
        self.selection = (caret, caret);
        self.composition = None;
        self.invalidate();
        self.emit_value("input");
    }

    fn preedit(&mut self, text: &str, cursor: Option<(usize, usize)>) {
        if !self.editable {
            return;
        }
        if let Some(range) = self.composition.take() {
            self.selection = range;
        }
        let (start, end) = ordered(self.selection);
        let available = if self.max_length == 0 {
            usize::MAX
        } else {
            self.max_length.saturating_sub(
                self.value[..start].chars().count() + self.value[end..].chars().count(),
            )
        };
        let inserted = text.chars().take(available).collect::<String>();
        self.value.replace_range(start..end, &inserted);
        let composition_end = start + inserted.len();
        self.composition = (!inserted.is_empty()).then_some((start, composition_end));
        let caret = cursor
            .map(|(_, end)| start + char_boundary_at_or_before(&inserted, end.min(inserted.len())))
            .unwrap_or(composition_end);
        self.selection = (caret, caret);
        self.invalidate();
        self.emit_value("input");
    }

    fn commit(&mut self, text: &str) {
        if let Some(range) = self.composition.take() {
            self.selection = range;
        }
        self.replace_selection(text);
    }

    fn move_caret(&mut self, next: usize, shift: bool) {
        let next = next.min(self.value.len());
        self.selection = if shift {
            (self.selection.0, next)
        } else {
            (next, next)
        };
        self.composition = None;
        self.invalidate();
    }

    fn handle_key(&mut self, key: DesktopTextInputKey, shift: bool) {
        let (start, end) = ordered(self.selection);
        match key {
            DesktopTextInputKey::Backspace if start != end => self.replace_selection(""),
            DesktopTextInputKey::Backspace => {
                let previous = previous_grapheme(&self.value, start);
                self.selection = (previous, start);
                self.replace_selection("");
            }
            DesktopTextInputKey::Delete if start != end => self.replace_selection(""),
            DesktopTextInputKey::Delete => {
                let next = next_grapheme(&self.value, end);
                self.selection = (end, next);
                self.replace_selection("");
            }
            DesktopTextInputKey::ArrowLeft => {
                let next = if !shift && start != end {
                    start
                } else {
                    previous_grapheme(&self.value, self.selection.1)
                };
                self.move_caret(next, shift);
            }
            DesktopTextInputKey::ArrowRight => {
                let next = if !shift && start != end {
                    end
                } else {
                    next_grapheme(&self.value, self.selection.1)
                };
                self.move_caret(next, shift);
            }
            DesktopTextInputKey::Home => self.move_caret(0, shift),
            DesktopTextInputKey::End => self.move_caret(self.value.len(), shift),
            DesktopTextInputKey::Enter if self.multiline => self.replace_selection("\n"),
            DesktopTextInputKey::Enter => self.emit_value("submit"),
        }
    }

    fn handle_input(&mut self, event: &DesktopTextInputEvent) {
        match event {
            DesktopTextInputEvent::Commit(text) => self.commit(text),
            DesktopTextInputEvent::Preedit { text, cursor } => self.preedit(text, *cursor),
            DesktopTextInputEvent::Key { key, shift } => self.handle_key(*key, *shift),
            DesktopTextInputEvent::SelectAll => {
                self.selection = (0, self.value.len());
                self.invalidate();
            }
            DesktopTextInputEvent::Cut => {
                let (start, end) = ordered(self.selection);
                if start != end {
                    self.replace_selection("");
                }
            }
            DesktopTextInputEvent::Paste(text) => self.replace_selection(text),
        }
    }

    fn selected_text(&self) -> Option<String> {
        let (start, end) = ordered(self.selection);
        (start != end).then(|| self.value[start..end].to_owned())
    }

    fn rasterize(&self, width: u32, height: u32, scale: f32) -> Option<DesktopRaster> {
        if width == 0 || height == 0 {
            return None;
        }
        let mut state = self.raster.lock().expect("Input raster lock poisoned");
        if let Some(cache) = &state.cached
            && cache.generation == self.generation
            && cache.width == width
            && cache.height == height
            && cache.scale_bits == scale.to_bits()
        {
            return Some(cache.raster.clone());
        }
        let mut text = text_rasterizer()
            .lock()
            .expect("Desktop text rasterizer lock poisoned");
        let pixels = self.draw_pixels(&mut text, width, height, scale);
        let raster = DesktopRaster::new(self.generation, width, height, pixels).ok()?;
        state.cached = Some(CachedRaster {
            generation: self.generation,
            width,
            height,
            scale_bits: scale.to_bits(),
            raster: raster.clone(),
        });
        Some(raster)
    }

    fn draw_pixels(
        &self,
        state: &mut TextRasterizer,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Vec<u8> {
        let mut pixels = vec![0; width as usize * height as usize * 4];
        let default_style = whisker_protocol::TextMeasureStyle::default();
        let style = self
            .text_style
            .as_ref()
            .map_or(&default_style, |style| &style.style);
        let font_size = style.font_size * scale;
        let line_height = match style.line_height {
            MeasureLineHeight::Normal => font_size * 1.2,
            MeasureLineHeight::LogicalPixels(value) => value * scale,
        };
        let mut buffer = Buffer::new(&mut state.font_system, Metrics::new(font_size, line_height));
        buffer.set_size(
            &mut state.font_system,
            Some(width as f32),
            Some(height as f32),
        );
        buffer.set_wrap(
            &mut state.font_system,
            if self.multiline {
                Wrap::Word
            } else {
                Wrap::None
            },
        );
        let family = style
            .font_families
            .iter()
            .map(|family| match family {
                MeasureFontFamily::System => Family::SansSerif,
                MeasureFontFamily::Named(name) => Family::Name(name),
            })
            .next()
            .unwrap_or(Family::SansSerif);
        let attrs = Attrs::new()
            .family(family)
            .weight(Weight(style.font_weight))
            .style(match style.font_style {
                MeasureFontStyle::Normal => Style::Normal,
                MeasureFontStyle::Italic => Style::Italic,
                MeasureFontStyle::Oblique => Style::Oblique,
            });
        let placeholder = self.value.is_empty() && !self.placeholder.is_empty();
        let display = if placeholder {
            self.placeholder.clone()
        } else if self.secure {
            "•".repeat(self.value.chars().count())
        } else {
            self.value.clone()
        };
        buffer.set_text(&mut state.font_system, &display, &attrs, Shaping::Advanced);
        buffer.shape_until_scroll(&mut state.font_system, false);

        if self.focused && !placeholder {
            let (start, end) = ordered(self.selection);
            let start = text_cursor(&display, self.display_offset(start));
            let end = text_cursor(&display, self.display_offset(end));
            for run in buffer.layout_runs() {
                if let Some((x, w)) = run.highlight(start, end) {
                    fill_rect(
                        &mut pixels,
                        width,
                        height,
                        x.floor() as i32,
                        run.line_top.floor() as i32,
                        w.ceil().max(1.0) as u32,
                        run.line_height.ceil() as u32,
                        parse_css_color(&self.selection_color, [51, 132, 255, 92]),
                    );
                }
            }
        }

        let foreground = if placeholder {
            parse_css_color(&self.placeholder_color, [128, 128, 128, 255])
        } else {
            self.text_style
                .as_ref()
                .map_or([0, 0, 0, 255], |style| paint_color(&style.paint.foreground))
        };
        buffer.draw(
            &mut state.font_system,
            &mut state.swash_cache,
            GlyphColor::rgba(foreground[0], foreground[1], foreground[2], foreground[3]),
            |x, y, w, h, color| {
                fill_rect(&mut pixels, width, height, x, y, w, h, color.as_rgba());
            },
        );

        if self.focused {
            let cursor = self.display_offset(self.selection.1);
            let cursor = text_cursor(&display, cursor.min(display.len()));
            let mut caret = None;
            for run in buffer.layout_runs() {
                if let Some((x, _)) = run.highlight(cursor, cursor) {
                    caret = Some((x, run.line_top, run.line_height));
                }
            }
            let (x, y, h) = caret.unwrap_or((0.0, 0.0, line_height));
            fill_rect(
                &mut pixels,
                width,
                height,
                x.floor() as i32,
                y.floor() as i32,
                scale.ceil().max(1.0) as u32,
                h.ceil() as u32,
                parse_css_color(&self.caret_color, [0, 122, 255, 255]),
            );
        }
        pixels
    }

    fn display_offset(&self, value_offset: usize) -> usize {
        if self.secure {
            self.value[..value_offset].chars().count() * '•'.len_utf8()
        } else {
            value_offset
        }
    }

    fn clear(&mut self) {
        self.selection = (0, self.value.len());
        self.replace_selection("");
    }
}

fn text_rasterizer() -> &'static Mutex<TextRasterizer> {
    static RASTERIZER: OnceLock<Mutex<TextRasterizer>> = OnceLock::new();
    RASTERIZER.get_or_init(|| {
        Mutex::new(TextRasterizer {
            font_system: FontSystem::new(),
            swash_cache: SwashCache::new(),
        })
    })
}

struct InputModule;

#[whisker_desktop::WhiskerModule]
impl WhiskerModule for InputModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new().name(MODULE_NAME).view(
            DesktopViewDefinition::new(MODULE_NAME, InputDesktopView::new)
                .prop(
                    "value",
                    set_string(|view, value| view.set_value(value)),
                    |view| view.set_value(""),
                )
                .prop(
                    "placeholder",
                    set_string(|view, value| {
                        view.placeholder = value.into();
                        view.invalidate();
                    }),
                    |view| {
                        view.placeholder.clear();
                        view.invalidate();
                    },
                )
                .prop(
                    "placeholder-color",
                    set_string(|view, value| {
                        view.placeholder_color = value.into();
                        view.invalidate();
                    }),
                    |view| {
                        view.placeholder_color.clear();
                        view.invalidate();
                    },
                )
                .prop(
                    "caret-color",
                    set_string(|view, value| {
                        view.caret_color = value.into();
                        view.invalidate();
                    }),
                    |view| {
                        view.caret_color.clear();
                        view.invalidate();
                    },
                )
                .prop(
                    "selection-color",
                    set_string(|view, value| {
                        view.selection_color = value.into();
                        view.invalidate();
                    }),
                    |view| {
                        view.selection_color.clear();
                        view.invalidate();
                    },
                )
                .prop(
                    "multiline",
                    set_bool(|view, value| {
                        view.multiline = value;
                        view.invalidate();
                    }),
                    |view| {
                        view.multiline = false;
                        view.invalidate();
                    },
                )
                .prop("lines", |_, value| assert_int(value), |_| {})
                .prop(
                    "secure",
                    set_bool(|view, value| {
                        view.secure = value;
                        view.invalidate();
                    }),
                    |view| {
                        view.secure = false;
                        view.invalidate();
                    },
                )
                .prop(
                    "editable",
                    set_bool(|view, value| {
                        view.editable = value;
                        if !value {
                            view.set_focus(false);
                        }
                    }),
                    |view| view.editable = true,
                )
                .prop(
                    "auto-focus",
                    set_bool(|view, value| {
                        if value {
                            view.set_focus(true);
                        }
                    }),
                    |_| {},
                )
                .prop(
                    "max-length",
                    |view, value| view.max_length = expect_int(value).max(0) as usize,
                    |view| view.max_length = 0,
                )
                .prop("keyboard-type", set_string(|_, _| {}), |_| {})
                .prop("return-key", set_string(|_, _| {}), |_| {})
                .prop("auto-capitalize", set_string(|_, _| {}), |_| {})
                .prop("autocorrect", set_bool(|_, _| {}), |_| {})
                .prop("spell-check", set_bool(|_, _| {}), |_| {})
                .event("input")
                .event("change")
                .event("focus")
                .event("blur")
                .event("submit")
                .command("focus", |view, _| view.set_focus(true))
                .command("blur", |view, _| view.set_focus(false))
                .command("clear", |view, _| view.clear())
                .command("setValue", |view, arguments| {
                    let WhiskerValue::Map(arguments) = arguments else {
                        return;
                    };
                    if let Some(WhiskerValue::String(value)) = arguments.get("value") {
                        view.set_value(value);
                    }
                })
                .text_style(|view, style| {
                    view.text_style = Some(style.clone());
                    view.invalidate();
                })
                .text_input(
                    |view| view.focused,
                    InputDesktopView::set_focus,
                    InputDesktopView::handle_input,
                    InputDesktopView::selected_text,
                )
                .raster_scaled(InputDesktopView::rasterize),
        )
    }
}

fn set_string(
    update: impl Fn(&mut InputDesktopView, &str) + Send + Sync + 'static,
) -> impl Fn(&mut InputDesktopView, &WhiskerValue) + Send + Sync + 'static {
    move |view, value| {
        let WhiskerValue::String(value) = value else {
            unreachable!("Desktop Host validates Input string property")
        };
        update(view, value);
    }
}

fn set_bool(
    update: impl Fn(&mut InputDesktopView, bool) + Send + Sync + 'static,
) -> impl Fn(&mut InputDesktopView, &WhiskerValue) + Send + Sync + 'static {
    move |view, value| {
        let WhiskerValue::Bool(value) = value else {
            unreachable!("Desktop Host validates Input boolean property")
        };
        update(view, *value);
    }
}

fn assert_int(value: &WhiskerValue) {
    let WhiskerValue::Int(_) = value else {
        unreachable!("Desktop Host validates Input integer property")
    };
}

fn expect_int(value: &WhiskerValue) -> i64 {
    let WhiskerValue::Int(value) = value else {
        unreachable!("Desktop Host validates Input integer property")
    };
    *value
}

fn ordered(selection: (usize, usize)) -> (usize, usize) {
    if selection.0 <= selection.1 {
        selection
    } else {
        (selection.1, selection.0)
    }
}

fn previous_grapheme(value: &str, offset: usize) -> usize {
    value[..offset]
        .grapheme_indices(true)
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn next_grapheme(value: &str, offset: usize) -> usize {
    value[offset..]
        .grapheme_indices(true)
        .nth(1)
        .map_or(value.len(), |(index, _)| offset + index)
}

fn char_boundary_at_or_before(value: &str, mut offset: usize) -> usize {
    while !value.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn text_cursor(text: &str, offset: usize) -> Cursor {
    let mut line = 0;
    let mut line_start = 0;
    for (index, byte) in text.bytes().enumerate() {
        if index >= offset {
            break;
        }
        if byte == b'\n' {
            line += 1;
            line_start = index + 1;
        }
    }
    Cursor::new(line, offset.saturating_sub(line_start))
}

#[allow(clippy::too_many_arguments)]
fn fill_rect(
    pixels: &mut [u8],
    canvas_width: u32,
    canvas_height: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    color: [u8; 4],
) {
    let right = x
        .saturating_add(width as i32)
        .max(0)
        .min(canvas_width as i32) as u32;
    let bottom = y
        .saturating_add(height as i32)
        .max(0)
        .min(canvas_height as i32) as u32;
    for py in y.max(0) as u32..bottom {
        for px in x.max(0) as u32..right {
            let index = ((py * canvas_width + px) * 4) as usize;
            let source_alpha = color[3] as f32 / 255.0;
            let destination_alpha = pixels[index + 3] as f32 / 255.0;
            let output_alpha = source_alpha + destination_alpha * (1.0 - source_alpha);
            if output_alpha > 0.0 {
                for channel in 0..3 {
                    let source = color[channel] as f32 / 255.0;
                    let destination = pixels[index + channel] as f32 / 255.0;
                    pixels[index + channel] = (((source * source_alpha
                        + destination * destination_alpha * (1.0 - source_alpha))
                        / output_alpha)
                        * 255.0)
                        .round() as u8;
                }
            }
            pixels[index + 3] = (output_alpha * 255.0).round() as u8;
        }
    }
}

fn parse_css_color(value: &str, fallback: [u8; 4]) -> [u8; 4] {
    if value.trim().is_empty() {
        return fallback;
    }
    csscolorparser::parse(value).map_or(fallback, |color| color.to_rgba8())
}

fn paint_color(value: &PaintColor) -> [u8; 4] {
    match value {
        PaintColor::Named(name) => parse_css_color(name, [0, 0, 0, 255]),
        PaintColor::Srgba {
            red,
            green,
            blue,
            alpha,
        } => [*red, *green, *blue, (*alpha * 255.0).round() as u8],
        PaintColor::Hsla {
            hue_degrees,
            saturation,
            lightness,
            alpha,
        } => parse_css_color(
            &format!("hsla({hue_degrees}, {saturation}%, {lightness}%, {alpha})"),
            [0, 0, 0, 255],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_is_grapheme_safe_and_emits_typed_values() {
        let mut input = InputDesktopView::new(DesktopEventEmitter::default());
        input.set_value("a👨‍👩‍👧‍👦");
        input.handle_key(DesktopTextInputKey::Backspace, false);
        assert_eq!(input.value, "a");
    }

    #[test]
    fn module_exports_one_editable_raster_element() {
        assert_eq!(InputModule::definition().factories().len(), 1);
    }

    #[test]
    fn secure_display_offsets_follow_original_selection() {
        let mut input = InputDesktopView::new(DesktopEventEmitter::default());
        input.set_value("aé日");
        input.secure = true;

        assert_eq!(input.display_offset(0), 0);
        assert_eq!(input.display_offset(1), '•'.len_utf8());
        assert_eq!(input.display_offset(3), 2 * '•'.len_utf8());
        assert_eq!(input.display_offset(input.value.len()), 3 * '•'.len_utf8());
    }

    #[test]
    fn raster_pixels_keep_straight_alpha_color_channels() {
        let mut pixels = vec![0; 4];
        fill_rect(&mut pixels, 1, 1, 0, 0, 1, 1, [120, 80, 40, 128]);

        assert_eq!(pixels, [120, 80, 40, 128]);
    }

    #[test]
    fn preedit_respects_max_length_and_utf8_cursor_boundaries() {
        let mut input = InputDesktopView::new(DesktopEventEmitter::default());
        input.max_length = 2;
        input.preedit("é日x", Some((0, 1)));

        assert_eq!(input.value, "é日");
        assert_eq!(input.selection, (0, 0));
        assert_eq!(input.composition, Some((0, "é日".len())));
    }
}
