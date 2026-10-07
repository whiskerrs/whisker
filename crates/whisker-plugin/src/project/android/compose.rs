//! Explicit, transactional composition of Android declarations

use super::*;
use anyhow::{Result, ensure};

trait Merge {
    fn merge(&mut self, other: &Self, path: &str) -> Result<()>;
}

macro_rules! public_merge {
    ($($ty:ty),+ $(,)?) => {$(
        impl $ty {
            /// Merge an additive contribution, leaving self unchanged on conflict
            ///
            /// Equal values are idempotent; differing scalar values are errors.
            /// Named objects merge recursively. Existing list order is retained
            /// and new unique entries append. Priority lists (dimensions and
            /// fallbacks) must agree when both sides specify a nonempty list.
            /// Complete manifest trees must agree; use explicit manifest edits
            /// to change selected elements. Raw DSL is compared as text only.
            ///
            /// Errors include the conflicting field path. Explicit replacements
            /// require the caller to assign that field deliberately. Run final
            /// ProjectIr::validate_structure after composing all contributions;
            /// individual contributions may still have unresolved references.
            pub fn merge_from(&mut self, contribution: &Self) -> Result<()> {
                let mut next = self.clone();
                next.merge(contribution, stringify!($ty))?;
                *self = next;
                Ok(())
            }
        }
    )+};
}

public_merge!(
    AndroidProjectIr,
    AndroidModule,
    AndroidBuild,
    AndroidSdk,
    AndroidVariants,
    AndroidVariantValues,
    AndroidSourceSet,
    GradleBuildScript,
    GradleSettings
);

fn same<T: PartialEq>(current: &T, incoming: &T, path: &str) -> Result<()> {
    ensure!(current == incoming, "conflicting declaration at {path}");
    Ok(())
}

fn optional<T: Clone + PartialEq>(
    current: &mut Option<T>,
    incoming: &Option<T>,
    path: &str,
) -> Result<()> {
    if let Some(value) = incoming {
        if let Some(existing) = current {
            same(existing, value, path)?;
        } else {
            *current = Some(value.clone());
        }
    }
    Ok(())
}

fn append<T: Clone + PartialEq>(current: &mut Vec<T>, incoming: &[T]) {
    for value in incoming {
        if !current.contains(value) {
            current.push(value.clone());
        }
    }
}

fn ordered<T: Clone + PartialEq>(current: &mut Vec<T>, incoming: &[T], path: &str) -> Result<()> {
    if incoming.is_empty() {
        return Ok(());
    }
    if current.is_empty() {
        current.extend_from_slice(incoming);
        return Ok(());
    }
    same(&current.as_slice(), &incoming, path)
}

fn values<K: Ord + Clone + std::fmt::Display, V: Clone + PartialEq>(
    current: &mut BTreeMap<K, V>,
    incoming: &BTreeMap<K, V>,
    path: &str,
) -> Result<()> {
    for (key, value) in incoming {
        if let Some(existing) = current.get(key) {
            same(existing, value, &format!("{path}[{key}]"))?;
        } else {
            current.insert(key.clone(), value.clone());
        }
    }
    Ok(())
}

fn objects<K: Ord + Clone + std::fmt::Display, V: Merge + Clone>(
    current: &mut BTreeMap<K, V>,
    incoming: &BTreeMap<K, V>,
    path: &str,
) -> Result<()> {
    for (key, value) in incoming {
        if let Some(existing) = current.get_mut(key) {
            existing.merge(value, &format!("{path}[{key}]"))?;
        } else {
            current.insert(key.clone(), value.clone());
        }
    }
    Ok(())
}

fn named<T: Clone + PartialEq>(
    current: &mut Vec<T>,
    incoming: &[T],
    key: impl Fn(&T) -> &str,
    path: &str,
) -> Result<()> {
    for value in incoming {
        if let Some(existing) = current.iter().find(|entry| key(entry) == key(value)) {
            same(existing, value, &format!("{path}[{}]", key(value)))?;
        } else {
            current.push(value.clone());
        }
    }
    Ok(())
}

impl Merge for AndroidProjectIr {
    fn merge(&mut self, other: &Self, path: &str) -> Result<()> {
        if self.application.is_empty() {
            self.application = other.application.clone();
        } else if !other.application.is_empty() {
            same(
                &self.application,
                &other.application,
                &format!("{path}.application"),
            )?;
        }
        objects(
            &mut self.modules,
            &other.modules,
            &format!("{path}.modules"),
        )?;
        self.settings
            .merge(&other.settings, &format!("{path}.settings"))?;
        self.root_build
            .merge(&other.root_build, &format!("{path}.root_build"))?;
        values(
            &mut self.properties,
            &other.properties,
            &format!("{path}.properties"),
        )?;
        // ProjectPath intentionally has no Display implementation: keep diagnostics portable.
        for (key, value) in &other.files {
            if let Some(existing) = self.files.get(key) {
                same(existing, value, &format!("{path}.files[{}]", key.as_str()))?;
            } else {
                self.files.insert(key.clone(), value.clone());
            }
        }
        Ok(())
    }
}

impl Merge for AndroidModule {
    fn merge(&mut self, other: &Self, path: &str) -> Result<()> {
        same(
            &self.directory,
            &other.directory,
            &format!("{path}.directory"),
        )?;
        self.build.merge(&other.build, &format!("{path}.build"))?;
        append(&mut self.dependencies, &other.dependencies);
        let role_path = format!("{path}.kind");
        match (&mut self.kind, &other.kind) {
            (AndroidModuleKind::Application(a), AndroidModuleKind::Application(b)) => {
                a.android
                    .merge(&b.android, &format!("{role_path}.android"))?;
                optional(
                    &mut a.application_id,
                    &b.application_id,
                    &format!("{role_path}.application_id"),
                )?;
                append(&mut a.dynamic_features, &b.dynamic_features);
                append(&mut a.asset_packs, &b.asset_packs);
            }
            (AndroidModuleKind::Library(a), AndroidModuleKind::Library(b)) => {
                a.merge(b, &role_path)?
            }
            (AndroidModuleKind::DynamicFeature(a), AndroidModuleKind::DynamicFeature(b)) => {
                same(&a.base, &b.base, &format!("{role_path}.base"))?;
                a.android
                    .merge(&b.android, &format!("{role_path}.android"))?;
            }
            (AndroidModuleKind::Test(a), AndroidModuleKind::Test(b)) => {
                same(&a.target, &b.target, &format!("{role_path}.target"))?;
                a.android
                    .merge(&b.android, &format!("{role_path}.android"))?;
            }
            (AndroidModuleKind::AssetPack(a), AndroidModuleKind::AssetPack(b)) => {
                same(
                    &a.pack_name,
                    &b.pack_name,
                    &format!("{role_path}.pack_name"),
                )?;
                same(&a.delivery, &b.delivery, &format!("{role_path}.delivery"))?;
                append(&mut a.assets, &b.assets);
            }
            (a, b) => same(a, b, &role_path)?,
        }
        Ok(())
    }
}

impl Merge for AndroidBuild {
    fn merge(&mut self, other: &Self, path: &str) -> Result<()> {
        same(
            &self.namespace,
            &other.namespace,
            &format!("{path}.namespace"),
        )?;
        self.sdk.merge(&other.sdk, &format!("{path}.sdk"))?;
        self.default_config
            .merge(&other.default_config, &format!("{path}.default_config"))?;
        append(
            &mut self.default_config_statements,
            &other.default_config_statements,
        );
        self.variants
            .merge(&other.variants, &format!("{path}.variants"))?;
        objects(
            &mut self.source_sets,
            &other.source_sets,
            &format!("{path}.source_sets"),
        )?;
        append(&mut self.statements, &other.statements);
        Ok(())
    }
}

impl Merge for AndroidSdk {
    fn merge(&mut self, other: &Self, path: &str) -> Result<()> {
        match (&mut self.compile, &other.compile) {
            (
                Some(AndroidCompileSdk::Release {
                    api: a,
                    minor: am,
                    extension: ae,
                }),
                Some(AndroidCompileSdk::Release {
                    api: b,
                    minor: bm,
                    extension: be,
                }),
            ) => {
                same(a, b, &format!("{path}.compile.api"))?;
                optional(am, bm, &format!("{path}.compile.minor"))?;
                optional(ae, be, &format!("{path}.compile.extension"))?;
            }
            _ => optional(
                &mut self.compile,
                &other.compile,
                &format!("{path}.compile"),
            )?,
        }
        optional(&mut self.min, &other.min, &format!("{path}.min"))?;
        optional(&mut self.target, &other.target, &format!("{path}.target"))
    }
}

impl Merge for AndroidVariants {
    fn merge(&mut self, other: &Self, path: &str) -> Result<()> {
        ordered(
            &mut self.flavor_dimensions,
            &other.flavor_dimensions,
            &format!("{path}.flavor_dimensions"),
        )?;
        objects(
            &mut self.build_types,
            &other.build_types,
            &format!("{path}.build_types"),
        )?;
        objects(
            &mut self.product_flavors,
            &other.product_flavors,
            &format!("{path}.product_flavors"),
        )
    }
}

impl Merge for AndroidBuildType {
    fn merge(&mut self, other: &Self, path: &str) -> Result<()> {
        ordered(
            &mut self.matching_fallbacks,
            &other.matching_fallbacks,
            &format!("{path}.matching_fallbacks"),
        )?;
        self.values
            .merge(&other.values, &format!("{path}.values"))?;
        append(&mut self.statements, &other.statements);
        Ok(())
    }
}

impl Merge for AndroidProductFlavor {
    fn merge(&mut self, other: &Self, path: &str) -> Result<()> {
        same(
            &self.dimension,
            &other.dimension,
            &format!("{path}.dimension"),
        )?;
        ordered(
            &mut self.matching_fallbacks,
            &other.matching_fallbacks,
            &format!("{path}.matching_fallbacks"),
        )?;
        values(
            &mut self.missing_dimension_strategies,
            &other.missing_dimension_strategies,
            &format!("{path}.missing_dimension_strategies"),
        )?;
        self.values
            .merge(&other.values, &format!("{path}.values"))?;
        append(&mut self.statements, &other.statements);
        Ok(())
    }
}

impl Merge for AndroidVariantValues {
    fn merge(&mut self, other: &Self, path: &str) -> Result<()> {
        values(
            &mut self.manifest_placeholders,
            &other.manifest_placeholders,
            &format!("{path}.manifest_placeholders"),
        )?;
        values(
            &mut self.build_config_fields,
            &other.build_config_fields,
            &format!("{path}.build_config_fields"),
        )?;
        for (kind, fields) in &other.res_values {
            values(
                self.res_values.entry(kind.clone()).or_default(),
                fields,
                &format!("{path}.res_values[{kind}]"),
            )?;
        }
        Ok(())
    }
}

impl Merge for AndroidSourceSet {
    fn merge(&mut self, other: &Self, path: &str) -> Result<()> {
        optional(
            &mut self.manifest,
            &other.manifest,
            &format!("{path}.manifest (use edit_manifest for element changes)"),
        )?;
        append(&mut self.sources, &other.sources);
        append(&mut self.android_resources, &other.android_resources);
        append(&mut self.java_resources, &other.java_resources);
        append(&mut self.aidl, &other.aidl);
        append(&mut self.shaders, &other.shaders);
        append(&mut self.baseline_profiles, &other.baseline_profiles);
        append(&mut self.assets, &other.assets);
        append(&mut self.jni_libraries, &other.jni_libraries);
        Ok(())
    }
}

impl Merge for GradleBuildScript {
    fn merge(&mut self, other: &Self, path: &str) -> Result<()> {
        append(&mut self.imports, &other.imports);
        append(&mut self.buildscript, &other.buildscript);
        append(&mut self.plugin_statements, &other.plugin_statements);
        named(
            &mut self.plugins,
            &other.plugins,
            |p| &p.id,
            &format!("{path}.plugins"),
        )?;
        append(&mut self.statements, &other.statements);
        Ok(())
    }
}

impl Merge for GradleSettings {
    fn merge(&mut self, other: &Self, path: &str) -> Result<()> {
        append(&mut self.imports, &other.imports);
        append(
            &mut self.plugin_management.statements,
            &other.plugin_management.statements,
        );
        let management = format!("{path}.plugin_management");
        named(
            &mut self.plugin_management.repositories,
            &other.plugin_management.repositories,
            |r| &r.id,
            &format!("{management}.repositories"),
        )?;
        append(
            &mut self.plugin_management.included_builds,
            &other.plugin_management.included_builds,
        );
        values(
            &mut self.plugin_management.plugins,
            &other.plugin_management.plugins,
            &format!("{management}.plugins"),
        )?;
        append(
            &mut self.plugin_management.resolution_strategy,
            &other.plugin_management.resolution_strategy,
        );
        append(&mut self.buildscript, &other.buildscript);
        named(
            &mut self.plugins,
            &other.plugins,
            |p| &p.id,
            &format!("{path}.plugins"),
        )?;
        let resolution = format!("{path}.dependency_resolution");
        optional(
            &mut self.dependency_resolution.repositories_mode,
            &other.dependency_resolution.repositories_mode,
            &format!("{resolution}.repositories_mode"),
        )?;
        named(
            &mut self.dependency_resolution.repositories,
            &other.dependency_resolution.repositories,
            |r| &r.id,
            &format!("{resolution}.repositories"),
        )?;
        append(
            &mut self.dependency_resolution.statements,
            &other.dependency_resolution.statements,
        );
        self.android_sdk
            .merge(&other.android_sdk, &format!("{path}.android_sdk"))?;
        append(&mut self.included_builds, &other.included_builds);
        append(&mut self.statements, &other.statements);
        Ok(())
    }
}
