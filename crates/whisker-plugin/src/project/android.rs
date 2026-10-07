//! Android project declarations, including non-Android Gradle modules
//!
//! Module roles select the structure of native configuration. Arbitrary Gradle
//! plugins remain possible through Custom and External modules. The model does
//! not resolve variants or execute Gradle; references are checked by CNG and
//! configuration/task dependency cycles are checked by Gradle.

use super::{ProjectFiles, ProjectPath, XmlElement};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod compose;
mod manifest;
mod validate;
pub use manifest::*;
pub(super) use validate::validate_android;

/// An Android application's Gradle project, keyed by project paths such as `:app`
///
/// Default is an empty composition input, not a valid finished application.
/// Plugins supply the primary application and modules before final validation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidProjectIr {
    /// ID of the primary application module. Empty means undeclared during
    /// composition; final validation requires an existing application module.
    /// Merge fills an empty ID, ignores an empty contribution, and rejects
    /// differing nonempty IDs.
    pub application: String,
    /// All included modules, including custom or externally owned modules.
    #[serde(default)]
    pub modules: BTreeMap<String, AndroidModule>,
    /// Settings script and dependency resolution scopes.
    #[serde(default)]
    pub settings: GradleSettings,
    /// Root build script; has the same ordered scopes as module scripts.
    #[serde(default)]
    pub root_build: GradleBuildScript,
    /// gradle.properties entries (not Gradle Wrapper properties).
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
    /// Files staged at the generated project root.
    #[serde(default)]
    pub files: ProjectFiles,
}

/// Common Gradle module structure, independent of its Android role
///
/// Every source path is relative to the generated project root. Kind-specific
/// settings determine which extension block a renderer emits; JVM, Custom, and
/// External modules never acquire an implicit `android` block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidModule {
    /// Module directory beneath the generated project root.
    pub directory: ProjectPath,
    /// Native role and only the configuration applicable to that role.
    pub kind: AndroidModuleKind,
    /// Scoped build script; must be empty for externally owned build files.
    #[serde(default)]
    pub build: GradleBuildScript,
    /// Dependencies; configuration names remain native Gradle names.
    /// External modules own their dependencies inside their build file instead.
    #[serde(default)]
    pub dependencies: Vec<GradleDependency>,
}

/// Module-specific configuration rather than one mandatory set of Android fields
///
/// Assembler-supplied plugin declarations must implement the selected role.
/// Native tooling validates plugin compatibility; CNG does not infer plugins
/// or versions from the role. Packaging relationships are not compile edges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "config",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AndroidModuleKind {
    /// Installable application and the products it packages.
    Application(AndroidApplication),
    /// Android library.
    Library(AndroidBuild),
    /// Feature with install-time, conditional, or on-demand delivery.
    DynamicFeature(AndroidDynamicFeature),
    /// Separate instrumentation test module using com.android.test.
    Test(AndroidTestModule),
    /// Play Asset Delivery module; does not have namespace or SDK fields.
    AssetPack(AndroidAssetPack),
    /// Java/Kotlin JVM module; configured through its common build script.
    Jvm,
    /// Other generated Gradle module, e.g. KMP, Fused Library, or an AI pack.
    /// Its common build script owns the native extension DSL.
    Custom,
    /// A staged build file entirely owned by its author.
    External {
        /// Build file path relative to the generated project root.
        build_file: ProjectPath,
    },
}

/// Application-only identity and packaging relationships
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidApplication {
    /// Standard Android extension configuration.
    pub android: AndroidBuild,
    /// Application ID; None leaves native convention/default configuration in charge.
    pub application_id: Option<String>,
    /// Dynamic feature IDs packaged with this application.
    #[serde(default)]
    pub dynamic_features: Vec<String>,
    /// Asset pack IDs packaged with this application.
    #[serde(default)]
    pub asset_packs: Vec<String>,
}

/// A feature and its base application
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidDynamicFeature {
    /// Standard Android extension configuration.
    pub android: AndroidBuild,
    /// Base application module ID; a renderer emits implementation(project(base)).
    pub base: String,
}

/// A separately built instrumentation test application
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidTestModule {
    /// Standard Android extension configuration.
    pub android: AndroidBuild,
    /// Application ID in the module graph, emitted as targetProjectPath.
    pub target: String,
}

/// Asset pack name, delivery policy, and staged asset directories
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidAssetPack {
    /// Native asset pack name.
    pub pack_name: String,
    /// Delivery timing.
    pub delivery: AssetPackDelivery,
    /// Project-relative asset directories.
    #[serde(default)]
    pub assets: Vec<ProjectPath>,
}

/// Delivery policy for an asset pack
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssetPackDelivery {
    /// Installed with the base application.
    InstallTime,
    /// Downloaded automatically after installation.
    FastFollow,
    /// Downloaded when requested at runtime.
    OnDemand,
}

/// Common configuration for Android app, library, feature, and test extensions
///
/// None SDK fields mean no module-level declaration, allowing settings defaults
/// to apply. Native defaults and SDK installation remain the backend's concern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidBuild {
    /// Namespace for generated Android code, independent of application ID.
    pub namespace: String,
    /// Optional module SDK declarations.
    #[serde(default)]
    pub sdk: AndroidSdk,
    /// Values emitted in defaultConfig before raw default-config statements.
    #[serde(default)]
    pub default_config: AndroidVariantValues,
    /// Additional defaultConfig Kotlin DSL (versions, test runner, etc.).
    #[serde(default)]
    pub default_config_statements: Vec<String>,
    /// Named build types and flavors, merged by name rather than repeated create calls.
    #[serde(default)]
    pub variants: AndroidVariants,
    /// Source sets, including main, tests, and variant-specific overlays.
    #[serde(default)]
    pub source_sets: BTreeMap<String, AndroidSourceSet>,
    /// Additional android-block DSL after all structured settings.
    /// May override them; CNG does not interpret or conflict-check these statements.
    #[serde(default)]
    pub statements: Vec<String>,
}

/// SDK declarations usable at settings or module scope
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidSdk {
    /// SDK used for compilation; None means inherited/unspecified.
    pub compile: Option<AndroidCompileSdk>,
    /// Lowest runtime API; None means inherited/unspecified.
    pub min: Option<AndroidApiLevel>,
    /// Target runtime behavior; None means inherited/unspecified.
    pub target: Option<AndroidApiLevel>,
}

/// Compile SDK identity, retaining preview and SDK extension information
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AndroidCompileSdk {
    /// Released SDK, with independently specified minor/extension levels.
    Release {
        /// Major API level.
        api: u32,
        /// Optional minor API level.
        minor: Option<u32>,
        /// Optional SDK extension level.
        extension: Option<u32>,
    },
    /// Unreleased SDK identified by its native codename.
    Preview {
        /// Native SDK codename.
        codename: String,
    },
    /// Vendor add-on SDK.
    AddOn {
        /// Vendor identifier.
        vendor: String,
        /// Add-on name.
        name: String,
        /// Base API level.
        api: u32,
    },
}

/// Release or preview runtime API for minSdk/targetSdk
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AndroidApiLevel {
    /// Released API level.
    Release(u32),
    /// Preview codename.
    Preview(String),
}

/// Variant identities and ordered flavor dimensions
///
/// An empty build-type map does not remove native default Debug/Release types.
/// A renderer configures each named object once, whether it already exists or
/// needs creating. Dimension order affects priority and is never auto-sorted.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidVariants {
    /// Priority order of flavor dimensions.
    #[serde(default)]
    pub flavor_dimensions: Vec<String>,
    /// Build types keyed by native name.
    #[serde(default)]
    pub build_types: BTreeMap<String, AndroidBuildType>,
    /// Product flavors keyed by native name.
    #[serde(default)]
    pub product_flavors: BTreeMap<String, AndroidProductFlavor>,
}

/// Contributions to one build type
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidBuildType {
    /// Ordered fallback build types, resolved by Gradle in dependency projects.
    #[serde(default)]
    pub matching_fallbacks: Vec<String>,
    /// Manifest placeholders and generated values.
    #[serde(default)]
    pub values: AndroidVariantValues,
    /// Extra statements inside this build type (signing, shrinking, suffixes, etc.).
    #[serde(default)]
    pub statements: Vec<String>,
}

/// Contributions to one product flavor
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidProductFlavor {
    /// Declared dimension containing this flavor.
    pub dimension: String,
    /// Ordered fallback flavors in dependency projects.
    #[serde(default)]
    pub matching_fallbacks: Vec<String>,
    /// Missing dependency dimension → ordered candidate flavors.
    #[serde(default)]
    pub missing_dimension_strategies: BTreeMap<String, Vec<String>>,
    /// Manifest placeholders and generated values.
    #[serde(default)]
    pub values: AndroidVariantValues,
    /// Extra statements inside this flavor (SDK overrides, suffixes, versions, etc.).
    #[serde(default)]
    pub statements: Vec<String>,
}

/// Keyed values that plugins commonly contribute to the same variant
///
/// Value strings are Kotlin expressions, not pre-escaped XML or literal Kotlin
/// strings. For example, a string-valued placeholder uses `"\"example\""`.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidVariantValues {
    /// Manifest placeholder name → Kotlin value expression.
    #[serde(default)]
    pub manifest_placeholders: BTreeMap<String, String>,
    /// BuildConfig field name → declared type and value expression.
    #[serde(default)]
    pub build_config_fields: BTreeMap<String, AndroidBuildConfigField>,
    /// Resource type → resource name → Kotlin value expression.
    #[serde(default)]
    pub res_values: BTreeMap<String, BTreeMap<String, String>>,
}

/// Type and value of a generated BuildConfig field
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidBuildConfigField {
    /// Native type name, such as String or boolean.
    pub type_name: String,
    /// Kotlin expression passed as the field's value to Gradle.
    pub value: String,
}

/// Static source directories and manifest for one source set
///
/// Paths are project-relative. `android_resources` maps to AGP `res`, while
/// `java_resources` maps to AGP `resources`. Generated task outputs must still
/// be connected through a Gradle plugin or Android Components DSL.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidSourceSet {
    /// Complete Manifest tree; native tooling merges source-set/library manifests.
    pub manifest: Option<XmlElement>,
    /// Java/Kotlin source directories.
    #[serde(default)]
    pub sources: Vec<ProjectPath>,
    /// Android resource directories, including qualified subdirectories.
    #[serde(default)]
    pub android_resources: Vec<ProjectPath>,
    /// Non-Android classpath resource directories.
    #[serde(default)]
    pub java_resources: Vec<ProjectPath>,
    /// AIDL source directories.
    #[serde(default)]
    pub aidl: Vec<ProjectPath>,
    /// Shader source directories.
    #[serde(default)]
    pub shaders: Vec<ProjectPath>,
    /// Baseline profile directories.
    #[serde(default)]
    pub baseline_profiles: Vec<ProjectPath>,
    /// Android asset directories.
    #[serde(default)]
    pub assets: Vec<ProjectPath>,
    /// Prebuilt JNI library directories.
    #[serde(default)]
    pub jni_libraries: Vec<ProjectPath>,
}

/// One dependency and its native Gradle configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradleDependency {
    /// Configuration such as implementation, debugImplementation, or kapt.
    pub configuration: String,
    /// Dependency notation.
    pub source: GradleDependencySource,
}

/// Structured project references and external dependency expressions
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum GradleDependencySource {
    /// Maven group:artifact:version coordinates.
    Maven(String),
    /// Included module ID; CNG checks existence, not task-graph acyclicity.
    Project(String),
    /// Kotlin expression, e.g. platform("group:bom:version") or a catalog alias.
    Kotlin(String),
}

/// Gradle build-script scopes in evaluation order
///
/// Render imports, buildscript, and plugins first; then module-specific generated
/// declarations and finally statements. Imports use fully-qualified names without
/// the import keyword. Plugin IDs are explicit, including for aliases, and declaration order is preserved.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradleBuildScript {
    /// Imports before all executable declarations.
    #[serde(default)]
    pub imports: Vec<String>,
    /// Statements inside the early buildscript block.
    #[serde(default)]
    pub buildscript: Vec<String>,
    /// Plugin declarations in application order, emitted once per ID.
    #[serde(default)]
    pub plugins: Vec<GradlePlugin>,
    /// Opaque declarations inside plugins, after structured plugin IDs.
    /// Preserves author-supplied Kotlin DSL without guessing identity.
    #[serde(default)]
    pub plugin_statements: Vec<String>,
    /// Ordinary Kotlin statements after structured declarations.
    #[serde(default)]
    pub statements: Vec<String>,
}

/// A plugin declaration identified by its actual plugin ID
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradlePlugin {
    /// Actual plugin ID, including when using a catalog alias.
    pub id: String,
    /// Literal version for id-based declarations, None uses native resolution.
    /// Mutually exclusive with alias.
    pub version: Option<String>,
    /// Catalog alias expression, such as libs.plugins.android.application.
    pub alias: Option<String>,
    /// Whether to apply the plugin in this scope.
    pub apply: bool,
}

/// A repository expression with stable identity and declaration order
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradleRepository {
    /// Stable key for composition; e.g. google or company-releases.
    pub id: String,
    /// Kotlin expression including any credentials/content-filter configuration.
    pub expression: String,
}

/// Early plugin resolution, before settings plugins are applied
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradlePluginManagement {
    /// Statements before repositories, e.g. conditional local included builds.
    #[serde(default)]
    pub statements: Vec<String>,
    /// Plugin resolution repositories in search order.
    #[serde(default)]
    pub repositories: Vec<GradleRepository>,
    /// Builds supplying plugins.
    #[serde(default)]
    pub included_builds: Vec<ProjectPath>,
    /// Default plugin versions, keyed by actual plugin ID.
    #[serde(default)]
    pub plugins: BTreeMap<String, String>,
    /// Statements inside resolutionStrategy.
    #[serde(default)]
    pub resolution_strategy: Vec<String>,
}

/// Project repository behavior in dependencyResolutionManagement
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GradleRepositoriesMode {
    /// Project repositories take precedence.
    PreferProject,
    /// Settings repositories take precedence.
    PreferSettings,
    /// Reject project-level repository declarations.
    FailOnProjectRepos,
}

/// Dependency resolution after settings plugins have been applied
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradleDependencyResolution {
    /// Native repository mode; None leaves the default unspecified.
    pub repositories_mode: Option<GradleRepositoriesMode>,
    /// Dependency repositories in search order.
    #[serde(default)]
    pub repositories: Vec<GradleRepository>,
    /// Additional scoped DSL, including version catalogs.
    #[serde(default)]
    pub statements: Vec<String>,
}

/// Settings scopes in native evaluation order
///
/// Render imports, pluginManagement, buildscript, settings plugins, dependency
/// resolution, Android SDK defaults, includes, then ordinary statements. SDK
/// defaults require an explicitly applied settings plugin; no plugin is inferred.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradleSettings {
    /// Imports at the start of settings.gradle.kts.
    #[serde(default)]
    pub imports: Vec<String>,
    /// Plugin repository/default-version/resolution configuration.
    #[serde(default)]
    pub plugin_management: GradlePluginManagement,
    /// Statements inside the early settings buildscript block.
    #[serde(default)]
    pub buildscript: Vec<String>,
    /// Plugins applied to Settings, separate from root Project plugins.
    #[serde(default)]
    pub plugins: Vec<GradlePlugin>,
    /// Repositories, mode, and version catalogs for project dependencies.
    #[serde(default)]
    pub dependency_resolution: GradleDependencyResolution,
    /// SDK defaults supplied through the Android settings plugin.
    #[serde(default)]
    pub android_sdk: AndroidSdk,
    /// Composite builds included at settings scope.
    #[serde(default)]
    pub included_builds: Vec<ProjectPath>,
    /// Ordinary settings-level Kotlin DSL after structured declarations.
    #[serde(default)]
    pub statements: Vec<String>,
}

#[cfg(test)]
mod tests;
