//! Whisker build orchestration.
//!
//! Cross-platform cargo + gradle + xcodebuild invocation, shared by
//! `whisker-dev-server`'s full reload path, the cli, and the
//! `whisker-build` binary that gradle / xcodebuild call into during
//! `whisker run`.
//!
//! ## Public surface
//!
//! - [`Profile`] — Debug / Release selector.
//! - [`capture`] — hot-reload patch capture shim wiring (rustc /
//!   linker workspace wrappers + cache dirs + env-var assembly).
//!   Consumed by the dev-server's full reload fat build (capture: Some)
//!   and the xcodebuild Build Phase path (capture: None).
//! - [`android`] — NDK toolchain resolution, `cargo rustc
//!   --crate-type dylib`, jniLibs staging, `gradle assemble{Debug,Release}`.
//! - [`ios`] — `cargo rustc` per iOS triple, lipo of simulator
//!   slices, `WhiskerDriver.xcframework` assembly, `xcodebuild` for
//!   the generated app project.
//! - [`modules`] — discover `[package.metadata.whisker]` deps via
//!   `cargo metadata` and resolve per-platform source contributions
//!   the host build needs to stage.
//!
//! Sync-only API. Dev-server callers wrap invocations in
//! `tokio::task::spawn_blocking`; the cli runs them directly.

pub mod android;
pub mod capture;
pub mod child_guard;
pub mod ios;
pub mod macos;
pub mod modules;
pub mod ui;
pub mod web;

pub use capture::{
    CaptureShims, capture_env_vars, capture_env_vars_all_crates, capture_env_vars_for_triple,
    target_linker_env_var, target_rustflags_env_var,
};

/// Build profile. Maps to `cargo --release` and to the
/// gradle assemble{Debug,Release} task.
///
/// An enum rather than `release: bool` so a call site can't pass the
/// wrong literal while reading as if it were right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Debug,
    Release,
}

impl Profile {
    /// `--release` for cargo when `Release`, no flag for `Debug`.
    pub fn cargo_flag(self) -> Option<&'static str> {
        match self {
            Profile::Debug => None,
            Profile::Release => Some("--release"),
        }
    }

    /// `release` / `debug` — gradle assemble task suffix and cargo
    /// `target/<triple>/<this>` segment.
    pub fn dir_name(self) -> &'static str {
        match self {
            Profile::Debug => "debug",
            Profile::Release => "release",
        }
    }
}
