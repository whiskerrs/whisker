//! Web Host implementation for `whisker-haptics` using the Vibration API.

use whisker_web::{ModuleDefinition, WhiskerModule, WhiskerValue};

const MODULE_NAME: &str = "whisker-haptics:WhiskerHaptics";

struct HapticsModule;

#[whisker_web::WhiskerModule]
impl WhiskerModule for HapticsModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new()
            .name(MODULE_NAME)
            .function("impact", |args, _| {
                let style = match string_argument(args, "impact") {
                    Ok(style) => style,
                    Err(error) => return error,
                };
                let duration = match style {
                    "light" => 10,
                    "medium" => 20,
                    "heavy" => 35,
                    _ => return invalid_value("impact", style),
                };
                vibrate(&[duration])
            })
            .function("selection", |args, _| {
                if !args.is_empty() {
                    return WhiskerValue::Error(
                        "WhiskerHaptics.selection does not accept arguments".into(),
                    );
                }
                vibrate(&[8])
            })
            .function("notification", |args, _| {
                let kind = match string_argument(args, "notification") {
                    Ok(kind) => kind,
                    Err(error) => return error,
                };
                let pattern: &[u32] = match kind {
                    "success" => &[20, 30, 20],
                    "warning" => &[30, 35, 30],
                    "error" => &[45, 30, 45],
                    _ => return invalid_value("notification", kind),
                };
                vibrate(pattern)
            })
    }
}

fn string_argument<'a>(args: &'a [WhiskerValue], operation: &str) -> Result<&'a str, WhiskerValue> {
    let [WhiskerValue::String(value)] = args else {
        return Err(WhiskerValue::Error(format!(
            "WhiskerHaptics.{operation} requires one string argument"
        )));
    };
    Ok(value)
}

fn invalid_value(operation: &str, value: &str) -> WhiskerValue {
    WhiskerValue::Error(format!(
        "WhiskerHaptics.{operation} received unsupported value {value:?}"
    ))
}

fn vibrate(pattern: &[u32]) -> WhiskerValue {
    let Some(window) = web_sys::window() else {
        return WhiskerValue::Error("browser Window is unavailable".into());
    };
    let values = js_sys::Array::new();
    for duration in pattern {
        values.push(&(*duration).into());
    }
    // Unsupported browsers return false. That is a capability fallback, not
    // an application error, so callers retain the cross-platform fire-and-
    // forget semantics.
    let _ = window.navigator().vibrate_with_pattern(&values.into());
    WhiskerValue::Null
}
