use gtk4::gdk;
use whisker_protocol::PaintColor;

pub(crate) fn rgba(color: &PaintColor) -> gdk::RGBA {
    let [red, green, blue, alpha] = match color {
        PaintColor::Named(name) => csscolorparser::parse(name)
            .map(|color| color.to_array())
            .unwrap_or([0.0, 0.0, 0.0, 0.0]),
        PaintColor::Srgba {
            red,
            green,
            blue,
            alpha,
        } => [
            f32::from(*red) / 255.0,
            f32::from(*green) / 255.0,
            f32::from(*blue) / 255.0,
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
    gdk::RGBA::new(red, green, blue, alpha)
}

pub(crate) fn is_transparent(color: &gdk::RGBA) -> bool {
    color.alpha() <= 0.0
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
    let base = lightness - chroma / 2.0;
    [red + base, green + base, blue + base, alpha]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_every_color_notation_to_srgb() {
        let named = rgba(&PaintColor::Named("red".into()));
        assert_eq!((named.red(), named.green(), named.alpha()), (1.0, 0.0, 1.0));
        let hsl = rgba(&PaintColor::Hsla {
            hue_degrees: 240.0,
            saturation: 100.0,
            lightness: 50.0,
            alpha: 0.5,
        });
        assert_eq!((hsl.blue(), hsl.red(), hsl.alpha()), (1.0, 0.0, 0.5));
        assert!(is_transparent(&rgba(&PaintColor::Named("bogus".into()))));
    }
}
