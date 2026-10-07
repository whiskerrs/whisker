//! Apple build products shared by iOS and macOS

use super::{ProjectFiles, ProjectPath, Resource, RustBuild};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod compose;
#[cfg(test)]
mod tests;
mod validate;
pub(super) use validate::validate_apple;

/// An iOS project declaration, independent of Xcode serialization
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IosProjectIr {
    /// Application, extension, library, and test targets.
    pub apple: AppleProjectIr,
}

/// A macOS project declaration, usable with Cargo-based or Xcode-based builds
///
/// Declaring targets does not require an `.xcodeproj`. A backend may build a
/// Rust executable with Cargo and assemble its bundle directly.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacosProjectIr {
    /// Application, helper, extension, library, and test targets.
    pub apple: AppleProjectIr,
}

/// A graph of Apple build products and their project-wide inputs
///
/// Map keys are stable IDs chosen by the assembler or plugin, not PBX object IDs.
/// Bundle identity and version live in each target's `info_plist`; build-setting
/// substitutions such as `$(PRODUCT_BUNDLE_IDENTIFIER)` remain literal strings.
/// A backend resolves them using that target's settings. Signing secrets and
/// provisioning operations are outside this model.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppleProjectIr {
    /// ID of the primary application target. Empty is undeclared during composition;
    /// final validation requires an existing application. Merge fills an empty ID.
    pub application: String,
    /// Build products keyed by stable target ID.
    #[serde(default)]
    pub targets: BTreeMap<String, AppleTarget>,
    /// Settings inherited by every target before target-local overrides.
    #[serde(default)]
    pub build_settings: BTreeMap<String, AppleBuildSetting>,
    /// Project configuration layers; an empty map leaves backend defaults in charge.
    #[serde(default)]
    pub configurations: BTreeMap<String, AppleBuildConfiguration>,
    /// Default configuration, when explicitly selected.
    pub default_configuration: Option<String>,
    /// Swift package sources keyed by stable package ID.
    #[serde(default)]
    pub swift_packages: BTreeMap<String, SwiftPackage>,
    /// Shared schemes keyed by scheme name.
    #[serde(default)]
    pub schemes: BTreeMap<String, AppleScheme>,
    /// Files staged at the generated project root.
    #[serde(default)]
    pub files: ProjectFiles,
}

/// One Apple build product, including its metadata and build inputs
///
/// `product_type` uses the native product identifier (for example,
/// `com.apple.product-type.app-extension`). It is deliberately open-ended:
/// renderer support and OS-specific validity are checked by the backend.
/// `Info.plist` and entitlements are complete dictionaries for this target,
/// rather than snippets appended to an implicit template.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppleTarget {
    /// Human-readable product name, separate from the stable target ID.
    pub product_name: String,
    /// Native product type identifier; required for Native and absent for Aggregate.
    pub product_type: Option<String>,
    /// Aggregate targets run dependencies/scripts without producing a native product.
    #[serde(default)]
    pub kind: AppleTargetKind,
    /// Cargo executable for this product, or library linked by its native sources.
    /// Absent when native sources/scripts supply the complete product.
    pub rust: Option<RustBuild>,
    /// Compiler inputs and optional per-file flags.
    #[serde(default)]
    pub sources: Vec<AppleSource>,
    /// Headers with explicit framework visibility.
    #[serde(default)]
    pub headers: Vec<AppleHeader>,
    /// Files copied relative to the bundle root, e.g. Contents/Library/LaunchAgents.
    /// Distinct from resource-root copies and embedded executable products.
    #[serde(default)]
    pub bundle_files: Vec<Resource>,
    /// Resources processed by native tools or copied into this product.
    #[serde(default)]
    pub resources: Vec<AppleResource>,
    /// Generated property-list resources, keyed by resource-root destination.
    /// Supports composable PrivacyInfo.xcprivacy and other native plist resources.
    #[serde(default)]
    pub resource_plists: BTreeMap<ProjectPath, PropertyListValue>,
    /// Complete target-local Info.plist, including any NSExtension dictionary.
    #[serde(default)]
    pub info_plist: BTreeMap<String, PropertyListValue>,
    /// Entitlements used when signing this product.
    #[serde(default)]
    pub entitlements: BTreeMap<String, PropertyListValue>,
    /// Settings common to every build configuration.
    #[serde(default)]
    pub build_settings: BTreeMap<String, AppleBuildSetting>,
    /// Configuration-specific overrides, keyed by names such as Debug/Release.
    #[serde(default)]
    pub configurations: BTreeMap<String, AppleBuildConfiguration>,
    /// Target ordering and libraries linked into this product.
    #[serde(default)]
    pub dependencies: Vec<AppleDependency>,
    /// Products embedded in this product; also imply a build dependency.
    #[serde(default)]
    pub embeds: Vec<AppleEmbed>,
    /// Ordered build scripts with an explicit phase position.
    #[serde(default)]
    pub scripts: Vec<AppleBuildScript>,
}

/// A source file compiled into an Apple target
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppleSource {
    /// Literal staged source or native expression for a generated source.
    pub path: AppleBuildPath,
    /// Xcode platform filters; empty applies to every destination.
    #[serde(default)]
    pub platform_filters: Vec<String>,
    /// Additional flags for this source only; argument boundaries are preserved.
    #[serde(default)]
    pub compiler_flags: Vec<String>,
}

/// Native resource processing, distinct from copying bytes into a bundle
///
/// Asset catalogs and storyboards require native compilation. Ordinary data
/// files and folder trees can instead use an explicit runtime destination.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AppleResource {
    /// Process a resource using the native toolchain's rules for its file type.
    Process {
        /// Project-relative resource, e.g. Assets.xcassets or Main.storyboard.
        path: ProjectPath,
    },
    /// Copy a file or directory without native resource compilation.
    Copy {
        /// Project-relative source and destination within the product's resource root.
        resource: Resource,
    },
}

/// An Apple target's build or link dependency
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AppleDependency {
    /// Depend on another product in this project.
    Target {
        /// Target ID in the same project.
        target: String,
        /// Also link its output; false is a build-order dependency only.
        link: bool,
        /// Weak-link this product when linking is enabled.
        #[serde(default)]
        weak: bool,
        /// Native platform filters; empty is unconditional.
        #[serde(default)]
        platform_filters: Vec<String>,
    },
    /// Link one product of a declared Swift package.
    SwiftProduct {
        /// ID in AppleProjectIr.swift_packages.
        package: String,
        /// Product exported by that package.
        product: String,
        /// Weak-link the resolved product, if supported by its product kind.
        #[serde(default)]
        weak: bool,
        /// Native platform filters; empty is unconditional.
        #[serde(default)]
        platform_filters: Vec<String>,
    },
    /// Link a system framework.
    SystemFramework {
        /// Framework name, including the `.framework` suffix.
        name: String,
        /// Use weak linking when this framework is optional at runtime.
        weak: bool,
        /// Native platform filters; empty is unconditional.
        #[serde(default)]
        platform_filters: Vec<String>,
    },
    /// Link an artifact produced by a build script, without staging it at generation time.
    BuildOutput {
        /// Native build path, for example $(BUILT_PRODUCTS_DIR)/Frameworks/Driver.framework.
        path: AppleBuildPath,
        /// Weak-link the artifact.
        #[serde(default)]
        weak: bool,
        /// Native platform filters.
        #[serde(default)]
        platform_filters: Vec<String>,
    },
    /// Link a staged library, framework, or XCFramework.
    File {
        /// Location in the generated project.
        path: ProjectPath,
        /// Whether to weak-link this file.
        #[serde(default)]
        weak: bool,
        /// Native platform filters; empty is unconditional.
        #[serde(default)]
        platform_filters: Vec<String>,
    },
}

/// A product copied into another product's bundle
///
/// Embedding is separate from linking. For example, an application embeds a
/// share extension without linking that extension's executable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppleEmbed {
    /// Product to embed; target references also imply a build dependency.
    pub source: AppleEmbedSource,
    /// Native platform filters; empty is unconditional.
    #[serde(default)]
    pub platform_filters: Vec<String>,
    /// Destination directory relative to the containing bundle root.
    pub destination: ProjectPath,
    /// Whether the embedded product must be signed on copy.
    pub code_sign_on_copy: bool,
    /// Strip headers from a copied framework when requested.
    #[serde(default)]
    pub remove_headers_on_copy: bool,
}

/// A Swift package location with an explicit revision requirement
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SwiftPackage {
    /// A package staged into the generated project.
    Local {
        /// Directory containing Package.swift.
        path: ProjectPath,
    },
    /// A remote package resolved by the native build tool.
    Remote {
        /// Repository URL.
        url: String,
        /// Explicit version, branch, or revision selection.
        requirement: SwiftRequirement,
    },
}

/// Selection rule for a remote Swift package
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SwiftRequirement {
    /// One exact semantic version.
    Exact(String),
    /// A minimum version, allowing compatible updates below the next major.
    UpToNextMajor(String),
    /// A minimum version, allowing updates below the next minor.
    UpToNextMinor(String),
    /// Half-open semantic-version range, lower inclusive and upper exclusive.
    Range {
        /// Inclusive lower bound.
        minimum: String,
        /// Exclusive upper bound.
        maximum: String,
    },
    /// A repository branch.
    Branch(String),
    /// A repository revision.
    Revision(String),
}

/// Scheme selection for building, running, and testing declared products
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppleScheme {
    /// Target IDs included in the build action.
    pub build_targets: Vec<String>,
    /// Optional executable target ID for the run action.
    pub run_target: Option<String>,
    /// Test target IDs for the test action.
    #[serde(default)]
    pub test_targets: Vec<String>,
    /// Configuration used by the run action.
    pub run_configuration: String,
    /// Configuration used by testing; None leaves the backend default unspecified.
    pub test_configuration: Option<String>,
    /// Configuration used by profiling.
    pub profile_configuration: Option<String>,
    /// Configuration used by static analysis.
    pub analyze_configuration: Option<String>,
    /// Per-action arguments, environment, and pre/post scripts.
    #[serde(default)]
    pub action_options: BTreeMap<AppleSchemeAction, AppleSchemeActionOptions>,
    /// Optional build-for overrides keyed by an ID already in build_targets.
    #[serde(default)]
    pub build_for: BTreeMap<String, AppleSchemeBuildFor>,
    /// Ordered references to staged .xctestplan files.
    #[serde(default)]
    pub test_plans: Vec<ProjectPath>,
    /// Default plan; must be present in test_plans when specified.
    pub default_test_plan: Option<ProjectPath>,
    /// Configuration used by the archive action.
    pub archive_configuration: String,
}

/// A target-local script with declared inputs and outputs
///
/// Literal paths refer to the generated project; expressions are expanded by
/// the native build system. Equal-position scripts retain declaration order.
/// Positions describe phase placement, not a guarantee of task execution order:
/// actual scheduling follows inputs/outputs. A backend rejects unsupported phases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppleBuildScript {
    /// Stable name within the target.
    pub name: String,
    /// Position relative to ordinary compiler/linker work.
    pub position: AppleScriptPosition,
    /// Shell executable, e.g. `/bin/sh`.
    pub shell: String,
    /// Script text interpreted by the specified shell.
    pub script: String,
    /// Inputs used for build-system dependency tracking.
    #[serde(default)]
    pub inputs: Vec<AppleBuildPath>,
    /// Outputs used for build-system dependency tracking.
    #[serde(default)]
    pub outputs: Vec<AppleBuildPath>,
    /// Input .xcfilelist references, expanded by the native build system.
    #[serde(default)]
    pub input_file_lists: Vec<AppleBuildPath>,
    /// Output .xcfilelist references.
    #[serde(default)]
    pub output_file_lists: Vec<AppleBuildPath>,
    /// Whether dependency analysis may skip this phase; None uses backend default.
    pub based_on_dependency_analysis: Option<bool>,
}

/// Placement of an Apple build script
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppleScriptPosition {
    /// Before source compilation.
    BeforeSources,
    /// After source compilation and before linking.
    BeforeLink,
    /// Placed after ordinary resource/embed phases; native dependencies govern execution.
    AfterResources,
}

/// A complete property-list value, including data and dates
///
/// Byte data uses a JSON byte array on the wire; a plist renderer encodes it as
/// base64. Dates use UTC RFC 3339 spelling. Native value validity (including
/// finite reals and valid dates) is the responsibility of the plist backend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PropertyListValue {
    /// UTF-8 string.
    String(String),
    /// Signed integer.
    Integer(i64),
    /// Finite floating-point number.
    Real(f64),
    /// Boolean.
    Boolean(bool),
    /// Raw bytes.
    Data(Vec<u8>),
    /// UTC date, e.g. `2026-01-01T00:00:00Z`.
    Date(String),
    /// Ordered values, including nested dictionaries.
    Array(Vec<PropertyListValue>),
    /// Dictionary entries ordered by key for deterministic serialization.
    Dict(BTreeMap<String, PropertyListValue>),
}

/// Scalar or ordered token-list Xcode build setting
///
/// Conditional keys such as SDK-qualified settings remain native strings.
/// Lists must match exactly during composition: reordering or deduplicating flags
/// can change their meaning. Build-setting expressions remain unexpanded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AppleBuildSetting {
    /// Scalar native value.
    String(String),
    /// Ordered native values, preserving argument boundaries and duplicates.
    List(Vec<String>),
}

/// One project/target configuration layer
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppleBuildConfiguration {
    /// Staged xcconfig file; includes and expressions are resolved by native tooling.
    pub xcconfig: Option<ProjectPath>,
    /// Inline settings applied over this layer's xcconfig.
    #[serde(default)]
    pub settings: BTreeMap<String, AppleBuildSetting>,
}

/// Literal staged path or a native build-setting path expression
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum AppleBuildPath {
    /// Project-relative path; JSON string form.
    Project(ProjectPath),
    /// Native expression, e.g. $(DERIVED_FILE_DIR)/Generated.swift; JSON object form.
    Expression {
        /// Expanded by native tools, never by CNG's staging path resolver.
        expression: String,
    },
}

/// Header file and its framework visibility
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppleHeader {
    /// Header source or generated build path.
    pub path: AppleBuildPath,
    /// Public, private, or project-only visibility.
    pub visibility: AppleHeaderVisibility,
}

/// Xcode header build-phase visibility
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppleHeaderVisibility {
    /// Exported public framework header.
    Public,
    /// Framework private header.
    Private,
    /// Used only while building this target.
    Project,
}

/// Source of an embedded product, independent of link dependencies
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AppleEmbedSource {
    /// Another target in this declaration.
    Target {
        /// Stable target ID.
        target: String,
    },
    /// Artifact produced by a declared build script rather than project staging.
    BuildOutput {
        /// Native path to the completed product.
        path: AppleBuildPath,
    },
    /// Staged dynamic framework, XCFramework, helper, or bundle.
    /// The backend selects the appropriate XCFramework slice before embedding.
    File {
        /// Path relative to the generated project.
        path: ProjectPath,
    },
    /// Dynamic product resolved from a declared Swift package.
    /// Native tooling decides whether the product is embeddable.
    SwiftProduct {
        /// Stable Swift package ID.
        package: String,
        /// Native package product name.
        product: String,
    },
}

/// Whether an Apple target emits a product or only coordinates build work
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppleTargetKind {
    /// Native product with a product_type identifier.
    #[default]
    Native,
    /// No product_type, compiled inputs, resources, or embedded products.
    Aggregate,
}

/// Native scheme action identity
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppleSchemeAction {
    /// Build action.
    Build,
    /// Run action.
    Run,
    /// Test action.
    Test,
    /// Profile action.
    Profile,
    /// Analyze action.
    Analyze,
    /// Archive action.
    Archive,
}

/// Action-local options; unsupported action/option combinations are backend errors
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppleSchemeActionOptions {
    /// Ordered arguments, preserving duplicates and value boundaries.
    #[serde(default)]
    pub arguments: Vec<String>,
    /// Environment values; no expansion by CNG.
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    /// Named scripts executed before this action, in declaration order.
    #[serde(default)]
    pub pre_actions: Vec<AppleSchemeScript>,
    /// Named scripts executed after this action, in declaration order.
    #[serde(default)]
    pub post_actions: Vec<AppleSchemeScript>,
}

/// A scheme pre/post action evaluated by native tooling
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppleSchemeScript {
    /// Stable identity in its pre/post list.
    pub name: String,
    /// Native shell script text.
    pub script: String,
    /// Optional target supplying the action's build-setting environment.
    pub environment_target: Option<String>,
}

/// Build-action participation; None preserves native defaults
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppleSchemeBuildFor {
    /// Build for running.
    pub running: Option<bool>,
    /// Build for testing.
    pub testing: Option<bool>,
    /// Build for profiling.
    pub profiling: Option<bool>,
    /// Build for archiving.
    pub archiving: Option<bool>,
    /// Build for analyzing.
    pub analyzing: Option<bool>,
}
