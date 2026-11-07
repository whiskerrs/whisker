use whisker::runtime::module_measurement::{
    MeasureFontFamily, MeasureFontStyle, MeasureLineHeight,
};
use whisker::{CustomMeasurePayload, ModuleMeasureContext, WhiskerValue};

pub(super) fn payload(context: ModuleMeasureContext<'_>) -> Option<CustomMeasurePayload> {
    let style = &context.text_style()?.style;
    let flag = |name| context.property(name) == Some(&WhiskerValue::Bool(true));
    let multiline = flag("multiline");
    let text = if flag("auto-size") {
        let value = match context.property("value") {
            Some(WhiskerValue::String(value)) => value.as_str(),
            _ => "",
        };
        content_text(value, flag("secure"))
    } else {
        let lines = match context.property("lines") {
            Some(WhiskerValue::Int(lines)) => *lines,
            _ => 0,
        };
        empty_lines_text(multiline, lines)
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

fn content_text(value: &str, secure: bool) -> String {
    if secure {
        "•".repeat(value.chars().count())
    } else {
        value.to_owned()
    }
}

/// Sizes a fixed field by measuring empty lines, which every Host's existing
/// version-1 measurer already handles, so older pinned Hosts need no change.
fn empty_lines_text(multiline: bool, lines: i64) -> String {
    let lines = if multiline { lines.max(1) } else { 1 };
    "\n".repeat(usize::try_from(lines - 1).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_field_measures_one_line_unless_multiline_lines_is_set() {
        assert_eq!(empty_lines_text(false, 0), "");
        assert_eq!(empty_lines_text(false, 4), "");
        assert_eq!(empty_lines_text(true, 0), "");
        assert_eq!(empty_lines_text(true, 3), "\n\n");
    }

    #[test]
    fn auto_size_masks_secure_content() {
        assert_eq!(content_text("abc", true), "•••");
        assert_eq!(content_text("abc", false), "abc");
    }
}
