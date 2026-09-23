use super::*;
use crate::project::validate::{acyclic, files, ids, reference, unique_paths};
use anyhow::{Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

pub(in crate::project) fn validate_apple(project: &AppleProjectIr) -> Result<()> {
    ids(project.targets.keys())?;
    ids(project.swift_packages.keys())?;
    ids(project.schemes.keys())?;
    ids(project.configurations.keys())?;
    reference(&project.targets, &project.application, "Apple application")?;
    ensure!(
        project.targets[&project.application].kind == AppleTargetKind::Native,
        "Apple application must produce a native product"
    );
    files(&project.files)?;
    if let Some(name) = &project.default_configuration {
        reference(&project.configurations, name, "default configuration")?;
    }
    // Conditional dependencies are checked independently for each native platform
    // token. An empty token selects only unconditional edges.
    let mut platforms = BTreeSet::from([String::new()]);
    for target in project.targets.values() {
        for dep in &target.dependencies {
            platforms.extend(filters(dep).iter().cloned());
        }
        for embed in &target.embeds {
            platforms.extend(embed.platform_filters.iter().cloned());
        }
    }
    for (id, target) in &project.targets {
        ensure!(
            !target.product_name.trim().is_empty(),
            "empty Apple product name: {id}"
        );
        match target.kind {
            AppleTargetKind::Native => ensure!(
                target
                    .product_type
                    .as_ref()
                    .is_some_and(|s| !s.trim().is_empty()),
                "native target {id} needs product_type"
            ),
            AppleTargetKind::Aggregate => ensure!(
                target.product_type.is_none()
                    && target.rust.is_none()
                    && target.sources.is_empty()
                    && target.headers.is_empty()
                    && target.resources.is_empty()
                    && target.bundle_files.is_empty()
                    && target.resource_plists.is_empty()
                    && target.embeds.is_empty()
                    && target.info_plist.is_empty()
                    && target.entitlements.is_empty()
                    && target
                        .dependencies
                        .iter()
                        .all(|d| matches!(d, AppleDependency::Target { link: false, .. })),
                "aggregate target {id} cannot declare native product inputs"
            ),
        }
        ids(target.configurations.keys())?;
        for name in target.configurations.keys() {
            if !project.configurations.is_empty() {
                reference(&project.configurations, name, "target configuration")?;
            }
        }
        let mut dependency_ids = BTreeSet::new();
        for d in &target.dependencies {
            ensure!(
                dependency_ids.insert(super::compose::dependency_key(d)),
                "duplicate Apple dependency in {id}"
            );
            unique_strings(filters(d), "platform filter")?;
            match d {
                AppleDependency::Target {
                    target, link, weak, ..
                } => {
                    reference(&project.targets, target, &format!("target {id} dependency"))?;
                    ensure!(!weak || *link, "weak target dependency requires linking");
                    ensure!(
                        !link || project.targets[target].kind == AppleTargetKind::Native,
                        "cannot link aggregate target"
                    );
                }
                AppleDependency::BuildOutput { path, .. } => build_path(path)?,
                AppleDependency::SwiftProduct {
                    package, product, ..
                } => {
                    reference(
                        &project.swift_packages,
                        package,
                        &format!("target {id} Swift package"),
                    )?;
                    ensure!(!product.trim().is_empty(), "empty Swift product");
                }
                _ => {}
            }
        }
        let mut embedded = BTreeSet::new();
        for embed in &target.embeds {
            unique_strings(&embed.platform_filters, "platform filter")?;
            ensure!(
                embedded.insert((
                    format!("{:?}", embed.source),
                    embed.platform_filters.clone()
                )),
                "duplicate embedded product in {id}"
            );
            match &embed.source {
                AppleEmbedSource::Target { target } => {
                    reference(
                        &project.targets,
                        target,
                        &format!("target {id} embedded product"),
                    )?;
                    ensure!(
                        project.targets[target].kind == AppleTargetKind::Native,
                        "cannot embed aggregate target"
                    );
                }
                AppleEmbedSource::SwiftProduct { package, product } => {
                    reference(&project.swift_packages, package, "embedded Swift package")?;
                    ensure!(!product.trim().is_empty(), "empty embedded Swift product");
                }
                AppleEmbedSource::File { .. } => {}
                AppleEmbedSource::BuildOutput { path } => build_path(path)?,
            }
        }
        unique_paths(
            target.bundle_files.iter().map(|r| &r.destination),
            "Apple bundle files",
        )?;
        unique_paths(
            target
                .resources
                .iter()
                .filter_map(|r| match r {
                    AppleResource::Copy { resource } => Some(&resource.destination),
                    _ => None,
                })
                .chain(target.resource_plists.keys()),
            "Apple resources",
        )?;
        let mut sources = BTreeSet::new();
        for source in &target.sources {
            ensure!(
                sources.insert((
                    format!("{:?}", source.path),
                    source.platform_filters.clone()
                )),
                "duplicate Apple source"
            );
            build_path(&source.path)?;
            unique_strings(&source.platform_filters, "source platform filter")?;
        }
        let mut headers = BTreeSet::new();
        for header in &target.headers {
            ensure!(
                headers.insert(format!("{:?}", header.path)),
                "duplicate Apple header"
            );
            build_path(&header.path)?;
        }
        let mut scripts = BTreeSet::new();
        for script in &target.scripts {
            ensure!(
                !script.name.trim().is_empty() && scripts.insert(&script.name),
                "empty or duplicate Apple script name"
            );
            ensure!(!script.shell.trim().is_empty(), "empty Apple script shell");
            for path in script
                .inputs
                .iter()
                .chain(&script.outputs)
                .chain(&script.input_file_lists)
                .chain(&script.output_file_lists)
            {
                build_path(path)?;
            }
        }
    }
    for platform in platforms {
        let active = |filters: &[String]| filters.is_empty() || filters.contains(&platform);
        let mut graph = BTreeMap::new();
        for (id, target) in &project.targets {
            let mut edges = BTreeSet::new();
            for dep in &target.dependencies {
                if let AppleDependency::Target {
                    target,
                    platform_filters,
                    ..
                } = dep
                    && active(platform_filters)
                {
                    edges.insert(target.as_str());
                }
            }
            for embed in &target.embeds {
                if let AppleEmbedSource::Target { target } = &embed.source
                    && active(&embed.platform_filters)
                {
                    edges.insert(target.as_str());
                }
            }
            graph.insert(id.as_str(), edges);
        }
        acyclic(graph)?;
    }
    for (id, scheme) in &project.schemes {
        for target in scheme
            .build_targets
            .iter()
            .chain(&scheme.test_targets)
            .chain(&scheme.run_target)
        {
            reference(&project.targets, target, &format!("scheme {id}"))?;
        }
        for config in [
            Some(&scheme.run_configuration),
            Some(&scheme.archive_configuration),
            scheme.test_configuration.as_ref(),
            scheme.profile_configuration.as_ref(),
            scheme.analyze_configuration.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            ensure!(!config.trim().is_empty(), "empty scheme configuration");
            if !project.configurations.is_empty() {
                reference(&project.configurations, config, "scheme configuration")?;
            }
        }
        for target in scheme.build_for.keys() {
            ensure!(
                scheme.build_targets.contains(target),
                "scheme build-for target is not in build_targets"
            );
        }
        for options in scheme.action_options.values() {
            for scripts in [&options.pre_actions, &options.post_actions] {
                let mut names = BTreeSet::new();
                for script in scripts {
                    ensure!(
                        !script.name.trim().is_empty() && names.insert(&script.name),
                        "duplicate or empty scheme script name"
                    );
                    if let Some(target) = &script.environment_target {
                        reference(&project.targets, target, "scheme action environment")?;
                    }
                }
            }
        }
        if let Some(plan) = &scheme.default_test_plan {
            ensure!(
                scheme.test_plans.contains(plan),
                "default test plan is not declared"
            );
        }
    }
    Ok(())
}
fn filters(dep: &AppleDependency) -> &[String] {
    match dep {
        AppleDependency::Target {
            platform_filters, ..
        }
        | AppleDependency::SwiftProduct {
            platform_filters, ..
        }
        | AppleDependency::SystemFramework {
            platform_filters, ..
        }
        | AppleDependency::BuildOutput {
            platform_filters, ..
        }
        | AppleDependency::File {
            platform_filters, ..
        } => platform_filters,
    }
}
fn unique_strings(values: &[String], name: &str) -> Result<()> {
    ids(values)?;
    ensure!(
        values.iter().collect::<BTreeSet<_>>().len() == values.len(),
        "duplicate {name}"
    );
    Ok(())
}
fn build_path(path: &AppleBuildPath) -> Result<()> {
    match path {
        AppleBuildPath::Project(path) => ensure!(
            !path.as_str().contains("$("),
            "use an Apple build path expression for build settings"
        ),
        AppleBuildPath::Expression { expression } => ensure!(
            !expression.trim().is_empty() && !expression.chars().any(char::is_control),
            "invalid Apple build path expression"
        ),
    }
    Ok(())
}
