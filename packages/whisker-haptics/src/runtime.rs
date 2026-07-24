//! `WhiskerHaptics` runtime API — hand-written wrapper over the
//! framework primitive: each method builds the raw `Vec<WhiskerValue>`
//! arg list, dispatches via
//! `whisker::module!("WhiskerHaptics").invoke(method, args)`, and
//! lifts the returned `WhiskerValue` into a typed result. `module!`
//! prepends this crate's name (→ `whisker-haptics:WhiskerHaptics`) so
//! module names never collide across crates.

use whisker::platform_module::{WhiskerModuleError, WhiskerValue};

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

impl NotificationType {
    fn as_str(self) -> &'static str {
        match self {
            NotificationType::Success => "success",
            NotificationType::Warning => "warning",
            NotificationType::Error => "error",
        }
    }
}

/// Typed Rust API for the `WhiskerHaptics` platform module. The
/// struct itself lives in `plugin.rs` (see its doc comment for why);
/// this `impl` block just adds the runtime methods.
impl WhiskerHaptics {
    /// Fire a physical "bump", scaled by `style`. Use when a tap
    /// resolves (e.g. inside an `on_tap` handler) — not on every
    /// touchstart, since a touch that turns into a scroll/drag and
    /// never becomes a real tap shouldn't buzz.
    pub fn impact(style: ImpactStyle) -> Result<(), WhiskerModuleError> {
        let result = whisker::module!("WhiskerHaptics").invoke(
            "impact",
            vec![WhiskerValue::String(style.as_str().to_string())],
        );
        match result {
            WhiskerValue::Error(msg) => Err(WhiskerModuleError(msg)),
            _ => Ok(()),
        }
    }

    /// Fire a light tick — for discrete value changes, e.g. a drag
    /// gesture starting.
    pub fn selection() -> Result<(), WhiskerModuleError> {
        let result = whisker::module!("WhiskerHaptics").invoke("selection", vec![]);
        match result {
            WhiskerValue::Error(msg) => Err(WhiskerModuleError(msg)),
            _ => Ok(()),
        }
    }

    /// Fire a longer pattern communicating success/warning/error.
    pub fn notification(kind: NotificationType) -> Result<(), WhiskerModuleError> {
        let result = whisker::module!("WhiskerHaptics").invoke(
            "notification",
            vec![WhiskerValue::String(kind.as_str().to_string())],
        );
        match result {
            WhiskerValue::Error(msg) => Err(WhiskerModuleError(msg)),
            _ => Ok(()),
        }
    }
}
