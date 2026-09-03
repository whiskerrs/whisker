use super::*;
use std::path::Path;

#[test]
fn config_defaults_pick_loopback_and_full_reload_only() {
    let cfg = Config::defaults_for(
        PathBuf::from("/tmp/ws"),
        "hello-world".to_string(),
        Target::Android,
    );
    assert_eq!(cfg.workspace_root, Path::new("/tmp/ws"));
    assert_eq!(cfg.package, "hello-world");
    assert_eq!(cfg.target, Target::Android);
    assert_eq!(cfg.bind_addr.port(), 9876);
    assert!(cfg.bind_addr.ip().is_loopback());
    assert_eq!(cfg.hot_patch_mode, HotPatchMode::FullReloadOnly);
    assert!(cfg.watch_paths.is_empty());
}

#[test]
fn target_variants_compare_by_value() {
    assert_eq!(Target::Android, Target::Android);
    assert_ne!(Target::Android, Target::IosSimulator);
}

#[test]
fn hot_patch_mode_variants_compare_by_value() {
    assert_eq!(HotPatchMode::Disabled, HotPatchMode::Disabled);
    assert_ne!(HotPatchMode::HotReload, HotPatchMode::FullReloadOnly,);
}

#[test]
fn dev_server_new_does_not_fail_for_a_well_formed_config() {
    let cfg = Config::defaults_for(
        PathBuf::from("/tmp/ws"),
        "hello-world".to_string(),
        Target::Android,
    );
    assert!(DevServer::new(cfg).is_ok());
}

// ----- original_binary_path ----------------------------------------

fn mk_config(workspace_root: PathBuf, target: Target) -> Config {
    let mut cfg = Config::defaults_for(workspace_root.clone(), "hello-world".into(), target);
    cfg.crate_dir = workspace_root.clone();
    match target {
        Target::Android => {
            cfg.android = Some(crate::AndroidParams {
                project_dir: workspace_root.join("android"),
                application_id: "rs.whisker.examples.helloworld".into(),
                launcher_activity: ".MainActivity".into(),
                abi: "arm64-v8a".into(),
            });
        }
        Target::IosSimulator => {
            cfg.ios = Some(crate::IosParams {
                project_dir: workspace_root.join("ios"),
                scheme: "HelloWorld".into(),
                bundle_id: "rs.whisker.examples.helloWorld".into(),
                device_override: None,
            });
        }
        Target::Macos | Target::Web => {}
    }
    cfg
}

#[test]
fn original_binary_path_finds_ios_simulator_dylib_under_target() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let ws = std::env::temp_dir().join(format!("whisker-dev-test-ios-{pid}-{n}"));
    let _ = std::fs::remove_dir_all(&ws);
    let triple = match std::env::consts::ARCH {
        "aarch64" => "aarch64-apple-ios-sim",
        "x86_64" => "x86_64-apple-ios",
        other => panic!("unsupported test host arch {other}"),
    };
    let release_dir = ws.join("target").join(triple).join("release");
    std::fs::create_dir_all(&release_dir).unwrap();
    let dylib = release_dir.join("libhello_world.dylib");
    std::fs::write(&dylib, b"fake-macho").unwrap();

    let cfg = mk_config(ws.clone(), Target::IosSimulator);
    let resolved = original_binary_path(&cfg).unwrap();
    assert_eq!(resolved, dylib);

    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn original_binary_path_errors_when_ios_simulator_dylib_missing() {
    let cfg = mk_config(PathBuf::from("/nonexistent/ws"), Target::IosSimulator);
    let res = original_binary_path(&cfg);
    assert!(res.is_err());
}

#[test]
fn original_binary_path_finds_android_so_under_gradle_output() {
    // Reads from the gradle plugin's `@OutputDirectory`, not from
    // `target/<triple>/debug/` — the latter can be cleaned out by
    // `cargo clean` while gradle still reports its task as
    // UP-TO-DATE (the cargo target dir is `@Internal`, not an
    // input). See `original_binary_path` for the rationale.
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let ws = std::env::temp_dir().join(format!("whisker-dev-test-orig-{pid}-{n}"));
    let _ = std::fs::remove_dir_all(&ws);
    // `mk_config` sets `crate_dir = ws` for Android, so the path
    // the patcher checks is `<ws>/gen/android/app/build/generated/
    // jniLibs/whiskerBuildDebug<AbiCamel>/<abi>/lib<pkg>.so`.
    let gradle_out_dir = ws
        .join("gen/android/app/build/generated/jniLibs")
        .join("whiskerBuildDebugArm64V8a")
        .join("arm64-v8a");
    std::fs::create_dir_all(&gradle_out_dir).unwrap();
    let so = gradle_out_dir.join("libhello_world.so");
    std::fs::write(&so, b"fake").unwrap();

    let cfg = mk_config(ws.clone(), Target::Android);
    let resolved = original_binary_path(&cfg).unwrap();
    assert_eq!(resolved, so);

    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn android_abi_to_camel_matches_gradle_plugin_naming() {
    // Mirrors `WhiskerProjectPlugin.kt::String.toCamelCase`. The
    // patcher's task-name suffix has to match exactly or the
    // gradle output path won't resolve.
    assert_eq!(android_abi_to_camel("arm64-v8a"), "Arm64V8a");
    assert_eq!(android_abi_to_camel("armeabi-v7a"), "ArmeabiV7a");
    assert_eq!(android_abi_to_camel("x86_64"), "X8664");
    assert_eq!(android_abi_to_camel("x86"), "X86");
}

#[test]
fn original_binary_path_errors_when_android_so_missing() {
    let cfg = mk_config(PathBuf::from("/nonexistent/ws"), Target::Android);
    let res = original_binary_path(&cfg);
    assert!(res.is_err());
}

// ----- target_os_for -----------------------------------------------

#[test]
fn target_os_for_maps_android_to_linux() {
    assert_eq!(target_os_for(Target::Android), hotpatch::LinkerOs::Linux);
}

#[test]
fn target_os_for_maps_ios_to_macos() {
    assert_eq!(
        target_os_for(Target::IosSimulator),
        hotpatch::LinkerOs::Macos,
    );
}

// ----- decide_action -----------------------------------------------

#[test]
fn rust_code_with_patcher_chooses_hot_reload() {
    assert_eq!(
        decide_action(ChangeKind::RustCode, true),
        LoopAction::HotReload,
    );
}

#[test]
fn rust_code_without_patcher_prompts_full_reload() {
    assert_eq!(
        decide_action(ChangeKind::RustCode, false),
        LoopAction::PromptFullReload,
    );
}

#[test]
fn cargo_toml_always_prompts_full_reload_even_with_patcher() {
    // Patcher can't reload deps — Cargo.toml needs a full
    // rebuild regardless of which mode we're in.
    assert_eq!(
        decide_action(ChangeKind::CargoToml, true),
        LoopAction::PromptFullReload,
    );
    assert_eq!(
        decide_action(ChangeKind::CargoToml, false),
        LoopAction::PromptFullReload,
    );
}

#[test]
fn other_changes_are_ignored() {
    assert_eq!(decide_action(ChangeKind::Other, true), LoopAction::Ignore);
    assert_eq!(decide_action(ChangeKind::Other, false), LoopAction::Ignore);
}

// ----- log_patch_diff (smoke: shouldn't panic) ---------------------

#[test]
fn log_patch_diff_handles_empty_report_silently() {
    let r = hotpatch::DiffReport {
        added: vec![],
        removed: vec![],
        weak: vec![],
    };
    log_patch_diff(&r); // no panic, no output
}

#[test]
fn log_patch_diff_summarises_added_and_removed() {
    let r = hotpatch::DiffReport {
        added: vec!["new1".into(), "new2".into()],
        removed: vec!["old1".into()],
        weak: vec![],
    };
    log_patch_diff(&r); // smoke — output goes to stderr
}
