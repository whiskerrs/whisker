//! Whisker module-system v1 — discovery + manifest parsing.
//!
//! When `whisker run` / `whisker build` builds an app crate for
//! iOS or Android, every cargo dependency may optionally contribute
//! native code (Obj-C / Obj-C++ on iOS, Kotlin / JNI on Android) to
//! the final host binary by shipping a `whisker.module.toml` next
//! to its `Cargo.toml`. The Whisker CLI walks the consuming app's
//! dep graph via `cargo metadata`, parses each module's manifest,
//! and feeds the per-platform source paths through to the
//! platform-specific build step.
//!
//! This module is platform-neutral — it just produces the
//! `ResolvedModule` list. `whisker-build::ios` and
//! `whisker-build::android` consume the list and decide what to do
//! with the paths (cc::Build on iOS, gradle source-set on Android).
//!
//! ## Schema v1
//!
//! ```toml
//! # packages/whisker-hello-element/whisker.module.toml
//! [ios]
//! native_sources = ["src/native/whisker_hello_element.mm"]
//!
//! [android]
//! # not used yet
//! ```
//!
//! All paths are resolved relative to the directory containing the
//! manifest. The resolver returns absolute paths so the downstream
//! cc::Build / gradle invocations don't have to know about the
//! module's source layout.
//!
//! Future extensions (e.g. exported headers, link flags, cargo
//! feature gates) land additively without breaking modules pinned
//! to the v1 schema.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use cargo_metadata::MetadataCommand;
use serde::Deserialize;

/// Top-level shape of `whisker.module.toml`.
///
/// Every section is optional so a module can declare just the
/// platform(s) it supports. Unknown sections / fields are rejected
/// to catch typos early (we don't want a typoed `iOS` section to
/// silently produce a no-op build).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestRaw {
    #[serde(default)]
    pub ios: Option<IosSectionRaw>,
    #[serde(default)]
    pub android: Option<AndroidSectionRaw>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IosSectionRaw {
    /// Paths to `.m` / `.mm` source files that should be compiled
    /// into the host dylib alongside the bridge code. Paths are
    /// relative to the manifest's directory.
    #[serde(default)]
    pub native_sources: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidSectionRaw {
    // Placeholder. Android module support lands in Phase 7-Φ.C.
    // Keeping the section parsed (rather than rejecting) so a
    // module can declare its Android intent today without breaking
    // the iOS build.
}

/// A single discovered module after its manifest has been resolved
/// against the cargo dep tree. `package` carries the cargo crate
/// name (handy for diagnostics) and `manifest_dir` is the absolute
/// path of the directory the `whisker.module.toml` lives in.
#[derive(Debug, Clone)]
pub struct ResolvedModule {
    pub package: String,
    pub manifest_dir: PathBuf,
    /// Absolute, existence-checked paths to `.m` / `.mm` sources.
    /// Empty when the module declares no iOS contributions.
    pub ios_native_sources: Vec<PathBuf>,
}

/// Walk the cargo dep graph of `app_package` (resolved at
/// `manifest_path`) and return every dependency that carries a
/// `whisker.module.toml`.
///
/// Ordering: `cargo metadata`'s topological order, deduplicated by
/// package id (a diamond dep landed twice gets resolved once).
/// Downstream consumers can rely on a stable order across calls
/// for the same workspace state.
///
/// Errors:
/// - `cargo metadata` failure (workspace broken, manifest_path
///   invalid, etc.) propagates with the `cargo_metadata` error.
/// - Manifest parse failure (`whisker.module.toml` exists but
///   invalid toml or has unknown sections) propagates with the
///   offending file path attached.
/// - Native-source path referenced in a manifest but not present
///   on disk errors with the missing absolute path attached. We
///   prefer eager failure over silently skipping — a missing source
///   almost certainly means the module's `whisker.module.toml` is
///   out of sync with its `src/native/` layout.
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

    // Find the resolved dep tree rooted at `app_package`. We walk
    // the resolution graph (which encodes activated features /
    // platform deps) rather than `metadata.packages` directly, so
    // we only see deps that would actually be linked into the app.
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

    // BFS the resolution graph. The `nodes` list keys by package
    // id; `deps` carries forward edges.
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
        // Don't include the root app itself — by convention an app
        // declares native sources directly, not through a sibling
        // `whisker.module.toml`.
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
        // Manifest dir = directory containing this dep's Cargo.toml.
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
        let manifest_file = manifest_dir.join("whisker.module.toml");
        if !manifest_file.is_file() {
            continue;
        }
        let raw_text = std::fs::read_to_string(&manifest_file)
            .with_context(|| format!("read {}", manifest_file.display()))?;
        let manifest: ManifestRaw = toml::from_str(&raw_text)
            .with_context(|| format!("parse {}", manifest_file.display()))?;
        let mut ios_sources: Vec<PathBuf> = Vec::new();
        if let Some(ios) = manifest.ios {
            for raw_path in ios.native_sources {
                let resolved_path = manifest_dir.join(&raw_path);
                let canonical = resolved_path.canonicalize().with_context(|| {
                    format!(
                        "module `{}` declares ios.native_sources = [..., {raw_path:?}] in {} \
                         but {} does not exist",
                        pkg.name,
                        manifest_file.display(),
                        resolved_path.display(),
                    )
                })?;
                ios_sources.push(canonical);
            }
        }
        resolved.push(ResolvedModule {
            package: pkg.name.clone(),
            manifest_dir,
            ios_native_sources: ios_sources,
        });
    }

    // Stable ordering by package name so two consecutive runs produce
    // the same WHISKER_IOS_MODULE_NATIVE_SOURCES env var value (and
    // cargo doesn't re-run the bridge build for spurious env-change
    // reasons).
    resolved.sort_by(|a, b| a.package.cmp(&b.package));
    Ok(resolved)
}

/// Flatten every discovered module's iOS sources into a single
/// colon-separated string, suitable for passing as the
/// `WHISKER_IOS_MODULE_NATIVE_SOURCES` env var to a cargo build.
/// Modules are kept in `discover`'s sort order; within a module
/// sources are kept in the manifest's declaration order.
pub fn ios_sources_env_value(modules: &[ResolvedModule]) -> String {
    let mut paths: Vec<String> = Vec::new();
    for m in modules {
        for p in &m.ios_native_sources {
            paths.push(p.to_string_lossy().into_owned());
        }
    }
    paths.join(":")
}
