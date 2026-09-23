//! Structural Android validation without evaluating Gradle configurations

use super::*;
use anyhow::{Result, ensure};
use std::collections::BTreeSet;

pub(in crate::project) fn validate_android(project: &AndroidProjectIr) -> Result<()> {
    ensure!(
        matches!(
            module(project, &project.application)?.kind,
            AndroidModuleKind::Application(_)
        ),
        "Android application must refer to an application module"
    );
    validate_script(&project.root_build)?;
    validate_plugins(&project.settings.plugins)?;
    validate_repositories(&project.settings.plugin_management.repositories)?;
    validate_repositories(&project.settings.dependency_resolution.repositories)?;
    validate_sdk(&project.settings.android_sdk)?;
    for (id, version) in &project.settings.plugin_management.plugins {
        nonempty(id, "plugin ID")?;
        nonempty(version, "plugin version")?;
    }
    let mut directories = BTreeSet::new();
    for (id, entry) in &project.modules {
        ensure!(
            id.starts_with(':')
                && id[1..].split(':').all(|part| !part.is_empty()
                    && !part
                        .chars()
                        .any(|c| c.is_whitespace() || matches!(c, '/' | '\\'))),
            "invalid Gradle module ID: {id:?}"
        );
        ensure!(
            directories.insert(&entry.directory),
            "duplicate Android module directory: {}",
            entry.directory.as_str()
        );
        validate_script(&entry.build)?;
        for dependency in &entry.dependencies {
            nonempty(&dependency.configuration, "dependency configuration")?;
            if let GradleDependencySource::Project(target) = &dependency.source {
                module(project, target)?;
            }
        }
        match &entry.kind {
            AndroidModuleKind::Application(app) => {
                validate_build(&app.android)?;
                unique(&app.dynamic_features, "dynamic feature")?;
                for target in &app.dynamic_features {
                    let AndroidModuleKind::DynamicFeature(feature) = &module(project, target)?.kind
                    else {
                        anyhow::bail!("module {target} is not a dynamic feature");
                    };
                    ensure!(
                        &feature.base == id,
                        "feature {target} belongs to {}, not {id}",
                        feature.base
                    );
                }
                unique(&app.asset_packs, "asset pack")?;
                let mut names = BTreeSet::new();
                for target in &app.asset_packs {
                    let AndroidModuleKind::AssetPack(pack) = &module(project, target)?.kind else {
                        anyhow::bail!("module {target} is not an asset pack");
                    };
                    ensure!(
                        names.insert(&pack.pack_name),
                        "duplicate asset pack name in {id}: {}",
                        pack.pack_name
                    );
                }
            }
            AndroidModuleKind::Library(build) => validate_build(build)?,
            AndroidModuleKind::DynamicFeature(feature) => {
                validate_build(&feature.android)?;
                let AndroidModuleKind::Application(base) = &module(project, &feature.base)?.kind
                else {
                    anyhow::bail!("feature {id} base must be an application");
                };
                ensure!(
                    base.dynamic_features.contains(id),
                    "base {} does not package feature {id}",
                    feature.base
                );
            }
            AndroidModuleKind::Test(test) => {
                validate_build(&test.android)?;
                ensure!(
                    matches!(
                        module(project, &test.target)?.kind,
                        AndroidModuleKind::Application(_)
                    ),
                    "test module {id} target must be an application"
                );
            }
            AndroidModuleKind::AssetPack(pack) => {
                ensure!(
                    pack.pack_name
                        .starts_with(|c: char| c.is_ascii_alphabetic())
                        && pack
                            .pack_name
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '_'),
                    "invalid asset pack name: {:?}",
                    pack.pack_name
                );
            }
            AndroidModuleKind::External { .. } => {
                ensure!(
                    entry.build == GradleBuildScript::default() && entry.dependencies.is_empty(),
                    "external module {id} owns its build file; generated build declarations are not allowed"
                );
            }
            AndroidModuleKind::Jvm | AndroidModuleKind::Custom => {}
        }
    }
    // A module-level union invents cycles across disjoint variants/classpaths.
    // Gradle alone resolves those configurations and their resulting task graph.
    Ok(())
}

fn module<'a>(project: &'a AndroidProjectIr, id: &str) -> Result<&'a AndroidModule> {
    project
        .modules
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("Android module references unknown ID {id:?}"))
}

fn validate_build(build: &AndroidBuild) -> Result<()> {
    nonempty(&build.namespace, "namespace")?;
    validate_sdk(&build.sdk)?;
    unique(&build.variants.flavor_dimensions, "flavor dimension")?;
    for (name, flavor) in &build.variants.product_flavors {
        nonempty(name, "flavor name")?;
        ensure!(
            build.variants.flavor_dimensions.contains(&flavor.dimension),
            "flavor {name} references undeclared dimension {:?}",
            flavor.dimension
        );
    }
    for name in build.variants.build_types.keys() {
        nonempty(name, "build type")?;
        ensure!(
            !build.variants.product_flavors.contains_key(name),
            "build type and flavor share name {name:?}"
        );
    }
    for (name, source_set) in &build.source_sets {
        nonempty(name, "source set")?;
        if let Some(manifest) = &source_set.manifest {
            ensure!(
                manifest.name == "manifest",
                "source set {name} manifest root must be <manifest>"
            );
        }
    }
    Ok(())
}

fn validate_sdk(sdk: &AndroidSdk) -> Result<()> {
    if let Some(compile) = &sdk.compile {
        match compile {
            AndroidCompileSdk::Release { api, .. } => {
                ensure!(*api > 0, "compile SDK API must be positive")
            }
            AndroidCompileSdk::Preview { codename } => nonempty(codename, "compile SDK codename")?,
            AndroidCompileSdk::AddOn { vendor, name, api } => {
                nonempty(vendor, "SDK vendor")?;
                nonempty(name, "SDK add-on name")?;
                ensure!(*api > 0, "SDK add-on API must be positive");
            }
        }
    }
    for level in [&sdk.min, &sdk.target].into_iter().flatten() {
        match level {
            AndroidApiLevel::Release(api) => ensure!(*api > 0, "runtime SDK API must be positive"),
            AndroidApiLevel::Preview(codename) => nonempty(codename, "runtime SDK codename")?,
        }
    }
    Ok(())
}

fn validate_script(script: &GradleBuildScript) -> Result<()> {
    validate_plugins(&script.plugins)
}

fn validate_plugins(plugins: &[GradlePlugin]) -> Result<()> {
    let mut ids = BTreeSet::new();
    for plugin in plugins {
        nonempty(&plugin.id, "plugin ID")?;
        ensure!(ids.insert(&plugin.id), "duplicate plugin ID: {}", plugin.id);
        ensure!(
            plugin.version.is_none() || plugin.alias.is_none(),
            "plugin {} cannot set both alias and version",
            plugin.id
        );
        if let Some(value) = &plugin.version {
            nonempty(value, "plugin version")?;
        }
        if let Some(value) = &plugin.alias {
            nonempty(value, "plugin alias")?;
        }
    }
    Ok(())
}

fn validate_repositories(repositories: &[GradleRepository]) -> Result<()> {
    let mut ids = BTreeSet::new();
    for repository in repositories {
        nonempty(&repository.id, "repository ID")?;
        nonempty(&repository.expression, "repository expression")?;
        ensure!(
            ids.insert(&repository.id),
            "duplicate repository ID: {}",
            repository.id
        );
    }
    Ok(())
}

fn unique(values: &[String], label: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        nonempty(value, label)?;
        ensure!(seen.insert(value), "duplicate {label}: {value}");
    }
    Ok(())
}

fn nonempty(value: &str, label: &str) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && !value.chars().any(char::is_control),
        "invalid {label}: {value:?}"
    );
    Ok(())
}
