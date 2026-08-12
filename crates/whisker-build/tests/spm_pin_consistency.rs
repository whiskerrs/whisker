//! Guard against whisker-SwiftPM pin drift between the generated
//! host project and the module packages.
//!
//! The pbxproj cng renders references the remote `whisker` package at
//! [`whisker_build::ios::WHISKER_IOS_SPM_VERSION`], while every
//! module under `packages/*/Package.swift` pins the same package with
//! `exact:`. When the two disagree, SwiftPM resolution fails for
//! every module-using app the moment its `gen/ios` is regenerated:
//!
//! ```text
//! Dependencies could not be resolved because 'whisker-webview'
//! depends on 'whisker' 0.1.1 and root depends on 'whisker' 0.1.2.
//! ```
//!
//! This test fails the build when a bump forgets either side.

use std::path::Path;
use whisker_build::ios::{WHISKER_IOS_SPM_URL, WHISKER_IOS_SPM_VERSION};

#[test]
fn module_package_swift_pins_match_whisker_ios_spm_version() {
    // Monorepo layout only: from a published crate there is no
    // `packages/` tree to check, and the guard is meaningless there.
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let packages = workspace_root.join("packages");
    if !packages.is_dir() {
        return;
    }

    let expected =
        format!(r#".package(url: "{WHISKER_IOS_SPM_URL}", exact: "{WHISKER_IOS_SPM_VERSION}")"#);
    let mut checked = 0usize;
    let mut bad: Vec<String> = Vec::new();

    for entry in std::fs::read_dir(&packages).expect("read packages/") {
        let manifest = entry.expect("dir entry").path().join("Package.swift");
        let Ok(contents) = std::fs::read_to_string(&manifest) else {
            continue; // package without an iOS half
        };
        for line in contents.lines() {
            if line.contains(WHISKER_IOS_SPM_URL) {
                checked += 1;
                if !line.contains(&expected) {
                    bad.push(format!("{}: {}", manifest.display(), line.trim()));
                }
            }
        }
    }

    assert!(
        checked > 0,
        "no Package.swift references {WHISKER_IOS_SPM_URL} under {} — \
         if module manifests moved, update this test's search path",
        packages.display(),
    );
    assert!(
        bad.is_empty(),
        "module Package.swift pins out of lockstep with \
         WHISKER_IOS_SPM_VERSION (= {WHISKER_IOS_SPM_VERSION}). When bumping the \
         whisker SwiftPM release, bump every module pin in the same change:\n{}",
        bad.join("\n"),
    );
}

/// The root `/Package.swift` is what REMOTE SwiftPM consumers resolve
/// when they depend on the `whisker` package tag; `platforms/ios/
/// Package.swift` is the in-repo development manifest. A Lynx roll
/// that bumps only the latter ships a release whose apps silently keep
/// the previous Lynx (the 0.1.9 incident).
#[test]
fn root_package_swift_lynx_pins_match_the_platform_manifest() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root_manifest = workspace_root.join("Package.swift");
    let platform_manifest = workspace_root.join("platforms/ios/Package.swift");
    let (Ok(root), Ok(platform)) = (
        std::fs::read_to_string(&root_manifest),
        std::fs::read_to_string(&platform_manifest),
    ) else {
        return; // published-crate layout; nothing to check
    };

    let lynx_lines = |s: &str| -> Vec<String> {
        s.lines()
            .map(str::trim)
            .filter(|l| {
                (l.contains("whiskerrs/lynx/releases/download") || l.contains("checksum:"))
                    && !l.starts_with("//")
            })
            .map(str::to_string)
            .collect()
    };
    let (root_pins, platform_pins) = (lynx_lines(&root), lynx_lines(&platform));
    assert!(
        !platform_pins.is_empty(),
        "no Lynx binaryTarget pins found in {} — update this test's matcher",
        platform_manifest.display(),
    );
    assert_eq!(
        root_pins, platform_pins,
        "Lynx binaryTarget pins differ between /Package.swift (what remote \
         SwiftPM consumers get) and platforms/ios/Package.swift — roll both \
         in the same change",
    );
}
