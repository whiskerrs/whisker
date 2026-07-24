//! `WhiskerStatusBar` runtime API — hand-written wrapper over the
//! framework primitive: each method builds the raw `Vec<WhiskerValue>`
//! arg list, dispatches via
//! `whisker::module!("WhiskerStatusBar").invoke(method, args)`, and
//! lifts the returned `WhiskerValue` into a typed result.
//!
//! **Android-only.** The native calls fire only on Android. On iOS they
//! are a no-op: the only status-bar API reachable from a view-less module
//! is the deprecated app-level `UIApplication.setStatusBarHidden`, and
//! that call corrupts `whisker-router`'s transform-based transition
//! animations whenever it lands around one (verified on-device). iOS
//! support is a TODO — it needs to drive the status bar through
//! `WhiskerViewController.prefersStatusBarHidden` (the non-deprecated,
//! view-controller-based API) instead. Until then the methods no-op on
//! iOS so call sites work unchanged on both platforms.

use whisker::platform_module::WhiskerModuleError;
#[cfg(target_os = "android")]
use whisker::platform_module::WhiskerValue;

use crate::plugin::WhiskerStatusBar;

/// Status-bar content color, matching `expo-status-bar`'s `style`.
/// [`Light`](StatusBarStyle::Light) draws light (white) icons for a dark
/// background; [`Dark`](StatusBarStyle::Dark) draws dark icons for a
/// light background.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusBarStyle {
    Light,
    Dark,
}

#[cfg(target_os = "android")]
impl StatusBarStyle {
    fn as_str(self) -> &'static str {
        match self {
            StatusBarStyle::Light => "light",
            StatusBarStyle::Dark => "dark",
        }
    }
}

/// Typed Rust API for the `WhiskerStatusBar` platform module. The struct
/// itself lives in `plugin.rs` (see its doc comment for why); this
/// `impl` block just adds the runtime methods.
impl WhiskerStatusBar {
    /// Show or hide the system status bar (Android only — see the module
    /// docs; no-op on iOS). Immediate on Android.
    pub fn set_hidden(hidden: bool) -> Result<(), WhiskerModuleError> {
        #[cfg(target_os = "android")]
        {
            let result = whisker::module!("WhiskerStatusBar")
                .invoke("setHidden", vec![WhiskerValue::Bool(hidden)]);
            if let WhiskerValue::Error(msg) = result {
                return Err(WhiskerModuleError(msg));
            }
        }
        #[cfg(not(target_os = "android"))]
        let _ = hidden;
        Ok(())
    }

    /// Set the status-bar content color (Android only; no-op on iOS).
    /// See [`StatusBarStyle`].
    pub fn set_style(style: StatusBarStyle) -> Result<(), WhiskerModuleError> {
        #[cfg(target_os = "android")]
        {
            let result = whisker::module!("WhiskerStatusBar").invoke(
                "setStyle",
                vec![WhiskerValue::String(style.as_str().to_string())],
            );
            if let WhiskerValue::Error(msg) = result {
                return Err(WhiskerModuleError(msg));
            }
        }
        #[cfg(not(target_os = "android"))]
        let _ = style;
        Ok(())
    }
}
