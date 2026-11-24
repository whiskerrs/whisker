//! The Cargo target directory a workspace builds into.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Workspace root and the `CARGO_TARGET_DIR` in effect when it was resolved.
type CacheKey = (PathBuf, Option<std::ffi::OsString>);

/// Directory Cargo writes `workspace_root`'s build outputs to.
///
/// Asks `cargo metadata`, so `CARGO_TARGET_DIR` and `build.target-dir` are
/// honored; falls back to `<workspace_root>/target` without a readable manifest.
pub fn cargo_target_dir(workspace_root: &Path) -> PathBuf {
    static CACHE: OnceLock<Mutex<HashMap<CacheKey, PathBuf>>> = OnceLock::new();
    let key = (
        workspace_root.to_path_buf(),
        std::env::var_os("CARGO_TARGET_DIR"),
    );
    let cache = CACHE.get_or_init(Default::default);
    if let Some(dir) = cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return dir;
    }
    let dir = query(workspace_root).unwrap_or_else(|| workspace_root.join("target"));
    if let Ok(mut c) = cache.lock() {
        c.insert(key, dir.clone());
    }
    dir
}

fn query(workspace_root: &Path) -> Option<PathBuf> {
    let manifest = workspace_root.join("Cargo.toml");
    if !manifest.is_file() {
        return None;
    }
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(manifest)
        .no_deps()
        .exec()
        .ok()?;
    Some(metadata.target_directory.into_std_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("whisker-target-dir-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "").unwrap();
        root
    }

    #[test]
    fn follows_build_target_dir_config() {
        let root = workspace("config");
        std::fs::create_dir_all(root.join(".cargo")).unwrap();
        std::fs::write(
            root.join(".cargo/config.toml"),
            "[build]\ntarget-dir = \"shared-target\"\n",
        )
        .unwrap();
        if std::env::var_os("CARGO_TARGET_DIR").is_none() {
            assert_eq!(
                cargo_target_dir(&root),
                std::fs::canonicalize(&root).unwrap().join("shared-target")
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn defaults_to_workspace_target_without_manifest() {
        let root =
            std::env::temp_dir().join(format!("whisker-target-dir-none-{}", std::process::id()));
        assert_eq!(cargo_target_dir(&root), root.join("target"));
    }
}
