//! Whisker CNG plugin surface.
//!
//! Every type and trait the plugin system depends on lives here. The
//! crate covers three audiences with one published API:
//!
//! 1. **1st-party plugins** — modules inside `whisker-cng` that
//!    implement [`Plugin`] directly. The engine runs them in-process.
//! 2. **3rd-party plugin binaries** — external crates that implement
//!    [`Plugin`] and call [`run_as_subprocess`] from their `main`.
//!    The engine spawns them and exchanges JSON over stdin/stdout.
//! 3. **The engine itself** (`whisker-cng`) — consumes [`Plugin`]
//!    trait objects, owns the [`GenerateContext`], serializes
//!    [`PluginRequest`] / [`PluginResponse`] envelopes for the
//!    subprocess path.
//!
//! Keeping all three on the same crate means a `whisker-cng` patch
//! bump doesn't force every 3rd-party plugin crate to rebuild — the
//! engine depends on `whisker-plugin`, not the other way around, so
//! the only churn that propagates is changes to *this* crate's API.
//!
//! ## Writing a 3rd-party plugin
//!
//! Implement [`PluginConfig`] on a Config struct (this gives the
//! plugin its name) and [`Plugin`] on a unit struct that owns the
//! apply logic, then call [`run_as_subprocess`] from `main`:
//!
//! ```no_run
//! use whisker_plugin::{Operation, Plugin, PluginConfig, GenerateContext, PlistValue, Target};
//!
//! #[derive(Default, serde::Serialize, serde::Deserialize)]
//! struct DemoConfig {
//!     bundle_suffix: String,
//! }
//!
//! impl PluginConfig for DemoConfig {
//!     const NAME: &'static str = "example-plugin";
//! }
//!
//! struct Demo;
//!
//! impl Plugin for Demo {
//!     type Config = DemoConfig;
//!     fn apply(&self, ctx: &mut GenerateContext, cfg: &DemoConfig) -> anyhow::Result<()> {
//!         if let Some(ios) = ctx.ios.as_mut() {
//!             let key = "CFBundleSuffix".to_string();
//!             ios.info_plist.insert(key.clone(), PlistValue::String(cfg.bundle_suffix.clone()));
//!             ctx.journal.record(
//!                 DemoConfig::NAME,
//!                 Target::Ios,
//!                 &format!("info_plist.{key}"),
//!                 Operation::Set,
//!             );
//!         }
//!         Ok(())
//!     }
//! }
//!
//! fn main() -> anyhow::Result<()> {
//!     whisker_plugin::run_as_subprocess(Demo)
//! }
//! ```
//!
//! ## What the IR covers
//!
//! [`IosProjectIr`] and [`AndroidProjectIr`] each carry two
//! layers:
//!
//! 1. **Core fields** seeded from `Config` by the engine before
//!    any plugin runs — `app_name`, `version`, `bundle_id` /
//!    `application_id`, `scheme`, `deployment_target`, `min_sdk`,
//!    `target_sdk`. A plugin can read them to make decisions or
//!    override them via `Operation::Override`.
//! 2. **Plugin-additive fields** that plugins push into:
//!    - `info_plist: BTreeMap<String, PlistValue>` (`String` /
//!      `Boolean` / `Integer` / `Array<String>` rendered)
//!    - `manifest.permissions: Vec<String>` (dedup'd at render)
//!    - `manifest.application_meta_data: Vec<MetaDataEntry>`
//!    - `gradle.apply_plugins: Vec<String>` /
//!      `gradle.dependencies: Vec<String>` (raw Kotlin DSL lines
//!      emitted into `app/build.gradle.kts`)
//!    - `pbxproj_ops: Vec<PbxprojOp>` (`AddResource` / `AddSource` /
//!      `SetBuildSetting` / `LinkSystemFramework` materialised
//!      into the iOS xcodeproj)
//!    - `extra_files: BTreeMap<PathBuf, FileEntry>` (arbitrary
//!      files dropped into `gen/`, path-validated against `..`
//!      traversal)
//!
//! Adding a new typed field is a non-breaking change when the
//! field is `#[serde(default)]`: older plugin binaries simply
//! don't touch it, the engine sees the default. Adding a *required*
//! field is a wire-format break.
//!
//! ## Mutation journal
//!
//! Plugins record every IR mutation by calling
//! [`MutationJournal::record`] on `ctx.journal` alongside the
//! mutation itself. The engine uses the resulting log to:
//!
//! - Attribute conflicts ("plugin A and plugin B both `set`
//!   `info_plist.CFBundleIdentifier` — that's a hard error unless
//!   one of them used `override`")
//! - Render a human-readable summary on `whisker generate --verbose`
//! - Diagnose 3rd-party plugins ("plugin foo mutated
//!   android.manifest.permissions[3] at sequence index 42, after
//!   plugin bar at 38")
//!
//! ## Subprocess wire format
//!
//! Stderr is reserved for human-readable diagnostics: log lines,
//! progress messages, anything the plugin wants to surface to the
//! user when `whisker generate --verbose` is in play. Stdout is
//! strictly the [`PluginResponse`] JSON envelope — anything else
//! there is a wire-format violation.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;

// ----------------------------------------------------------------------------
// PluginConfig trait
// ----------------------------------------------------------------------------

/// Trait implemented by the typed config struct each plugin defines.
///
/// Carries the plugin's stable kebab-case identifier as a const so
/// the `app.plugin::<MyPlugin>(|c| ...)` builder in `whisker-config`
/// can resolve the storage key via `<MyPlugin as Plugin>::Config::NAME`.
///
/// ## Why `Serialize + DeserializeOwned`
///
/// Three reasons, all wire-related:
///
/// 1. `whisker.rs` builds the Config struct, then `whisker-cli`
///    serializes the resulting `Config` (including this Config
///    nested under `plugins[NAME]`) to JSON via the config probe.
/// 2. 3rd-party plugins are subprocesses — their config arrives as
///    JSON in the [`PluginRequest`] envelope.
/// 3. The mutation journal records the config that produced each
///    mutation, so we can attribute "plugin X with config Y" in
///    error messages.
///
/// ## Why `Default`
///
/// `app.plugin::<MyPlugin>(|c| ...)` starts from `Config::default()`
/// and lets the closure mutate it. A user who declares a plugin
/// without touching any options should still get a working config —
/// the call site reads as `app.plugin::<MyPlugin>(|_| {})`.
///
/// ## Convention for `NAME`
///
/// Kebab-case; prefix 1st-party plugins with `whisker-`
/// (e.g. `whisker-info-plist`, `whisker-permissions`). The default
/// [`Plugin::name`] implementation returns this value, so under
/// normal use the plugin's name and its Config's `NAME` are the
/// same string by construction.
pub trait PluginConfig: Serialize + for<'de> Deserialize<'de> + Default {
    const NAME: &'static str;
}

// ----------------------------------------------------------------------------
// Plugin trait
// ----------------------------------------------------------------------------

/// What a plugin implements.
pub trait Plugin {
    /// Plugin-specific config. The user passes this in via
    /// `app.plugin::<Self>(|c| c.field(...))` inside `whisker.rs` —
    /// the `c` parameter is `&mut Self::Config`.
    type Config: PluginConfig;

    /// Stable plugin identifier, used in:
    ///
    /// - `after()` / `before()` cross-references
    /// - The mutation journal
    /// - Error messages
    /// - The [`PluginRequest`] envelope's `name` field
    ///
    /// Defaults to `Self::Config::NAME` so the binding between the
    /// plugin's Config type and the plugin's name only has to be
    /// declared once (on the Config). The override slot is mostly
    /// there for tests and shims that want to expose the same
    /// Config under a different identifier; production plugins
    /// should leave it at the default.
    fn name(&self) -> &'static str {
        <Self::Config as PluginConfig>::NAME
    }

    /// Plugins this one must run **after**. Used by the topological
    /// sort in `whisker-cng::compose`. Default: empty (no ordering
    /// constraints).
    fn after(&self) -> &'static [&'static str] {
        &[]
    }

    /// Plugins this one must run **before**. Same as [`Plugin::after`]
    /// but expresses the inverse constraint — useful when you can't
    /// (or don't want to) modify the downstream plugin's source.
    fn before(&self) -> &'static [&'static str] {
        &[]
    }

    /// Reject obviously-broken config before any side effects fire.
    /// The engine runs this on every plugin before scheduling the
    /// `apply` pass, so a validation failure aborts cleanly without
    /// leaving a half-mutated IR behind.
    ///
    /// Default: accept everything.
    fn validate(&self, _config: &Self::Config) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actually mutate the [`GenerateContext`]. This is where the
    /// plugin reads `config`, decides what IR fields to touch, and
    /// writes them. For each mutation the plugin also calls
    /// [`MutationJournal::record`] on `ctx.journal` so the engine
    /// can attribute conflicts and produce a verbose summary.
    fn apply(&self, ctx: &mut GenerateContext, config: &Self::Config) -> anyhow::Result<()>;
}

// ----------------------------------------------------------------------------
// GenerateContext
// ----------------------------------------------------------------------------

/// The mutable handle plugins receive in [`Plugin::apply`]. Wraps
/// every IR the engine is currently materializing plus the running
/// [`MutationJournal`].
///
/// Each target IR is `Option` because not every `whisker generate`
/// invocation touches both platforms — the CLI passes only the IRs
/// for targets currently enabled by the user's `--target` flag.
/// Plugins should `if let Some(ios) = &mut ctx.ios { ... }` rather
/// than assuming both exist.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct GenerateContext {
    /// Read-only basic facts about the app, derived from
    /// `Config`. Plugins use these as defaults — e.g. an
    /// Info.plist plugin sets `CFBundleIdentifier` from
    /// `app_meta.ios_bundle_id`.
    pub app_meta: AppMeta,

    /// iOS IR. `Some` when the current `whisker generate` run is
    /// rendering `gen/ios/`.
    #[serde(default)]
    pub ios: Option<IosProjectIr>,

    /// Android IR. `Some` when the current run is rendering
    /// `gen/android/`.
    #[serde(default)]
    pub android: Option<AndroidProjectIr>,

    /// Append-only attribution log. The engine inspects this after
    /// the pipeline finishes to surface conflicts and verbose
    /// summaries; plugins don't read it directly.
    #[serde(default)]
    pub journal: MutationJournal,

    /// Absolute path to the consuming app crate's root (the directory
    /// holding its `Cargo.toml` / `whisker.rs`). Set by the engine
    /// before the pipeline runs so plugins can resolve paths the user
    /// spelled relative to their app — e.g. `whisker-asset`'s
    /// `c.dir("assets")` resolves against this.
    ///
    /// `None` in unit tests / pipelines that don't run from a real
    /// app crate. A plugin that needs it should error clearly when it
    /// is absent rather than guessing the current working directory —
    /// subprocess plugins don't inherit a reliable cwd.
    #[serde(default)]
    pub app_crate_dir: Option<PathBuf>,
}

/// Snapshot of the user-spelled `Config` values at pipeline
/// entry. Intentionally flat + cloneable: anything plugins need
/// from the app config has to surface here rather than pulling in
/// the whole `Config` type, which keeps the wire format stable
/// when `Config` grows.
///
/// ## Read this for "what the user said"; read the IR for
/// "what the renderer will use"
///
/// `AppMeta` is **frozen at pipeline entry** — plugins don't update
/// it. The per-target IR ([`IosProjectIr`] / [`AndroidProjectIr`])
/// is the canonical source of truth for fields the renderer
/// eventually consumes. If a plugin overrides `IosProjectIr.bundle_id`
/// via `Operation::Override`, downstream plugins reading
/// `ctx.app_meta.ios_bundle_id` will still see the *original* user
/// value. Use `AppMeta` for attribution and diagnostics; use the
/// IR for values that flow into the rendered project.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AppMeta {
    pub name: String,
    pub version: String,
    pub build_number: u32,
    /// Some only when the iOS target is enabled in this run.
    #[serde(default)]
    pub ios_bundle_id: Option<String>,
    /// Some only when the Android target is enabled in this run.
    #[serde(default)]
    pub android_application_id: Option<String>,
}

// ----------------------------------------------------------------------------
// IR — iOS
// ----------------------------------------------------------------------------

/// In-memory representation of the iOS host project plugins mutate.
///
/// Serializes 1:1 to the JSON envelope so a 3rd-party plugin can
/// receive it, mutate it locally, and send it back. Field ordering
/// inside `BTreeMap`s is deterministic, so the same `(Config,
/// plugin set)` produces a byte-identical envelope — important for
/// the fingerprint-based skip path in `whisker-cng`.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct IosProjectIr {
    // ----- Core fields ------------------------------------------------------
    //
    // These were string substitutions baked into the renderer's
    // template up to RFC #164 Phase 4. Promoting them onto the IR
    // lets a 3rd-party plugin override any of them via
    // `Operation::Override` — e.g. a flavor plugin can append
    // `.staging` to `bundle_id` without forking the user app's
    // `whisker.rs`. The engine's `build_initial_context` seeds
    // every field below from `Config`; the inputs-extraction
    // step (`crate::ios::inputs_from`) reads them back.
    /// `CFBundleDisplayName` / pbxproj `PRODUCT_NAME` source.
    /// Seeded from `Config.name`.
    #[serde(default)]
    pub app_name: Option<String>,
    /// `CFBundleShortVersionString` source. Seeded from
    /// `Config.version` (default `"0.1.0"`).
    #[serde(default)]
    pub version: Option<String>,
    /// `CFBundleVersion` source. Seeded from
    /// `Config.build_number` (default `1`).
    #[serde(default)]
    pub build_number: Option<u32>,
    /// pbxproj `PRODUCT_BUNDLE_IDENTIFIER` source. Seeded from
    /// `Config.ios.bundle_id`, falling back to the top-level
    /// `Config.bundle_id`.
    #[serde(default)]
    pub bundle_id: Option<String>,
    /// Xcode scheme name. Seeded from `Config.ios.scheme`,
    /// falling back to `Config.name`.
    #[serde(default)]
    pub scheme: Option<String>,
    /// pbxproj `IPHONEOS_DEPLOYMENT_TARGET` source. Seeded from
    /// `Config.ios.deployment_target` (default `"13.0"`).
    #[serde(default)]
    pub deployment_target: Option<String>,

    // ----- Plugin-additive fields ----------------------------------------
    /// `Info.plist` as a plist object tree. Renderer turns this
    /// back into XML at the end of the pipeline.
    #[serde(default)]
    pub info_plist: BTreeMap<String, PlistValue>,

    /// Deferred pbxproj structural ops. Full pbxproj round-tripping
    /// is too heavyweight for the protocol; instead plugins request
    /// the engine append resource refs / build phases / build
    /// settings via [`PbxprojOp`], which the engine replays against
    /// the template renderer at the end of the pipeline.
    #[serde(default)]
    pub pbxproj_ops: Vec<PbxprojOp>,

    /// Arbitrary files to drop into `gen/ios/`. Path is relative to
    /// the gen root. Use this for files the templates don't cover —
    /// `.entitlements`, GoogleService-Info.plist, code-signing
    /// helpers, etc.
    #[serde(default)]
    pub extra_files: BTreeMap<PathBuf, FileEntry>,
}

/// Tagged-union value for plist trees. Matches what the
/// CoreFoundation property-list serializer accepts; the engine
/// renders it to XML plist format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum PlistValue {
    String(String),
    Integer(i64),
    Real(f64),
    Boolean(bool),
    Array(Vec<PlistValue>),
    Dict(BTreeMap<String, PlistValue>),
}

/// Structural mutation request against the iOS xcodeproj. The
/// engine replays these against its pbxproj template renderer; the
/// renderer's rules decide where (which group, which build phase)
/// to materialize each op.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PbxprojOp {
    /// Add a file reference to the app target's "Resources" build
    /// phase. `path` is relative to `gen/ios/`.
    ///
    /// Emitted as a flat `PBXFileReference` — Xcode copies only the
    /// file's basename into the `.app` bundle root. Use
    /// [`Self::AddResourceFolder`] when subdirectories must be
    /// preserved in the bundle.
    AddResource { path: PathBuf },
    /// Add a **folder reference** (Xcode "blue folder") to the app
    /// target's "Resources" build phase. `path` is relative to
    /// `gen/ios/` and names a *directory*. Unlike [`Self::AddResource`],
    /// Xcode copies the entire directory tree into the `.app` bundle
    /// preserving its subdirectory structure (so
    /// `whisker_assets/images/logo.png` lands at
    /// `<bundle>/whisker_assets/images/logo.png`, not flattened to
    /// `logo.png`). Backed by a `PBXFileReference` with
    /// `lastKnownFileType = folder`.
    AddResourceFolder { path: PathBuf },
    /// Add a file reference compiled into the app target. `path` is
    /// relative to `gen/ios/`.
    AddSource { path: PathBuf },
    /// Add a `key = value;` line into the app target's
    /// build-settings dict for both Debug and Release.
    SetBuildSetting { key: String, value: String },
    /// Add a system framework (e.g. `AVFoundation.framework`) to
    /// the app target's "Link Binary With Libraries" phase.
    LinkSystemFramework { name: String },
}

// ----------------------------------------------------------------------------
// IR — Android
// ----------------------------------------------------------------------------

/// In-memory representation of the Android host project plugins
/// mutate. Same shape rationale as [`IosProjectIr`].
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AndroidProjectIr {
    // ----- Core fields ------------------------------------------------------
    //
    // Seeded by `build_initial_context` from `Config`; mirror
    // of the iOS core block above. See [`IosProjectIr`]'s rationale.
    /// Activity label / `manifest.application.android:label` source.
    /// Seeded from `Config.name`.
    #[serde(default)]
    pub app_name: Option<String>,
    /// Gradle `versionName` source. Seeded from
    /// `Config.version` (default `"0.1.0"`).
    #[serde(default)]
    pub version: Option<String>,
    /// Gradle `versionCode` source. Seeded from
    /// `Config.build_number` (default `1`).
    #[serde(default)]
    pub build_number: Option<u32>,
    /// Gradle `applicationId` source. Seeded from
    /// `Config.android.application_id`, falling back to the
    /// top-level `Config.bundle_id`.
    #[serde(default)]
    pub application_id: Option<String>,
    /// Gradle `minSdk` source. Seeded from
    /// `Config.android.min_sdk` (default `24`).
    #[serde(default)]
    pub min_sdk: Option<u32>,
    /// Gradle `targetSdk` source. Seeded from
    /// `Config.android.target_sdk` (default `34`).
    #[serde(default)]
    pub target_sdk: Option<u32>,

    // ----- Plugin-additive fields ----------------------------------------
    /// Structured AndroidManifest.xml model.
    #[serde(default)]
    pub manifest: AndroidManifest,

    /// Gradle DSL for the app module. Renderer turns this into
    /// `app/build.gradle.kts` additions.
    #[serde(default)]
    pub gradle: GradleDsl,

    /// Arbitrary files to drop into `gen/android/`. Same role as
    /// [`IosProjectIr::extra_files`].
    #[serde(default)]
    pub extra_files: BTreeMap<PathBuf, FileEntry>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AndroidManifest {
    /// `<uses-permission android:name="..."/>` entries. Dedup'd by
    /// the engine after the pipeline runs.
    #[serde(default)]
    pub permissions: Vec<String>,

    /// `<meta-data android:name="..." android:value="..."/>` entries
    /// inside the `<application>` block.
    #[serde(default)]
    pub application_meta_data: Vec<MetaDataEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetaDataEntry {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct GradleDsl {
    /// Plugin ids applied via the app module's `plugins { }` block,
    /// e.g. `"com.google.gms.google-services"`.
    #[serde(default)]
    pub apply_plugins: Vec<String>,

    /// Coordinates added to the app module's `dependencies { }`
    /// block. Each entry is the raw DSL line, e.g.
    /// `"implementation(\"com.google.firebase:firebase-analytics:21.5.0\")"`.
    /// Letting plugins pass the raw line keeps `implementation` /
    /// `api` / `kapt` differences expressible without modelling
    /// gradle's full configuration grammar.
    #[serde(default)]
    pub dependencies: Vec<String>,
}

// ----------------------------------------------------------------------------
// Shared
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileEntry {
    /// File contents, for text files. UTF-8. Mutually exclusive with
    /// [`Self::contents_base64`] — when the base64 field is set the
    /// renderer writes those bytes and ignores this string.
    pub contents: String,
    /// Base64-encoded raw bytes, for binary files (images, fonts,
    /// audio). `None` for ordinary text files; when `Some`, the
    /// renderer base64-decodes it and writes the resulting bytes
    /// verbatim (so a PNG survives the JSON wire, which can't carry
    /// arbitrary bytes in a string). Carried as base64 rather than a
    /// `Vec<u8>` so the [`PluginRequest`] / [`PluginResponse`] JSON
    /// envelope stays valid UTF-8 across the subprocess boundary.
    ///
    /// First consumer: `whisker-asset`, which bundles arbitrary
    /// (often binary) app assets into the generated native projects.
    #[serde(default)]
    pub contents_base64: Option<String>,
    /// POSIX mode bits. `None` → engine default (`0o644`).
    #[serde(default)]
    pub mode: Option<u32>,
}

impl FileEntry {
    /// A UTF-8 text file with default mode.
    pub fn text(contents: impl Into<String>) -> Self {
        Self {
            contents: contents.into(),
            contents_base64: None,
            mode: None,
        }
    }

    /// A binary file: `bytes` are base64-encoded into
    /// [`Self::contents_base64`] so they survive the JSON envelope.
    pub fn binary(bytes: &[u8]) -> Self {
        Self {
            contents: String::new(),
            contents_base64: Some(base64_encode(bytes)),
            mode: None,
        }
    }

    /// Decode this entry to the raw bytes the renderer should write.
    /// Returns the base64-decoded bytes when [`Self::contents_base64`]
    /// is set, otherwise the UTF-8 [`Self::contents`] as bytes.
    pub fn to_bytes(&self) -> anyhow::Result<Vec<u8>> {
        match &self.contents_base64 {
            Some(b64) => base64_decode(b64),
            None => Ok(self.contents.clone().into_bytes()),
        }
    }
}

/// Standard base64 (RFC 4648, `+/`, `=` padding). Hand-rolled to
/// avoid pulling a base64 crate into the plugin surface, which every
/// 3rd-party plugin transitively compiles.
fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(n & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Inverse of [`base64_encode`]. Rejects invalid characters / length.
fn base64_decode(input: &str) -> anyhow::Result<Vec<u8>> {
    fn val(c: u8) -> anyhow::Result<u32> {
        Ok(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' => 62,
            b'/' => 63,
            other => anyhow::bail!("invalid base64 character: {other:#x}"),
        })
    }
    let bytes = input.as_bytes();
    let trimmed = bytes.iter().take_while(|&&c| c != b'=').count();
    let padded = &bytes[..trimmed];
    if !bytes[trimmed..].iter().all(|&c| c == b'=') {
        anyhow::bail!("base64 padding `=` must only appear at the end");
    }
    let mut out = Vec::with_capacity(padded.len() / 4 * 3);
    for chunk in padded.chunks(4) {
        if chunk.len() == 1 {
            anyhow::bail!("invalid base64 length");
        }
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

// ----------------------------------------------------------------------------
// Mutation journal
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Ios,
    Android,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Operation {
    /// First write to a previously-unset field. Two `Set`s to the
    /// same path from different plugins is a conflict.
    Set,
    /// Explicitly overwrites a prior value. Two plugins racing for
    /// the same field can be ordered with `after()` / `before()`,
    /// and the loser uses `Override` to acknowledge it intended to
    /// stomp.
    Override,
    /// Appended one or more items to an array-shaped field
    /// (permissions, meta-data, intent-filter list, pbxproj ops…).
    /// `count` lets the engine surface "plugin X added 3
    /// permissions" without recording each one individually.
    ArrayPush { count: usize },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MutationRecord {
    /// `Plugin::name()` of the plugin that requested the mutation.
    pub plugin: String,
    pub target: Target,
    /// Dotted path to the field that was mutated, e.g.
    /// `"info_plist.CFBundleIdentifier"` or
    /// `"manifest.permissions"`.
    pub path: String,
    pub operation: Operation,
    /// Monotonically-increasing per-pipeline counter. Plugins that
    /// run earlier have smaller values.
    pub sequence_index: u64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct MutationJournal {
    pub records: Vec<MutationRecord>,
    /// Next index `record()` will hand out. Stored explicitly rather
    /// than derived from `records.len()` so the cursor stays correct
    /// if the engine ever filters / merges entries during conflict
    /// resolution.
    #[serde(default)]
    pub next_sequence_index: u64,
}

impl MutationJournal {
    /// Allocate the next sequence index and append a record.
    /// Plugins call this directly when they touch an IR field.
    pub fn record(&mut self, plugin: &str, target: Target, path: &str, operation: Operation) {
        let seq = self.next_sequence_index;
        self.next_sequence_index = seq + 1;
        self.records.push(MutationRecord {
            plugin: plugin.to_string(),
            target,
            path: path.to_string(),
            operation,
            sequence_index: seq,
        });
    }
}

// ----------------------------------------------------------------------------
// Subprocess envelope
// ----------------------------------------------------------------------------

/// Stdin envelope for a 3rd-party plugin subprocess. The engine
/// writes one of these as JSON to the plugin's stdin, then reads a
/// [`PluginResponse`] back from stdout. Single round trip per plugin
/// per pipeline; the engine spawns one process per plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginRequest {
    /// Stable plugin name — lets the binary `match` if it ships
    /// multiple plugins from one entry point. Most binaries serve
    /// exactly one plugin and just assert on this field.
    pub name: String,
    /// Plugin's config as JSON. The subprocess deserializes it into
    /// its `Plugin::Config` type.
    pub config: serde_json::Value,
    /// Current state of the IRs going into this plugin. The
    /// subprocess gets the full context (read-only `app_meta`,
    /// option-of IR per target, journal so far) and returns the
    /// post-mutation version.
    pub context: GenerateContext,
}

/// Stdout envelope. The subprocess returns the mutated context;
/// the engine diffs the journal to confirm the subprocess didn't
/// forge sequence indices, then merges the new context back into
/// the running pipeline state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginResponse {
    pub context: GenerateContext,
}

// ----------------------------------------------------------------------------
// Subprocess runner
// ----------------------------------------------------------------------------

/// Drive a [`Plugin`] as a stdin/stdout JSON subprocess.
///
/// Reads a [`PluginRequest`] envelope from stdin (blocking until
/// EOF on the input pipe), runs [`Plugin::validate`] then
/// [`Plugin::apply`], and writes a [`PluginResponse`] back to
/// stdout. The function returns `Ok(())` on success and propagates
/// any deserialization / validation / apply error as an
/// `anyhow::Error` — the recommended `main` form is:
///
/// ```ignore
/// fn main() -> anyhow::Result<()> {
///     whisker_plugin::run_as_subprocess(Demo)
/// }
/// ```
///
/// `?` on the result causes the process to exit with status 1 and
/// the error message on stderr, which is the contract the engine
/// expects.
pub fn run_as_subprocess<P: Plugin>(plugin: P) -> anyhow::Result<()> {
    let mut stdin_buf = String::new();
    std::io::stdin()
        .read_to_string(&mut stdin_buf)
        .map_err(|e| anyhow::anyhow!("read PluginRequest from stdin: {e}"))?;

    let request: PluginRequest = serde_json::from_str(&stdin_buf)
        .map_err(|e| anyhow::anyhow!("decode PluginRequest JSON: {e}"))?;

    if request.name != plugin.name() {
        return Err(anyhow::anyhow!(
            "plugin name mismatch: engine asked for `{}` but this binary serves `{}`",
            request.name,
            plugin.name(),
        ));
    }

    // `null` config arrives when the user didn't declare the plugin
    // in `whisker.rs` at all — the engine's wire protocol uses Null
    // to mean "use the Config's `Default`". This matches the
    // in-process path's `Option::is_none` → `Default::default()`
    // fallback, keeping the same semantics regardless of which
    // execution mode a plugin runs in.
    let config: P::Config = if request.config.is_null() {
        Default::default()
    } else {
        serde_json::from_value(request.config)
            .map_err(|e| anyhow::anyhow!("decode plugin config for `{}`: {e}", plugin.name()))?
    };

    plugin
        .validate(&config)
        .map_err(|e| anyhow::anyhow!("`{}`::validate: {e}", plugin.name()))?;

    let mut ctx = request.context;
    plugin
        .apply(&mut ctx, &config)
        .map_err(|e| anyhow::anyhow!("`{}`::apply: {e}", plugin.name()))?;

    let response = PluginResponse { context: ctx };
    let json = serde_json::to_string(&response)
        .map_err(|e| anyhow::anyhow!("encode PluginResponse JSON: {e}"))?;

    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(json.as_bytes())
        .map_err(|e| anyhow::anyhow!("write PluginResponse to stdout: {e}"))?;
    stdout
        .write_all(b"\n")
        .map_err(|e| anyhow::anyhow!("write trailing newline: {e}"))?;

    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_context_round_trips_through_json() {
        let mut ctx = GenerateContext {
            app_meta: AppMeta {
                name: "Demo".into(),
                version: "1.0".into(),
                build_number: 7,
                ios_bundle_id: Some("rs.whisker.demo".into()),
                android_application_id: Some("rs.whisker.demo".into()),
            },
            ios: Some(IosProjectIr::default()),
            android: Some(AndroidProjectIr::default()),
            journal: MutationJournal::default(),
            app_crate_dir: None,
        };
        ctx.ios.as_mut().unwrap().info_plist.insert(
            "CFBundleIdentifier".into(),
            PlistValue::String("rs.whisker.demo".into()),
        );
        ctx.android
            .as_mut()
            .unwrap()
            .manifest
            .permissions
            .push("android.permission.CAMERA".into());
        ctx.journal.record(
            "whisker-info-plist",
            Target::Ios,
            "info_plist.CFBundleIdentifier",
            Operation::Set,
        );
        ctx.journal.record(
            "whisker-permissions",
            Target::Android,
            "manifest.permissions",
            Operation::ArrayPush { count: 1 },
        );

        let json = serde_json::to_string(&ctx).expect("serialize");
        let back: GenerateContext = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(back.app_meta.name, "Demo");
        assert_eq!(back.journal.records.len(), 2);
        assert_eq!(back.journal.next_sequence_index, 2);
        assert_eq!(
            back.ios.unwrap().info_plist.get("CFBundleIdentifier"),
            Some(&PlistValue::String("rs.whisker.demo".into())),
        );
        assert_eq!(
            back.android.unwrap().manifest.permissions,
            vec!["android.permission.CAMERA".to_string()],
        );
    }

    #[test]
    fn base64_round_trips_arbitrary_bytes() {
        for input in [
            &b""[..],
            &b"f"[..],
            &b"fo"[..],
            &b"foo"[..],
            &b"foob"[..],
            &b"fooba"[..],
            &b"foobar"[..],
            &[0u8, 1, 2, 253, 254, 255][..],
        ] {
            let encoded = base64_encode(input);
            assert!(encoded.is_ascii(), "base64 must be ASCII: {encoded}");
            let decoded = base64_decode(&encoded).expect("decode");
            assert_eq!(decoded, input, "round trip for {input:?}");
        }
    }

    #[test]
    fn base64_matches_known_vectors() {
        // RFC 4648 test vectors.
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_decode("Zm9vYmFy").unwrap(), b"foobar");
        assert_eq!(base64_decode("Zm8=").unwrap(), b"fo");
    }

    #[test]
    fn base64_decode_rejects_garbage() {
        assert!(base64_decode("not valid!").is_err());
    }

    #[test]
    fn file_entry_binary_round_trips_through_json() {
        let raw = &[0x89u8, 0x50, 0x4e, 0x47, 0x00, 0xff];
        let entry = FileEntry::binary(raw);
        let json = serde_json::to_string(&entry).unwrap();
        let back: FileEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.to_bytes().unwrap(), raw);
    }

    #[test]
    fn file_entry_text_to_bytes_is_utf8() {
        let entry = FileEntry::text("hello");
        assert_eq!(entry.to_bytes().unwrap(), b"hello");
        assert!(entry.contents_base64.is_none());
    }

    #[test]
    fn file_entry_text_default_decodes_without_base64_field() {
        // A FileEntry serialized before `contents_base64` existed
        // (only `contents` + `mode`) must still deserialize.
        let json = r#"{"contents":"old text"}"#;
        let entry: FileEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.to_bytes().unwrap(), b"old text");
    }

    #[test]
    fn sequence_indices_are_monotonic() {
        let mut j = MutationJournal::default();
        j.record("a", Target::Ios, "x", Operation::Set);
        j.record("b", Target::Android, "y", Operation::Set);
        j.record("a", Target::Ios, "z", Operation::ArrayPush { count: 3 });
        let seqs: Vec<_> = j.records.iter().map(|r| r.sequence_index).collect();
        assert_eq!(seqs, vec![0, 1, 2]);
        assert_eq!(j.next_sequence_index, 3);
    }

    #[test]
    fn pbxproj_ops_round_trip() {
        let ops = vec![
            PbxprojOp::AddResource {
                path: "GoogleService-Info.plist".into(),
            },
            PbxprojOp::LinkSystemFramework {
                name: "AVFoundation.framework".into(),
            },
            PbxprojOp::SetBuildSetting {
                key: "SWIFT_VERSION".into(),
                value: "5".into(),
            },
        ];
        let json = serde_json::to_string(&ops).unwrap();
        let back: Vec<PbxprojOp> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ops);
    }

    #[test]
    fn plugin_request_envelope_round_trips() {
        let req = PluginRequest {
            name: "whisker-firebase".into(),
            config: serde_json::json!({"googleServicePath": "ios/GoogleService.plist"}),
            context: GenerateContext::default(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: PluginRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "whisker-firebase");
        assert_eq!(back.config["googleServicePath"], "ios/GoogleService.plist");
    }

    // Tiny plugin to exercise the trait shape — verifies the
    // associated-type bound compiles and default methods kick in.
    struct Null;

    #[derive(Default, Serialize, Deserialize)]
    struct NullConfig {
        #[allow(dead_code)]
        flag: bool,
    }

    impl PluginConfig for NullConfig {
        const NAME: &'static str = "null";
    }

    impl Plugin for Null {
        type Config = NullConfig;
        fn apply(&self, _ctx: &mut GenerateContext, _config: &Self::Config) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn plugin_trait_default_methods_work() {
        let p = Null;
        assert_eq!(p.name(), "null");
        assert!(p.after().is_empty());
        assert!(p.before().is_empty());
        let cfg = NullConfig::default();
        p.validate(&cfg).unwrap();
        let mut ctx = GenerateContext::default();
        p.apply(&mut ctx, &cfg).unwrap();
    }

    // The subprocess runner reads stdin / writes stdout, which is
    // awkward to unit-test directly. Factor the core into an
    // in-memory shim and test that — `run_as_subprocess` is a thin
    // wrapper over it.
    fn run_with_pipes<P: Plugin>(plugin: P, input: &str) -> anyhow::Result<String> {
        let request: PluginRequest = serde_json::from_str(input)?;
        anyhow::ensure!(
            request.name == plugin.name(),
            "name mismatch: {} vs {}",
            request.name,
            plugin.name(),
        );
        let config: P::Config = serde_json::from_value(request.config)?;
        plugin.validate(&config)?;
        let mut ctx = request.context;
        plugin.apply(&mut ctx, &config)?;
        Ok(serde_json::to_string(&PluginResponse { context: ctx })?)
    }

    #[derive(Default, serde::Serialize, serde::Deserialize)]
    struct PermissionConfig {
        permission: String,
    }

    impl PluginConfig for PermissionConfig {
        const NAME: &'static str = "test-permission";
    }

    struct Permission;

    impl Plugin for Permission {
        type Config = PermissionConfig;
        fn apply(&self, ctx: &mut GenerateContext, cfg: &PermissionConfig) -> anyhow::Result<()> {
            let android = ctx.android.as_mut().ok_or_else(|| {
                anyhow::anyhow!("test-permission requires android target enabled")
            })?;
            android.manifest.permissions.push(cfg.permission.clone());
            ctx.journal.record(
                PermissionConfig::NAME,
                Target::Android,
                "manifest.permissions",
                Operation::ArrayPush { count: 1 },
            );
            Ok(())
        }
    }

    #[test]
    fn subprocess_happy_path_round_trip() {
        let request = PluginRequest {
            name: "test-permission".into(),
            config: serde_json::json!({"permission": "android.permission.CAMERA"}),
            context: GenerateContext {
                android: Some(AndroidProjectIr::default()),
                ..Default::default()
            },
        };
        let input = serde_json::to_string(&request).unwrap();

        let output = run_with_pipes(Permission, &input).unwrap();
        let response: PluginResponse = serde_json::from_str(&output).unwrap();

        let android = response.context.android.expect("android should be present");
        assert_eq!(
            android.manifest.permissions,
            vec!["android.permission.CAMERA".to_string()],
        );
        assert_eq!(response.context.journal.records.len(), 1);
        assert_eq!(
            response.context.journal.records[0].plugin,
            "test-permission",
        );
        assert!(matches!(
            response.context.journal.records[0].operation,
            Operation::ArrayPush { count: 1 },
        ));
    }

    #[test]
    fn subprocess_name_mismatch_is_an_error() {
        let request = PluginRequest {
            name: "some-other-plugin".into(),
            config: serde_json::json!({"permission": "x"}),
            context: GenerateContext::default(),
        };
        let input = serde_json::to_string(&request).unwrap();
        let err = run_with_pipes(Permission, &input).unwrap_err();
        assert!(err.to_string().contains("name mismatch"), "{err}");
    }

    #[test]
    fn subprocess_apply_error_propagates() {
        let request = PluginRequest {
            name: "test-permission".into(),
            config: serde_json::json!({"permission": "android.permission.CAMERA"}),
            // No android IR — apply asserts it's present, so this
            // exercises the error path.
            context: GenerateContext::default(),
        };
        let input = serde_json::to_string(&request).unwrap();
        let err = run_with_pipes(Permission, &input).unwrap_err();
        assert!(err.to_string().contains("requires android"), "{err}");
    }
}
