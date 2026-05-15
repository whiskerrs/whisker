//! Android SDK discovery for Whisker-internal xtask steps. The
//! NDK-toolchain side (clang / clang++ / ar resolution) lived here
//! before P3 — that now belongs to `whisker-build`, since the only
//! consumer was user-app builds. xtask still needs `$ANDROID_HOME` to
//! locate the JDK and the specific NDK version Lynx's gn/ninja
//! toolchain pins (`build_lynx_aar` reads `ndk/21.1.6352462`), so the
//! `android_home` helper survives here.

use anyhow::Result;
use std::path::PathBuf;

pub fn android_home() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("ANDROID_HOME").map(PathBuf::from) {
        if p.is_dir() {
            return Ok(p);
        }
    }
    // macOS default install location.
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let cand = home.join("Library/Android/sdk");
        if cand.is_dir() {
            return Ok(cand);
        }
    }
    anyhow::bail!(
        "ANDROID_HOME not set and no SDK found at the default macOS \
         location ($HOME/Library/Android/sdk)."
    )
}
