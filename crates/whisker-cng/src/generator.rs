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
    selection: &crate::CargoSelection,
) -> Result<GenerationReport> {
    anyhow::ensure!(
        selection.target.is_none() || targets.len() == 1,
        "--cargo-target requires exactly one generation platform"
    );
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
    let mut projects = std::collections::BTreeMap::new();
    for &target in targets {
        if projects.contains_key(&target) {
            continue;
        }
        let selection = selection.for_platform(target);
        selection.validate_platform(target)?;
        let graph =
            ProjectDependencyGraph::resolve_with_selection(&manifest, &package.name, &selection)?;
        projects.insert(
            target,
            sync_with_graph(
                target,
                &config,
                crate_dir,
                workspace_root,
                &package.name,
                &graph,
                &selection,
            )?,
        );
    }
    Ok(GenerationReport {
        schema_version: 1,
        selection: selection.clone(),
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
    let selection = crate::CargoSelection::load(workspace_root, package, target)?;
    let graph = ProjectDependencyGraph::resolve_with_selection(
        &workspace_root.join("Cargo.toml"),
        package,
        &selection,
    )
    .with_context(|| format!("resolve Whisker dependencies for `{package}`"))?;
    sync_with_graph(
        target,
        app_config,
        crate_dir,
        workspace_root,
        package,
        &graph,
        &selection,
    )
}

fn sync_with_graph(
    target: Target,
    app_config: &Config,
    crate_dir: &Path,
    workspace_root: &Path,
    package: &str,
    graph: &ProjectDependencyGraph,
    selection: &crate::CargoSelection,
) -> Result<PlatformSync> {
    selection.validate_platform(target)?;
    if matches!(
        target,
        Target::Web | Target::Macos | Target::Windows | Target::Linux
    ) {
        anyhow::ensure!(
            selection
                .features
                .iter()
                .all(|feature| !feature.contains('/')),
            "Cargo Host generation requires application feature names; declare a Cargo feature in the application to forward dependency features"
        );
    }
    let result = match target {
        Target::Android => sync_android(
            app_config,
            crate_dir,
            workspace_root,
            package,
            graph,
            selection,
        ),
        Target::Ios => sync_ios(
            app_config,
            crate_dir,
            workspace_root,
            package,
            graph,
            selection,
        ),
        Target::Macos => sync_macos(
            app_config,
            crate_dir,
            workspace_root,
            package,
            graph,
            selection,
        ),
        Target::Windows | Target::Linux => sync_desktop(
            target,
            app_config,
            crate_dir,
            workspace_root,
            package,
            graph,
            selection,
        ),
        Target::Web => sync_web(
            app_config,
            crate_dir,
            workspace_root,
            package,
            graph,
            selection,
        ),
    }?;
    selection.save(&result.gen_dir)?;
    graph.save(&result.gen_dir)?;
    Ok(result)
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
    selection: &crate::CargoSelection,
) -> Result<PlatformSync> {
    // The Settings plugin reads `workspace` as a `file(...)`, which
    // Gradle resolves relative to `gen/android/`. Pass an absolute
    // path — the template embeds it verbatim, and absolute keeps the
    // generated tree independent of where `gen/android` sits on disk.
    let workspace_path = workspace_root.to_path_buf();
    let discovered = &graph.cng_plugins;
    let builtins = Engine::with_builtins();
    anyhow::ensure!(
        discovered.iter().all(|p| !builtins.contains_plugin(&p.name)
            && p.name != crate::android::application::ApplicationPlugin::NAME),
        "discovered Android plugins must not reuse a built-in plugin name"
    );

    let binaries = build_discovered_plugins(workspace_root, discovered)?;
    let has_legacy = discovered
        .iter()
        .any(|p| p.protocol == crate::discovery::PluginProtocol::Legacy);
    let mut legacy = if has_legacy {
        Engine::with_builtins()
    } else {
        Engine::new()
    }
    .with_app_crate_dir(crate_dir);
    let mut project_binaries = Vec::new();
    for (plugin, binary) in discovered.iter().zip(binaries) {
        match plugin.protocol {
            crate::discovery::PluginProtocol::Legacy => {
                legacy.register_subprocess(
                    SubprocessPlugin::new(&plugin.name, binary)
                        .after(plugin.after.clone())
                        .before(plugin.before.clone()),
                );
            }
            crate::discovery::PluginProtocol::Project => {
                project_binaries.push((&plugin.name, binary));
            }
        }
    }
    // A legacy binary sees only the legacy context, before application declarations.
    // Never send the declarative IR through a context-replacement mobile binary.
    let project_names: std::collections::BTreeSet<_> = discovered
        .iter()
        .filter(|p| p.protocol == crate::discovery::PluginProtocol::Project)
        .map(|p| p.name.as_str())
        .chain([crate::android::application::ApplicationPlugin::NAME])
        .collect();
    let mut legacy_config: Config = serde_json::from_value(serde_json::to_value(app_config)?)?;
    legacy_config
        .plugins
        .retain(|name, _| has_legacy && !project_names.contains(name.as_str()));
    let mut project_config: Config = serde_json::from_value(serde_json::to_value(app_config)?)?;
    project_config
        .plugins
        .retain(|name, _| !has_legacy || project_names.contains(name.as_str()));
    let inputs = crate::android::inputs_from_with_engine(
        &legacy,
        &legacy_config,
        package.replace('-', "_"),
        workspace_path,
        package.to_string(),
        WHISKER_SDK_VERSION.to_string(),
        WHISKER_GRADLE_PLUGIN_VERSION.to_string(),
        WHISKER_MAVEN_URL.to_string(),
    )?;
    let template_version = inputs.template_version;
    let mut projects = if has_legacy {
        // Built-in and legacy changes are already in inputs. The application
        // plugin imports them once, before the new project contributions.
        crate::ProjectEngine::with_initializer(crate::android::application::ApplicationPlugin::new(
            inputs,
        ))
    } else {
        crate::ProjectEngine::with_android_application(inputs)
    }
    .with_app_crate_dir(crate_dir);
    for (name, binary) in project_binaries {
        projects.register_subprocess(name, binary);
    }
    let result = projects
        .compose(
            &project_config,
            &whisker_plugin::project::ProjectIr::Android(Box::default()),
        )
        .context("compose Android project plugins")?;
    let whisker_plugin::project::ProjectIr::Android(project) = result.project else {
        unreachable!("project engine preserves platform")
    };
    anyhow::ensure!(
        project.application == ":app" && project.modules[":app"].directory.as_str() == "app",
        "Whisker Android build driver requires primary module :app at app; additional modules may use other paths"
    );
    let inputs = crate::android::AndroidProjectInputs {
        project: *project,
        app_crate_dir: Some(crate_dir.to_path_buf()),
        cargo_selection: selection.clone(),
        template_version,
    };
    let gen_dir = crate_dir.join("gen/android");
    let template_version = inputs.template_version;
    let regenerated =
        crate::android::sync_project(&gen_dir, &inputs).context("render gen/android")?;
    // Gradle is the build driver after CNG finishes. Seed the module report
    // that the Settings/Project plugins share so a fresh generated project
    // can immediately run `./gradlew assembleDebug` without a preceding
    // `whisker run` or `whisker build` invocation.
    let mut report = crate::modules::build_modules_report_from_resolved(
        workspace_root,
        package,
        &graph.modules,
    )?;
    report.selection = selection.clone();
    crate::modules::write_gradle_report(workspace_root, &report)
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
    selection: &crate::CargoSelection,
) -> Result<PlatformSync> {
    let gen_dir = crate_dir.join("gen/ios");
    let discovered = &graph.cng_plugins;
    let builtin = Engine::with_builtins();
    anyhow::ensure!(
        discovered.iter().all(|p| !builtin.contains_plugin(&p.name)
            && p.name != crate::ios::application::ApplicationPlugin::NAME),
        "discovered iOS plugins must not reuse a built-in plugin name"
    );
    let has_legacy = discovered
        .iter()
        .any(|p| p.protocol == crate::discovery::PluginProtocol::Legacy);
    let mut legacy = if has_legacy {
        Engine::with_builtins()
    } else {
        Engine::new()
    }
    .with_app_crate_dir(crate_dir);
    let mut project_binaries = Vec::new();
    for (plugin, binary) in discovered
        .iter()
        .zip(build_discovered_plugins(workspace_root, discovered)?)
    {
        match plugin.protocol {
            crate::discovery::PluginProtocol::Legacy => {
                legacy.register_subprocess(
                    SubprocessPlugin::new(&plugin.name, binary)
                        .after(plugin.after.clone())
                        .before(plugin.before.clone()),
                );
            }
            crate::discovery::PluginProtocol::Project => {
                project_binaries.push((&plugin.name, binary));
            }
        }
    }
    let project_names: std::collections::BTreeSet<_> = discovered
        .iter()
        .filter(|p| p.protocol == crate::discovery::PluginProtocol::Project)
        .map(|p| p.name.as_str())
        .chain([crate::ios::application::ApplicationPlugin::NAME])
        .collect();
    let mut legacy_config: Config = serde_json::from_value(serde_json::to_value(app_config)?)?;
    legacy_config
        .plugins
        .retain(|name, _| has_legacy && !project_names.contains(name.as_str()));
    let mut project_config: Config = serde_json::from_value(serde_json::to_value(app_config)?)?;
    project_config
        .plugins
        .retain(|name, _| !has_legacy || project_names.contains(name.as_str()));
    let mut inputs = crate::ios::inputs_from_with_engine(
        &legacy,
        &legacy_config,
        gen_dir.join("whisker_modules"),
        workspace_root.to_path_buf(),
        package.to_string(),
    )?;
    inputs.cargo_selection = selection.clone();
    inputs.modules = graph.modules.clone();
    let project_name = inputs.scheme.clone();
    let template_version = inputs.template_version;
    let mut projects = if has_legacy {
        crate::ProjectEngine::with_initializer(crate::ios::application::ApplicationPlugin::new(
            inputs,
        ))
    } else {
        crate::ProjectEngine::with_ios_application(inputs)
    }
    .with_app_crate_dir(crate_dir);
    for (name, binary) in project_binaries {
        projects.register_subprocess(name, binary);
    }
    let result = projects
        .compose(
            &project_config,
            &whisker_plugin::project::ProjectIr::Ios(Default::default()),
        )
        .context("compose iOS project plugins")?;
    let whisker_plugin::project::ProjectIr::Ios(project) = result.project else {
        unreachable!()
    };
    anyhow::ensure!(
        project
            .apple
            .schemes
            .get(&project_name)
            .is_some_and(|s| s.run_target.as_ref() == Some(&project.apple.application))
            && project.apple.targets[&project.apple.application].product_name == project_name,
        "Whisker iOS build driver requires the configured main scheme and application product name"
    );
    let regenerated = crate::ios::sync_project(
        &gen_dir,
        &crate::ios::IosProjectInputs {
            project,
            project_name,
            app_crate_dir: Some(crate_dir.to_path_buf()),
            cargo_selection: selection.clone(),
            template_version,
        },
    )
    .context("render gen/ios")?;
    Ok(PlatformSync {
        gen_dir,
        regenerated,
        template_version: Some(template_version),
    })
}

fn sync_macos(
    app_config: &Config,
    crate_dir: &Path,
    workspace_root: &Path,
    package: &str,
    graph: &ProjectDependencyGraph,
    selection: &crate::CargoSelection,
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
    inputs.cargo_selection = selection.clone();
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
    let app_name = inputs.app_name.clone();
    let generated_package = inputs.generated_package.clone();
    let discovered: Vec<_> = graph
        .cng_plugins
        .iter()
        .filter(|p| p.protocol == crate::discovery::PluginProtocol::Project)
        .cloned()
        .collect();
    let builtin = Engine::with_builtins();
    anyhow::ensure!(
        discovered.iter().all(|p| !builtin.contains_plugin(&p.name)
            && p.name != crate::macos::application::ApplicationPlugin::NAME),
        "discovered macOS plugins must not reuse a built-in plugin name"
    );
    let mut project_config: Config = serde_json::from_value(serde_json::to_value(app_config)?)?;
    for plugin in graph
        .cng_plugins
        .iter()
        .filter(|p| p.protocol == crate::discovery::PluginProtocol::Legacy)
    {
        project_config.plugins.remove(&plugin.name);
    }
    let mut engine =
        crate::ProjectEngine::with_macos_application(inputs).with_app_crate_dir(crate_dir);
    for (plugin, binary) in discovered
        .iter()
        .zip(build_discovered_plugins(workspace_root, &discovered)?)
    {
        engine.register_subprocess(&plugin.name, binary);
    }
    let result = engine
        .compose(
            &project_config,
            &whisker_plugin::project::ProjectIr::Macos(Default::default()),
        )
        .context("compose macOS project plugins")?;
    let whisker_plugin::project::ProjectIr::Macos(project) = result.project else {
        unreachable!()
    };
    let target = &project.apple.targets[&project.apple.application];
    anyhow::ensure!(
        target.product_name == app_name
            && target
                .rust
                .as_ref()
                .is_some_and(|r| r.package == generated_package && r.target == generated_package),
        "macOS build/run driver requires the configured app and generated binary names"
    );
    let regenerated = crate::macos::sync_project(
        &gen_dir,
        &crate::macos::MacosProjectInputs {
            project,
            app_crate_dir: Some(crate_dir.to_path_buf()),
            cargo_selection: selection.clone(),
            template_version,
        },
    )?;
    Ok(PlatformSync {
        gen_dir,
        regenerated,
        template_version: Some(template_version),
    })
}

fn sync_desktop(
    target: Target,
    app_config: &Config,
    crate_dir: &Path,
    workspace_root: &Path,
    package: &str,
    graph: &ProjectDependencyGraph,
    selection: &crate::CargoSelection,
) -> Result<PlatformSync> {
    let gen_dir = crate_dir.join("gen").join(target.as_str());
    // Inside the Whisker monorepo, point at the in-tree Host so examples
    // exercise the current checkout. Installed projects use the published
    // crate at the generator's matching version.
    let in_tree_host = workspace_root.join("platforms").join(target.as_str());
    let dependency = if in_tree_host.join("Cargo.toml").is_file() {
        format!("{{ path = {:?} }}", in_tree_host.display().to_string())
    } else {
        format!("{:?}", env!("CARGO_PKG_VERSION"))
    };
    let mut inputs = crate::desktop::inputs_from(
        app_config,
        target,
        package.to_string(),
        crate_dir.to_path_buf(),
        dependency,
    )?;
    let in_tree_desktop = workspace_root.join("platforms/desktop");
    if in_tree_desktop.join("Cargo.toml").is_file() {
        inputs.desktop_dependency =
            format!("{{ path = {:?} }}", in_tree_desktop.display().to_string());
    }
    inputs.cargo_selection = selection.clone();
    let module_platform = if target == Target::Windows {
        crate::modules::ModulePlatform::Windows
    } else {
        crate::modules::ModulePlatform::Linux
    };
    inputs.element_modules = graph
        .modules
        .iter()
        .cloned()
        .filter_map(|module| {
            let contribution = module.rust_host(module_platform)?.clone();
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
    let application_name = if target == Target::Windows {
        crate::desktop::application::WindowsApplicationPlugin::NAME
    } else {
        crate::desktop::application::LinuxApplicationPlugin::NAME
    };
    let empty = inputs.empty_project()?;
    let discovered: Vec<_> = graph
        .cng_plugins
        .iter()
        .filter(|p| p.protocol == crate::discovery::PluginProtocol::Project)
        .cloned()
        .collect();
    let builtin = Engine::with_builtins();
    anyhow::ensure!(
        discovered
            .iter()
            .all(|p| !builtin.contains_plugin(&p.name) && p.name != application_name),
        "discovered desktop plugins must not reuse a built-in plugin name"
    );
    let mut project_config: Config = serde_json::from_value(serde_json::to_value(app_config)?)?;
    for plugin in graph
        .cng_plugins
        .iter()
        .filter(|p| p.protocol == crate::discovery::PluginProtocol::Legacy)
    {
        project_config.plugins.remove(&plugin.name);
    }
    let mut engine = if target == Target::Windows {
        crate::ProjectEngine::with_windows_application(inputs)
    } else {
        crate::ProjectEngine::with_linux_application(inputs)
    }
    .with_app_crate_dir(crate_dir);
    for (plugin, binary) in discovered
        .iter()
        .zip(build_discovered_plugins(workspace_root, &discovered)?)
    {
        engine.register_subprocess(&plugin.name, binary);
    }
    let result = engine
        .compose(&project_config, &empty)
        .context("compose desktop project plugins")?;
    let regenerated = crate::desktop::sync_project(
        &gen_dir,
        &crate::desktop::DesktopProjectInputs {
            project: result.project,
            app_crate_dir: Some(crate_dir.to_path_buf()),
            cargo_selection: selection.clone(),
        },
    )?;
    Ok(PlatformSync {
        gen_dir,
        regenerated,
        template_version: Some(1),
    })
}

fn sync_web(
    app_config: &Config,
    crate_dir: &Path,
    workspace_root: &Path,
    package: &str,
    graph: &ProjectDependencyGraph,
    selection: &crate::CargoSelection,
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
    inputs.cargo_selection = selection.clone();
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
    let base_path = inputs.base_path.clone();
    let generated_package = inputs.generated_package.clone();
    // Legacy mobile plugins have no Web context. Preserve their previous no-op
    // behavior without starting their executables; new project plugins run here.
    let discovered: Vec<_> = graph
        .cng_plugins
        .iter()
        .filter(|p| p.protocol == crate::discovery::PluginProtocol::Project)
        .cloned()
        .collect();
    let builtin = Engine::with_builtins();
    anyhow::ensure!(
        discovered.iter().all(|p| !builtin.contains_plugin(&p.name)
            && p.name != crate::web::application::ApplicationPlugin::NAME),
        "discovered Web plugins must not reuse a built-in plugin name"
    );
    let mut project_config: Config = serde_json::from_value(serde_json::to_value(app_config)?)?;
    for plugin in graph
        .cng_plugins
        .iter()
        .filter(|p| p.protocol == crate::discovery::PluginProtocol::Legacy)
    {
        project_config.plugins.remove(&plugin.name);
    }
    let mut engine =
        crate::ProjectEngine::with_web_application(inputs).with_app_crate_dir(crate_dir);
    for (plugin, binary) in discovered
        .iter()
        .zip(build_discovered_plugins(workspace_root, &discovered)?)
    {
        engine.register_subprocess(&plugin.name, binary);
    }
    let result = engine
        .compose(
            &project_config,
            &whisker_plugin::project::ProjectIr::Web(Box::default()),
        )
        .context("compose Web project plugins")?;
    let whisker_plugin::project::ProjectIr::Web(project) = result.project else {
        unreachable!()
    };
    anyhow::ensure!(
        project.base_path == base_path,
        "Web build/run driver requires the configured base_path; set it in Config.web"
    );
    anyhow::ensure!(
        project
            .wasm
            .as_ref()
            .is_some_and(|w| w.package == generated_package),
        "Web build/run driver requires the configured generated package name"
    );
    let regenerated = crate::web::sync_project(
        &gen_dir,
        &crate::web::WebProjectInputs {
            project: *project,
            app_crate_dir: Some(crate_dir.to_path_buf()),
            cargo_selection: selection.clone(),
            template_version,
        },
    )
    .context("render gen/web")?;
    Ok(PlatformSync {
        gen_dir,
        regenerated,
        template_version: Some(template_version),
    })
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
