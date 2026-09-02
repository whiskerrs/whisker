//! `WhiskerHaptics` runtime API — hand-written wrapper over the
//! framework primitive: each method builds the raw `Vec<WhiskerValue>`
//! arg list, dispatches via
//! `whisker::module!("WhiskerHaptics").invoke(method, args)`, and
//! lifts the returned `WhiskerValue` into a typed result. `module!`
//! prepends this crate's name (→ `whisker-haptics:WhiskerHaptics`) so
//! module names never collide across crates.

use whisker::platform_module::WhiskerModuleError;
#[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
use whisker::platform_module::WhiskerValue;

use crate::plugin::WhiskerHaptics;

/// Physical "bump" intensity for [`WhiskerHaptics::impact`]. Matches
/// `expo-haptics`'s `ImpactFeedbackStyle` (`Soft`/`Rigid` aren't
/// exposed — no call site needs them, and Android has no equivalent
/// predefined effect).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImpactStyle {
    Light,
    Medium,
    Heavy,
}

#[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
impl ImpactStyle {
    fn as_str(self) -> &'static str {
        match self {
            ImpactStyle::Light => "light",
            ImpactStyle::Medium => "medium",
            ImpactStyle::Heavy => "heavy",
        }
    }
}

/// Outcome pattern for [`WhiskerHaptics::notification`]. Matches
/// `expo-haptics`'s `NotificationFeedbackType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationType {
    Success,
    Warning,
    Error,
}

#[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
impl NotificationType {
    fn as_str(self) -> &'static str {
        match self {
            NotificationType::Success => "success",
            NotificationType::Warning => "warning",
            NotificationType::Error => "error",
        }
    }
}

/// Typed Rust API for the `WhiskerHaptics` platform module. The struct
/// itself lives in `plugin.rs` — one unit struct serves as both the
/// plugin and this namespace.
impl WhiskerHaptics {
    /// Fire a physical "bump", scaled by `style`. Use when a tap
    /// resolves (e.g. inside an `on_tap` handler) — not on every
    /// touchstart, since a touch that turns into a scroll/drag and
    /// never becomes a real tap shouldn't buzz.
    pub fn impact(style: ImpactStyle) -> Result<(), WhiskerModuleError> {
        #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
        {
            return invoke(
                "impact",
                vec![WhiskerValue::String(style.as_str().to_string())],
            );
        }
        #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
        {
            let _ = style;
            Ok(())
        }
    }

    /// Fire a light tick — for discrete value changes, e.g. a drag
    /// gesture starting.
    pub fn selection() -> Result<(), WhiskerModuleError> {
        #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
        {
            return invoke("selection", vec![]);
        }
        #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
        Ok(())
    }

    /// Fire a longer pattern communicating success/warning/error.
    pub fn notification(kind: NotificationType) -> Result<(), WhiskerModuleError> {
        #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
        {
            return invoke(
                "notification",
                vec![WhiskerValue::String(kind.as_str().to_string())],
            );
        }
        #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
        {
            let _ = kind;
            Ok(())
        }
    }
}

#[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
fn invoke(function: &str, arguments: Vec<WhiskerValue>) -> Result<(), WhiskerModuleError> {
    let result = whisker::module!("WhiskerHaptics").invoke(function, arguments);
    match result {
        WhiskerValue::Error(msg) => Err(WhiskerModuleError(msg)),
        _ => Ok(()),
    }
}

#[cfg(all(
    test,
    not(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))
))]
mod tests {
    use super::*;

    #[test]
    fn desktop_fallback_is_a_successful_no_op() {
        assert!(WhiskerHaptics::impact(ImpactStyle::Heavy).is_ok());
        assert!(WhiskerHaptics::selection().is_ok());
        assert!(WhiskerHaptics::notification(NotificationType::Success).is_ok());
    }
}
