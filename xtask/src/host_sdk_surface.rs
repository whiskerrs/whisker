use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN_PRODUCTION_MARKERS: &[&str] = &[
    "WHISKER_HOST_CONFORMANCE",
    "ForTesting",
    "ConformanceFrame",
    "ConformanceTouch",
    "pointerInputObserver",
    "textInspectionObserver",
];

fn production_host_sources(root: &Path, relative_paths: &[&str]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for relative_path in relative_paths {
        let path = root.join(relative_path);
        if path.is_dir() {
            collect_sources(&path, &mut files);
        } else {
            files.push(path);
        }
    }
    files
}

fn collect_sources(directory: &Path, files: &mut Vec<PathBuf>) {
    let mut entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()))
        .map(|entry| entry.expect("read production Host source entry").path())
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_sources(&path, files);
        } else if matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("kt" | "swift")
        ) {
            files.push(path);
        }
    }
}

#[test]
fn ios_production_host_sources_exclude_test_support() {
    let root = super::workspace_root().expect("resolve workspace root");
    for package in ["Package.swift", "platforms/ios/Package.swift"] {
        assert_swift_runtime_target_has_no_conformance_define(&root.join(package));
    }
    assert_no_test_support(&root, &["platforms/ios/Sources/WhiskerRuntime"]);
}

#[test]
fn android_production_host_sources_exclude_test_support() {
    let root = super::workspace_root().expect("resolve workspace root");
    assert_no_test_support(
        &root,
        &[
            "platforms/android/module/src/main",
            "platforms/android/runtime/src/main",
        ],
    );
}

fn assert_no_test_support(root: &Path, relative_paths: &[&str]) {
    let leaks = production_host_sources(root, relative_paths)
        .into_iter()
        .flat_map(|path| {
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            FORBIDDEN_PRODUCTION_MARKERS
                .iter()
                .filter(|marker| source.contains(**marker))
                .map(|marker| {
                    format!(
                        "{} contains forbidden production marker {marker}",
                        path.strip_prefix(root).unwrap_or(&path).display()
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert!(
        leaks.is_empty(),
        "Host conformance support leaked into the production SDK:\n{}",
        leaks.join("\n")
    );
}

fn assert_swift_runtime_target_has_no_conformance_define(package: &Path) {
    let source = fs::read_to_string(package)
        .unwrap_or_else(|error| panic!("read {}: {error}", package.display()));
    let target_start = source
        .find(".target(\n            name: \"WhiskerRuntime\"")
        .unwrap_or_else(|| panic!("{} has no WhiskerRuntime target", package.display()));
    let target = &source[target_start..];
    let target_end = target.find("\n        ),").unwrap_or_else(|| {
        panic!(
            "{} has an unterminated WhiskerRuntime target",
            package.display()
        )
    });
    let target = &target[..target_end];
    assert!(
        !target.contains("WHISKER_HOST_CONFORMANCE"),
        "{} compiles the production WhiskerRuntime target with Host conformance support",
        package.display()
    );
}
