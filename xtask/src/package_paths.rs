use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

const MARKERS: &[&str] = &["include_str!(", "include_bytes!(", "include!(", "#[path"];

/// Every file a source names through a literal `include_*!` or `#[path]`,
/// relative to the crate root, or `None` when it escapes the crate.
fn referenced_paths(crate_root: &Path, source: &Path, text: &str) -> Vec<(usize, Option<PathBuf>)> {
    let directory = source.parent().expect("source has a parent");
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let code = line.split("//").next().unwrap_or_default();
        for marker in MARKERS {
            let Some(start) = code.find(marker) else {
                continue;
            };
            let rest = code[start + marker.len()..].trim_start_matches([' ', '=']);
            let Some(rest) = rest.strip_prefix('"') else {
                continue;
            };
            let Some(length) = rest.find('"') else {
                continue;
            };
            let literal = &rest[..length];
            found.push((index + 1, within(crate_root, &directory.join(literal))));
        }
    }
    found
}

fn within(crate_root: &Path, path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            other => normalized.push(other),
        }
    }
    normalized
        .strip_prefix(crate_root)
        .ok()
        .map(Path::to_path_buf)
}

/// Test, bench and example sources may read monorepo fixtures: users never
/// compile them, and `cargo publish` verifies only the library.
fn is_library_source(relative: &Path) -> bool {
    relative
        .extension()
        .is_some_and(|extension| extension == "rs")
        && !relative.components().any(|component| {
            matches!(
                component.as_os_str().to_str(),
                Some("tests" | "benches" | "examples")
            )
        })
}

fn packaged_files(root: &Path, name: &str) -> BTreeSet<PathBuf> {
    let output = Command::new(super::cargo())
        .current_dir(root)
        .args(["package", "--list", "--allow-dirty", "-p", name])
        .output()
        .expect("run cargo package --list");
    assert!(
        output.status.success(),
        "cargo package --list -p {name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("package list is UTF-8")
        .lines()
        .map(PathBuf::from)
        .collect()
}

#[test]
fn published_sources_only_reference_files_inside_their_package() {
    let root = super::workspace_root().expect("resolve workspace root");
    let metadata = super::capture(Command::new(super::cargo()).current_dir(&root).args([
        "metadata",
        "--no-deps",
        "--format-version",
        "1",
    ]))
    .expect("cargo metadata");
    let metadata: serde_json::Value = serde_json::from_str(&metadata).expect("parse metadata");

    let mut violations = Vec::new();
    for package in metadata["packages"].as_array().expect("packages") {
        if package["publish"] == serde_json::json!([]) {
            continue;
        }
        let name = package["name"].as_str().expect("package name");
        let manifest = PathBuf::from(package["manifest_path"].as_str().expect("manifest path"));
        let crate_root = manifest.parent().expect("manifest directory");
        let files = packaged_files(&root, name);
        for relative in files.iter().filter(|file| is_library_source(file)) {
            let source = crate_root.join(relative);
            let text = fs::read_to_string(&source)
                .unwrap_or_else(|error| panic!("read {}: {error}", source.display()));
            for (line, target) in referenced_paths(crate_root, &source, &text) {
                if !target.as_ref().is_some_and(|target| files.contains(target)) {
                    violations.push(format!("{name}: {}:{line}", relative.display()));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "these references name a file the published package does not contain, so the \
         crates.io tarball cannot build them:\n{}",
        violations.join("\n")
    );
}

#[test]
fn references_resolve_against_the_including_file() {
    let crate_root = Path::new("/w/crates/a");
    let source = crate_root.join("src/ios/mod.rs");
    let text = r#"
        const A: &str = include_str!("../../templates/a.txt");
        #[path = "../../../b/src/speech.rs"]
        mod speech;
        // include_str!("../../../commented.txt")
        include!(concat!(env!("OUT_DIR"), "/generated.rs"));
    "#;
    assert_eq!(
        referenced_paths(crate_root, &source, text),
        vec![(2, Some(PathBuf::from("templates/a.txt"))), (3, None)]
    );
}

#[test]
fn only_library_sources_are_checked() {
    assert!(is_library_source(Path::new("src/lib.rs")));
    assert!(is_library_source(Path::new("src/project/tests.rs")));
    assert!(!is_library_source(Path::new(
        "src/tests/host_conformance.rs"
    )));
    assert!(!is_library_source(Path::new("tests/composition.rs")));
}
