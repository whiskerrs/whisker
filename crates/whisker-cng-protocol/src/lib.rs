//! Stable wire protocol shared between `whisker-cng` (the engine
//! that runs plugins) and 1st- / 3rd-party plugin binaries.
//!
//! ## Why this crate exists separately
//!
//! `whisker-cng` is the project generator: it takes an `AppConfig`,
//! runs a pipeline of plugins, and writes `gen/{android,ios}/` to
//! disk. Plugins mutate a typed IR — `IosProjectIr` /
//! `AndroidProjectIr` — instead of editing the rendered files as
//! strings. 1st-party plugins are linked in-process; 3rd-party
//! plugins are subprocesses launched by `whisker-cng` that exchange
//! `PluginRequest` / `PluginResponse` over stdin/stdout JSON.
//!
//! Both surfaces — the in-process [`Plugin`] trait and the JSON
//! envelope — live here so a 3rd-party plugin crate can depend
//! on `whisker-cng-protocol` *without* pulling in `whisker-cng`'s
//! heavy machinery (template engine, fingerprinting, pbxproj
//! renderer, etc.). The split lets us bump `whisker-cng` patch
//! versions without forcing every plugin crate to rebuild.
//!
//! ## What the IR covers — current scope (Phase 1)
//!
//! The IRs are intentionally minimal in this first cut. They expose
//! the surfaces RFC #164's Phase 2 built-in plugins actually need —
//! Info.plist key/value tree, AndroidManifest's
//! permissions/intent-filter set, gradle plugin/dependency lists,
//! and a free-form `extra_files` bag for everything else (resources,
//! source files, etc.). Pbxproj structural edits are tracked as a
//! deferred operation list; the engine replays them against the
//! template renderer rather than the protocol carrying a full
//! pbxproj object graph.
//!
//! Adding a new typed field to an IR is a non-breaking change if
//! the field is `#[serde(default)]`: older plugin binaries simply
//! don't touch it, the engine sees the default. Adding a *required*
//! field is a wire-format break.
//!
//! ## Mutation journal
//!
//! Every mutation a plugin makes through the [`GenerateContext`] is
//! recorded in [`MutationJournal`]. The engine uses it to:
//!
//! - Attribute conflicts ("plugin A and plugin B both `set`
//!   `info_plist.CFBundleIdentifier` — that's a hard error unless
//!   one of them used `override`")
//! - Render a human-readable summary on `whisker generate --verbose`
//! - Diagnose 3rd-party plugins ("plugin foo mutated
//!   android.manifest.permissions[3] at sequence index 42, after
//!   plugin bar at 38")
//!
//! Plugin code never touches the journal directly — it's wired
//! through helper methods on the IR types so the engine records
//! every operation automatically.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

// ----------------------------------------------------------------------------
// Plugin trait
// ----------------------------------------------------------------------------

/// In-process plugin surface. 1st-party plugins (the ones bundled
/// inside `whisker-cng`) implement this directly; 3rd-party plugin
/// binaries implement [`Plugin`] in their own crate and ship the
/// implementation via the [`run_as_subprocess`] helper in the
/// `whisker-plugin` crate (which wires the trait up to stdin/stdout
/// JSON).
///
/// ## Why `Config` is `Serialize + DeserializeOwned`
///
/// Three reasons, all wire-related:
///
/// 1. The user's `whisker.rs` declares plugin config as a Rust
///    struct via `app.plugin::<MyPluginCfg>(|cfg| ...)`. That value
///    has to ride along through the config probe → whisker-cli →
///    whisker-cng path, all of which use JSON.
/// 2. 3rd-party plugins are subprocesses — their config arrives as
///    JSON in the stdin envelope.
/// 3. The mutation journal records the config that produced each
///    mutation, so we can attribute "plugin X with config Y" in
///    error messages.
///
/// `Default` is required so config blocks omitted in `whisker.rs`
/// fall back to a sensible value instead of erroring.
pub trait Plugin {
    /// Plugin-specific config. The user passes this in via
    /// `app.plugin::<Cfg>(|c| c.field(...))` inside `whisker.rs`.
    type Config: Serialize + for<'de> Deserialize<'de> + Default;

    /// Stable plugin identifier, used in:
    ///
    /// - The `[package.metadata.whisker.plugins.<name>]` declaration
    ///   in a user crate's `Cargo.toml`
    /// - `after()` / `before()` cross-references
    /// - The mutation journal
    /// - Error messages
    ///
    /// Convention: kebab-case, prefix 1st-party plugins with
    /// `whisker-` (e.g. `whisker-info-plist`, `whisker-permissions`).
    fn name(&self) -> &'static str;

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
    /// calls into the IR's methods. The journal entries are added
    /// automatically by those methods — plugins never construct
    /// `MutationRecord`s by hand.
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
    /// `AppConfig`. Plugins use these as defaults — e.g. an
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
}

/// Subset of `AppConfig` plugins are allowed to read. Intentionally
/// flat + cloneable: anything plugins need from the app config has
/// to surface here rather than pulling in the whole `AppConfig`
/// type, which keeps the wire format stable when `AppConfig` grows.
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
/// inside `BTreeMap`s is deterministic, so the same `(AppConfig,
/// plugin set)` produces a byte-identical envelope — important for
/// the fingerprint-based skip path in `whisker-cng`.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct IosProjectIr {
    /// `Info.plist` as a plist object tree. Renderer turns this
    /// back into XML at the end of the pipeline.
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
    AddResource { path: PathBuf },
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
    /// Structured AndroidManifest.xml model.
    pub manifest: AndroidManifest,

    /// Gradle DSL for the app module. Renderer turns this into
    /// `app/build.gradle.kts` additions.
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

    /// `<intent-filter>` entries added to the launcher activity.
    /// Most plugins won't touch this — it exists for deep-link and
    /// custom-scheme plugins.
    #[serde(default)]
    pub launcher_intent_filters: Vec<IntentFilter>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetaDataEntry {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IntentFilter {
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    /// `<data android:scheme="..." android:host="..." .../>` rows.
    #[serde(default)]
    pub data: Vec<IntentFilterData>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct IntentFilterData {
    #[serde(default)]
    pub scheme: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub path_prefix: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
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
    /// File contents. Always UTF-8 today; binary support will come
    /// when a 1st-party plugin actually needs it.
    pub contents: String,
    /// POSIX mode bits. `None` → engine default (`0o644`).
    #[serde(default)]
    pub mode: Option<u32>,
}

// ----------------------------------------------------------------------------
// Mutation journal
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Ios,
    Android,
    /// Anything that isn't tied to a single platform — currently
    /// unused, kept for forward compatibility.
    Shared,
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
    /// Last-allocated sequence index. Wraps the journal's "what's
    /// next" cursor explicit so resuming an interrupted pipeline
    /// (a follow-up feature) doesn't drift.
    #[serde(default)]
    pub next_sequence_index: u64,
}

impl MutationJournal {
    /// Allocate the next sequence index and append a record.
    /// 1st-party callers go through helper methods on the IR types;
    /// this is the primitive both they and the subprocess
    /// deserializer rely on.
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
    struct NullPlugin;

    #[derive(Default, Serialize, Deserialize)]
    struct NullConfig {
        #[allow(dead_code)]
        flag: bool,
    }

    impl Plugin for NullPlugin {
        type Config = NullConfig;
        fn name(&self) -> &'static str {
            "null"
        }
        fn apply(&self, _ctx: &mut GenerateContext, _config: &Self::Config) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn plugin_trait_default_methods_work() {
        let p = NullPlugin;
        assert_eq!(p.name(), "null");
        assert!(p.after().is_empty());
        assert!(p.before().is_empty());
        let cfg = NullConfig::default();
        p.validate(&cfg).unwrap();
        let mut ctx = GenerateContext::default();
        p.apply(&mut ctx, &cfg).unwrap();
    }
}
