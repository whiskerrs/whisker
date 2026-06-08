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

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use whisker_app_config::AppConfig;
use whisker_cng::{discover_plugins, DiscoveredPlugin, Engine, SubprocessPlugin};
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
        Target::IosSimulator => sync_ios(app_config, crate_dir, workspace_root, package),
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
    let engine = build_engine_with_discovered_plugins(workspace_root, package)?;
    let inputs = whisker_cng::android::inputs_from_with_engine(
        &engine,
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
    package: &str,
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
    let engine = build_engine_with_discovered_plugins(workspace_root, package)?;
    let inputs = whisker_cng::ios::inputs_from_with_engine(
        &engine,
        app_config,
        whisker_runtime,
        whisker_modules,
        workspace_root.to_path_buf(),
        package.to_string(),
    )?;
    // whisker-cng renders the full Xcode project directly (pbxproj +
    // xcworkspacedata + sources). No xcodegen subprocess needed —
    // see crates/whisker-cng/src/ios.rs for the rationale.
    let regenerated = whisker_cng::sync_ios(&gen_dir, &inputs).context("render gen/ios")?;
    Ok(PlatformSync {
        gen_dir,
        regenerated,
    })
}

/// Build a [`whisker_cng::Engine`] populated with built-ins plus
/// every 3rd-party plugin discovered via `[package.metadata.whisker.plugins]`
/// in the user app's dep graph. Each discovered plugin's `[[bin]]`
/// target gets `cargo build`d (debug profile, workspace target dir)
/// and registered as a [`SubprocessPlugin`] pointing at the
/// resulting binary.
fn build_engine_with_discovered_plugins(
    workspace_root: &Path,
    user_package: &str,
) -> Result<Engine> {
    let manifest_path = workspace_root.join("Cargo.toml");
    let discovered = discover_plugins(&manifest_path, user_package)
        .with_context(|| format!("discover Whisker CNG plugins for `{user_package}`"))?;

    let mut engine = Engine::with_builtins();
    if discovered.is_empty() {
        return Ok(engine);
    }

    // Single `cargo build` invocation listing every plugin's
    // `--bin` + `--package` pair. Cheaper than spawning cargo once
    // per plugin and shares the build graph / unit cache.
    build_discovered_plugins(workspace_root, &discovered)?;

    let target_dir = workspace_root.join("target/debug");
    for plugin in discovered {
        let binary_path = target_dir.join(&plugin.bin_target_name);
        if !binary_path.exists() {
            return Err(anyhow!(
                "discovered plugin `{}` (from crate `{}`) declared bin = `{}` \
                 but `cargo build` did not produce `{}`. Check that the bin \
                 target is declared correctly in `{}/Cargo.toml`.",
                plugin.name,
                plugin.source_crate,
                plugin.bin_target_name,
                binary_path.display(),
                plugin.source_manifest_dir.display(),
            ));
        }
        engine.register_subprocess(
            SubprocessPlugin::new(plugin.name.clone(), binary_path)
                .after(plugin.after.clone())
                .before(plugin.before.clone()),
        );
    }
    Ok(engine)
}

/// Run a single `cargo build` that builds every discovered
/// plugin's `[[bin]]` target. We use the workspace's existing
/// `target/debug` so subsequent runs are no-op when the plugin
/// crates haven't changed (cargo's own incremental cache).
///
/// Output streams through the curated `Step::pipe` machinery so the
/// cargo progress (`    Compiling …` / `    Finished …` lines) folds
/// into a single spinner row instead of leaking unfiltered ahead of
/// the dev loop's `── whisker run ──` section header — and, with
/// the TUI on, ahead of the inline status bar where stray
/// `eprintln!`s race the viewport redraw.
fn build_discovered_plugins(workspace_root: &Path, discovered: &[DiscoveredPlugin]) -> Result<()> {
    let bins: Vec<&str> = discovered
        .iter()
        .map(|p| p.bin_target_name.as_str())
        .collect();
    let step = whisker_build::ui::step("compile", format!("plugins ({})", bins.join(", ")));
    let mut cmd = Command::new("cargo");
    cmd.arg("build").current_dir(workspace_root);
    for plugin in discovered {
        cmd.arg("--bin")
            .arg(&plugin.bin_target_name)
            .arg("--package")
            .arg(&plugin.source_crate);
    }
    let status = step
        .pipe(&mut cmd)
        .with_context(|| "spawn `cargo build` for discovered Whisker CNG plugin binaries")?;
    if !status.success() {
        step.fail(format!("{status}"));
        return Err(anyhow!(
            "`cargo build` for discovered Whisker CNG plugin binaries exited with {status}. \
             Re-run with `RUST_BACKTRACE=1 cargo build --bin <bin> --package <crate>` to see \
             the underlying compile error."
        ));
    }
    step.done("");
    Ok(())
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
