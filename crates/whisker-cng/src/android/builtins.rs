//! Existing configuration helpers contributing to the declarative main module.
//! Configuration and icon decoding stay shared with the mobile implementations.
use super::application;
use super::*;
use whisker_plugin::project::*;
use whisker_plugin::{GenerateContext, Plugin, PluginConfig};

pub(crate) fn contribution<P: Plugin>(
    plugin: &P,
    ctx: &ProjectContext,
    cfg: &P::Config,
) -> Result<ProjectUpdate> {
    let ProjectIr::Android(original) = &ctx.project else {
        return Ok(ProjectUpdate::Keep);
    };
    let mut scratch = GenerateContext {
        android: Some(Default::default()),
        app_crate_dir: ctx.app_crate_dir.clone(),
        ..Default::default()
    };
    Plugin::apply(plugin, &mut scratch, cfg)?;
    if scratch.journal.records.is_empty() {
        return Ok(ProjectUpdate::Keep);
    }
    let delta = scratch.android.expect("Android builtin context");
    let mut project = original.as_ref().clone();
    let module = project
        .modules
        .get_mut(&project.application)
        .context("missing primary Android module")?;
    let AndroidModuleKind::Application(app) = &mut module.kind else {
        anyhow::bail!("primary Android module is not an application");
    };
    if !delta.manifest.permissions.is_empty()
        || !delta.manifest.application_attributes.is_empty()
        || !delta.manifest.application_meta_data.is_empty()
    {
        let manifest = app
            .android
            .source_sets
            .get_mut("main")
            .and_then(|s| s.manifest.as_mut())
            .context("main app Manifest required by built-in")?;
        for permission in delta.manifest.permissions {
            let node = XmlNode::Element(application::element(
                "uses-permission",
                &[("android:name", &permission)],
                vec![],
            ));
            if !manifest.children.contains(&node) {
                manifest.children.insert(0, node);
            }
        }
        let application = manifest
            .children
            .iter_mut()
            .find_map(|n| match n {
                XmlNode::Element(e) if e.name == "application" => Some(e),
                _ => None,
            })
            .context("Manifest application required by built-in")?;
        for a in delta.manifest.application_attributes {
            application.attributes.insert(a.name, a.value);
        }
        for m in delta.manifest.application_meta_data {
            application
                .children
                .push(XmlNode::Element(application::element(
                    "meta-data",
                    &[("android:name", &m.name), ("android:value", &m.value)],
                    vec![],
                )));
        }
    }
    for p in delta.gradle.apply_plugins {
        application::add_plugin(&mut module.build, &p);
    }
    if !delta.gradle.dependencies.is_empty() {
        module.build.statements.push(format!(
            "dependencies {{\n{}\n}}",
            delta.gradle.dependencies.join("\n")
        ));
    }
    for (path, entry) in delta.extra_files {
        crate::render::validate_extra_file_path(&path)?;
        project.files.insert(
            ProjectPath::new(path.to_string_lossy())?,
            ProjectFile::Generated { entry },
        );
    }
    if project == **original {
        Ok(ProjectUpdate::Keep)
    } else {
        Ok(ProjectUpdate::Replace {
            project: Box::new(ProjectIr::Android(Box::new(project))),
            reason: format!(
                "apply explicit {} configuration to the main Android module",
                P::Config::NAME
            ),
        })
    }
}
impl crate::ProjectEngine {
    /// Register the Android implementations of existing built-ins. iOS-only
    /// configurations remain accepted with no Android contribution.
    pub fn with_android_builtins() -> Self {
        let mut engine = Self::new();
        engine.register_mobile_builtins();
        engine
    }

    /// Compose an Android app from empty declarations: application first, then
    /// the existing configuration helpers and any registered project plugins.
    pub fn with_android_application(inputs: AndroidInputs) -> Self {
        let mut engine = Self::with_initializer(application::ApplicationPlugin::new(inputs));
        engine.register_mobile_builtins();
        engine
    }
}
