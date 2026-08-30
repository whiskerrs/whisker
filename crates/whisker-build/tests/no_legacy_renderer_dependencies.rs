//! Prevent executable source and build manifests from regaining dependencies
//! on the renderer Whisker replaced with its Host protocol.

use std::path::Path;

#[test]
fn active_sources_do_not_reference_the_legacy_renderer() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if !workspace.join("platforms").is_dir() {
        return;
    }

    let forbidden = [
        concat!("com.", "lynx"),
        concat!("whiskerrs/", "lynx"),
        concat!("lynx", "-android"),
        concat!("lib", "lynx"),
        concat!("Lynx", ".framework"),
        concat!("Lynx", "UI"),
        concat!("T", "ASM"),
        concat!("run_on_", "main_thread"),
        concat!("lynx", "_maven"),
        concat!("Whisker", "LynxAliases"),
    ];

    let mut violations = Vec::new();
    for root in ["crates", "packages", "platforms"] {
        visit(&workspace.join(root), &forbidden, &mut violations);
    }
    for name in ["Cargo.toml", "Cargo.lock", "Package.swift"] {
        inspect(&workspace.join(name), &forbidden, &mut violations);
    }

    assert!(
        violations.is_empty(),
        "legacy renderer references returned to active source/build files:\n{}",
        violations.join("\n")
    );
}

fn visit(path: &Path, forbidden: &[&str], violations: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if should_skip_directory(&path) {
                continue;
            }
            visit(&path, forbidden, violations);
        } else if is_active_source(&path) {
            inspect(&path, forbidden, violations);
        }
    }
}

fn should_skip_directory(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("target" | "build" | "gen" | "tests" | ".build" | ".gradle")
    )
}

fn is_active_source(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if matches!(
        name,
        "Cargo.toml" | "Package.swift" | "build.gradle.kts" | "settings.gradle.kts"
    ) {
        return true;
    }
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("rs" | "kt" | "kts" | "swift" | "c" | "cc" | "cpp" | "h" | "hpp")
    )
}

fn inspect(path: &Path, forbidden: &[&str], violations: &mut Vec<String>) {
    let Ok(source) = std::fs::read_to_string(path) else {
        return;
    };
    for (index, line) in source.lines().enumerate() {
        for token in forbidden {
            if line.contains(token) {
                violations.push(format!(
                    "{}:{} contains {token:?}",
                    path.display(),
                    index + 1
                ));
            }
        }
    }
}
