use whisker_protocol::PaintColor;

pub(crate) fn css_color(value: &PaintColor) -> String {
    match value {
        PaintColor::Named(name) => name.clone(),
        PaintColor::Srgba {
            red,
            green,
            blue,
            alpha,
        } => format!("rgba({red}, {green}, {blue}, {alpha})"),
        PaintColor::Hsla {
            hue_degrees,
            saturation,
            lightness,
            alpha,
        } => format!("hsla({hue_degrees}, {saturation}%, {lightness}%, {alpha})"),
    }
}
