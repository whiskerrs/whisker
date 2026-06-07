//! Glue between `whisker-cng` and the CLI.
//!
//! Responsibilities split:
//!
//! - `whisker-cng` owns the *pure* renderer: AppConfig + paths → files
//!   on disk. No shelling out, no environment assumptions. Pure logic
//!   so it stays unit-testable against tempdirs.
//! - This module decides *where* the gen dirs live (always
//!   `<crate_dir>/gen/{android,ios}`), resolves the Whisker native
//!   runtime paths (today: `<workspace>/native/{android,ios}`), and
//!   handles the side-effect bits that follow a sync — running
//!   `xcodegen generate` after iOS regeneration so the
//!   `<scheme>.xcodeproj` is fresh before `xcodebuild` runs.
//!
//! Public entry point: [`sync_for_target`]. The cli's `run` and
//! `build` subcommands call this before kicking off the rest of the
//! build pipeline.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use whisker_app_config::AppConfig;
use whisker_dev_server::Target;

/// Run the platform-appropriate sync for `target`. Returns the gen
/// directory the caller should hand to gradle / xcodebuild — useful
/// even for the fast-path (`regenerated == false`) case.
pub fn sync_for_target(
    target: Target,
    app_config: &AppConfig,
    crate_dir: &Path,
    workspace_root: &Path,
    package: &str,
) -> Result<PlatformSync> {
    match target {
        Target::Android => sync_android(app_config, crate_dir, workspace_root, package),
        Target::IosSimulator => sync_ios(app_config, crate_dir, workspace_root),
        Target::Host => Ok(PlatformSync {
            gen_dir: crate_dir.to_path_buf(),
            regenerated: false,
        }),
    }
}

/// Outcome of one sync_native pass.
#[derive(Debug, Clone)]
pub struct PlatformSync {
    /// Where the generated project tree lives — `gen/android/` or
    /// `gen/ios/` under `crate_dir`. For `Target::Host` this is just
    /// `crate_dir` (no native project to generate).
    pub gen_dir: PathBuf,
    /// `true` if the renderer rewrote files this pass, `false` if the
    /// fingerprint matched and the existing tree was reused.
    pub regenerated: bool,
}

/// SDK version pinned into the cng-generated
/// `app/build.gradle.kts` (`rs.whisker:whisker-runtime-android:<this>`).
/// Bumped alongside the `sdk-v*` release tag.
///
/// 0.1.1 rolls forward the transitive Lynx pin baked into the SDK's
/// POM from `v3.8.0-whisker.4` (initial SDK release) to
/// `v3.8.0-whisker.6`. The newer Lynx exposes `lynx_capi_abi_version()`
/// which the Step-6 dlopen-based bridge requires; without this bump,
/// downstream apps that pull `whisker-runtime-android:0.1.0`
/// transitively get the older Lynx and the bridge loader aborts on
/// "undefined symbol: lynx_capi_abi_version" at engine_attach time.
const WHISKER_SDK_VERSION: &str = "0.1.1";
/// Gradle plugin version pinned into the generated
/// `settings.gradle.kts` `pluginManagement.plugins` + `plugins`
/// blocks. Bumped independently from the SDK via the
/// `gradle-plugin-v*` release tag.
///
/// 0.3.0 was the first version with the two-JAR split (Settings
/// plugin / Project plugin in separate Maven artifacts). 0.4.0
/// adds two fixes that surfaced during the first Step-5 e2e:
///   - `WhiskerBuildTask.workspace` switched from `@InputDirectory`
///     to `@Internal` so Gradle stops walking the cargo workspace
///     tree (which contains other subprojects' `build/` dirs)
///     and refusing the build for implicit dependencies.
///   - `WhiskerProjectPlugin` now wires the aggregator Kotlin
///     generator into `variant.sources.java` (which AGP 8.6's
///     Kotlin compile actually depends on) rather than `.kotlin`
///     alone, plus places the staged `.so` into a nested
///     `<jniLibsDir>/<abi>/` subdir so AGP's `mergeJniLibFolders`
///     recognises the layout.
const WHISKER_GRADLE_PLUGIN_VERSION: &str = "0.4.0";
const WHISKER_MAVEN_URL: &str = "https://whiskerrs.github.io/whisker/maven";
const LYNX_MAVEN_URL: &str = "https://whiskerrs.github.io/lynx/maven";

fn sync_android(
    app_config: &AppConfig,
    crate_dir: &Path,
    workspace_root: &Path,
    package: &str,
) -> Result<PlatformSync> {
    // Settings plugin reads `workspace` as a `file(...)` — Gradle
    // resolves that relative to the settings.gradle.kts directory
    // (= `gen/android/`). Hand the renderer the absolute path; the
    // template embeds it verbatim. Absolute keeps the generated
    // tree independent of `gen/android`'s on-disk depth, at the cost
    // of looking less portable in diffs (acceptable — these files
    // are AUTO-GENERATED and not meant to be committed).
    let workspace_path = workspace_root.to_path_buf();
    let inputs = whisker_cng::android::inputs_from(
        app_config,
        package.replace('-', "_"),
        workspace_path,
        package.to_string(),
        WHISKER_SDK_VERSION.to_string(),
        WHISKER_GRADLE_PLUGIN_VERSION.to_string(),
        WHISKER_MAVEN_URL.to_string(),
        LYNX_MAVEN_URL.to_string(),
    )?;
    let gen_dir = crate_dir.join("gen/android");
    let regenerated = whisker_cng::sync_android(&gen_dir, &inputs).context("render gen/android")?;
    Ok(PlatformSync {
        gen_dir,
        regenerated,
    })
}

fn sync_ios(
    app_config: &AppConfig,
    crate_dir: &Path,
    workspace_root: &Path,
) -> Result<PlatformSync> {
    let gen_dir = crate_dir.join("gen/ios");
    let whisker_runtime = workspace_root.join("platforms/ios");
    // `gen/ios/whisker_modules/` is populated lazily by
    // `whisker-build::ios::stage_module_swift_sources` later in the
    // pipeline (between cargo build and xcodebuild). The pbxproj
    // template's `XCLocalSwiftPackageReference` for WhiskerModules
    // needs an *absolute* path to that directory at sync time, so we
    // pre-compute it here even though the contents will land later.
    let whisker_modules = gen_dir.join("whisker_modules");
    let inputs = whisker_cng::ios::inputs_from(app_config, whisker_runtime, whisker_modules)?;
    // whisker-cng renders the full Xcode project directly (pbxproj +
    // xcworkspacedata + sources). No xcodegen subprocess needed —
    // see crates/whisker-cng/src/ios.rs for the rationale.
    let regenerated = whisker_cng::sync_ios(&gen_dir, &inputs).context("render gen/ios")?;
    Ok(PlatformSync {
        gen_dir,
        regenerated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_target_returns_crate_dir_without_regenerating() {
        let cfg = AppConfig::default();
        let crate_dir = PathBuf::from("/tmp/crate");
        let ws = PathBuf::from("/tmp/ws");
        let sync = sync_for_target(Target::Host, &cfg, &crate_dir, &ws, "pkg").unwrap();
        assert_eq!(sync.gen_dir, crate_dir);
        assert!(!sync.regenerated);
    }
}
