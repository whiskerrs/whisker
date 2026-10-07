use super::*;
use crate::project::merge::{
    self, Merge, append, equal, named, objects, optional, public_merge, values,
};
use anyhow::Result;

public_merge!(
    IosProjectIr,
    MacosProjectIr,
    AppleProjectIr,
    AppleTarget,
    AppleBuildConfiguration,
    PropertyListValue,
    AppleScheme,
    AppleSchemeActionOptions
);

impl Merge for IosProjectIr {
    fn merge(&mut self, other: &Self, path: &str) -> Result<()> {
        self.apple.merge(&other.apple, path)
    }
}
impl Merge for MacosProjectIr {
    fn merge(&mut self, other: &Self, path: &str) -> Result<()> {
        self.apple.merge(&other.apple, path)
    }
}
impl Merge for AppleProjectIr {
    fn merge(&mut self, b: &Self, p: &str) -> Result<()> {
        if self.application.is_empty() {
            self.application = b.application.clone();
        } else if !b.application.is_empty() {
            equal(
                &self.application,
                &b.application,
                &format!("{p}.application"),
            )?;
        }
        objects(&mut self.targets, &b.targets, &format!("{p}.targets"))?;
        values(
            &mut self.build_settings,
            &b.build_settings,
            &format!("{p}.build_settings"),
        )?;
        objects(
            &mut self.configurations,
            &b.configurations,
            &format!("{p}.configurations"),
        )?;
        optional(
            &mut self.default_configuration,
            &b.default_configuration,
            &format!("{p}.default_configuration"),
        )?;
        values(
            &mut self.swift_packages,
            &b.swift_packages,
            &format!("{p}.swift_packages"),
        )?;
        objects(&mut self.schemes, &b.schemes, &format!("{p}.schemes"))?;
        values(&mut self.files, &b.files, &format!("{p}.files"))
    }
}
impl Merge for AppleBuildConfiguration {
    fn merge(&mut self, b: &Self, p: &str) -> Result<()> {
        optional(&mut self.xcconfig, &b.xcconfig, &format!("{p}.xcconfig"))?;
        values(&mut self.settings, &b.settings, &format!("{p}.settings"))
    }
}
impl Merge for AppleTarget {
    fn merge(&mut self, b: &Self, p: &str) -> Result<()> {
        equal(
            &self.product_name,
            &b.product_name,
            &format!("{p}.product_name"),
        )?;
        equal(
            &self.product_type,
            &b.product_type,
            &format!("{p}.product_type"),
        )?;
        equal(&self.kind, &b.kind, &format!("{p}.kind"))?;
        optional(&mut self.rust, &b.rust, &format!("{p}.rust"))?;
        named(
            &mut self.sources,
            &b.sources,
            |s| (format!("{:?}", s.path), s.platform_filters.clone()),
            &format!("{p}.sources"),
        )?;
        named(
            &mut self.headers,
            &b.headers,
            |h| format!("{:?}", h.path),
            &format!("{p}.headers"),
        )?;
        named(
            &mut self.resources,
            &b.resources,
            |r| match r {
                AppleResource::Process { path } => format!("process:{}", path.as_str()),
                AppleResource::Copy { resource } => {
                    format!("copy:{}", resource.destination.as_str())
                }
            },
            &format!("{p}.resources"),
        )?;
        named(
            &mut self.bundle_files,
            &b.bundle_files,
            |r| r.destination.clone(),
            &format!("{p}.bundle_files"),
        )?;
        objects(
            &mut self.resource_plists,
            &b.resource_plists,
            &format!("{p}.resource_plists"),
        )?;
        objects(
            &mut self.info_plist,
            &b.info_plist,
            &format!("{p}.info_plist"),
        )?;
        objects(
            &mut self.entitlements,
            &b.entitlements,
            &format!("{p}.entitlements"),
        )?;
        values(
            &mut self.build_settings,
            &b.build_settings,
            &format!("{p}.build_settings"),
        )?;
        objects(
            &mut self.configurations,
            &b.configurations,
            &format!("{p}.configurations"),
        )?;
        named(
            &mut self.dependencies,
            &b.dependencies,
            dependency_key,
            &format!("{p}.dependencies"),
        )?;
        named(
            &mut self.embeds,
            &b.embeds,
            |e| (format!("{:?}", e.source), e.platform_filters.clone()),
            &format!("{p}.embeds"),
        )?;
        named(
            &mut self.scripts,
            &b.scripts,
            |s| s.name.clone(),
            &format!("{p}.scripts"),
        )
    }
}
pub(super) fn dependency_key(d: &AppleDependency) -> (String, Vec<String>) {
    match d {
        AppleDependency::Target {
            target,
            platform_filters,
            ..
        } => (format!("target:{target}"), platform_filters.clone()),
        AppleDependency::SwiftProduct {
            package,
            product,
            platform_filters,
            ..
        } => (
            format!("package:{package}/{product}"),
            platform_filters.clone(),
        ),
        AppleDependency::SystemFramework {
            name,
            platform_filters,
            ..
        } => (format!("system:{name}"), platform_filters.clone()),
        AppleDependency::BuildOutput {
            path,
            platform_filters,
            ..
        } => (format!("build-output:{path:?}"), platform_filters.clone()),
        AppleDependency::File {
            path,
            platform_filters,
            ..
        } => (format!("file:{}", path.as_str()), platform_filters.clone()),
    }
}
impl Merge for AppleScheme {
    fn merge(&mut self, b: &Self, p: &str) -> Result<()> {
        append(&mut self.build_targets, &b.build_targets);
        append(&mut self.test_targets, &b.test_targets);
        optional(
            &mut self.run_target,
            &b.run_target,
            &format!("{p}.run_target"),
        )?;
        equal(
            &self.run_configuration,
            &b.run_configuration,
            &format!("{p}.run_configuration"),
        )?;
        equal(
            &self.archive_configuration,
            &b.archive_configuration,
            &format!("{p}.archive_configuration"),
        )?;
        optional(
            &mut self.test_configuration,
            &b.test_configuration,
            &format!("{p}.test_configuration"),
        )?;
        optional(
            &mut self.profile_configuration,
            &b.profile_configuration,
            &format!("{p}.profile_configuration"),
        )?;
        optional(
            &mut self.analyze_configuration,
            &b.analyze_configuration,
            &format!("{p}.analyze_configuration"),
        )?;
        objects(
            &mut self.action_options,
            &b.action_options,
            &format!("{p}.action_options"),
        )?;
        objects(&mut self.build_for, &b.build_for, &format!("{p}.build_for"))?;
        append(&mut self.test_plans, &b.test_plans);
        optional(
            &mut self.default_test_plan,
            &b.default_test_plan,
            &format!("{p}.default_test_plan"),
        )
    }
}
impl Merge for PropertyListValue {
    fn merge(&mut self, b: &Self, p: &str) -> Result<()> {
        match (&mut *self, b) {
            (Self::Dict(a), Self::Dict(b)) => objects(a, b, p),
            // Arrays retain native order/identity. Append or replace them explicitly.
            _ => merge::equal(self, b, p),
        }
    }
}

impl Merge for AppleSchemeActionOptions {
    fn merge(&mut self, b: &Self, p: &str) -> Result<()> {
        if self.arguments.is_empty() {
            self.arguments = b.arguments.clone();
        } else if !b.arguments.is_empty() {
            equal(&self.arguments, &b.arguments, &format!("{p}.arguments"))?;
        }
        values(
            &mut self.environment,
            &b.environment,
            &format!("{p}.environment"),
        )?;
        named(
            &mut self.pre_actions,
            &b.pre_actions,
            |s| s.name.clone(),
            &format!("{p}.pre_actions"),
        )?;
        named(
            &mut self.post_actions,
            &b.post_actions,
            |s| s.name.clone(),
            &format!("{p}.post_actions"),
        )
    }
}
impl Merge for AppleSchemeBuildFor {
    fn merge(&mut self, b: &Self, p: &str) -> Result<()> {
        optional(&mut self.running, &b.running, &format!("{p}.running"))?;
        optional(&mut self.testing, &b.testing, &format!("{p}.testing"))?;
        optional(&mut self.profiling, &b.profiling, &format!("{p}.profiling"))?;
        optional(&mut self.archiving, &b.archiving, &format!("{p}.archiving"))?;
        optional(&mut self.analyzing, &b.analyzing, &format!("{p}.analyzing"))
    }
}
