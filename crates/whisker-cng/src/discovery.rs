//! Plugin discovery — walks a user crate's cargo dep graph and
//! returns every dependency declaring a Whisker CNG plugin in its
//! `Cargo.toml`'s `[package.metadata.whisker.plugins.<name>]` table.
//!
//! ## Schema
//!
//! A plugin crate's `Cargo.toml` opts in like this:
//!
//! ```toml
//! [package.metadata.whisker.plugins.my-plugin]
//! bin = "my-plugin-cng"          # the [[bin]] name in this crate
//! after = ["whisker-info-plist"] # optional ordering hints
//! before = []
//! ```
//!
//! One crate may declare multiple plugins by adding more entries
//! under `plugins.<name>`. The Whisker CLI consumes the resulting
//! [`DiscoveredPlugin`] list, builds each plugin's `[[bin]]` target
//! through `cargo build`, and registers a
//! [`crate::SubprocessPlugin`] pointing at the resulting binary.
//!
//! ## Why discovery instead of explicit registration
//!
//! In Phase 2+ the user's `whisker.rs` will declare typed plugin
//! Configs via `app.plugin::<MyConfig>(|c| ...)`. The Config type's
//! `PluginConfig::NAME` matches the discovery table's key, so
//! "what plugin runs" is decided entirely by what crates the app
//! depends on plus how the user spelled `app.plugin::<…>(…)`. The
//! CLI never needs an `Engine::register(...)` call site for
//! 3rd-party plugins.
//!
//! ## Scope (Phase 1 PR 3c)
//!
//! This module is metadata-only: it discovers the *declarations*,
//! not the binaries. Building each plugin's bin target + wiring it
//! into [`crate::Engine`] happens in a downstream PR (likely as
//! part of `whisker-build` once it gets a plugin-build step).
//!
//! Shared with [`whisker_build::modules::discover`] in spirit: both
//! walk the same cargo metadata. Kept here rather than there to
//! avoid a circular dep — `whisker-build` already depends on
//! `whisker-cng` via the transitive sync calls.

use anyhow::{anyhow, Context, Result};
use cargo_metadata::MetadataCommand;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ============================================================================
// Public types
// ============================================================================

/// A plugin declared by a dep of the user app, after the dep's
/// `[package.metadata.whisker.plugins.<name>]` table has been
/// resolved against the cargo dep graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPlugin {
    /// The plugin's stable name. Matches `Plugin::name()` /
    /// `PluginConfig::NAME` and the `AppConfig.plugins` map key.
    pub name: String,
    /// The cargo package that declared this plugin. Used for
    /// diagnostic messages and to anchor `bin_target_name` to a
    /// specific `cargo build --bin <target> --package <source_crate>`
    /// invocation.
    pub source_crate: String,
    /// The directory containing the source crate's `Cargo.toml`.
    /// Mirrors `whisker_build::modules::ResolvedModule::manifest_dir`.
    pub source_manifest_dir: PathBuf,
    /// `[[bin]]` target name inside the source crate that, when
    /// compiled, produces the plugin binary the engine spawns.
    /// Resolution to an actual file path is the caller's job (it
    /// needs cargo build + target-dir lookup).
    pub bin_target_name: String,
    pub after: Vec<String>,
    pub before: Vec<String>,
}

// ============================================================================
// Public API
// ============================================================================

/// Walk the cargo dep graph rooted at `app_package` (resolved via
/// `manifest_path`) and return every plugin declared in the
/// transitive deps' `[package.metadata.whisker.plugins.<name>]`
/// tables.
///
/// The iteration order is deterministic for a given dep tree
/// (DFS over `cargo metadata`'s resolution graph + alphabetical
/// within each crate's plugin table), but not part of the
/// stability contract — downstream consumers that need a
/// specific ordering should sort by `name` themselves.
///
/// Two plugins with the same `name` across different deps is a
/// hard error — there's no way to disambiguate them at dispatch
/// time.
///
/// Errors:
/// - `cargo metadata` failure (workspace broken, manifest_path
///   invalid).
/// - Plugin metadata parse error (unknown / typoed field under
///   `plugins.<name>`) — `deny_unknown_fields` is on for the
///   per-plugin entry so typos surface immediately.
/// - Duplicate plugin name across two crates.
pub fn discover_plugins(manifest_path: &Path, app_package: &str) -> Result<Vec<DiscoveredPlugin>> {
    let metadata = MetadataCommand::new()
        .manifest_path(manifest_path)
        .exec()
        .with_context(|| {
            format!(
                "cargo metadata failed for {} (package: {app_package})",
                manifest_path.display(),
            )
        })?;

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

    // DFS the resolve graph collecting dep package ids. Skip the
    // root app itself — a CNG plugin lives in a dep, not in the
    // consuming app.
    let mut visit: Vec<&cargo_metadata::PackageId> = vec![&root_id];
    let mut seen: std::collections::HashSet<&cargo_metadata::PackageId> = Default::default();
    let mut dep_ids: Vec<cargo_metadata::PackageId> = Vec::new();
    while let Some(pkg_id) = visit.pop() {
        if !seen.insert(pkg_id) {
            continue;
        }
        if let Some(node) = resolve.nodes.iter().find(|n| &n.id == pkg_id) {
            for d in &node.deps {
                visit.push(&d.pkg);
            }
        }
        if pkg_id != &root_id {
            dep_ids.push(pkg_id.clone());
        }
    }

    let mut discovered: Vec<DiscoveredPlugin> = Vec::new();
    for id in dep_ids {
        let pkg = metadata
            .packages
            .iter()
            .find(|p| p.id == id)
            .expect("dep id came from resolve.nodes, must exist in metadata.packages");

        // The `whisker` table may also carry `[ios]` / `[android]`
        // module sections that `whisker_build::modules::discover`
        // consumes. We only care about the `plugins` sub-table here
        // and read it as a serde_json::Value so the module schema
        // can evolve independently.
        let Some(whisker_meta) = pkg.metadata.get("whisker") else {
            continue;
        };
        let Some(plugins_value) = whisker_meta.get("plugins") else {
            continue;
        };

        let plugins_map: BTreeMap<String, PluginEntryRaw> =
            serde_json::from_value(plugins_value.clone()).with_context(|| {
                format!(
                    "parse [package.metadata.whisker.plugins] in {}",
                    pkg.manifest_path,
                )
            })?;

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

        for (name, entry) in plugins_map {
            discovered.push(DiscoveredPlugin {
                name,
                source_crate: pkg.name.clone(),
                source_manifest_dir: manifest_dir.clone(),
                bin_target_name: entry.bin,
                after: entry.after,
                before: entry.before,
            });
        }
    }

    check_no_duplicate_names(&discovered)?;
    Ok(discovered)
}

// ============================================================================
// Internal
// ============================================================================

/// Shape of one `[package.metadata.whisker.plugins.<name>]` entry.
/// `deny_unknown_fields` so a typoed key (e.g. `after`s plural
/// confusion) surfaces immediately rather than getting silently
/// dropped.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginEntryRaw {
    bin: String,
    #[serde(default)]
    after: Vec<String>,
    #[serde(default)]
    before: Vec<String>,
}

fn check_no_duplicate_names(plugins: &[DiscoveredPlugin]) -> Result<()> {
    let mut seen: BTreeMap<&str, &DiscoveredPlugin> = BTreeMap::new();
    for p in plugins {
        if let Some(prior) = seen.insert(p.name.as_str(), p) {
            return Err(anyhow!(
                "plugin name `{}` declared by both `{}` and `{}` — \
                 plugin names must be globally unique across the dep graph. \
                 Rename one of the `[package.metadata.whisker.plugins.<name>]` \
                 entries.",
                p.name,
                prior.source_crate,
                p.source_crate,
            ));
        }
    }
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Most discovery testing happens end-to-end against a tempdir
    // workspace fixture (`tests/discovery.rs`). The unit tests here
    // cover the pure pieces — duplicate detection + the typed-shape
    // deserializer.

    fn p(
        name: &str,
        source_crate: &str,
        bin: &str,
        after: &[&str],
        before: &[&str],
    ) -> DiscoveredPlugin {
        DiscoveredPlugin {
            name: name.into(),
            source_crate: source_crate.into(),
            source_manifest_dir: PathBuf::from("/fake"),
            bin_target_name: bin.into(),
            after: after.iter().map(|s| s.to_string()).collect(),
            before: before.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn duplicate_check_passes_with_distinct_names() {
        let list = vec![
            p("a", "crate-a", "bin-a", &[], &[]),
            p("b", "crate-b", "bin-b", &[], &[]),
        ];
        check_no_duplicate_names(&list).unwrap();
    }

    #[test]
    fn duplicate_check_fails_with_two_crates_claiming_the_same_name() {
        let list = vec![
            p("conflict", "crate-a", "bin-a", &[], &[]),
            p("conflict", "crate-b", "bin-b", &[], &[]),
        ];
        let err = check_no_duplicate_names(&list).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("conflict"), "{msg}");
        assert!(msg.contains("crate-a"), "{msg}");
        assert!(msg.contains("crate-b"), "{msg}");
    }

    #[test]
    fn entry_deserializes_with_only_bin() {
        let v: PluginEntryRaw =
            serde_json::from_value(serde_json::json!({"bin": "only-bin"})).unwrap();
        assert_eq!(v.bin, "only-bin");
        assert!(v.after.is_empty());
        assert!(v.before.is_empty());
    }

    #[test]
    fn entry_deserializes_with_all_fields() {
        let v: PluginEntryRaw = serde_json::from_value(serde_json::json!({
            "bin": "my-bin",
            "after": ["a", "b"],
            "before": ["c"],
        }))
        .unwrap();
        assert_eq!(v.bin, "my-bin");
        assert_eq!(v.after, vec!["a", "b"]);
        assert_eq!(v.before, vec!["c"]);
    }

    #[test]
    fn entry_rejects_unknown_fields() {
        let err = serde_json::from_value::<PluginEntryRaw>(serde_json::json!({
            "bin": "x",
            "aftr": ["typo-of-after"],
        }))
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("aftr"), "{msg}");
    }

    #[test]
    fn entry_rejects_missing_bin() {
        let err = serde_json::from_value::<PluginEntryRaw>(serde_json::json!({
            "after": [],
        }))
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("bin"), "{msg}");
    }
}
