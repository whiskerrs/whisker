//! `whisker-haptics` — haptic feedback for the giga reader UI.
//!
//! **API shape — 5 (Static methods).** See
//! [`docs/module-api-design.md`](https://github.com/whiskerrs/whisker/blob/main/docs/module-api-design.md)
//! §"Shape 5". Three stateless one-shot operations, namespaced under
//! the unit struct [`WhiskerHaptics`]:
//!
//! - [`impact`](WhiskerHaptics::impact) — a physical "bump", scaled
//!   by [`ImpactStyle`]. Use for a button/card tap resolving.
//! - [`selection`](WhiskerHaptics::selection) — a light tick, for
//!   discrete value changes (e.g. a drag gesture starting).
//! - [`notification`](WhiskerHaptics::notification) — a longer
//!   pattern communicating success/warning/error.
//!
//! Mirrors [`expo-haptics`](https://docs.expo.dev/versions/latest/sdk/haptics/)'s
//! three functions exactly — this app's only RN precedent
//! (`useReaderProgressBar.ts`) uses `selectionAsync()` +
//! `impactAsync(Light)` and nothing else.
//!
//! Deliberately **not async**: the native module DSL only supports
//! synchronous `Function`s today (no `AsyncFunction` yet). Firing a
//! haptic is already effectively instant on both platforms, so no
//! `resource()`/`run_blocking()` wrapping is needed at call sites.
//!
//! ## Usage
//!
//! ```ignore
//! use whisker_haptics::{ImpactStyle, WhiskerHaptics};
//!
//! let _ = WhiskerHaptics::impact(ImpactStyle::Light);
//! ```
//!
//! ## Android permission
//!
//! Requires `android.permission.VIBRATE` — injected automatically by
//! this crate's [`plugin`] when the consuming app opts in via
//! `app.plugin::<WhiskerHaptics>(|c| c)` in `whisker.rs`.
//!
//! ## Native source
//!
//! - iOS: `crates/whisker-haptics/ios/Sources/WhiskerHaptics/HapticsModule.swift`
//! - Android: `crates/whisker-haptics/android/src/main/kotlin/rs/whisker/modules/haptics/HapticsModule.kt`

/// Plugin (`Android VIBRATE` manifest injection). Always compiles —
/// independent of the `runtime` feature so the `whisker.rs` config
/// probe (which pulls this crate with `default-features = false`)
/// can still resolve `WhiskerHaptics`.
mod plugin;
pub use plugin::*;

/// `WhiskerHaptics` runtime API. Gated behind the default-on
/// `runtime` feature so the config probe build path can skip the
/// heavyweight `whisker` umbrella crate (Lynx bridge, driver, render
/// layer). Apps depending on `whisker-haptics` for actual haptic
/// calls get this re-exported automatically; the probe only sees the
/// plugin types.
#[cfg(feature = "runtime")]
mod runtime;
#[cfg(feature = "runtime")]
pub use runtime::{ImpactStyle, NotificationType};
