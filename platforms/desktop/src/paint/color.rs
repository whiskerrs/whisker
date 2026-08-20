use glyphon::Color as TextColor;
use whisker_protocol::PaintColor;

pub(crate) fn text_color(color: &PaintColor, opacity: f32) -> TextColor {
    let [red, green, blue, alpha] = srgba(color, opacity);
    TextColor::rgba(
        (red * 255.0).round() as u8,
        (green * 255.0).round() as u8,
        (blue * 255.0).round() as u8,
        (alpha * 255.0).round() as u8,
    )
}

pub(super) fn linear_color(color: &PaintColor, opacity: f32) -> [f32; 4] {
    let [red, green, blue, alpha] = srgba(color, opacity);
    [
        srgb_to_linear(red),
        srgb_to_linear(green),
        srgb_to_linear(blue),
        alpha,
    ]
}

pub(crate) fn srgba(color: &PaintColor, opacity: f32) -> [f32; 4] {
    let mut color = match color {
        PaintColor::Named(name) => csscolorparser::parse(name)
            .map(|color| color.to_array())
            .unwrap_or([0.0, 0.0, 0.0, 0.0]),
        PaintColor::Srgba {
            red,
            green,
            blue,
            alpha,
        } => [
            *red as f32 / 255.0,
            *green as f32 / 255.0,
            *blue as f32 / 255.0,
            *alpha,
        ],
        PaintColor::Hsla {
            hue_degrees,
            saturation,
            lightness,
            alpha,
        } => hsl_to_srgba(
            *hue_degrees,
            *saturation / 100.0,
            *lightness / 100.0,
            *alpha,
        ),
    };
    color[3] *= opacity;
    color
}

fn hsl_to_srgba(hue: f32, saturation: f32, lightness: f32, alpha: f32) -> [f32; 4] {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue.rem_euclid(360.0) / 60.0;
    let x = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let (red, green, blue) = match sector as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let match_value = lightness - chroma / 2.0;
    [
        red + match_value,
        green + match_value,
        blue + match_value,
        alpha,
    ]
}

pub(crate) fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!((srgb_to_linear(0.02) - 0.02 / 12.92).abs() < f32::EPSILON);
        assert!(srgb_to_linear(1.0) > 0.99);
    }
}
