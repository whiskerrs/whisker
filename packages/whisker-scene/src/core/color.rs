use whisker::css::{Angle, Color};

/// sRGB channels in `0.0..=1.0` and alpha.
pub(crate) fn srgb_rgba(color: Color) -> [f32; 4] {
    let (r, g, b, a) = match color {
        Color::Named(name) => {
            let [r, g, b] = name.srgb();
            (r, g, b, 1.0)
        }
        Color::Transparent => (0, 0, 0, 0.0),
        Color::Rgba(r, g, b, a) => (r, g, b, a),
        Color::Hsla { h, s, l, a } => {
            let degrees = match h {
                Angle::Deg(degrees) => degrees,
                Angle::Rad(radians) => radians.to_degrees(),
                Angle::Turn(turns) => turns * 360.0,
            };
            let [r, g, b] = hsl_to_srgb(degrees, s / 100.0, l / 100.0);
            return [r, g, b, a.clamp(0.0, 1.0)];
        }
    };
    [
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        a.clamp(0.0, 1.0),
    ]
}

/// Linear-light channels and alpha, as shaders blend them.
pub(crate) fn linear_rgba(color: Color) -> [f32; 4] {
    let [r, g, b, a] = srgb_rgba(color);
    [to_linear(r), to_linear(g), to_linear(b), a]
}

fn to_linear(channel: f32) -> f32 {
    if channel <= 0.040_45 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

fn hsl_to_srgb(hue: f32, saturation: f32, lightness: f32) -> [f32; 3] {
    let (saturation, lightness) = (saturation.clamp(0.0, 1.0), lightness.clamp(0.0, 1.0));
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue.rem_euclid(360.0) / 60.0;
    let x = chroma * (1.0 - (sector % 2.0 - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = lightness - chroma / 2.0;
    [r + m, g + m, b + m]
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker::css::NamedColor;

    #[test]
    fn resolves_every_color_form_to_the_same_srgb() {
        let red = [1.0, 0.0, 0.0, 1.0];
        assert_eq!(srgb_rgba(Color::Named(NamedColor::Red)), red);
        assert_eq!(srgb_rgba(Color::rgb(255, 0, 0)), red);
        assert_eq!(srgb_rgba(Color::hsl(0.0, 100.0, 50.0)), red);
        assert_eq!(srgb_rgba(Color::Transparent), [0.0; 4]);
    }

    #[test]
    fn decodes_srgb_to_linear() {
        let [r, g, b, a] = linear_rgba(Color::rgba(255, 128, 0, 0.5));
        assert_eq!((r, b, a), (1.0, 0.0, 0.5));
        assert!((g - 0.2158).abs() < 1e-3);
    }
}
