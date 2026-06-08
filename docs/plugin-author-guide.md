# Whisker Plugin Author Guide

This guide walks through writing a Whisker **plugin** — a cargo crate
that contributes entries to the generated iOS / Android host project
(Info.plist keys, AndroidManifest permissions, Gradle dependencies,
pbxproj build phases, dropped-in resource files…) without forking the
templates.

Inspired by Expo's [config plugins](https://docs.expo.dev/config-plugins/introduction/),
which let an npm package mutate the generated native projects from a
typed config block in `app.config.js`. Whisker plugins do the same
thing from `whisker.rs`, but the contract is a Rust trait
([`Plugin`](../crates/whisker-plugin/src/lib.rs)) and the wire is
JSON between a 3rd-party subprocess binary and the engine.

> Read [`module-author-guide.md`](module-author-guide.md) first if you
> want to know what a Whisker *Module* is. A module ships Rust +
> Kotlin + Swift sources and exposes them as a typed Rust API; a
> plugin ships a build-time binary that mutates the generated host
> project. Many crates ship both (`whisker-audio` is one — it carries
> a `Player` runtime AND a `WhiskerAudio` plugin); the
> [combining](#combining-a-plugin-with-a-module-runtime) section below
> covers how the two halves coexist in one crate.

## What a Whisker plugin is

A Whisker plugin is a cargo crate that:

1. Implements [`Plugin`](../crates/whisker-plugin/src/lib.rs) on a
   unit struct and [`PluginConfig`](../crates/whisker-plugin/src/lib.rs)
   on a typed Config struct.
2. Ships a thin `[[bin]]` entry point that calls
   `whisker_plugin::run_as_subprocess(MyPlugin)`.
3. Carries a `[package.metadata.whisker.plugins.<name>]` table in its
   `Cargo.toml` so `whisker-build` discovers and runs that binary
   during every `whisker build` / `whisker run`.

The plugin's `apply` method receives a
[`GenerateContext`](../crates/whisker-plugin/src/lib.rs) — an in-memory
representation of the iOS xcodeproj + Info.plist and the Android
Gradle project + AndroidManifest — and mutates it in place. The
engine then renders the final files.

## Distribution model — cargo only

A published Whisker plugin is **just a crate on crates.io**.
The `.crate` tarball contains:

```
whisker-foo-0.1.0/
├── Cargo.toml           ← carries [package.metadata.whisker.plugins.foo]
├── README.md
├── src/
│   ├── lib.rs           ← Plugin + PluginConfig impls
│   └── plugin.rs        ← (if you split the impl out of lib.rs)
└── bin/
    └── whisker_foo_plugin.rs   ← subprocess `main()`
```

No `Package.swift`, no `build.gradle.kts`, no native source dirs —
unless this crate is *also* a module, in which case those files live
alongside (see [combining](#combining-a-plugin-with-a-module-runtime)).

When the user app builds, `whisker-build` does the following:

1. `cargo metadata` enumerates every transitive dependency of the app.
2. For each dep, it inspects `Cargo.toml` for
   `[package.metadata.whisker.plugins.<name>]` tables. Each entry
   points at a `bin = "..."` target inside the crate.
3. The plugin binary is built (`cargo build --bin <name>`).
4. During the generate pass, the engine spawns each plugin binary,
   writes a [`PluginRequest`](../crates/whisker-plugin/src/lib.rs) to
   stdin (current IR state + the user's Config from `whisker.rs`),
   reads a [`PluginResponse`](../crates/whisker-plugin/src/lib.rs)
   from stdout, and merges the mutated context back into the pipeline.

Step 2 works identically whether the dep was resolved from a local
`path = "..."`, a git ref, or `~/.cargo/registry/src/index.crates.io-*/`.

## Authoring a plugin

### 1. Create the cargo crate

```toml
# Cargo.toml
[package]
name = "whisker-foo"
version = "0.1.0"
edition = "2021"
license = "MIT OR Apache-2.0"
description = "Whisker plugin — short tagline that shows up on crates.io."

# Explicit include so `cargo publish` ships the bin and the plugin
# source (and any platform files, if you also ship a module).
include = [
    "Cargo.toml",
    "src/**/*.rs",
    "bin/**/*.rs",
    "README.md",
]

[lib]
crate-type = ["rlib"]

# The module-system marker. Whisker-build walks the consuming app's
# dep tree, picks out every dep carrying it, and either wires in its
# module sources (if any), runs its plugin (if any), or both.
[package.metadata.whisker]

# Plugin registration. The bare table after `plugins.` is the plugin's
# stable kebab-case name; it must match `<Plugin::Config>::NAME`.
[package.metadata.whisker.plugins.whisker-foo]
bin = "whisker-foo-plugin"

[[bin]]
name = "whisker-foo-plugin"
path = "bin/whisker_foo_plugin.rs"
test = false
bench = false
doc = false

[dependencies]
whisker-plugin = "0.1"
serde = { version = "1", features = ["derive"] }
anyhow = "1"
```

### 2. Define `PluginConfig`

The Config struct is the typed surface the user spells in
`whisker.rs`. It must derive `Default`, `Serialize`, `Deserialize` —
the engine `Default`s it on the no-arg-closure path and
serializes/deserializes it across the subprocess wire.

```rust
// src/lib.rs (or src/plugin.rs)
use serde::{Deserialize, Serialize};
use whisker_plugin::{GenerateContext, Operation, PlistValue, Plugin, PluginConfig, Target};

#[derive(Default, Serialize, Deserialize)]
pub struct WhiskerFooConfig {
    /// Doc comment shows up in IDE tooltips when the user types
    /// `c.` inside `app.plugin::<WhiskerFoo>(|c| …)`.
    #[serde(default)]
    pub bundle_suffix: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
}

impl WhiskerFooConfig {
    /// Fluent setters — by convention, named after the field they
    /// touch. The user's `whisker.rs` reads as:
    ///   c.bundle_suffix(".staging").permissions(["RECORD_AUDIO"])
    pub fn bundle_suffix(&mut self, s: impl Into<String>) -> &mut Self {
        self.bundle_suffix = Some(s.into());
        self
    }
    pub fn permissions(&mut self, p: impl IntoIterator<Item = impl Into<String>>) -> &mut Self {
        self.permissions = p.into_iter().map(Into::into).collect();
        self
    }
}

impl PluginConfig for WhiskerFooConfig {
    /// Stable kebab-case identifier. Must match the table key in
    /// `Cargo.toml`'s `[package.metadata.whisker.plugins.<name>]`.
    const NAME: &'static str = "whisker-foo";
}
```

#### Naming convention

- Plugin struct: `WhiskerFoo`, `Firebase`, `MyCompanyAnalytics` —
  **no `Plugin` suffix**. The user types this name in `whisker.rs`,
  so it should read like the thing being enabled, not the
  implementation noun.
- Config struct: `<PluginName>Config`. Always paired 1:1 with the
  plugin struct.
- `NAME` const: kebab-case. 1st-party plugins prefix `whisker-`
  (`whisker-info-plist`, `whisker-permissions`); 3rd-party plugins
  use whatever stable identifier won't collide.

### 3. Define the `Plugin`

The plugin struct itself is usually a unit struct — the apply logic
runs against the Config alone, with no per-plugin state:

```rust
pub struct WhiskerFoo;

impl Plugin for WhiskerFoo {
    type Config = WhiskerFooConfig;

    fn apply(&self, ctx: &mut GenerateContext, cfg: &WhiskerFooConfig) -> anyhow::Result<()> {
        // ---- iOS contributions ---------------------------------------------
        if let Some(ios) = ctx.ios.as_mut() {
            if let Some(suffix) = cfg.bundle_suffix.as_ref() {
                // Read the core field, mutate it, journal the change as
                // Override (since it was previously seeded from AppConfig).
                let new_id = format!(
                    "{}{}",
                    ios.bundle_id.as_deref().unwrap_or("rs.whisker.app"),
                    suffix,
                );
                ios.bundle_id = Some(new_id);
                ctx.journal.record(
                    WhiskerFooConfig::NAME,
                    Target::Ios,
                    "bundle_id",
                    Operation::Override,
                );
            }
        }

        // ---- Android contributions -----------------------------------------
        if let Some(android) = ctx.android.as_mut() {
            for perm in &cfg.permissions {
                android.manifest.permissions.push(perm.clone());
            }
            if !cfg.permissions.is_empty() {
                ctx.journal.record(
                    WhiskerFooConfig::NAME,
                    Target::Android,
                    "manifest.permissions",
                    Operation::ArrayPush { count: cfg.permissions.len() },
                );
            }
        }

        Ok(())
    }
}
```

Three additional trait slots are available; most plugins leave all
three at their defaults:

| Slot | Default | Use when |
|---|---|---|
| `fn name(&self)` | `Self::Config::NAME` | You want the plugin to register under a different identifier than its Config's `NAME`. Rare — almost always leave it. |
| `fn after(&self)` / `fn before(&self)` | `&[]` | Your plugin reads a field that another plugin writes (`after`), or your plugin sets a default that another plugin should be able to override (`before`). |
| `fn validate(&self, cfg)` | `Ok(())` | The Config admits combinations the type system can't reject. Run this *before* mutating the IR so a bad config aborts cleanly. |

### 4. What the IR covers

[`GenerateContext`](../crates/whisker-plugin/src/lib.rs) carries two
target IRs (each `Option`, since `whisker generate` may run for one
platform only) plus a shared `app_meta` and `journal`:

```rust
pub struct GenerateContext {
    pub app_meta: AppMeta,                    // read-only snapshot of AppConfig
    pub ios: Option<IosProjectIr>,
    pub android: Option<AndroidProjectIr>,
    pub journal: MutationJournal,
}
```

Each per-target IR has two layers:

**Core fields** — seeded from `AppConfig` by the engine before any
plugin runs. Read them as defaults; override them via
`Operation::Override` when you intentionally stomp the user's value
(e.g. a flavor plugin appending `.staging` to the bundle id):

| Field | iOS | Android |
|---|---|---|
| `app_name` | `CFBundleDisplayName` / pbxproj `PRODUCT_NAME` | `manifest.application.android:label` |
| `version` | `CFBundleShortVersionString` | Gradle `versionName` |
| `build_number` | `CFBundleVersion` | Gradle `versionCode` |
| `bundle_id` / `application_id` | pbxproj `PRODUCT_BUNDLE_IDENTIFIER` | Gradle `applicationId` |
| `scheme` / — | Xcode scheme name | (n/a) |
| `deployment_target` / `min_sdk` + `target_sdk` | `IPHONEOS_DEPLOYMENT_TARGET` | Gradle SDK levels |

**Plugin-additive fields** — empty at pipeline start, plugins push
into them:

| Field | What lands where |
|---|---|
| `ios.info_plist: BTreeMap<String, PlistValue>` | Rendered to `Info.plist` XML. `PlistValue` is `String`/`Integer`/`Real`/`Boolean`/`Array`/`Dict`. |
| `ios.pbxproj_ops: Vec<PbxprojOp>` | `AddResource` / `AddSource` / `SetBuildSetting` / `LinkSystemFramework` replayed against the xcodeproj template. |
| `ios.extra_files: BTreeMap<PathBuf, FileEntry>` | Dropped into `gen/ios/<path>`. Path-validated against `..` traversal. |
| `android.manifest.permissions: Vec<String>` | `<uses-permission>` entries (dedup'd at render time). |
| `android.manifest.application_meta_data: Vec<MetaDataEntry>` | `<meta-data>` inside `<application>`. |
| `android.gradle.apply_plugins: Vec<String>` | Plugin ids added to the app module's `plugins { }` block. |
| `android.gradle.dependencies: Vec<String>` | Raw DSL lines appended to `dependencies { }` (e.g. `implementation("com.google.firebase:firebase-analytics:21.5.0")`). |
| `android.extra_files: BTreeMap<PathBuf, FileEntry>` | Dropped into `gen/android/<path>`. |

> **`app_meta` vs the IR.** `app_meta` is frozen at pipeline entry —
> if a plugin overrides `ios.bundle_id` via `Operation::Override`,
> downstream plugins reading `ctx.app_meta.ios_bundle_id` still see
> the original user value. Use `app_meta` for attribution and
> diagnostics; use the IR for fields that flow into the rendered
> project.

### 5. Mutation journal

Every IR write should be paired with a `ctx.journal.record(...)`
call. The journal is how the engine attributes conflicts, surfaces
"who set this" in `whisker generate --verbose`, and orders plugin
output deterministically.

Three operation kinds:

| `Operation` | Meaning |
|---|---|
| `Set` | First write to a previously-unset field. Two `Set`s to the same path from different plugins is a hard error. |
| `Override` | Explicit stomp of a prior value. Use this when overwriting a core field (seeded from `AppConfig`) or another plugin's write. Pair with `after()` so the ordering is intentional. |
| `ArrayPush { count }` | Appended `count` items to an array-shaped field (permissions, meta-data, pbxproj ops…). `count` lets the engine summarize "plugin X added 3 permissions" without recording each push individually. |

```rust
// Adding one permission — push, then record.
android.manifest.permissions.push("android.permission.RECORD_AUDIO".into());
ctx.journal.record(
    WhiskerFooConfig::NAME,
    Target::Android,
    "manifest.permissions",          // dotted path to the IR field
    Operation::ArrayPush { count: 1 },
);
```

Dotted paths are by convention: top-level field, dot, sub-field for
nested fields (`manifest.permissions`, `info_plist.UIBackgroundModes`).
The engine doesn't parse them — they're for human eyes in error
messages.

### 6. Write the subprocess binary

The binary is a five-line wrapper:

```rust
// bin/whisker_foo_plugin.rs
fn main() -> anyhow::Result<()> {
    whisker_plugin::run_as_subprocess(whisker_foo::WhiskerFoo)
}
```

[`run_as_subprocess`](../crates/whisker-plugin/src/lib.rs) handles
the wire format: reads `PluginRequest` from stdin (config + current
IR), runs `validate` then `apply`, writes `PluginResponse` (mutated
IR) back to stdout. `?` propagates any deserialization / validation /
apply error as an `anyhow::Error`, which causes the process to exit
1 with the message on stderr — exactly what the engine expects.

Stderr is yours for human-readable diagnostics: log lines, progress
messages, anything you want to surface when the user runs `whisker
generate --verbose`. **Don't write anything else to stdout** —
anything that isn't the `PluginResponse` JSON envelope is a wire-format
violation.

### 7. Test the plugin

Plugin logic is plain Rust — test it directly against an in-memory
`GenerateContext`. No subprocess, no JSON round-trip needed:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use whisker_plugin::{AndroidProjectIr, IosProjectIr};

    fn ctx_both() -> GenerateContext {
        GenerateContext {
            ios: Some(IosProjectIr::default()),
            android: Some(AndroidProjectIr::default()),
            ..Default::default()
        }
    }

    #[test]
    fn default_config_contributes_nothing() {
        let mut ctx = ctx_both();
        WhiskerFoo.apply(&mut ctx, &WhiskerFooConfig::default()).unwrap();
        assert!(ctx.ios.unwrap().info_plist.is_empty());
        assert!(ctx.android.unwrap().manifest.permissions.is_empty());
        assert!(ctx.journal.records.is_empty());
    }

    #[test]
    fn bundle_suffix_overrides_core_field() {
        let mut cfg = WhiskerFooConfig::default();
        cfg.bundle_suffix(".staging");
        let mut ctx = ctx_both();
        ctx.ios.as_mut().unwrap().bundle_id = Some("rs.whisker.foo".into());
        WhiskerFoo.apply(&mut ctx, &cfg).unwrap();
        assert_eq!(
            ctx.ios.unwrap().bundle_id.as_deref(),
            Some("rs.whisker.foo.staging"),
        );
    }
}
```

The "no IR target" path is worth a dedicated test — `whisker generate`
can run for a single platform, so `ctx.ios.is_none()` and
`ctx.android.is_none()` are both reachable in production. Don't
unwrap; `if let Some(...) = ctx.<target>.as_mut() { ... }`.

## Consumer side — `whisker.rs`

A Whisker app that wants to use `whisker-foo`:

```toml
# app/Cargo.toml
[dependencies]
whisker-foo = "0.1"
```

```rust
// app/whisker.rs
pub fn configure(app: &mut whisker_app_config::AppConfig) {
    app.name("My App")
        .bundle_id("rs.example.myapp")
        // ...

    app.plugin::<whisker_foo::WhiskerFoo>(|c| {
        c.bundle_suffix(".staging")
            .permissions(["android.permission.RECORD_AUDIO"]);
    });

    // Plugins that take no config still go through the same call so
    // the API stays uniform — the closure body is just empty.
    app.plugin::<whisker_other::WhiskerOther>(|_| {});
}
```

No `Podfile`, `Package.swift`, or `build.gradle` edits required on
the consumer side. `whisker build` / `whisker run` picks up the new
plugin automatically via `cargo metadata` + the
`[package.metadata.whisker.plugins.*]` table.

## Combining a plugin with a module runtime

When a crate ships both a runtime (Rust + Kotlin + Swift sources,
exposed as a typed Rust API per [`module-author-guide.md`](module-author-guide.md))
AND a plugin (build-time `apply` that contributes Info.plist /
manifest entries), the two halves are gated apart with a cargo
feature so the **config probe build path stays fast**.

The config probe is the small binary `whisker-cli` builds from your
app's `whisker.rs` to extract the typed `AppConfig` (including
plugin configs). It pulls every plugin crate with
`default-features = false`. If the plugin crate's heavy runtime is
behind a non-default feature, the probe builds in seconds; otherwise
it'd build the whole Lynx bridge + driver + render layer for every
plugin dep on every cold probe.

The convention:

```toml
# Cargo.toml
[features]
default = ["runtime"]
runtime = ["dep:whisker"]

[dependencies]
# Heavy umbrella crate — gated.
whisker = { workspace = true, optional = true }

# Plugin support — always on. Light deps only.
whisker-plugin = { workspace = true }
serde = { workspace = true }
anyhow = { workspace = true }
```

```rust
// src/lib.rs

// Always available — independent of the `runtime` feature so the
// config probe can resolve the Plugin type.
mod plugin;
pub use plugin::*;

// Player runtime — gated behind the default-on `runtime` feature
// so the probe path stays cheap.
#[cfg(feature = "runtime")]
mod runtime;
#[cfg(feature = "runtime")]
pub use runtime::*;
```

Apps depending on `whisker-foo` for actual runtime use get the
default-on feature, which pulls `whisker` and re-exports the runtime
API — no feature-flag dance needed at the consumer.

> **Don't name the plugin module `cng`** (the internal codename for
> the engine that drives the plugin pipeline). Plugin authors and
> end-users only care about the plugin abstraction; "CNG" is an
> implementation noun. Conventional names: `plugin` (private mod with
> a re-export, as above) or putting the impl directly in `lib.rs` if
> the crate is plugin-only.

## End-to-end reference: `whisker-audio`

`whisker-audio` is the canonical "module + plugin in one crate"
example. The plugin half mirrors `expo-audio`'s config-plugin
surface; the module half exposes a `Player` runtime.

- [`packages/whisker-audio/Cargo.toml`](../packages/whisker-audio/Cargo.toml) —
  combined module + plugin manifest. Note the `runtime` feature gate
  and the `[package.metadata.whisker.plugins.whisker-audio]` table.
- [`packages/whisker-audio/src/plugin.rs`](../packages/whisker-audio/src/plugin.rs) —
  `WhiskerAudio` + `WhiskerAudioConfig` definitions, four fluent
  setters, `apply` writes `NSMicrophoneUsageDescription` /
  `UIBackgroundModes` / `RECORD_AUDIO`.
- [`packages/whisker-audio/bin/whisker_audio_plugin.rs`](../packages/whisker-audio/bin/whisker_audio_plugin.rs) —
  five-line subprocess wrapper.
- [`examples/podcast/whisker.rs`](../examples/podcast/whisker.rs) —
  consumer-side `app.plugin::<WhiskerAudio>(|c| …)` call.

## What goes where — quick reference

| Symbol | Crate | Used by |
|---|---|---|
| `Plugin` trait | `whisker-plugin` | Plugin implementor |
| `PluginConfig` trait + `NAME` const | `whisker-plugin` | Config struct |
| `GenerateContext`, `AppMeta` | `whisker-plugin` | `Plugin::apply` body |
| `IosProjectIr`, `AndroidProjectIr` | `whisker-plugin` | The targets you mutate |
| `PlistValue`, `PbxprojOp`, `MetaDataEntry`, `FileEntry` | `whisker-plugin` | IR value types |
| `MutationJournal`, `Operation`, `Target` | `whisker-plugin` | `ctx.journal.record(...)` |
| `run_as_subprocess` | `whisker-plugin` | The `bin/` `main()` |
| `app.plugin::<P>` | `whisker-app-config` | Consumer's `whisker.rs` |
| `[package.metadata.whisker.plugins.<name>]` table | `Cargo.toml` | Engine discovery |
