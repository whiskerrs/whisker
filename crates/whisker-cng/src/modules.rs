//! Whisker module-system — discovery + manifest parsing.
//!
//! A dependency becomes a Whisker module by declaring its supported
//! platforms under `[package.metadata.whisker.module.platforms]`.
//! Each platform is either implemented entirely by the common Rust
//! crate (`kind = "common"`) or points at one explicit Host manifest.
//! CNG resolves those declarations from Cargo's dependency graph and
//! feeds each Host contribution to the corresponding build system.
//!
//! This module is platform-neutral — it just produces the
//! `ResolvedModule` list. CNG renderers turn the list into complete
//! Host-project dependencies; build orchestration only compiles the
//! generated project.
//!
//! ## Schema
//!
//! ```toml
//! [package.metadata.whisker.module.platforms]
//! android = { manifest = "build.gradle.kts" }
//! ios = { manifest = "Package.swift" }
//! web = { manifest = "web/Cargo.toml" }
//! desktop = { kind = "common" }
//! ```
//!
//! All paths are resolved relative to the directory containing the
//! manifest (the crate's `Cargo.toml`). The resolver returns
//! absolute paths so downstream build-system integration does not
//! have to know where Cargo originally found the module.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use cargo_metadata::{Metadata, MetadataCommand};
use serde::Serialize;
use sha2::{Digest, Sha256};

mod platforms;

use platforms::{ManifestRaw, resolve_legacy_platforms, resolve_platforms};
pub use platforms::{
    ModulePlatform, NativeManifestKind, ResolvedModulePlatforms, ResolvedNativeManifest,
    ResolvedPlatformImplementation, ResolvedRustHostSource, ResolvedRustModuleContribution,
};

/// A single discovered module after its metadata has been resolved
/// against the cargo dep tree. `package` carries the cargo crate
/// name (handy for diagnostics) and `manifest_dir` is the absolute
/// path of the directory the crate's `Cargo.toml` lives in.
#[derive(Debug, Clone, Serialize)]
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
    /// Explicit support and implementation selected for each Host.
    pub platforms: ResolvedModulePlatforms,
}

impl ResolvedModule {
    pub fn rust_host(&self, platform: ModulePlatform) -> Option<&ResolvedRustModuleContribution> {
        let implementation = match platform {
            ModulePlatform::Macos => self
                .platforms
                .macos
                .as_ref()
                .or(self.platforms.desktop.as_ref()),
            ModulePlatform::Windows => self
                .platforms
                .windows
                .as_ref()
                .or(self.platforms.desktop.as_ref()),
            ModulePlatform::Linux => self
                .platforms
                .linux
                .as_ref()
                .or(self.platforms.desktop.as_ref()),
            ModulePlatform::Android => self.platforms.android.as_ref(),
            ModulePlatform::Ios => self.platforms.ios.as_ref(),
            ModulePlatform::Web => self.platforms.web.as_ref(),
            ModulePlatform::Desktop => self.platforms.desktop.as_ref(),
        }?;
        match implementation {
            ResolvedPlatformImplementation::RustHost(host) => Some(host),
            ResolvedPlatformImplementation::Common
            | ResolvedPlatformImplementation::NativeManifest(_) => None,
        }
    }

    pub fn native_manifest(&self, platform: ModulePlatform) -> Option<&ResolvedNativeManifest> {
        let implementation = match platform {
            ModulePlatform::Android => self.platforms.android.as_ref(),
            ModulePlatform::Ios => self.platforms.ios.as_ref(),
            _ => None,
        }?;
        match implementation {
            ResolvedPlatformImplementation::NativeManifest(manifest) => Some(manifest),
            ResolvedPlatformImplementation::Common
            | ResolvedPlatformImplementation::RustHost(_) => None,
        }
    }
}

/// Walk the cargo dep graph of `app_package` (resolved at
/// `manifest_path`) and return every dependency that declares a
/// `[package.metadata.whisker.module.platforms]` table in its `Cargo.toml`.
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
/// - A declared manifest that is absent or has the wrong kind/package name
///   errors eagerly rather than silently dropping a Host implementation.
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
    discover_from_metadata(&metadata, app_package)
}

pub(crate) fn discover_from_metadata(
    metadata: &Metadata,
    app_package: &str,
) -> Result<Vec<ResolvedModule>> {
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
        // module metadata.
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
        // `module` is the current opt-in. During the package-by-package
        // migration, an old platform field or a conventional native manifest
        // also identifies a legacy module. `plugins` alone never does.
        let Some(whisker_meta) = pkg.metadata.get("whisker") else {
            continue;
        };
        let manifest: ManifestRaw =
            serde_json::from_value(whisker_meta.clone()).with_context(|| {
                format!("parse [package.metadata.whisker] in {}", pkg.manifest_path,)
            })?;
        let package_version = pkg.version.to_string();
        let has_legacy_fields = manifest.ios.is_some()
            || manifest.android.is_some()
            || manifest.desktop.is_some()
            || manifest.web.is_some();
        let has_conventional_native_manifest = manifest_dir.join("build.gradle.kts").is_file()
            || manifest_dir.join("Package.swift").is_file();
        if manifest.module.is_none() && !has_legacy_fields && !has_conventional_native_manifest {
            continue;
        }
        if manifest.module.is_some() && has_legacy_fields {
            return Err(anyhow!(
                "module `{}` mixes [package.metadata.whisker.module.platforms] with legacy metadata.whisker platform tables",
                pkg.name,
            ));
        }

        let platforms = if let Some(module) = manifest.module.as_ref() {
            resolve_platforms(
                &pkg.name,
                &package_version,
                pkg.source
                    .as_ref()
                    .is_some_and(|source| source.repr.starts_with("registry+")),
                &manifest_dir,
                &module.platforms,
            )?
        } else {
            resolve_legacy_platforms(&pkg.name, &package_version, &manifest_dir, &manifest)?
        };
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
            platforms,
        });
    }

    // Stable order keeps the gen tree byte-identical between runs, so
    // gradle / cargo don't re-run downstream tasks on a permutation.
    resolved.sort_by(|a, b| a.package.cmp(&b.package));
    Ok(resolved)
}

/// Flatten every discovered module's Android Kotlin sources into a
/// colon-separated string. The Android orchestration uses these
/// paths to extend Gradle's main source set.
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
    /// Android-side surface. `None` when Android is unsupported or common-only.
    pub android: Option<AndroidModuleReport>,
    /// iOS-side surface. `None` when iOS is unsupported or common-only.
    pub ios: Option<IosModuleReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AndroidModuleReport {
    /// Absolute path the Gradle Settings Plugin uses for
    /// `settings.project(":<crate>").projectDir = file(this)`.
    /// This is the directory containing the declared `build.gradle.kts`.
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
    let resolved = discover(&workspace_root.join("Cargo.toml"), user_package)
        .with_context(|| format!("discover modules for `{user_package}`"))?;
    write_gradle_module_cache(workspace_root, user_package, &resolved)
}

/// Write the Gradle module cache from an already-resolved CNG graph.
pub fn write_gradle_module_cache(
    workspace_root: &Path,
    user_package: &str,
    resolved: &[ResolvedModule],
) -> Result<()> {
    let report = build_modules_report_from_resolved(workspace_root, user_package, resolved)?;
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
/// Platform availability comes from the resolved module declaration, never
/// from directory guessing. Legacy source lists remain reportable during the
/// package migration.
pub fn build_modules_report(workspace_root: &Path, user_package: &str) -> Result<ModulesReport> {
    let manifest_path = workspace_root.join("Cargo.toml");
    let resolved = discover(&manifest_path, user_package)
        .with_context(|| format!("discover modules for `{user_package}`"))?;

    build_modules_report_from_resolved(workspace_root, user_package, &resolved)
}

fn build_modules_report_from_resolved(
    workspace_root: &Path,
    user_package: &str,
    resolved: &[ResolvedModule],
) -> Result<ModulesReport> {
    let lock_path = workspace_root.join("Cargo.lock");
    let cargo_lock_sha256 =
        sha256_file(&lock_path).with_context(|| format!("hash {}", lock_path.display()))?;

    let modules: Vec<ModulesReportModule> = resolved
        .iter()
        .cloned()
        .map(|m| {
            let android = m
                .native_manifest(ModulePlatform::Android)
                .filter(|manifest| manifest.kind == NativeManifestKind::Gradle)
                .map(|manifest| AndroidModuleReport {
                    subproject_dir: manifest
                        .path
                        .parent()
                        .expect("native manifest has a parent")
                        .to_path_buf(),
                    behaviors_class: crate_to_behaviors_class(&m.package),
                });
            let swift_manifest = m
                .native_manifest(ModulePlatform::Ios)
                .filter(|manifest| manifest.kind == NativeManifestKind::SwiftPm);
            let ios = if swift_manifest.is_some() || !m.ios_swift_sources.is_empty() {
                Some(IosModuleReport {
                    swift_module: if swift_manifest.is_some() {
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
    use std::sync::atomic::{AtomicU64, Ordering};

    fn tempdir() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "whisker-module-discovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn plugin_only_dependency_is_not_a_module() {
        let root = tempdir();
        std::fs::create_dir_all(root.join("app/src")).unwrap();
        std::fs::create_dir_all(root.join("plugin/src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"app\", \"plugin\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("app/Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nplugin = { path = \"../plugin\" }\n",
        )
        .unwrap();
        std::fs::write(root.join("app/src/lib.rs"), "pub fn app() {}\n").unwrap();
        std::fs::write(
            root.join("plugin/Cargo.toml"),
            "[package]\nname = \"plugin\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[package.metadata.whisker.plugins.example]\nbin = \"example-plugin\"\n",
        )
        .unwrap();
        std::fs::write(root.join("plugin/src/lib.rs"), "pub fn plugin() {}\n").unwrap();

        let modules = discover(&root.join("Cargo.toml"), "app").unwrap();
        assert!(modules.is_empty());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn desktop_is_the_fallback_for_os_specific_hosts() {
        let module = ResolvedModule {
            package: "whisker-map".into(),
            manifest_dir: PathBuf::from("/module"),
            ios_swift_sources: Vec::new(),
            android_kotlin_sources: Vec::new(),
            android_jni_sources: Vec::new(),
            platforms: ResolvedModulePlatforms {
                desktop: Some(ResolvedPlatformImplementation::RustHost(
                    ResolvedRustModuleContribution {
                        package: "whisker-map-desktop".into(),
                        source: ResolvedRustHostSource::Registry {
                            version: "1.2.3".into(),
                        },
                    },
                )),
                ..Default::default()
            },
        };
        assert_eq!(
            module.rust_host(ModulePlatform::Macos).unwrap().package,
            "whisker-map-desktop"
        );
    }

    #[test]
    fn discovers_rfc0004_rust_host_crates_by_convention() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .join("Cargo.toml");
        let modules = discover(&workspace, "host-smoke").unwrap();
        let svg = modules
            .iter()
            .find(|module| module.package == "whisker-svg")
            .unwrap();
        let desktop = svg.rust_host(ModulePlatform::Desktop).unwrap();
        assert_eq!(desktop.package, "whisker-svg-desktop-host");
        assert!(matches!(
            &desktop.source,
            ResolvedRustHostSource::Path(path) if path.ends_with("whisker-svg/desktop")
        ));
        let web = svg.rust_host(ModulePlatform::Web).unwrap();
        assert_eq!(web.package, "whisker-svg-web-host");
        assert!(matches!(
            &web.source,
            ResolvedRustHostSource::Path(path) if path.ends_with("whisker-svg/web")
        ));
    }

    #[test]
    fn discovers_router_history_as_a_service_only_web_host() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .join("Cargo.toml");
        let modules = discover(&workspace, "whisker-router-example").unwrap();
        let router = modules
            .iter()
            .find(|module| module.package == "whisker-router")
            .unwrap();
        let web = router.rust_host(ModulePlatform::Web).unwrap();
        assert_eq!(web.package, "whisker-router-web-host");
        assert!(matches!(
            &web.source,
            ResolvedRustHostSource::Path(path) if path.ends_with("whisker-router/web")
        ));
    }
}
