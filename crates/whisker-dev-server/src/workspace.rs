//! Workspace path-dep walker.
//!
//! Hot-reload coverage (#103): the file watcher and the patcher both
//! need to know which sub-crates the user app depends on so an edit
//! to e.g. `examples/podcast/crates/podcast-ui-kit/src/top_nav.rs`
//! produces a tier-1 patch instead of being silently ignored.
//!
//! [`discover_path_deps`] runs `cargo metadata` against the user
//! crate's manifest and returns every path-dep reachable from it,
//! including the user crate itself. "Path dep" here means a
//! workspace member (or `path = "..."` dep) — anything whose
//! `Package.source` is `None`. Registry deps (crates.io) and git
//! deps are excluded; their sources are out-of-tree and not worth
//! watching.
//!
//! The returned tuples carry the **rustc-form crate name** (hyphens
//! replaced with underscores) so callers can look them up in the
//! captured rustc-args map directly. The src dir is the absolute
//! path to `<manifest_dir>/src/` (the conventional location; binary
//! / examples-only crates that don't have `src/` are skipped at
//! watch time when notify refuses to attach).

use anyhow::{Context, Result};
use cargo_metadata::MetadataCommand;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// One path-dep crate as the dev loop sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathDepCrate {
    /// Rustc-style crate name (hyphens → underscores). This matches
    /// the key the `whisker-rustc-shim` writes into its capture cache.
    pub crate_name: String,
    /// Absolute path to the crate's `src/` directory. Used as a
    /// watch root and as a prefix for "which crate did this path
    /// come from?" lookups.
    pub src_dir: PathBuf,
}

/// Walk `cargo metadata` for the user crate at `manifest_path` and
/// return every path-dep reachable from it, including the root
/// package itself. Topologically ordered (parents before deps).
///
/// "Path dep" = `Package.source.is_none()`. That covers workspace
/// members and explicit `path = "..."` deps. Registry / git deps
/// are skipped — their sources live outside the workspace and
/// changes there would imply a `Cargo.lock` bump, which full reload
/// rebuild already covers.
pub fn discover_path_deps(manifest_path: &Path, app_package: &str) -> Result<Vec<PathDepCrate>> {
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
        .context("cargo metadata returned no resolve graph")?;
    let root_id = resolve
        .root
        .as_ref()
        .cloned()
        .or_else(|| {
            metadata
                .packages
                .iter()
                .find(|p| p.name == app_package)
                .map(|p| p.id.clone())
        })
        .with_context(|| format!("cargo package `{app_package}` not found in the workspace"))?;

    let mut out: Vec<PathDepCrate> = Vec::new();
    let mut visit: Vec<&cargo_metadata::PackageId> = vec![&root_id];
    let mut seen: HashSet<&cargo_metadata::PackageId> = HashSet::new();

    while let Some(pkg_id) = visit.pop() {
        if !seen.insert(pkg_id) {
            continue;
        }
        let Some(pkg) = metadata.packages.iter().find(|p| &p.id == pkg_id) else {
            continue;
        };
        // Registry / git deps have a `source`; we only watch deps
        // whose source is None (= workspace path-dep).
        if pkg.source.is_some() {
            continue;
        }
        if let Some(manifest_dir) = pkg.manifest_path.parent() {
            let src_dir = manifest_dir.join("src");
            out.push(PathDepCrate {
                crate_name: pkg.name.replace('-', "_"),
                src_dir: src_dir.into(),
            });
        }
        if let Some(node) = resolve.nodes.iter().find(|n| &n.id == pkg_id) {
            for dep in &node.deps {
                visit.push(&dep.pkg);
            }
        }
    }
    Ok(out)
}

/// Given a debounced batch of changed paths, return the rustc-form
/// crate name they all map to via longest-prefix match against
/// `crates`. Returns `None` if the batch spans multiple crates or
/// any path is outside every known src dir — the dev loop falls back
/// to a full reload in that case (we can patch one crate per
/// debounced batch, not two).
pub fn identify_crate_for_paths(paths: &[PathBuf], crates: &[PathDepCrate]) -> Option<String> {
    let mut found: Option<&str> = None;
    for p in paths {
        let hit = best_crate_for(p, crates)?;
        match found {
            None => found = Some(hit),
            Some(prev) if prev != hit => return None,
            _ => {}
        }
    }
    found.map(str::to_owned)
}

/// Longest-prefix match: pick the crate whose `src_dir` is the most
/// specific prefix of `path`. Handles the (rare) case where one
/// crate's manifest dir nests inside another — the inner crate wins.
fn best_crate_for<'a>(path: &Path, crates: &'a [PathDepCrate]) -> Option<&'a str> {
    let mut best: Option<(&str, usize)> = None;
    for c in crates {
        if path.starts_with(&c.src_dir) {
            let depth = c.src_dir.components().count();
            if best.map(|(_, d)| depth > d).unwrap_or(true) {
                best = Some((&c.crate_name, depth));
            }
        }
    }
    best.map(|(n, _)| n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cr(name: &str, dir: &str) -> PathDepCrate {
        PathDepCrate {
            crate_name: name.into(),
            src_dir: PathBuf::from(dir),
        }
    }

    #[test]
    fn identify_returns_the_matching_crate() {
        let crates = vec![
            cr("podcast", "/ws/examples/podcast/src"),
            cr(
                "podcast_ui_kit",
                "/ws/examples/podcast/crates/podcast-ui-kit/src",
            ),
        ];
        let paths = vec![PathBuf::from(
            "/ws/examples/podcast/crates/podcast-ui-kit/src/top_nav.rs",
        )];
        assert_eq!(
            identify_crate_for_paths(&paths, &crates),
            Some("podcast_ui_kit".into())
        );
    }

    #[test]
    fn identify_returns_none_when_paths_span_multiple_crates() {
        let crates = vec![
            cr("podcast", "/ws/examples/podcast/src"),
            cr(
                "podcast_ui_kit",
                "/ws/examples/podcast/crates/podcast-ui-kit/src",
            ),
        ];
        let paths = vec![
            PathBuf::from("/ws/examples/podcast/src/lib.rs"),
            PathBuf::from("/ws/examples/podcast/crates/podcast-ui-kit/src/top_nav.rs"),
        ];
        assert_eq!(identify_crate_for_paths(&paths, &crates), None);
    }

    #[test]
    fn identify_returns_none_when_no_crate_matches() {
        let crates = vec![cr("podcast", "/ws/examples/podcast/src")];
        let paths = vec![PathBuf::from("/some/unrelated/path/foo.rs")];
        assert_eq!(identify_crate_for_paths(&paths, &crates), None);
    }

    #[test]
    fn identify_picks_the_deeper_match_when_src_dirs_nest() {
        // Defensive: if a sub-crate's src_dir lives inside another
        // crate's src_dir (unusual but possible with custom layouts),
        // the inner crate wins.
        let crates = vec![
            cr("outer", "/ws/foo/src"),
            cr("inner", "/ws/foo/src/inner_pkg/src"),
        ];
        let paths = vec![PathBuf::from("/ws/foo/src/inner_pkg/src/lib.rs")];
        assert_eq!(
            identify_crate_for_paths(&paths, &crates),
            Some("inner".into())
        );
    }

    #[test]
    fn identify_handles_single_crate_batch() {
        let crates = vec![cr("podcast", "/ws/podcast/src")];
        let paths = vec![
            PathBuf::from("/ws/podcast/src/lib.rs"),
            PathBuf::from("/ws/podcast/src/main.rs"),
        ];
        assert_eq!(
            identify_crate_for_paths(&paths, &crates),
            Some("podcast".into())
        );
    }
}
