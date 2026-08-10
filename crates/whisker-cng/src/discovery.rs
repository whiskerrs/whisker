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
//! Discovery, rather than explicit registration, is what makes "which
//! plugins run" a function of the app's dep graph alone: a plugin's
//! `PluginConfig::NAME` matches the table key, so the CLI never needs
//! an `Engine::register(...)` call site for a 3rd-party plugin.
//!
//! This module resolves declarations only, never binaries. It parallels
//! [`whisker_build::modules::discover`] over the same cargo metadata,
//! but lives here because `whisker-build` already depends on
//! `whisker-cng` and the reverse would cycle.

use anyhow::{Context, Result, anyhow};
use cargo_metadata::MetadataCommand;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A plugin declared by a dep of the user app, after the dep's
/// `[package.metadata.whisker.plugins.<name>]` table has been
/// resolved against the cargo dep graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPlugin {
    /// The plugin's stable name. Matches `Plugin::name()` /
    /// `PluginConfig::NAME` and the `Config.plugins` map key.
    pub name: String,
    /// The cargo package that declared this plugin — anchors
    /// `bin_target_name` to a specific `cargo build --bin <target>
    /// --package <source_crate>` invocation.
    pub source_crate: String,
    /// The directory containing the source crate's `Cargo.toml`.
    pub source_manifest_dir: PathBuf,
    /// `[[bin]]` target name inside the source crate that, when
    /// compiled, produces the plugin binary the engine spawns.
    /// Resolving it to a file path is the caller's job.
    pub bin_target_name: String,
    pub after: Vec<String>,
    pub before: Vec<String>,
}

/// Walk the cargo dep graph rooted at `app_package` (resolved via
/// `manifest_path`) and return every plugin declared in the
/// transitive deps' `[package.metadata.whisker.plugins.<name>]`
/// tables.
///
/// The iteration order is deterministic for a given dep tree but not
/// part of the stability contract — callers that need a specific
/// ordering should sort by `name` themselves.
///
/// Errors on a `cargo metadata` failure, a parse error in a plugin
/// entry, or the same plugin `name` declared by two different crates
/// (which has no disambiguation at dispatch time).
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

    // Skip the root app itself — a CNG plugin lives in a dep, not in
    // the consuming app.
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

        // Read only the `plugins` sub-table, untyped, so the sibling
        // `[ios]` / `[android]` module schema that
        // `whisker_build::modules::discover` owns can evolve
        // independently.
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

/// Shape of one `[package.metadata.whisker.plugins.<name>]` entry.
/// `deny_unknown_fields` so a typoed key surfaces instead of getting
/// silently dropped.
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

#[cfg(test)]
mod tests {
    use super::*;

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
