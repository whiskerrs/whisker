//! `whisker-status-bar` — imperative status-bar control, mirroring
//! [`expo-status-bar`](https://docs.expo.dev/versions/latest/sdk/status-bar/)'s
//! `setStatusBarHidden` / `setStatusBarStyle`.
//!
//! **API shape — 5 (Static methods).** Two stateless one-shot
//! operations namespaced under the unit struct [`WhiskerStatusBar`]:
//!
//! - [`set_hidden`](WhiskerStatusBar::set_hidden) — show/hide the system
//!   status bar.
//! - [`set_style`](WhiskerStatusBar::set_style) — set the content color
//!   ([`StatusBarStyle::Light`] = light icons for a dark background,
//!   [`StatusBarStyle::Dark`] = dark icons for a light background).
//!
//! **Android-only** — both methods no-op on iOS, Web, and Desktop. The only status-bar API
//! a view-less module can reach there is the deprecated app-level
//! `UIApplication.setStatusBarHidden`, which corrupts `whisker-router`'s
//! transform-based transition animations; iOS support needs
//! `WhiskerViewController.prefersStatusBarHidden` instead.
//!
//! Deliberately **not async** — the native module DSL only supports
//! synchronous `Function`s today, and toggling the status bar is
//! effectively instant. The native handler hops to the UI thread itself
//! (`runOnUiThread`), so callers may invoke from a reactive-thread
//! `on_mount` / `on_cleanup` without wrapping.
//!
//! ## Usage
//!
//! ```ignore
//! use whisker_status_bar::WhiskerStatusBar;
//!
//! let _ = WhiskerStatusBar::set_hidden(true);
//! ```
//!
//! ## Native source
//!
//! - Android: `packages/whisker-status-bar/android/src/main/kotlin/rs/whisker/modules/status_bar/StatusBarModule.kt`

/// Plugin (iOS `Info.plist` injection). Always compiles — independent
/// of the `runtime` feature so the `whisker.rs` config probe (which
/// pulls this crate with `default-features = false`) can resolve
/// `WhiskerStatusBar`.
mod plugin;
pub use plugin::*;

/// `WhiskerStatusBar` runtime API. Gated behind the default-on
/// `runtime` feature so the config-probe build path can skip the
/// heavyweight `whisker` umbrella crate.
#[cfg(feature = "runtime")]
mod runtime;
#[cfg(feature = "runtime")]
pub use runtime::StatusBarStyle;
