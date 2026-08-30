//! Glue between `whisker-cng` and the CLI.
//!
//! Responsibilities split:
//!
//! - `whisker-cng` owns the *pure* renderer: Config + paths → files
//!   on disk. No shelling out, no environment assumptions. Pure logic
//!   so it stays unit-testable against tempdirs.
//! - This module decides *where* the gen dirs live (always
//!   `<crate_dir>/gen/<platform>`), resolves the Whisker native
//!   runtime paths (`<workspace>/platforms/ios`), and handles the
//!   side-effect bits a sync needs — pinning the SDK / Gradle plugin
//!   versions and building the app's discovered CNG plugins.
//!
//! Public entry point: [`sync_for_target`]. The cli's `run` and
//! `build` subcommands call this before kicking off the rest of the
//! build pipeline.

use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::process::Command;
use whisker_cng::{DiscoveredPlugin, Engine, SubprocessPlugin, discover_plugins};
use whisker_config::Config;
use whisker_dev_server::Target;

/// Run the platform-appropriate sync for `target`. Returns the gen
/// directory the caller should hand to gradle / xcodebuild — useful
/// even for the fast-path (`regenerated == false`) case.
pub fn sync_for_target(
    target: Target,
    app_config: &Config,
    crate_dir: &Path,
    workspace_root: &Path,
    package: &str,
) -> Result<PlatformSync> {
    match target {
        Target::Android => sync_android(app_config, crate_dir, workspace_root, package),
        Target::IosSimulator => sync_ios(app_config, crate_dir, workspace_root, package),
        Target::Macos => sync_macos(app_config, crate_dir, workspace_root, package),
        Target::Web => sync_web(app_config, crate_dir, workspace_root, package),
    }
}

/// Outcome of one sync_native pass.
#[derive(Debug, Clone)]
pub struct PlatformSync {
    /// Where the generated project tree lives — `gen/android/` or
    /// `gen/ios/` under `crate_dir`.
    pub gen_dir: PathBuf,
    /// `true` if the renderer rewrote files this pass, `false` if the
    /// fingerprint matched and the existing tree was reused.
    pub regenerated: bool,
    /// cng template version that drives `gen/` regeneration (Android only;
    /// `None` for iOS). Surfaced in the run log so a "reused cached gen/"
    /// line tells the user which template generation is on disk.
    pub template_version: Option<u32>,
}

/// SDK version pinned into the cng-generated
/// `app/build.gradle.kts` (`rs.whisker:whisker-runtime-android:<this>`).
/// Bumped alongside the `sdk-v*` release tag.
///
/// Not every `sdk-v*` tag needs a bump here — read the SDK diff and
/// move this only when apps must pick the release up, such as a Host
/// runtime ABI change or a Kotlin API consumed by applications/modules.
// 0.1.20 is the first Android SDK release that ships WhiskerView in the
// standalone whisker-runtime-android AAR.
const WHISKER_SDK_VERSION: &str = "0.1.20";
/// Gradle plugin version pinned into the generated
/// `settings.gradle.kts` `pluginManagement.plugins` + `plugins`
/// blocks. Bumped independently from the SDK via the
/// `gradle-plugin-v*` release tag. The Settings plugin and the
/// Project plugin ship as separate Maven artifacts but share this
/// version.
const WHISKER_GRADLE_PLUGIN_VERSION: &str = "0.4.1";
const WHISKER_MAVEN_URL: &str = "https://whiskerrs.github.io/whisker/maven";
const LYNX_MAVEN_URL: &str = "https://whiskerrs.github.io/lynx/maven";

fn sync_android(
    app_config: &Config,
    crate_dir: &Path,
    workspace_root: &Path,
    package: &str,
) -> Result<PlatformSync> {
    // The Settings plugin reads `workspace` as a `file(...)`, which
    // Gradle resolves relative to `gen/android/`. Pass an absolute
    // path — the template embeds it verbatim, and absolute keeps the
    // generated tree independent of where `gen/android` sits on disk.
    let workspace_path = workspace_root.to_path_buf();
    let engine = build_engine_with_discovered_plugins(crate_dir, workspace_root, package)?;
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
    let template_version = inputs.template_version;
    let regenerated = whisker_cng::sync_android(&gen_dir, &inputs).context("render gen/android")?;
    // Gradle is the build driver after CNG finishes. Seed the module report
    // that the Settings/Project plugins share so a fresh generated project
    // can immediately run `./gradlew assembleDebug` without a preceding
    // `whisker run` or `whisker build` invocation.
    whisker_build::modules::refresh_gradle_module_cache(workspace_root, package)
        .context("stage Android module dependency report")?;
    Ok(PlatformSync {
        gen_dir,
        regenerated,
        template_version: Some(template_version),
    })
}

fn sync_ios(
    app_config: &Config,
    crate_dir: &Path,
    workspace_root: &Path,
    package: &str,
) -> Result<PlatformSync> {
    let gen_dir = crate_dir.join("gen/ios");
    // `whisker-build::ios::stage_module_swift_sources` fills
    // `gen/ios/whisker_modules/` later (between cargo build and
    // xcodebuild), but the pbxproj template's
    // `XCLocalSwiftPackageReference` needs its absolute path now.
    let whisker_modules = gen_dir.join("whisker_modules");
    let engine = build_engine_with_discovered_plugins(crate_dir, workspace_root, package)?;
    let inputs = whisker_cng::ios::inputs_from_with_engine(
        &engine,
        app_config,
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
        template_version: None,
    })
}

fn sync_macos(
    app_config: &Config,
    crate_dir: &Path,
    workspace_root: &Path,
    package: &str,
) -> Result<PlatformSync> {
    let gen_dir = crate_dir.join("gen/macos");
    // Inside the Whisker monorepo, point at the in-tree Host so examples
    // exercise the current checkout. Installed projects use the published
    // crate at the CLI's matching version.
    let in_tree_host = workspace_root.join("platforms/macos");
    let dependency = if in_tree_host.join("Cargo.toml").is_file() {
        format!("{{ path = {:?} }}", in_tree_host.display().to_string())
    } else {
        format!("{:?}", env!("CARGO_PKG_VERSION"))
    };
    let mut inputs = whisker_cng::macos::inputs_from(
        app_config,
        package.to_string(),
        crate_dir.to_path_buf(),
        dependency,
    )?;
    let in_tree_desktop = workspace_root.join("platforms/desktop");
    if in_tree_desktop.join("Cargo.toml").is_file() {
        inputs.whisker_desktop_dependency =
            format!("{{ path = {:?} }}", in_tree_desktop.display().to_string());
    }
    inputs.element_modules =
        whisker_build::modules::discover(&workspace_root.join("Cargo.toml"), package)?
            .into_iter()
            .filter_map(|module| {
                let contribution = module.desktop?;
                let host_dependency = match contribution.source {
                    whisker_build::modules::ResolvedRustHostSource::Path(path) => {
                        whisker_cng::RustHostDependency::Path(path)
                    }
                    whisker_build::modules::ResolvedRustHostSource::Registry { version } => {
                        whisker_cng::RustHostDependency::Registry { version }
                    }
                };
                Some(whisker_cng::RustElementModuleInput {
                    package: module.package,
                    crate_path: module.manifest_dir,
                    host_package: contribution.package,
                    host_dependency,
                })
            })
            .collect();
    let template_version = inputs.template_version;
    let regenerated = whisker_cng::sync_macos(&gen_dir, &inputs).context("render gen/macos")?;
    Ok(PlatformSync {
        gen_dir,
        regenerated,
        template_version: Some(template_version),
    })
}

fn sync_web(
    app_config: &Config,
    crate_dir: &Path,
    workspace_root: &Path,
    package: &str,
) -> Result<PlatformSync> {
    let gen_dir = crate_dir.join("gen/web");
    let in_tree_host = workspace_root.join("platforms/web");
    let dependency = if in_tree_host.join("Cargo.toml").is_file() {
        format!("{{ path = {:?} }}", in_tree_host.display().to_string())
    } else {
        format!("{:?}", env!("CARGO_PKG_VERSION"))
    };
    let mut inputs = whisker_cng::web::inputs_from(
        app_config,
        package.to_string(),
        crate_dir.to_path_buf(),
        dependency,
    )?;
    inputs.element_modules =
        whisker_build::modules::discover(&workspace_root.join("Cargo.toml"), package)?
            .into_iter()
            .filter_map(|module| {
                let contribution = module.web?;
                let host_dependency = match contribution.source {
                    whisker_build::modules::ResolvedRustHostSource::Path(path) => {
                        whisker_cng::RustHostDependency::Path(path)
                    }
                    whisker_build::modules::ResolvedRustHostSource::Registry { version } => {
                        whisker_cng::RustHostDependency::Registry { version }
                    }
                };
                Some(whisker_cng::RustElementModuleInput {
                    package: module.package,
                    crate_path: module.manifest_dir,
                    host_package: contribution.package,
                    host_dependency,
                })
            })
            .collect();
    let template_version = inputs.template_version;
    let regenerated = whisker_cng::sync_web(&gen_dir, &inputs).context("render gen/web")?;
    Ok(PlatformSync {
        gen_dir,
        regenerated,
        template_version: Some(template_version),
    })
}

/// Build a [`whisker_cng::Engine`] populated with built-ins plus
/// every 3rd-party plugin discovered via `[package.metadata.whisker.plugins]`
/// in the user app's dep graph. Each discovered plugin's `[[bin]]`
/// target gets `cargo build`d (debug profile, workspace target dir)
/// and registered as a [`SubprocessPlugin`] pointing at the
/// resulting binary.
fn build_engine_with_discovered_plugins(
    crate_dir: &Path,
    workspace_root: &Path,
    user_package: &str,
) -> Result<Engine> {
    let manifest_path = workspace_root.join("Cargo.toml");
    let discovered = discover_plugins(&manifest_path, user_package)
        .with_context(|| format!("discover Whisker CNG plugins for `{user_package}`"))?;

    // Stamp the app crate dir onto the engine so subprocess plugins
    // (e.g. `whisker-asset`) can resolve paths the user spelled
    // relative to their crate — they don't inherit a reliable cwd.
    let mut engine = Engine::with_builtins().with_app_crate_dir(crate_dir);
    if discovered.is_empty() {
        return Ok(engine);
    }

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

/// Build every discovered plugin's `[[bin]]` target, one `cargo
/// build` per plugin. We use the workspace's existing `target/debug`
/// so subsequent runs are no-op when a plugin crate hasn't changed
/// (cargo's own incremental cache).
///
/// One invocation per plugin, not a single batched `-p A -p B --bin
/// X --bin Y` command: a bare-name `-p` spec resolves against the
/// current build's package selection only, so mixing a workspace
/// member with a non-member patched path dependency in one invocation
/// intermittently fails with "package ID specification `<name>` did
/// not match any packages" even where each package resolves on its
/// own. Separate builds also attribute errors per plugin.
///
/// Output streams through `Step::pipe` so cargo progress folds into
/// one spinner row per plugin rather than leaking ahead of the dev
/// loop's section header — and, under the TUI, racing the viewport
/// redraw.
fn build_discovered_plugins(workspace_root: &Path, discovered: &[DiscoveredPlugin]) -> Result<()> {
    for plugin in discovered {
        let step = whisker_build::ui::step("compile", format!("plugin ({})", plugin.name));
        let mut cmd = Command::new("cargo");
        cmd.arg("build")
            .arg("--bin")
            .arg(&plugin.bin_target_name)
            .arg("--package")
            .arg(&plugin.source_crate)
            .current_dir(workspace_root);
        let status = step.pipe(&mut cmd).with_context(|| {
            format!(
                "spawn `cargo build` for discovered Whisker CNG plugin `{}`",
                plugin.name,
            )
        })?;
        if !status.success() {
            step.fail(format!("{status}"));
            return Err(anyhow!(
                "`cargo build` for discovered Whisker CNG plugin `{}` (crate `{}`) exited with \
                 {status}. Re-run with `RUST_BACKTRACE=1 cargo build --bin {} --package {}` to \
                 see the underlying compile error.",
                plugin.name,
                plugin.source_crate,
                plugin.bin_target_name,
                plugin.source_crate,
            ));
        }
        step.done("");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn tempdir() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "whisker-cli-rfc0004-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn host_smoke_dependency_wires_toggle_into_generated_rust_hosts() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap();
        let crate_dir = tempdir();
        let mut config = Config::default();
        config.name("Host Smoke").bundle_id("rs.whisker.hostsmoke");

        let macos =
            sync_for_target(Target::Macos, &config, &crate_dir, workspace, "host-smoke").unwrap();
        let macos_source = std::fs::read_to_string(macos.gen_dir.join("src/main.rs")).unwrap();
        assert!(macos_source.contains("whisker_toggle::__whisker_element_module_definition()"));
        assert!(
            macos_source.contains("whisker_toggle_desktop_host::__whisker_module_definition()")
        );

        let web =
            sync_for_target(Target::Web, &config, &crate_dir, workspace, "host-smoke").unwrap();
        let web_source = std::fs::read_to_string(web.gen_dir.join("src/lib.rs")).unwrap();
        assert!(web_source.contains("whisker_toggle::__whisker_element_module_definition()"));
        assert!(web_source.contains("whisker_toggle_web_host::__whisker_module_definition()"));
        std::fs::remove_dir_all(crate_dir).ok();
    }
}
