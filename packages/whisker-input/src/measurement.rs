use whisker::runtime::module_measurement::{
    MeasureFontFamily, MeasureFontStyle, MeasureLineHeight,
};
use whisker::{CustomMeasurePayload, ModuleMeasureContext, WhiskerValue};

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
    Some(CustomMeasurePayload {
        version: 1,
        data: WhiskerValue::map([
            (
                "scale_factor",
                WhiskerValue::Float(context.scale_factor() as f64),
            ),
            ("text", WhiskerValue::String(text)),
            ("multiline", WhiskerValue::Bool(multiline)),
            (
                "font_family",
                match style.font_families.first() {
                    Some(MeasureFontFamily::Named(name)) => WhiskerValue::String(name.clone()),
                    _ => WhiskerValue::Null,
                },
            ),
            ("font_size", WhiskerValue::Float(style.font_size as f64)),
            ("font_weight", WhiskerValue::Int(style.font_weight as i64)),
            (
                "italic",
                WhiskerValue::Bool(style.font_style != MeasureFontStyle::Normal),
            ),
            (
                "line_height",
                match style.line_height {
                    MeasureLineHeight::Normal => WhiskerValue::Null,
                    MeasureLineHeight::LogicalPixels(value) => WhiskerValue::Float(value as f64),
                },
            ),
            (
                "letter_spacing",
                WhiskerValue::Float(style.letter_spacing as f64),
            ),
        ]),
    })
}
