//! End-to-end check on the `extra_files` IR pass-through (RFC #164
//! B-direction PR 3): built-in plugin → engine → `inputs_from` →
//! renderer drops the file into `gen/{ios,android}/`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use whisker_cng::plugins::android_extra_files::AndroidExtraFiles;
use whisker_cng::plugins::ios_extra_files::IosExtraFiles;
use whisker_config::Config;

fn unique_tempdir() -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let p = std::env::temp_dir().join(format!("whisker-cng-extra-files-{pid}-{n}"));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn base_ios_app() -> Config {
    let mut a = Config::default();
    a.name("HelloWorld")
        .bundle_id("rs.whisker.examples.helloWorld");
    a
}

fn base_android_app() -> Config {
    let mut a = Config::default();
    a.name("HelloWorld").android(|x| {
        x.application_id("rs.whisker.examples.helloworld");
    });
    a
}

fn sync_ios(app: &Config) -> (PathBuf, PathBuf) {
    let inputs = whisker_cng::ios::inputs_from(
        app,
        PathBuf::from("/abs/platforms/ios"),
        PathBuf::from("/abs/gen/ios/whisker_modules"),
        PathBuf::from("/abs/workspace"),
        "hello-world".into(),
    )
    .unwrap();
    let tmp = unique_tempdir();
    let out = tmp.join("gen/ios");
    whisker_cng::ios::sync(&out, &inputs).unwrap();
    (tmp, out)
}

fn sync_android(app: &Config) -> (PathBuf, PathBuf) {
    let inputs = whisker_cng::android::inputs_from(
        app,
        "hello_world".into(),
        PathBuf::from("../.."),
        "hello-world".into(),
        "0.1.0".into(),
        "0.1.0".into(),
        "https://whiskerrs.github.io/whisker/maven".into(),
        "https://whiskerrs.github.io/lynx/maven".into(),
    )
    .unwrap();
    let tmp = unique_tempdir();
    let out = tmp.join("gen/android");
    whisker_cng::android::sync(&out, &inputs).unwrap();
    (tmp, out)
}

// ============================================================================
// iOS
// ============================================================================

#[test]
fn ios_extra_file_lands_at_the_declared_relative_path() {
    let mut app = base_ios_app();
    app.plugin::<IosExtraFiles>(|c| {
        c.add("Sources/Helper.swift", "// helper code\n");
    });
    let (tmp, out) = sync_ios(&app);
    let helper = out.join("Sources/Helper.swift");
    assert!(helper.is_file(), "missing: {}", helper.display());
    assert_eq!(
        std::fs::read_to_string(&helper).unwrap(),
        "// helper code\n"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ios_extra_file_creates_intermediate_directories() {
    let mut app = base_ios_app();
    app.plugin::<IosExtraFiles>(|c| {
        c.add(
            "Resources/Config/Endpoints/prod.json",
            "{\"api\": \"prod\"}",
        );
    });
    let (tmp, out) = sync_ios(&app);
    let path = out.join("Resources/Config/Endpoints/prod.json");
    assert!(path.is_file(), "missing: {}", path.display());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[cfg(unix)]
#[test]
fn ios_extra_file_mode_is_applied_on_unix() {
    use std::os::unix::fs::PermissionsExt;
    let mut app = base_ios_app();
    app.plugin::<IosExtraFiles>(|c| {
        c.add_with_mode("Scripts/run.sh", "#!/bin/sh\necho ok\n", 0o755);
    });
    let (tmp, out) = sync_ios(&app);
    let script = out.join("Scripts/run.sh");
    let mode = std::fs::metadata(&script).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o755, "mode was {mode:o}");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ios_extra_file_with_absolute_path_is_rejected() {
    let mut app = base_ios_app();
    app.plugin::<IosExtraFiles>(|c| {
        c.add("/etc/passwd", "malicious");
    });
    let inputs = whisker_cng::ios::inputs_from(
        &app,
        PathBuf::from("/abs/platforms/ios"),
        PathBuf::from("/abs/gen/ios/whisker_modules"),
        PathBuf::from("/abs/workspace"),
        "hello-world".into(),
    )
    .unwrap();
    let tmp = unique_tempdir();
    let out = tmp.join("gen/ios");
    let err = whisker_cng::ios::sync(&out, &inputs).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("must be relative"), "{msg}");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ios_extra_file_with_parent_dir_traversal_is_rejected() {
    let mut app = base_ios_app();
    app.plugin::<IosExtraFiles>(|c| {
        c.add("Sources/../escape.swift", "escape attempt");
    });
    let inputs = whisker_cng::ios::inputs_from(
        &app,
        PathBuf::from("/abs/platforms/ios"),
        PathBuf::from("/abs/gen/ios/whisker_modules"),
        PathBuf::from("/abs/workspace"),
        "hello-world".into(),
    )
    .unwrap();
    let tmp = unique_tempdir();
    let out = tmp.join("gen/ios");
    let err = whisker_cng::ios::sync(&out, &inputs).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains(".."), "{msg}");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ios_no_plugin_means_no_extra_files_written() {
    let app = base_ios_app();
    let (tmp, out) = sync_ios(&app);
    // No spurious files outside the known set.
    assert!(out.join("Info.plist").exists());
    assert!(!out.join("Sources/Helper.swift").exists());
    let _ = std::fs::remove_dir_all(&tmp);
}

// ============================================================================
// Android
// ============================================================================

#[test]
fn android_extra_file_lands_at_the_declared_relative_path() {
    let mut app = base_android_app();
    app.plugin::<AndroidExtraFiles>(|c| {
        c.add("app/google-services.json", "{\"project\": \"demo\"}");
    });
    let (tmp, out) = sync_android(&app);
    let gs = out.join("app/google-services.json");
    assert!(gs.is_file(), "missing: {}", gs.display());
    assert_eq!(
        std::fs::read_to_string(&gs).unwrap(),
        "{\"project\": \"demo\"}",
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[cfg(unix)]
#[test]
fn android_extra_file_mode_is_applied_on_unix() {
    use std::os::unix::fs::PermissionsExt;
    let mut app = base_android_app();
    app.plugin::<AndroidExtraFiles>(|c| {
        c.add_with_mode("scripts/precheck.sh", "#!/bin/sh\n", 0o755);
    });
    let (tmp, out) = sync_android(&app);
    let script = out.join("scripts/precheck.sh");
    let mode = std::fs::metadata(&script).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o755, "mode was {mode:o}");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn android_extra_file_with_absolute_path_is_rejected() {
    let mut app = base_android_app();
    app.plugin::<AndroidExtraFiles>(|c| {
        c.add("/etc/passwd", "malicious");
    });
    let inputs = whisker_cng::android::inputs_from(
        &app,
        "hello_world".into(),
        PathBuf::from("../.."),
        "hello-world".into(),
        "0.1.0".into(),
        "0.1.0".into(),
        "https://whiskerrs.github.io/whisker/maven".into(),
        "https://whiskerrs.github.io/lynx/maven".into(),
    )
    .unwrap();
    let tmp = unique_tempdir();
    let out = tmp.join("gen/android");
    let err = whisker_cng::android::sync(&out, &inputs).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("must be relative"), "{msg}");
    let _ = std::fs::remove_dir_all(&tmp);
}

// ============================================================================
// Realistic: Firebase google-services.json
// ============================================================================

#[test]
fn android_firebase_google_services_json_drops_at_app_root() {
    // What a real `whisker-firebase` plugin (Phase 4 dogfood)
    // would do for Android — drop `google-services.json` next to
    // the app's `build.gradle.kts`.
    let mut app = base_android_app();
    app.plugin::<AndroidExtraFiles>(|c| {
        c.add(
            "app/google-services.json",
            "{\"project_info\": {\"project_id\": \"demo\"}}",
        );
    });
    let (tmp, out) = sync_android(&app);
    let gs = out.join("app/google-services.json");
    assert!(
        gs.is_file(),
        "google-services.json missing: {}",
        gs.display()
    );
    let content = std::fs::read_to_string(&gs).unwrap();
    assert!(content.contains("project_id"));
    let _ = std::fs::remove_dir_all(&tmp);
}
