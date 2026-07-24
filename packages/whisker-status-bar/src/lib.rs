//! `whisker-status-bar` — imperative status-bar control, mirroring
//! [`expo-status-bar`](https://docs.expo.dev/versions/latest/sdk/status-bar/)'s
//! `setStatusBarHidden` / `setStatusBarStyle`.
//!
//! **API shape — 5 (Static methods).** Two stateless one-shot
//! operations namespaced under the unit struct [`WhiskerStatusBar`]:
//!
//! - [`set_hidden`](WhiskerStatusBar::set_hidden) — show/hide the system
//!   status bar (animated fade on iOS).
//! - [`set_style`](WhiskerStatusBar::set_style) — set the content color
//!   ([`StatusBarStyle::Light`] = light icons for a dark background,
//!   [`StatusBarStyle::Dark`] = dark icons for a light background).
//!
//! Deliberately **not async** — the native module DSL only supports
//! synchronous `Function`s today, and toggling the status bar is
//! effectively instant on both platforms. The native handlers hop to
//! the UI thread themselves ([`DispatchQueue.main`] / `runOnUiThread`),
//! so callers may invoke from a reactive-thread `on_mount`/`on_cleanup`
//! without wrapping.
//!
//! ## iOS setup (handled automatically)
//!
//! iOS routes status-bar appearance through the foreground view
//! controller by default, which a view-less module can't reach. This
//! crate's [`plugin`] injects `UIViewControllerBasedStatusBarAppearance
//! = false` into `Info.plist` (when the app opts in via
//! `app.plugin::<WhiskerStatusBar>(|c| c)`), which re-enables the
//! app-level `UIApplication` status-bar APIs the native module calls.
//! Android needs no such setup — `WindowInsetsControllerCompat` drives
//! the status bar directly off the host activity's window.
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
//! - iOS: `crates/whisker-status-bar/ios/Sources/WhiskerStatusBar/StatusBarModule.swift`
//! - Android: `crates/whisker-status-bar/android/src/main/kotlin/rs/whisker/modules/status_bar/StatusBarModule.kt`

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
