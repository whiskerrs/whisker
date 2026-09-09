use std::collections::HashMap;

use glyphon::{ContentType, CustomGlyph, RasterizeCustomGlyphRequest, RasterizedCustomGlyph};
use parley::PositionedLayoutItem;
use swash::scale::{Render, ScaleContext, Source, StrikeWith, image::Content};
use whisker_protocol::TextContent;

use super::paragraph::PreparedParagraph;
use crate::paint::color::text_color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParagraphRasterError {
    Capacity,
    InvalidFont,
    GlyphIndex,
    ImageSize,
}

impl std::fmt::Display for ParagraphRasterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Capacity => "paragraph atlas capacity",
            Self::InvalidFont => "invalid resolved font",
            Self::GlyphIndex => "glyph index exceeds font format",
            Self::ImageSize => "paragraph glyph exceeds atlas dimensions",
        })
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct FontKey {
    id: u64,
    index: u32,
    size: u32,
    variations: Vec<i16>,
    embolden: bool,
    skew: u32,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct GlyphKey {
    font: FontKey,
    glyph: u16,
}

#[derive(Default)]
pub(crate) struct ParagraphRasterCache {
    scaler: ScaleContext,
    fonts: HashMap<(u64, u32), swash::CacheKey>,
    ids: HashMap<GlyphKey, u16>,
    images: Vec<Option<swash::scale::image::Image>>,
    masks: HashMap<u16, u16>,
}

impl ParagraphRasterCache {
    pub(crate) fn reset(&mut self) {
        self.ids.clear();
        self.images.clear();
        self.fonts.clear();
        self.masks.clear();
    }

    pub(crate) fn prepare(
        &mut self,
        paragraph: &PreparedParagraph,
        content: &TextContent,
        scale: f32,
        opacity: f32,
    ) -> Result<Vec<CustomGlyph>, ParagraphRasterError> {
        let mut glyphs = Vec::new();
        let mut shadows = Vec::new();
        for (line, geometry) in paragraph.layout.lines().zip(&paragraph.lines) {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                let run = glyph_run.run();
                let font = run.font();
                let synthesis = run.synthesis();
                let font_key = FontKey {
                    id: font.data.id(),
                    index: font.index,
                    size: (run.font_size() * scale).to_bits(),
                    variations: run.normalized_coords().to_vec(),
                    embolden: synthesis.embolden(),
                    skew: synthesis.skew().unwrap_or_default().to_bits(),
                };
                let mut font_ref =
                    swash::FontRef::from_index(font.data.as_ref(), font.index as usize)
                        .ok_or(ParagraphRasterError::InvalidFont)?;
                font_ref.key = *self.fonts.entry((font_key.id, font.index)).or_default();
                let mut scaler = self
                    .scaler
                    .builder(font_ref)
                    .size(run.font_size() * scale)
                    .normalized_coords(run.normalized_coords())
                    .hint(true)
                    .build();
                let paint = glyph_run
                    .style()
                    .brush
                    .checked_sub(1)
                    .and_then(|index| content.runs.get(index as usize))
                    .map_or(&content.paint, |run| &run.paint);
                for glyph in glyph_run.positioned_glyphs() {
                    let glyph_id =
                        u16::try_from(glyph.id).map_err(|_| ParagraphRasterError::GlyphIndex)?;
                    let key = GlyphKey {
                        font: font_key.clone(),
                        glyph: glyph_id,
                    };
                    let id = if let Some(id) = self.ids.get(&key) {
                        *id
                    } else {
                        let id = u16::try_from(self.images.len())
                            .map_err(|_| ParagraphRasterError::Capacity)?;
                        let mut render = Render::new(&[
                            Source::ColorOutline(0),
                            Source::ColorBitmap(StrikeWith::BestFit),
                            Source::Outline,
                        ]);
                        render.format(swash::zeno::Format::Alpha);
                        if synthesis.embolden() {
                            render.embolden(run.font_size() * scale / 24.0);
                        }
                        if let Some(skew) = synthesis.skew() {
                            render.transform(Some(swash::zeno::Transform::skew(
                                swash::zeno::Angle::from_degrees(skew),
                                swash::zeno::Angle::ZERO,
                            )));
                        }
                        let image = render.render(&mut scaler, glyph_id);
                        self.images.push(image);
                        self.ids.insert(key, id);
                        id
                    };
                    let Some(image) = &self.images[id as usize] else {
                        continue;
                    };
                    let placement = image.placement;
                    if placement.width == 0 || placement.height == 0 {
                        continue;
                    }
                    u16::try_from(placement.width).map_err(|_| ParagraphRasterError::ImageSize)?;
                    u16::try_from(placement.height).map_err(|_| ParagraphRasterError::ImageSize)?;
                    let glyph = CustomGlyph {
                        id,
                        left: glyph.x + placement.left as f32 / scale,
                        top: glyph.y + geometry.shift
                            - geometry.run_shift(
                                super::paragraph::run_alignment(
                                    &content.payload,
                                    glyph_run.style().brush,
                                ),
                                run.metrics(),
                            )
                            - placement.top as f32 / scale,
                        width: placement.width as f32 / scale,
                        height: placement.height as f32 / scale,
                        color: Some(text_color(&paint.foreground, opacity)),
                        snap_to_physical_pixel: true,
                        metadata: 0,
                    };
                    for shadow in &paint.shadows {
                        let mask = Self::shadow_mask(&mut self.images, &mut self.masks, id)?;
                        let radius = shadow.blur_radius.min(12.0);
                        let samples = [
                            (0.0, 0.0),
                            (-radius, 0.0),
                            (radius, 0.0),
                            (0.0, -radius),
                            (0.0, radius),
                            (-radius * 0.7, -radius * 0.7),
                            (radius * 0.7, -radius * 0.7),
                            (-radius * 0.7, radius * 0.7),
                            (radius * 0.7, radius * 0.7),
                        ];
                        let offsets = if radius > 0.0 {
                            &samples[..]
                        } else {
                            &samples[..1]
                        };
                        for (x, y) in offsets {
                            shadows.push(CustomGlyph {
                                id: mask,
                                left: glyph.left + shadow.offset_x + x,
                                top: glyph.top + shadow.offset_y + y,
                                color: Some(text_color(
                                    &shadow.color,
                                    opacity / offsets.len() as f32,
                                )),
                                ..glyph
                            });
                        }
                    }
                    glyphs.push(glyph);
                }
            }
        }
        if shadows.is_empty() {
            return Ok(glyphs);
        }
        shadows.extend(glyphs);
        Ok(shadows)
    }

    fn shadow_mask(
        images: &mut Vec<Option<swash::scale::image::Image>>,
        masks: &mut HashMap<u16, u16>,
        id: u16,
    ) -> Result<u16, ParagraphRasterError> {
        let image = images[id as usize].as_ref().expect("rasterized glyph");
        if image.content == Content::Mask {
            return Ok(id);
        }
        if let Some(mask) = masks.get(&id) {
            return Ok(*mask);
        }
        let mask = u16::try_from(images.len()).map_err(|_| ParagraphRasterError::Capacity)?;
        let mut image = image.clone();
        image.data = image.data.chunks_exact(4).map(|pixel| pixel[3]).collect();
        image.content = Content::Mask;
        images.push(Some(image));
        masks.insert(id, mask);
        Ok(mask)
    }

    pub(crate) fn rasterize(
        &self,
        request: RasterizeCustomGlyphRequest,
    ) -> Option<RasterizedCustomGlyph> {
        let image = self.images.get(request.id as usize)?.as_ref()?;
        if image.placement.width != u32::from(request.width)
            || image.placement.height != u32::from(request.height)
        {
            return None;
        }
        Some(RasterizedCustomGlyph {
            data: image.data.clone(),
            content_type: match image.content {
                Content::Color => ContentType::Color,
                Content::Mask => ContentType::Mask,
                Content::SubpixelMask => return None,
            },
        })
    }
}
