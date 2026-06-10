//! Whisker build orchestration.
//!
//! Cross-platform cargo + gradle + xcodebuild invocation, shared by
//! `whisker-dev-server`'s Tier 2 cold rebuild path, the cli, and the
//! `whisker-build` binary that gradle / xcodebuild call into during
//! `whisker run`.
//!
//! ## Public surface
//!
//! - [`Profile`] — Debug / Release selector.
//! - [`capture`] — Tier 1 hot-patch capture shim wiring (rustc /
//!   linker workspace wrappers + cache dirs + env-var assembly).
//!   Consumed by the dev-server's Tier 2 fat build (capture: Some)
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
//! No Lynx fetcher anymore. iOS resolves the four Lynx xcframeworks
//! via SPM `binaryTarget(url:checksum:)` in `platforms/ios/Package.
//! swift`; Android pulls `rs.whisker:lynx-android:<ver>` from the
//! `whiskerrs.github.io/lynx/maven` repository transitively via the
//! SDK pom. No `~/.cache/whisker/lynx/` directory is ever written.
//!
//! Sync-only API. Dev-server callers wrap invocations in
//! `tokio::task::spawn_blocking`; the cli runs them directly.

pub mod android;
pub mod capture;
pub mod child_guard;
pub mod ios;
pub mod modules;
pub mod ui;

pub use capture::{
    capture_env_vars, capture_env_vars_for_triple, target_linker_env_var, target_rustflags_env_var,
    CaptureShims,
};

/// Build profile. Maps to `cargo --release` and to the
/// gradle assemble{Debug,Release} task.
///
/// Why an enum (and not just `release: bool`): keeping the
/// semantics explicit at the API boundary stops the wrong literal
/// from sneaking through (`true` for "I want debug" because the
/// caller misread the field name). The cost is two characters.
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
