//! Host SDK sources shipped next to the resolved `whisker` crate.
//!
//! Cargo checks out the whole repository for git and path dependencies, so
//! the `platforms/` tree at the locked revision sits beside the `whisker`
//! crate. Building the Hosts from that tree keeps the native runtime on the
//! same revision as the Rust crates. Registry packages carry only the crate
//! and continue to use the published SDK.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Whisker crate directory depth below the repository root (`crates/whisker`).
const CRATE_DEPTH: usize = 2;

/// SwiftPM derives a path package's identity from its directory name. Module
/// packages pin the remote `whisker` package, and the local tree must share
/// that identity to replace it.
const SWIFT_PACKAGE_IDENTITY: &str = "whisker";

/// Return the repository root holding both Host source trees.
pub(crate) fn source_root(metadata: &cargo_metadata::Metadata) -> Option<PathBuf> {
    let selected = metadata.resolve.as_ref()?;
    metadata
        .packages
        .iter()
        .filter(|package| package.name == "whisker")
        .filter(|package| selected.nodes.iter().any(|node| node.id == package.id))
        .find_map(|package| find_from_crate(package.manifest_path.parent()?.as_std_path()))
}

/// Walk from the `whisker` crate directory up to the repository root.
pub(crate) fn find_from_crate(crate_dir: &Path) -> Option<PathBuf> {
    crate_dir
        .ancestors()
        .take(CRATE_DEPTH + 1)
        .find(|dir| is_source_root(dir))
        .map(Path::to_path_buf)
}

fn is_source_root(dir: &Path) -> bool {
    dir.join("platforms/ios/Sources/WhiskerRuntime").is_dir()
        && dir.join("platforms/android/runtime").is_dir()
}

/// Expose `root` to SwiftPM under the `whisker` package identity.
///
/// Cargo names git checkouts after the revision, so those trees are linked
/// from `<gen_dir>/.whisker-host-sdk/whisker`. The link lives outside the
/// platform directories that each sync rewrites.
pub(crate) fn swift_package_root(gen_dir: &Path, root: &Path) -> Result<PathBuf> {
    if root
        .file_name()
        .is_some_and(|name| name == SWIFT_PACKAGE_IDENTITY)
    {
        return Ok(root.to_path_buf());
    }
    let link = gen_dir
        .join(".whisker-host-sdk")
        .join(SWIFT_PACKAGE_IDENTITY);
    if std::fs::read_link(&link).is_ok_and(|target| target == root) {
        return Ok(link);
    }
    let parent = link.parent().expect("link has a parent");
    std::fs::create_dir_all(parent).with_context(|| format!("mkdir -p {}", parent.display()))?;
    if std::fs::symlink_metadata(&link).is_ok() {
        std::fs::remove_file(&link).with_context(|| format!("rm {}", link.display()))?;
    }
    link_dir(root, &link)?;
    Ok(link)
}

#[cfg(unix)]
fn link_dir(target: &Path, link: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link)
        .with_context(|| format!("ln -s {} {}", target.display(), link.display()))
}

#[cfg(windows)]
fn link_dir(target: &Path, link: &Path) -> Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
        .with_context(|| format!("mklink /D {} {}", link.display(), target.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn tempdir() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "whisker-host-sdk-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn checkout(root: &Path) {
        std::fs::create_dir_all(root.join("crates/whisker")).unwrap();
        std::fs::create_dir_all(root.join("platforms/ios/Sources/WhiskerRuntime")).unwrap();
        std::fs::create_dir_all(root.join("platforms/android/runtime")).unwrap();
    }

    #[test]
    fn finds_host_sources_beside_the_whisker_crate() {
        let tmp = tempdir();
        let root = tmp.join("4c4fdcf");
        checkout(&root);
        assert_eq!(find_from_crate(&root.join("crates/whisker")), Some(root));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn registry_crates_have_no_host_sources() {
        let tmp = tempdir();
        let crate_dir = tmp.join("registry/whisker-0.1.0");
        std::fs::create_dir_all(&crate_dir).unwrap();
        assert_eq!(find_from_crate(&crate_dir), None);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn git_dependency_graph_resolves_its_host_sources() {
        let tmp = tempdir();
        let root = tmp.join("4c4fdcf");
        checkout(&root);
        std::fs::write(
            root.join("crates/whisker/Cargo.toml"),
            "[package]\nname = 'whisker'\nversion = '0.0.0'\nedition = '2024'\n[lib]\npath = 'lib.rs'\n",
        )
        .unwrap();
        std::fs::write(root.join("crates/whisker/lib.rs"), "").unwrap();
        let app = tmp.join("app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(
            app.join("Cargo.toml"),
            "[package]\nname = 'app'\nversion = '0.0.0'\nedition = '2024'\n[lib]\npath = 'lib.rs'\n[dependencies]\nwhisker = { path = '../4c4fdcf/crates/whisker' }\n[workspace]\n",
        )
        .unwrap();
        std::fs::write(app.join("lib.rs"), "").unwrap();
        let graph = crate::ProjectDependencyGraph::resolve(&app.join("Cargo.toml"), "app").unwrap();
        assert_eq!(
            graph.host_sdk.map(|p| p.canonicalize().unwrap()),
            Some(root.canonicalize().unwrap())
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn swift_aggregator_uses_the_local_host_package() {
        let tmp = tempdir();
        let root = tmp.join("whisker");
        checkout(&root);
        let files =
            crate::ios_modules::module_files(&[], &root, crate::modules::ModulePlatform::Ios);
        let manifest = files
            .iter()
            .find(|(path, _)| path.as_str() == "whisker_modules/Package.swift")
            .map(|(_, entry)| String::from_utf8(entry.to_bytes().unwrap()).unwrap())
            .unwrap();
        assert!(manifest.contains(&format!(
            ".package(name: \"whisker\", path: {:?})",
            root.display().to_string()
        )));
        assert!(!manifest.contains(crate::ios_modules::WHISKER_IOS_SPM_URL));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn swift_package_keeps_the_whisker_identity() {
        let tmp = tempdir();
        let named = tmp.join("whisker");
        checkout(&named);
        let gen_dir = tmp.join("gen");
        assert_eq!(swift_package_root(&gen_dir, &named).unwrap(), named);

        let revision = tmp.join("4c4fdcf");
        checkout(&revision);
        let link = swift_package_root(&gen_dir, &revision).unwrap();
        assert_eq!(link.file_name().unwrap(), SWIFT_PACKAGE_IDENTITY);
        assert_eq!(std::fs::read_link(&link).unwrap(), revision);

        let next = tmp.join("2ccc7c5");
        checkout(&next);
        let link = swift_package_root(&gen_dir, &next).unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), next);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
