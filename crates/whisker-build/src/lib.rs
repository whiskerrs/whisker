//! Whisker build orchestration.
//!
//! Cross-platform cargo + gradle + xcodebuild invocation, shared by
//! `whisker-cli`'s `build` subcommand and `whisker-dev-server`'s
//! Tier 2 cold rebuild path.
//!
//! ## Public surface
//!
//! - [`Profile`] — Debug / Release selector.
//! - [`capture`] — Tier 1 hot-patch capture shim wiring (rustc /
//!   linker workspace wrappers + cache dirs + env-var assembly).
//!   Consumed by both the cli's prod build (capture: None) and the
//!   dev-server's Tier 2 fat build (capture: Some).
//! - [`android`] — NDK toolchain resolution, `cargo rustc
//!   --crate-type dylib`, jniLibs staging, `gradle assemble{Debug,Release}`.
//! - [`ios`] — `cargo rustc` per iOS triple, lipo of simulator
//!   slices, `WhiskerDriver.xcframework` assembly, `xcodebuild` for
//!   the generated app project.
//! - [`lynx`] — Lynx artifact fetcher: downloads the pinned
//!   `whiskerrs/lynx` release, verifies SHA-256, unpacks into
//!   `~/.cache/whisker/lynx/<version>/`. `WHISKER_LYNX_DIR` env var
//!   overrides for local builds.
//!
//! Sync-only API. Dev-server callers wrap invocations in
//! `tokio::task::spawn_blocking`; the cli runs them directly.

pub mod android;
pub mod capture;
pub mod ios;
pub mod lynx;

pub use capture::{
    capture_env_vars, target_linker_env_var, target_rustflags_env_var, CaptureShims,
};
pub use lynx::{
    cache_dir as lynx_cache_dir, cache_version_root as lynx_cache_version_root,
    ensure_lynx_android, ensure_lynx_ios, link_into_workspace as link_lynx_into_workspace,
    LynxPlatform, LYNX_FORK_TAG, LYNX_VERSION,
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
