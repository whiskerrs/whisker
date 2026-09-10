use super::*;
use whisker_desktop::{WhiskerMeasureRequest, WhiskerMeasuredSize};
use whisker_protocol::{AvailableSpace, TextMeasureStyle};

mod data {
    use serde::Deserialize;

    pub const VERSION: u16 = 1;

    #[derive(Clone, Debug, Deserialize)]
    pub struct InputMeasureData {
        pub text: String,
        pub multiline: bool,
        pub font_family: Option<String>,
        pub font_size: f32,
        pub font_weight: u16,
        pub italic: bool,
        pub line_height: Option<f32>,
        pub letter_spacing: f32,
    }
}

pub(super) fn text_buffer(
    fonts: &mut FontSystem,
    text: &str,
    style: &TextMeasureStyle,
    multiline: bool,
    width: Option<f32>,
    height: Option<f32>,
    scale: f32,
) -> (Buffer, f32) {
    let font_size = style.font_size * scale;
    let line_height = match style.line_height {
        MeasureLineHeight::Normal => font_size * 1.2,
        MeasureLineHeight::LogicalPixels(value) => value * scale,
    };
    let mut buffer = Buffer::new(fonts, Metrics::new(font_size, line_height));
    buffer.set_size(fonts, width, height);
    buffer.set_wrap(
        fonts,
        if multiline {
            Wrap::WordOrGlyph
        } else {
            Wrap::None
        },
    );
    let family = match style.font_families.first() {
        Some(MeasureFontFamily::Named(name)) => Family::Name(name),
        _ => Family::SansSerif,
    };
    let attrs = Attrs::new()
        .family(family)
        .weight(Weight(style.font_weight))
        .style(match style.font_style {
            MeasureFontStyle::Normal => Style::Normal,
            MeasureFontStyle::Italic => Style::Italic,
            MeasureFontStyle::Oblique => Style::Oblique,
        })
        .letter_spacing(style.letter_spacing * scale);
    let shaped_text = if text.is_empty() || text.ends_with('\n') {
        format!("{text}\u{200b}")
    } else {
        text.to_owned()
    };
    buffer.set_text(fonts, &shaped_text, &attrs, Shaping::Advanced);
    buffer.shape_until_scroll(fonts, false);
    (buffer, line_height)
}

pub(super) fn measure(request: &WhiskerMeasureRequest) -> Option<WhiskerMeasuredSize> {
    if request.payload_version != data::VERSION {
        return None;
    }
    let input: data::InputMeasureData = request.payload.deserialize_into().ok()?;
    let style = TextMeasureStyle {
        font_families: vec![
            input
                .font_family
                .map(MeasureFontFamily::Named)
                .unwrap_or(MeasureFontFamily::System),
        ],
        font_size: input.font_size,
        font_weight: input.font_weight,
        font_style: if input.italic {
            MeasureFontStyle::Italic
        } else {
            MeasureFontStyle::Normal
        },
        line_height: input
            .line_height
            .map(MeasureLineHeight::LogicalPixels)
            .unwrap_or(MeasureLineHeight::Normal),
        letter_spacing: input.letter_spacing,
        ..TextMeasureStyle::default()
    };
    let mut state = text_rasterizer().lock().ok()?;
    let width = request.known_dimensions[0].or(match request.available_space[0] {
        AvailableSpace::Definite(width) => Some(width),
        AvailableSpace::MinContent => Some(0.0),
        AvailableSpace::MaxContent => None,
    });
    let (buffer, line_height) = text_buffer(
        &mut state.font_system,
        &input.text,
        &style,
        input.multiline,
        width,
        None,
        1.0,
    );
    let mut measured_width = 0.0_f32;
    let mut measured_height = line_height;
    for run in buffer.layout_runs() {
        measured_width = measured_width.max(run.line_w);
        measured_height = measured_height.max(run.line_top + run.line_height);
    }
    Some(WhiskerMeasuredSize::new(
        request.known_dimensions[0].unwrap_or(measured_width),
        request.known_dimensions[1].unwrap_or(measured_height),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(text: &str, width: f32) -> WhiskerMeasureRequest {
        WhiskerMeasureRequest {
            known_dimensions: [Some(width), None],
            available_space: [AvailableSpace::Definite(width), AvailableSpace::MaxContent],
            payload_version: data::VERSION,
            payload: WhiskerValue::map([
                ("scale_factor", WhiskerValue::Float(1.0)),
                ("text", WhiskerValue::String(text.into())),
                ("multiline", WhiskerValue::Bool(true)),
                ("font_family", WhiskerValue::Null),
                ("font_size", WhiskerValue::Float(16.0)),
                ("font_weight", WhiskerValue::Int(400)),
                ("italic", WhiskerValue::Bool(false)),
                ("line_height", WhiskerValue::Float(24.0)),
                ("letter_spacing", WhiskerValue::Float(0.0)),
            ]),
        }
    }

    #[test]
    fn intrinsic_height_tracks_lines_wrapping_and_empty_content() {
        let empty = measure(&request("", 200.0)).unwrap();
        let one = measure(&request("Hello", 200.0)).unwrap();
        let two = measure(&request("Hello\n", 200.0)).unwrap();
        let text = "The quick brown fox jumps over the lazy dog";
        let wide = measure(&request(text, 400.0)).unwrap();
        let narrow = measure(&request(text, 80.0)).unwrap();
        assert_eq!(empty.height, 24.0);
        assert_eq!(one.height, empty.height);
        assert_eq!(two.height, 48.0);
        assert!(narrow.height > wide.height);
        let mut fixed = request(text, 80.0);
        fixed.known_dimensions[1] = Some(32.0);
        assert_eq!(measure(&fixed).unwrap().height, 32.0);
        fixed.payload_version = 2;
        assert!(measure(&fixed).is_none());
    }
}
