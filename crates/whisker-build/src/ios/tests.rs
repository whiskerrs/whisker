use super::*;

#[test]
fn generated_entrypoint_registers_the_built_in_element_module() {
    let source = render_register_all_swift(&[]);
    assert!(source.contains("builtInModule.registerWithWhisker()"));
    assert!(source.contains("@_exported import WhiskerRuntime"));
    assert!(!source.contains("BuiltInElementBindings"));
}

#[test]
fn generated_aggregator_links_the_released_host_runtime_for_consumers() {
    let manifest = render_modules_package_swift(None, &[]);
    assert!(manifest.contains(&format!(
        ".package(url: {WHISKER_IOS_SPM_URL:?}, exact: {WHISKER_IOS_SPM_VERSION:?})"
    )));
    assert!(manifest.contains(".product(name: \"WhiskerRuntime\", package: \"whisker\")"));
    assert!(!manifest.contains(".package(name: \"whisker\", path:"));
}

#[test]
fn generated_aggregator_prefers_the_in_tree_host_runtime() {
    let manifest = render_modules_package_swift(Some(Path::new("/workspace/whisker")), &[]);
    assert!(manifest.contains(".package(name: \"whisker\", path: \"/workspace/whisker\")"));
    assert!(!manifest.contains(WHISKER_IOS_SPM_URL));
    assert!(manifest.contains(".product(name: \"WhiskerRuntime\", package: \"whisker\")"));
}

/// Every module package and the generated aggregator pin the runtime
/// `exact:` to [`WHISKER_IOS_SPM_VERSION`]. One manifest left on the old
/// version makes the whole graph unresolvable for every consumer
/// ("root depends on 0.1.4 and X depends on 0.1.5"), and nothing
/// else in the workspace notices — the manifests are data to Rust.
#[test]
fn module_packages_pin_the_runtime_version_the_cli_generates() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/<name> sits two levels under the workspace root")
        .to_path_buf();
    let packages = workspace.join("packages");
    // Absent when this crate is built from its published .crate,
    // which carries no sibling packages to check.
    let Ok(entries) = std::fs::read_dir(&packages) else {
        return;
    };

    let expected = format!(
        "\"https://github.com/whiskerrs/whisker.git\", exact: \"{WHISKER_IOS_SPM_VERSION}\""
    );
    let mut stale = Vec::new();
    for entry in entries.flatten() {
        let manifest = entry.path().join("Package.swift");
        let Ok(source) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        if source.contains("whiskerrs/whisker.git") && !source.contains(&expected) {
            stale.push(manifest.display().to_string());
        }
    }
    assert!(
        stale.is_empty(),
        "these manifests do not pin {WHISKER_IOS_SPM_VERSION}: {stale:#?}"
    );
}

#[test]
fn bridge_exports_have_leading_underscore() {
    // ld64's `-exported_symbol` expects the Mach-O C symbol form.
    // Dropping the underscore would silently leave the symbol
    // out of `.dynsym` and Swift would fail to link the bridge.
    for sym in BRIDGE_EXPORTS {
        assert!(
            sym.starts_with('_'),
            "BRIDGE_EXPORTS entry missing leading underscore: {sym}",
        );
    }
}

#[test]
fn framework_info_plist_contains_executable_name() {
    let plist = framework_info_plist(DEFAULT_MIN_OS);
    assert!(plist.contains("<string>WhiskerDriver</string>"));
    assert!(plist.contains("FMWK"));
}

/// The app and its embedded framework have to agree on the minimum,
/// or App Store validation rejects the upload (90208).
#[test]
fn framework_minimum_os_follows_the_apps_deployment_target() {
    assert!(framework_info_plist("15.0").contains("<string>15.0</string>"));
}

#[test]
fn missing_xcodeproj_errors() {
    let tmp = std::env::temp_dir().join("whisker-cli-build_ios-test");
    let _ = std::fs::create_dir_all(&tmp);
    let dd = tmp.join("derived");
    let args = XcodebuildArgs {
        gen_ios: &tmp,
        scheme: "X",
        sdk: "iphonesimulator",
        configuration: "Release",
        xcodeproj_name: "X",
        derived_data: &dd,
        whisker_runtime_path: None,
        whisker_ios_macros_path: None,
    };
    let err = run_xcodebuild_app(&args).unwrap_err();
    assert!(
        err.to_string().contains("Xcode project missing"),
        "got: {err:#}",
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn unknown_sdk_errors() {
    let tmp = std::env::temp_dir().join("whisker-cli-build_ios-sdk-test");
    let _ = std::fs::create_dir_all(&tmp);
    let proj = tmp.join("X.xcodeproj");
    let _ = std::fs::create_dir_all(&proj);
    let dd = tmp.join("derived");
    let args = XcodebuildArgs {
        gen_ios: &tmp,
        scheme: "X",
        sdk: "bogus",
        configuration: "Release",
        xcodeproj_name: "X",
        derived_data: &dd,
        whisker_runtime_path: None,
        whisker_ios_macros_path: None,
    };
    let err = run_xcodebuild_app(&args).unwrap_err();
    assert!(err.to_string().contains("unknown SDK"), "got: {err:#}");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ios_platform_major_reads_the_declared_floor() {
    assert_eq!(
        parse_ios_platform_major("    platforms: [.iOS(.v15), .macOS(.v13)],"),
        Some(15)
    );
}

#[test]
fn ios_platform_major_ignores_a_manifest_without_an_ios_floor() {
    assert_eq!(
        parse_ios_platform_major("    platforms: [.macOS(.v13)],"),
        None
    );
}

/// Every retained-View function the generated Swift Host calls has to be in
/// [`BRIDGE_EXPORTS`], or it never lands in the dylib's `.dynsym`
/// and the app fails to link. The list is hand-maintained, so this
/// enforces it mechanically.
#[test]
fn swift_call_sites_are_all_exported() {
    fn swift_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                swift_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "swift") {
                out.push(path);
            }
        }
    }

    // Monorepo layout only — a published crate has no CNG templates.
    let ios = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../whisker-cng/src/templates/ios/Sources");
    if !ios.is_dir() {
        return;
    }
    let mut files = Vec::new();
    swift_files(&ios, &mut files);
    assert!(!files.is_empty(), "no .swift files under {}", ios.display());

    let mut missing: Vec<String> = Vec::new();
    for file in &files {
        let Ok(contents) = std::fs::read_to_string(file) else {
            continue;
        };
        for line in contents.lines() {
            let code = line.split("//").next().unwrap_or("");
            let mut rest = code;
            while let Some(at) = rest.find("whisker_view_") {
                let tail = &rest[at..];
                let name: String = tail
                    .chars()
                    .take_while(|c| c.is_ascii_lowercase() || *c == '_' || c.is_ascii_digit())
                    .collect();
                // Only call sites — a bare mention in a type
                // signature or a string isn't a link dependency.
                if tail[name.len()..].starts_with('(') {
                    let exported = format!("_{name}");
                    if !BRIDGE_EXPORTS.contains(&exported.as_str()) && !missing.contains(&exported)
                    {
                        missing.push(exported);
                    }
                }
                rest = &tail[name.len().max(1)..];
            }
        }
    }
    assert!(
        missing.is_empty(),
        "Swift calls these bridge functions but BRIDGE_EXPORTS omits them, \
             so the dylib won't export them and the app won't link: {missing:?}",
    );
}
