//! Platform generation, plugin preparation, and Host version selection.

use crate::runner::{GenerationReport, GenerationTarget as Target, PlatformSync};
use crate::{DiscoveredPlugin, Engine, ProjectDependencyGraph, SubprocessPlugin};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use whisker_config::Config;

pub(crate) fn generate_config(
    manifest: &Path,
    config: Config,
    targets: &[Target],
) -> Result<GenerationReport> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(manifest)
        .no_deps()
        .exec()
        .context("resolve application workspace")?;
    let manifest = manifest.canonicalize()?;
    let package = metadata
        .packages
        .iter()
        .find(|package| package.manifest_path.as_std_path() == manifest)
        .context("--manifest-path must identify an application package, not a virtual workspace")?;
    let crate_dir = manifest.parent().context("manifest has no parent")?;
    let workspace_root = metadata.workspace_root.as_std_path();
    let targets = if targets.is_empty() {
        &Target::ALL[..]
    } else {
        targets
    };
    let graph = ProjectDependencyGraph::resolve(manifest.as_path(), &package.name)?;
    let mut projects = std::collections::BTreeMap::new();
    for &target in targets {
        if projects.contains_key(&target) {
            continue;
        }
        projects.insert(
            target,
            sync_with_graph(
                target,
                &config,
                crate_dir,
                workspace_root,
                &package.name,
                &graph,
            )?,
        );
    }
    Ok(GenerationReport {
        schema_version: 1,
        crate_dir: crate_dir.to_path_buf(),
        workspace_root: workspace_root.to_path_buf(),
        package: package.name.clone(),
        config,
        projects,
    })
}

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
    let graph = ProjectDependencyGraph::resolve(&workspace_root.join("Cargo.toml"), package)
        .with_context(|| format!("resolve Whisker dependencies for `{package}`"))?;
    sync_with_graph(
        target,
        app_config,
        crate_dir,
        workspace_root,
        package,
        &graph,
    )
}

fn sync_with_graph(
    target: Target,
    app_config: &Config,
    crate_dir: &Path,
    workspace_root: &Path,
    package: &str,
    graph: &ProjectDependencyGraph,
) -> Result<PlatformSync> {
    match target {
        Target::Android => sync_android(app_config, crate_dir, workspace_root, package, graph),
        Target::Ios => sync_ios(app_config, crate_dir, workspace_root, package, graph),
        Target::Macos => sync_macos(app_config, crate_dir, workspace_root, package, graph),
        Target::Web => sync_web(app_config, crate_dir, workspace_root, package, graph),
    }
}

/// SDK version pinned into the cng-generated
/// `app/build.gradle.kts` (`rs.whisker:whisker-runtime-android:<this>`).
/// Bumped alongside the `sdk-v*` release tag.
///
/// Not every `sdk-v*` tag needs a bump here — read the SDK diff and
/// move this only when apps must pick the release up, such as a Host
/// runtime ABI change or a Kotlin API consumed by applications/modules.
// 0.1.21 is the first published Android SDK release that ships WhiskerView in
// the standalone whisker-runtime-android AAR. 0.1.20 failed before publishing.
const WHISKER_SDK_VERSION: &str = "0.1.25";
/// Gradle plugin version pinned into the generated
/// `settings.gradle.kts` `pluginManagement.plugins` + `plugins`
/// blocks. Bumped independently from the SDK via the
/// `gradle-plugin-v*` release tag. The Settings plugin and the
/// Project plugin ship as separate Maven artifacts but share this
/// version.
// 0.5.0 ships the package-scoped module-report contract and the build-process
// fixes required by the standalone new-architecture Android Host.
const WHISKER_GRADLE_PLUGIN_VERSION: &str = "0.5.0";
const WHISKER_MAVEN_URL: &str = "https://whiskerrs.github.io/whisker/maven";

fn sync_android(
    app_config: &Config,
    crate_dir: &Path,
    workspace_root: &Path,
    package: &str,
    graph: &ProjectDependencyGraph,
) -> Result<PlatformSync> {
    // The Settings plugin reads `workspace` as a `file(...)`, which
    // Gradle resolves relative to `gen/android/`. Pass an absolute
    // path — the template embeds it verbatim, and absolute keeps the
    // generated tree independent of where `gen/android` sits on disk.
    let workspace_path = workspace_root.to_path_buf();
    let engine =
        build_engine_with_discovered_plugins(crate_dir, workspace_root, &graph.cng_plugins)?;
    let inputs = crate::android::inputs_from_with_engine(
        &engine,
        app_config,
        package.replace('-', "_"),
        workspace_path,
        package.to_string(),
        WHISKER_SDK_VERSION.to_string(),
        WHISKER_GRADLE_PLUGIN_VERSION.to_string(),
        WHISKER_MAVEN_URL.to_string(),
    )?;
    let gen_dir = crate_dir.join("gen/android");
    let template_version = inputs.template_version;
    let regenerated = crate::sync_android(&gen_dir, &inputs).context("render gen/android")?;
    // Gradle is the build driver after CNG finishes. Seed the module report
    // that the Settings/Project plugins share so a fresh generated project
    // can immediately run `./gradlew assembleDebug` without a preceding
    // `whisker run` or `whisker build` invocation.
    crate::modules::write_gradle_module_cache(workspace_root, package, &graph.modules)
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
    graph: &ProjectDependencyGraph,
) -> Result<PlatformSync> {
    let gen_dir = crate_dir.join("gen/ios");
    // CNG fills `gen/ios/whisker_modules/` in the same transaction as
    // the Xcode project. The pbxproj references that local package.
    let whisker_modules = gen_dir.join("whisker_modules");
    let engine =
        build_engine_with_discovered_plugins(crate_dir, workspace_root, &graph.cng_plugins)?;
    let mut inputs = crate::ios::inputs_from_with_engine(
        &engine,
        app_config,
        whisker_modules,
        workspace_root.to_path_buf(),
        package.to_string(),
    )?;
    inputs.modules = graph.modules.clone();
    // whisker-cng renders the full Xcode project directly (pbxproj +
    // xcworkspacedata + sources). No xcodegen subprocess needed —
    // see crates/whisker-cng/src/ios.rs for the rationale.
    let regenerated = crate::sync_ios(&gen_dir, &inputs).context("render gen/ios")?;
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
    graph: &ProjectDependencyGraph,
) -> Result<PlatformSync> {
    let gen_dir = crate_dir.join("gen/macos");
    // Inside the Whisker monorepo, point at the in-tree Host so examples
    // exercise the current checkout. Installed projects use the published
    // crate at the generator's matching version.
    let in_tree_host = workspace_root.join("platforms/macos");
    let dependency = if in_tree_host.join("Cargo.toml").is_file() {
        format!("{{ path = {:?} }}", in_tree_host.display().to_string())
    } else {
        format!("{:?}", env!("CARGO_PKG_VERSION"))
    };
    let mut inputs = crate::macos::inputs_from(
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
    inputs.element_modules = graph
        .modules
        .iter()
        .cloned()
        .filter_map(|module| {
            let contribution = module
                .rust_host(crate::modules::ModulePlatform::Macos)?
                .clone();
            let host_dependency = match contribution.source {
                crate::modules::ResolvedRustHostSource::Path(path) => {
                    crate::RustHostDependency::Path(path)
                }
                crate::modules::ResolvedRustHostSource::Registry { version } => {
                    crate::RustHostDependency::Registry { version }
                }
            };
            Some(crate::RustElementModuleInput {
                package: module.package,
                crate_path: module.manifest_dir,
                host_package: contribution.package,
                host_dependency,
            })
        })
        .collect();
    let template_version = inputs.template_version;
    let regenerated = crate::sync_macos(&gen_dir, &inputs).context("render gen/macos")?;
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
    graph: &ProjectDependencyGraph,
) -> Result<PlatformSync> {
    let gen_dir = crate_dir.join("gen/web");
    let in_tree_host = workspace_root.join("platforms/web");
    let dependency = if in_tree_host.join("Cargo.toml").is_file() {
        format!("{{ path = {:?} }}", in_tree_host.display().to_string())
    } else {
        format!("{:?}", env!("CARGO_PKG_VERSION"))
    };
    let mut inputs = crate::web::inputs_from(
        app_config,
        package.to_string(),
        crate_dir.to_path_buf(),
        dependency,
    )?;
    inputs.element_modules = graph
        .modules
        .iter()
        .cloned()
        .filter_map(|module| {
            let contribution = module
                .rust_host(crate::modules::ModulePlatform::Web)?
                .clone();
            let host_dependency = match contribution.source {
                crate::modules::ResolvedRustHostSource::Path(path) => {
                    crate::RustHostDependency::Path(path)
                }
                crate::modules::ResolvedRustHostSource::Registry { version } => {
                    crate::RustHostDependency::Registry { version }
                }
            };
            Some(crate::RustElementModuleInput {
                package: module.package,
                crate_path: module.manifest_dir,
                host_package: contribution.package,
                host_dependency,
            })
        })
        .collect();
    let template_version = inputs.template_version;
    let regenerated = crate::sync_web(&gen_dir, &inputs).context("render gen/web")?;
    Ok(PlatformSync {
        gen_dir,
        regenerated,
        template_version: Some(template_version),
    })
}

/// Build a [`crate::Engine`] populated with built-ins plus
/// every 3rd-party plugin discovered via `[package.metadata.whisker.plugins]`
/// in the user app's dep graph. Each discovered plugin's `[[bin]]`
/// target gets `cargo build`d (debug profile, workspace target dir)
/// and registered as a [`SubprocessPlugin`] pointing at the
/// resulting binary.
fn build_engine_with_discovered_plugins(
    crate_dir: &Path,
    workspace_root: &Path,
    discovered: &[DiscoveredPlugin],
) -> Result<Engine> {
    // Stamp the app crate dir onto the engine so subprocess plugins
    // (e.g. `whisker-asset`) can resolve paths the user spelled
    // relative to their crate — they don't inherit a reliable cwd.
    let mut engine = Engine::with_builtins().with_app_crate_dir(crate_dir);
    if discovered.is_empty() {
        return Ok(engine);
    }

    let binaries = build_discovered_plugins(workspace_root, discovered)?;
    for (plugin, binary_path) in discovered.iter().cloned().zip(binaries) {
        engine.register_subprocess(
            SubprocessPlugin::new(plugin.name.clone(), binary_path)
                .after(plugin.after.clone())
                .before(plugin.before.clone()),
        );
    }
    Ok(engine)
}

/// Compile plugin executables into the workspace target directory.
fn build_discovered_plugins(
    workspace_root: &Path,
    discovered: &[DiscoveredPlugin],
) -> Result<Vec<PathBuf>> {
    let host = host_target()?;
    let mut binaries = Vec::new();
    for plugin in discovered {
        let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .arg("build")
            .arg("--quiet")
            .arg("--manifest-path")
            .arg(workspace_root.join("Cargo.toml"))
            .arg("--package")
            .arg(&plugin.source_crate)
            .arg("--bin")
            .arg(&plugin.bin_target_name)
            .arg("--target")
            .arg(&host)
            .arg("--target-dir")
            .arg(workspace_root.join("target"))
            .arg("--message-format=json-render-diagnostics")
            .current_dir(workspace_root)
            .output()
            .with_context(|| format!("build CNG plugin `{}`", plugin.name))?;
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        anyhow::ensure!(
            output.status.success(),
            "CNG plugin `{}` build failed: {}",
            plugin.name,
            output.status
        );
        let binary = cargo_metadata::Message::parse_stream(std::io::Cursor::new(&output.stdout))
            .filter_map(Result::ok)
            .find_map(|message| match message {
                cargo_metadata::Message::CompilerArtifact(artifact)
                    if artifact.target.name == plugin.bin_target_name =>
                {
                    artifact.executable
                }
                _ => None,
            })
            .with_context(|| format!("no executable produced for plugin `{}`", plugin.name))?;
        binaries.push(binary.into_std_path_buf());
    }
    Ok(binaries)
}

pub(crate) fn host_target() -> Result<String> {
    let output = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .arg("-vV")
        .output()
        .context("read Rust host target")?;
    anyhow::ensure!(output.status.success(), "rustc -vV failed");
    String::from_utf8(output.stdout)?
        .lines()
        .find_map(|line| line.strip_prefix("host: ").map(str::to_owned))
        .context("rustc did not report a host target")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn tempdir() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "whisker-cng-project-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn fixture_modules_are_wired_into_generated_hosts() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .join("tests/cng-module-fixture");
        let crate_dir = tempdir();
        let mut config = Config::default();
        config
            .name("CNG Module Fixture")
            .bundle_id("rs.whisker.cngfixture");

        let macos = sync_for_target(
            Target::Macos,
            &config,
            &crate_dir,
            &workspace,
            "cng-module-fixture",
        )
        .unwrap();
        let macos_source = std::fs::read_to_string(macos.gen_dir.join("src/main.rs")).unwrap();
        assert!(macos_source.contains("cng_test_widget::__whisker_element_module_definition()"));
        assert!(macos_source.contains("cng_test_widget_desktop::__whisker_module_definition()"));
        assert!(!macos_source.contains("cng_test_service"));

        let web = sync_for_target(
            Target::Web,
            &config,
            &crate_dir,
            &workspace,
            "cng-module-fixture",
        )
        .unwrap();
        let web_source = std::fs::read_to_string(web.gen_dir.join("src/lib.rs")).unwrap();
        assert!(web_source.contains("cng_test_widget::__whisker_element_module_definition()"));
        assert!(web_source.contains("cng_test_widget_web::__whisker_module_definition()"));
        assert!(web_source.contains("cng_test_service_web::__whisker_module_definition()"));

        let ios = sync_for_target(
            Target::Ios,
            &config,
            &crate_dir,
            &workspace,
            "cng-module-fixture",
        )
        .unwrap();
        let package =
            std::fs::read_to_string(ios.gen_dir.join("whisker_modules/Package.swift")).unwrap();
        let registrar = std::fs::read_to_string(
            ios.gen_dir
                .join("whisker_modules/Sources/WhiskerModules/RegisterAll.swift"),
        )
        .unwrap();
        assert!(package.contains("CngTestWidget"));
        assert!(registrar.contains("_whiskerRegisterModules_CngTestWidget()"));
        assert!(!registrar.contains("CngTestService"));
        std::fs::remove_dir_all(crate_dir).ok();
    }
}
