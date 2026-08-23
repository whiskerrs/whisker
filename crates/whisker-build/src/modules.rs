//! Whisker module-system — discovery + manifest parsing.
//!
//! When `whisker run` builds an app crate for
//! iOS or Android, every cargo dependency may optionally contribute
//! native code (Swift / Obj-C++ on iOS, Kotlin / JNI on Android) to
//! the final host binary. A crate opts in by declaring a
//! `[package.metadata.whisker]` table in its `Cargo.toml`. The
//! Whisker CLI walks the consuming app's dep graph via `cargo
//! metadata`, picks out every dependency carrying that table, and
//! feeds each platform contribution through to its build system.
//!
//! This module is platform-neutral — it just produces the
//! `ResolvedModule` list. `whisker-build::ios` and
//! `whisker-build::android` consume the list and decide what to do
//! with each module (the SwiftPM aggregator on iOS, the gradle
//! settings include on Android).
//!
//! ## Schema
//!
//! ```toml
//! # packages/whisker-video/Cargo.toml
//! [package.metadata.whisker]
//! # The bare table is the marker — its presence identifies this
//! # crate as a Whisker module. Platform code + build manifests live
//! # at the package root in `android/` and `ios/` (Expo-style):
//! #   android/build.gradle.kts   — AGP library
//! #   ios/Package.swift          — SwiftPM library
//! # whisker-build discovers those per-platform manifests directly,
//! # so no source-file list is needed for DSL modules.
//! ```
//!
//! All paths are resolved relative to the directory containing the
//! manifest (the crate's `Cargo.toml`). The resolver returns
//! absolute paths so downstream build-system integration does not
//! have to know where Cargo originally found the module.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use cargo_metadata::MetadataCommand;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Top-level shape of the `[package.metadata.whisker]` table.
///
/// Every section is optional so a module can declare just the
/// platform(s) it supports — the bare table (no sub-sections) is a
/// valid marker for a pure-DSL module that ships only Swift /
/// Kotlin via its `ios/` / `android/` dirs. Unknown sections /
/// fields are rejected to catch typos early (we don't want a
/// typoed `iOS` section to silently produce a no-op build).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestRaw {
    #[serde(default)]
    pub ios: Option<IosSectionRaw>,
    #[serde(default)]
    pub android: Option<AndroidSectionRaw>,
    /// Rust Desktop Host crate published beside the common module crate.
    #[serde(default)]
    pub desktop: Option<RustHostSectionRaw>,
    /// Rust Web Host crate published beside the common module crate.
    #[serde(default)]
    pub web: Option<RustHostSectionRaw>,
    /// `[package.metadata.whisker.plugins.<name>]` entries —
    /// consumed by `whisker_cng::discovery`, not by the module
    /// system. Captured here as raw JSON so its presence doesn't
    /// trip the `deny_unknown_fields` check on this struct.
    /// Validation of the plugin entry's shape lives in
    /// `whisker_cng::discovery::PluginEntryRaw`.
    #[serde(default)]
    pub plugins: Option<serde_json::Value>,
}

/// Published package identity for a Rust Host contribution.
///
/// A local checkout still discovers the nested manifest by convention. This
/// declaration survives crates.io packaging, which deliberately excludes
/// nested Cargo packages from the outer crate archive.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustHostSectionRaw {
    /// Cargo package name of the separately published Host library.
    pub package: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IosSectionRaw {
    /// Paths to Swift source files that get staged into the
    /// consuming app's gen tree under
    /// `gen/ios/Sources/WhiskerModules/<crate-name>/` and compiled
    /// into the host app target alongside its own Swift sources.
    /// Mirror of `[android].kotlin_sources` for symmetry.
    ///
    /// Existence-checked but otherwise inert: iOS staging keys off a
    /// module's `ios/Package.swift`, not this list.
    #[serde(default)]
    pub swift_sources: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidSectionRaw {
    /// Paths to Kotlin / Java source files (`*.kt`, `*.java`) that
    /// should be compiled into the host app's APK alongside the
    /// runtime's own Kotlin sources. Paths are relative to the
    /// manifest's directory.
    ///
    /// Most modules use these to declare a `LynxUI` subclass that
    /// the [`behaviors`] list below points the Lynx engine at.
    #[serde(default)]
    pub kotlin_sources: Vec<String>,
    /// Paths to JNI C / C++ source files (`*.c`, `*.cc`, `*.cpp`)
    /// — for modules that need to drop into native code on Android
    /// (cross-language calls, raw NDK APIs, etc.). Less common than
    /// kotlin_sources; most native_element modules can stay in
    /// Kotlin.
    #[serde(default)]
    pub jni_sources: Vec<String>,
}

/// A single discovered module after its metadata has been resolved
/// against the cargo dep tree. `package` carries the cargo crate
/// name (handy for diagnostics) and `manifest_dir` is the absolute
/// path of the directory the crate's `Cargo.toml` lives in.
#[derive(Debug, Clone)]
pub struct ResolvedModule {
    pub package: String,
    pub manifest_dir: PathBuf,
    /// Absolute, existence-checked paths to `.swift` sources.
    /// Empty when the module declares no Swift contributions.
    pub ios_swift_sources: Vec<PathBuf>,
    /// Absolute, existence-checked paths to Kotlin / Java sources
    /// for the Android build. Empty when the module declares no
    /// Android Kotlin contributions.
    pub android_kotlin_sources: Vec<PathBuf>,
    /// Absolute, existence-checked paths to JNI C / C++ sources
    /// for the Android build. Empty by default — most native_element
    /// modules use Kotlin, not JNI.
    pub android_jni_sources: Vec<PathBuf>,
    /// Rust-native Desktop module contribution.
    pub desktop: Option<ResolvedRustModuleContribution>,
    /// Browser DOM module contribution.
    pub web: Option<ResolvedRustModuleContribution>,
}

/// One existence-checked Rust Host library shipped inside a module package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRustModuleContribution {
    /// Cargo package name declared by the Host library.
    pub package: String,
    /// How the generated Host should depend on this library.
    pub source: ResolvedRustHostSource,
}

/// Location of one Rust Host library after module discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedRustHostSource {
    /// A nested crate present in a path or git checkout.
    Path(PathBuf),
    /// A separately published crate used when Cargo unpacked the outer module
    /// without its nested packages.
    Registry { version: String },
}

/// Walk the cargo dep graph of `app_package` (resolved at
/// `manifest_path`) and return every dependency that declares a
/// `[package.metadata.whisker]` table in its `Cargo.toml`.
///
/// Ordering: `cargo metadata`'s topological order, deduplicated by
/// package id (a diamond dep landed twice gets resolved once).
/// Downstream consumers can rely on a stable order across calls
/// for the same workspace state.
///
/// Errors:
/// - `cargo metadata` failure (workspace broken, manifest_path
///   invalid, etc.) propagates with the `cargo_metadata` error.
/// - Metadata parse failure (`[package.metadata.whisker]` exists
///   but has unknown sections / fields) propagates with the
///   offending crate name attached.
/// - Native-source path referenced in the table but not present on disk, or
///   an invalid conventional `desktop/Cargo.toml` / `web/Cargo.toml`, errors
///   eagerly rather than silently dropping a Host implementation.
pub fn discover(manifest_path: &Path, app_package: &str) -> Result<Vec<ResolvedModule>> {
    let metadata = MetadataCommand::new()
        .manifest_path(manifest_path)
        .exec()
        .with_context(|| {
            format!(
                "cargo metadata failed for {} (package: {app_package})",
                manifest_path.display(),
            )
        })?;

    // Walk the resolution graph rather than `metadata.packages`: it
    // encodes activated features / platform deps, so only deps that
    // would really be linked into the app show up.
    let resolve = metadata
        .resolve
        .as_ref()
        .ok_or_else(|| anyhow!("cargo metadata returned no resolve graph"))?;
    let root_id = resolve
        .root
        .as_ref()
        .filter(|id| {
            metadata
                .packages
                .iter()
                .any(|p| &p.id == *id && p.name == app_package)
        })
        .cloned()
        .or_else(|| {
            metadata
                .packages
                .iter()
                .find(|p| p.name == app_package)
                .map(|p| p.id.clone())
        })
        .ok_or_else(|| anyhow!("cargo package `{app_package}` not found in the workspace"))?;

    let mut visit: Vec<&cargo_metadata::PackageId> = vec![&root_id];
    let mut seen: std::collections::HashSet<&cargo_metadata::PackageId> = Default::default();
    let mut module_pkg_ids: Vec<cargo_metadata::PackageId> = Vec::new();

    while let Some(pkg_id) = visit.pop() {
        if !seen.insert(pkg_id) {
            continue;
        }
        if let Some(node) = resolve.nodes.iter().find(|n| &n.id == pkg_id) {
            for dep in &node.deps {
                visit.push(&dep.pkg);
            }
        }
        // The root app declares native sources directly, never through
        // a `[package.metadata.whisker]` table.
        if pkg_id != &root_id {
            module_pkg_ids.push(pkg_id.clone());
        }
    }

    let mut resolved: Vec<ResolvedModule> = Vec::new();
    for id in module_pkg_ids {
        let pkg = metadata
            .packages
            .iter()
            .find(|p| p.id == id)
            .expect("dep id came from `resolve.nodes`; must exist in metadata.packages");
        let manifest_dir = pkg
            .manifest_path
            .parent()
            .map(|p| PathBuf::from(p.as_str()))
            .ok_or_else(|| {
                anyhow!(
                    "dep `{}` manifest_path has no parent: {}",
                    pkg.name,
                    pkg.manifest_path,
                )
            })?;
        // A `[package.metadata.whisker]` table is the opt-in marker;
        // deps without one are not modules.
        let Some(whisker_meta) = pkg.metadata.get("whisker") else {
            continue;
        };
        let manifest: ManifestRaw =
            serde_json::from_value(whisker_meta.clone()).with_context(|| {
                format!("parse [package.metadata.whisker] in {}", pkg.manifest_path,)
            })?;
        let package_version = pkg.version.to_string();
        let desktop = resolve_rust_host_crate(
            &pkg.name,
            &package_version,
            "desktop",
            &manifest_dir,
            manifest.desktop.as_ref(),
        )?;
        let web = resolve_rust_host_crate(
            &pkg.name,
            &package_version,
            "web",
            &manifest_dir,
            manifest.web.as_ref(),
        )?;
        let mut ios_swift: Vec<PathBuf> = Vec::new();
        if let Some(ios) = manifest.ios {
            for raw_path in ios.swift_sources {
                let resolved_path = manifest_dir.join(&raw_path);
                let canonical = resolved_path.canonicalize().with_context(|| {
                    format!(
                        "module `{}` declares metadata.whisker.ios.swift_sources = \
                         [..., {raw_path:?}] but {} does not exist",
                        pkg.name,
                        resolved_path.display(),
                    )
                })?;
                ios_swift.push(canonical);
            }
        }
        let mut android_kotlin: Vec<PathBuf> = Vec::new();
        let mut android_jni: Vec<PathBuf> = Vec::new();
        if let Some(android) = manifest.android {
            for raw_path in android.kotlin_sources {
                let resolved_path = manifest_dir.join(&raw_path);
                let canonical = resolved_path.canonicalize().with_context(|| {
                    format!(
                        "module `{}` declares metadata.whisker.android.kotlin_sources = \
                         [..., {raw_path:?}] but {} does not exist",
                        pkg.name,
                        resolved_path.display(),
                    )
                })?;
                android_kotlin.push(canonical);
            }
            for raw_path in android.jni_sources {
                let resolved_path = manifest_dir.join(&raw_path);
                let canonical = resolved_path.canonicalize().with_context(|| {
                    format!(
                        "module `{}` declares metadata.whisker.android.jni_sources = \
                         [..., {raw_path:?}] but {} does not exist",
                        pkg.name,
                        resolved_path.display(),
                    )
                })?;
                android_jni.push(canonical);
            }
        }
        resolved.push(ResolvedModule {
            package: pkg.name.clone(),
            manifest_dir,
            ios_swift_sources: ios_swift,
            android_kotlin_sources: android_kotlin,
            android_jni_sources: android_jni,
            desktop,
            web,
        });
    }

    // Stable order keeps the gen tree byte-identical between runs, so
    // gradle / cargo don't re-run downstream tasks on a permutation.
    resolved.sort_by(|a, b| a.package.cmp(&b.package));
    Ok(resolved)
}

fn resolve_rust_host_crate(
    package: &str,
    package_version: &str,
    target: &str,
    manifest_dir: &Path,
    published: Option<&RustHostSectionRaw>,
) -> Result<Option<ResolvedRustModuleContribution>> {
    if let Some(published) = published
        && published.package.trim().is_empty()
    {
        return Err(anyhow!(
            "module `{package}` metadata.whisker.{target}.package must not be empty"
        ));
    }
    let cargo_toml = manifest_dir.join(target).join("Cargo.toml");
    if !cargo_toml.is_file() {
        return Ok(published.map(|published| ResolvedRustModuleContribution {
            package: published.package.clone(),
            source: ResolvedRustHostSource::Registry {
                version: package_version.to_string(),
            },
        }));
    }
    let source = std::fs::read_to_string(&cargo_toml)
        .with_context(|| format!("read {} Host manifest {}", target, cargo_toml.display()))?;
    let manifest: toml::Value = toml::from_str(&source)
        .with_context(|| format!("parse {} Host manifest {}", target, cargo_toml.display()))?;
    let host_package = manifest
        .get("package")
        .and_then(|value| value.get("name"))
        .and_then(toml::Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| {
            anyhow!(
                "module `{package}` {target}/Cargo.toml must declare a non-empty [package].name"
            )
        })?;
    if let Some(published) = published
        && published.package != host_package
    {
        return Err(anyhow!(
            "module `{package}` metadata.whisker.{target}.package is {:?}, but {target}/Cargo.toml declares package {:?}",
            published.package,
            host_package,
        ));
    }
    let host_root = cargo_toml
        .parent()
        .expect("Cargo.toml has a parent")
        .canonicalize()
        .with_context(|| format!("canonicalize {} Host crate", cargo_toml.display()))?;
    Ok(Some(ResolvedRustModuleContribution {
        package: host_package.to_string(),
        source: ResolvedRustHostSource::Path(host_root),
    }))
}

/// Flatten every discovered module's Android Kotlin sources into a
/// colon-separated string. The Android orchestration uses these
/// paths to extend gradle's main source set (see
/// `whisker-build::android`).
pub fn android_kotlin_sources_env_value(modules: &[ResolvedModule]) -> String {
    let mut paths: Vec<String> = Vec::new();
    for m in modules {
        for p in &m.android_kotlin_sources {
            paths.push(p.to_string_lossy().into_owned());
        }
    }
    paths.join(":")
}

/// Same shape, JNI sources. Currently only consumed by the Android
/// orchestration when a module needs C/C++ code on Android (rare;
/// most modules stick to Kotlin).
pub fn android_jni_sources_env_value(modules: &[ResolvedModule]) -> String {
    let mut paths: Vec<String> = Vec::new();
    for m in modules {
        for p in &m.android_jni_sources {
            paths.push(p.to_string_lossy().into_owned());
        }
    }
    paths.join(":")
}

// ----- JSON report for build-system plugins ---------------------------------
//
// The Kotlin Settings Plugin / Swift Build Tool Plugin can't link
// against this crate, so they read module discovery as stdout JSON
// from `whisker modules`. The shape below is that wire schema:
// additive changes only, a rename needs a version bump.

/// Per-Whisker-module JSON record returned by
/// [`build_modules_report`]. Per-platform fields are `Option` so
/// modules that ship only one platform serialise cleanly (consumers
/// can filter `has_android` / `has_ios` rather than parsing
/// "android": {} stubs).
#[derive(Debug, Clone, Serialize)]
pub struct ModulesReportModule {
    /// Cargo crate name (e.g., `"whisker-router"`).
    pub crate_name: String,
    /// Absolute path to the directory containing the module's
    /// `Cargo.toml`.
    pub manifest_dir: PathBuf,
    /// Android-side surface. `None` when the module has no `android/`
    /// directory at the package root.
    pub android: Option<AndroidModuleReport>,
    /// iOS-side surface. `None` when the module declares neither
    /// `ios_swift_sources` nor an `ios/Package.swift`.
    pub ios: Option<IosModuleReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AndroidModuleReport {
    /// Absolute path the Gradle Settings Plugin uses for
    /// `settings.project(":<crate>").projectDir = file(this)`.
    /// This is the package root (manifest_dir) — the Expo-style
    /// layout keeps `build.gradle.kts` at the root and points its
    /// Kotlin source set at the `android/` subdirectory.
    pub subproject_dir: PathBuf,
    /// `<PascalCase(crate_name)>Behaviors` — the KSP-emitted object
    /// name. Lives in package `rs.whisker.runtime.generated`, same as
    /// the aggregator, so the aggregator references it without an
    /// import.
    pub behaviors_class: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IosModuleReport {
    /// SwiftPM module / framework name. `Some` whenever the module
    /// ships an `ios/Package.swift`; `None` for legacy-shape modules
    /// whose Swift sources are declared via
    /// `[package.metadata.whisker.ios] swift_sources` instead.
    pub swift_module: Option<String>,
    /// `.swift` source paths declared via the legacy
    /// `[package.metadata.whisker.ios] swift_sources = [...]`. Empty
    /// for the common Expo-style case (Swift lives in
    /// `ios/Package.swift`).
    pub swift_sources: Vec<PathBuf>,
}

/// Top-level JSON payload — what `whisker modules` writes to
/// stdout.
#[derive(Debug, Clone, Serialize)]
pub struct ModulesReport {
    /// Hex SHA-256 of the workspace's `Cargo.lock`. Consumers (the
    /// Gradle Settings Plugin) key their disk cache on this — Sync
    /// reuses the cached JSON when the lock file hasn't changed.
    pub cargo_lock_sha256: String,
    /// The user app crate the discovery resolved against. Echoed
    /// back so consumers can sanity-check their `whisker { userPackage = ... }`
    /// declaration matches.
    pub user_package: String,
    /// Stable-ordered (alphabetical by `crate_name`) list of modules.
    pub modules: Vec<ModulesReportModule>,
}

/// Rewrite the Gradle plugins' on-disk module-report cache with a
/// fresh discovery pass. Call this BEFORE every CLI-driven gradle
/// invocation (`whisker build appbundle|apk`, the dev loop's
/// assemble).
///
/// The Settings plugin validates its cache against the `Cargo.lock`
/// hash alone, which goes stale two ways: the cache file lives in the
/// workspace-level `target/whisker/`, so in a multi-app workspace one
/// app's report is reused for another at the same lock hash; and
/// `[package.metadata.whisker]` edits in a path-dep don't touch
/// `Cargo.lock` at all. Either way the modules silently vanish from
/// the APK. Pre-writing a fresh report makes the plugin's cache read
/// current whatever its version. Both filenames are written — the
/// shared legacy name (plugin ≤0.4.1) and the per-package one.
pub fn refresh_gradle_module_cache(workspace_root: &Path, user_package: &str) -> Result<()> {
    let report = build_modules_report(workspace_root, user_package)
        .with_context(|| format!("build modules report for `{user_package}`"))?;
    let json = serde_json::to_string_pretty(&report).context("serialize modules report")?;
    let dir = workspace_root.join("target/whisker");
    std::fs::create_dir_all(&dir).with_context(|| format!("mkdir {}", dir.display()))?;
    for name in [
        format!("module-info-{user_package}.json"),
        "module-info.json".to_string(),
    ] {
        let path = dir.join(name);
        std::fs::write(&path, &json).with_context(|| format!("write {}", path.display()))?;
    }
    Ok(())
}

/// Build a [`ModulesReport`] from a workspace + user package. Combines
/// [`discover`], `Cargo.lock` hashing, and per-platform availability
/// classification.
///
/// Detection rules — both follow the Expo-style "manifest at the
/// package root, source under a per-platform subdir":
///   * Android: `<manifest_dir>/build.gradle.kts` exists. The
///     `subproject_dir` reported is `manifest_dir` (the AGP library
///     module is rooted at the package root; its Kotlin source set
///     points at `android/` internally).
///   * iOS: `<manifest_dir>/Package.swift` exists, OR
///     `ios_swift_sources` is non-empty.
pub fn build_modules_report(workspace_root: &Path, user_package: &str) -> Result<ModulesReport> {
    let manifest_path = workspace_root.join("Cargo.toml");
    let resolved = discover(&manifest_path, user_package)
        .with_context(|| format!("discover modules for `{user_package}`"))?;

    let lock_path = workspace_root.join("Cargo.lock");
    let cargo_lock_sha256 =
        sha256_file(&lock_path).with_context(|| format!("hash {}", lock_path.display()))?;

    let modules: Vec<ModulesReportModule> = resolved
        .into_iter()
        .map(|m| {
            let android = if m.manifest_dir.join("build.gradle.kts").is_file() {
                Some(AndroidModuleReport {
                    subproject_dir: m.manifest_dir.clone(),
                    behaviors_class: crate_to_behaviors_class(&m.package),
                })
            } else {
                None
            };
            let swift_pkg = m.manifest_dir.join("Package.swift");
            let has_ios = !m.ios_swift_sources.is_empty() || swift_pkg.is_file();
            let ios = if has_ios {
                Some(IosModuleReport {
                    swift_module: if swift_pkg.is_file() {
                        Some(crate_to_swift_module(&m.package))
                    } else {
                        None
                    },
                    swift_sources: m.ios_swift_sources,
                })
            } else {
                None
            };
            ModulesReportModule {
                crate_name: m.package,
                manifest_dir: m.manifest_dir,
                android,
                ios,
            }
        })
        .collect();

    Ok(ModulesReport {
        cargo_lock_sha256,
        user_package: user_package.to_string(),
        modules,
    })
}

/// `whisker-router` → `WhiskerRouterBehaviors`. Public so the Gradle
/// Project Plugin can derive the same FQN from JSON without re-
/// implementing the rule. (The aggregator only needs the short class
/// name — the FQN is `rs.whisker.runtime.generated.<class>`.)
pub fn crate_to_behaviors_class(crate_name: &str) -> String {
    let mut out = pascal_case(crate_name);
    out.push_str("Behaviors");
    out
}

/// `whisker-router` → `WhiskerRouter`. SwiftPM module names follow
/// the package name in `ios/Package.swift`; the canonical Expo-style
/// layout uses the PascalCase form of the crate name.
pub fn crate_to_swift_module(crate_name: &str) -> String {
    pascal_case(crate_name)
}

fn pascal_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut next_upper = true;
    for ch in s.chars() {
        if ch == '-' || ch == '_' {
            next_upper = true;
            continue;
        }
        if next_upper {
            out.extend(ch.to_uppercase());
            next_upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_rfc0004_rust_host_crates_by_convention() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .join("Cargo.toml");
        let modules = discover(&workspace, "host-smoke").unwrap();
        let toggle = modules
            .iter()
            .find(|module| module.package == "whisker-toggle")
            .unwrap();
        let desktop = toggle.desktop.as_ref().unwrap();
        assert_eq!(desktop.package, "whisker-toggle-desktop-host");
        assert!(matches!(
            &desktop.source,
            ResolvedRustHostSource::Path(path) if path.ends_with("whisker-toggle/desktop")
        ));
        let web = toggle.web.as_ref().unwrap();
        assert_eq!(web.package, "whisker-toggle-web-host");
        assert!(matches!(
            &web.source,
            ResolvedRustHostSource::Path(path) if path.ends_with("whisker-toggle/web")
        ));
    }

    #[test]
    fn published_rust_host_falls_back_to_the_outer_package_version() {
        let contribution = resolve_rust_host_crate(
            "whisker-toggle",
            "1.2.3",
            "desktop",
            Path::new("/definitely/not/a/module/package"),
            Some(&RustHostSectionRaw {
                package: "whisker-toggle-desktop-host".into(),
            }),
        )
        .unwrap()
        .unwrap();
        assert_eq!(contribution.package, "whisker-toggle-desktop-host");
        assert_eq!(
            contribution.source,
            ResolvedRustHostSource::Registry {
                version: "1.2.3".into()
            }
        );
    }
}
