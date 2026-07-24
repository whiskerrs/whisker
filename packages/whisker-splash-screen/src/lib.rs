//! `whisker-splash-screen` — native splash screen generation, mirroring
//! [`expo-splash-screen`](https://docs.expo.dev/versions/latest/sdk/splash-screen/)'s
//! config plugin.
//!
//! A build-time config ([`WhiskerSplashScreenConfig`], set in
//! `whisker.rs`) — the splash image, background color, and resize mode —
//! that the [`plugin`] turns into the native launch screen:
//!
//! * **Android**: an Android 12 `SplashScreen` theme
//!   (`windowSplashScreenBackground` + `windowSplashScreenAnimatedIcon`),
//!   pointed at by the app's `<application android:theme>`, with
//!   `installSplashScreen()` injected into `MainActivity` and the
//!   `androidx.core:core-splashscreen` backport added.
//! * **iOS**: a `LaunchScreen.storyboard` (image centered on the
//!   background color) wired via `UILaunchStoryboardName`.
//!
//! It's a **static** splash: shown by the OS at launch and auto-hidden
//! when the first frame paints (Expo's default, minus `preventAutoHide`).
//! The imperative `prevent_auto_hide`/`hide` runtime control is a later
//! increment (it needs a native module).
//!
//! ## Usage in `whisker.rs`
//!
//! ```ignore
//! use whisker_splash_screen::{WhiskerSplashScreen, ResizeMode};
//!
//! app.plugin::<WhiskerSplashScreen>(|c| {
//!     c.image("assets/splash.png")
//!         .resize_mode(ResizeMode::Contain)
//!         .background_color("#ffffff");
//! });
//! ```

mod plugin;
pub use plugin::{ResizeMode, SplashDarkConfig, WhiskerSplashScreen, WhiskerSplashScreenConfig};
