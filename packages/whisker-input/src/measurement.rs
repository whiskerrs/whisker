use whisker::runtime::module_measurement::{
    MeasureFontFamily, MeasureFontStyle, MeasureLineHeight,
};
use whisker::{CustomMeasurePayload, ModuleMeasureContext, WhiskerValue};

mod data {
    use serde::{Deserialize, Serialize};

    pub const VERSION: u16 = 1;

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct InputMeasureData {
        pub scale_factor: f32,
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

pub(super) fn payload(context: ModuleMeasureContext<'_>) -> Option<CustomMeasurePayload> {
    if context.property("auto-size") != Some(&WhiskerValue::Bool(true)) {
        return None;
    }
    let style = &context.text_style()?.style;
    let text = match context.property("value") {
        Some(WhiskerValue::String(value)) => value.clone(),
        _ => String::new(),
    };
    let multiline = context.property("multiline") == Some(&WhiskerValue::Bool(true));
    let secure = context.property("secure") == Some(&WhiskerValue::Bool(true));
    let text = if secure {
        "•".repeat(text.chars().count())
    } else {
        text
    };
    let input = data::InputMeasureData {
        scale_factor: context.scale_factor(),
        text,
        multiline,
        font_family: style.font_families.first().and_then(|family| match family {
            MeasureFontFamily::System => None,
            MeasureFontFamily::Named(name) => Some(name.clone()),
        }),
        font_size: style.font_size,
        font_weight: style.font_weight,
        italic: style.font_style != MeasureFontStyle::Normal,
        line_height: match style.line_height {
            MeasureLineHeight::Normal => None,
            MeasureLineHeight::LogicalPixels(value) => Some(value),
        },
        letter_spacing: style.letter_spacing,
    };
    Some(CustomMeasurePayload {
        version: data::VERSION,
        data: serde_json::to_vec(&input).expect("validated input measurement values"),
    })
}
