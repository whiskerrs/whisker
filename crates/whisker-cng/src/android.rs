//! Render the Android host project under `gen/android/` from an
//! [`AppConfig`].
//!
//! The output mirrors a small AGP-flavoured Android Studio project:
//!
//! ```text
//! gen/android/
//! ├── app/
//! │   ├── build.gradle.kts
//! │   └── src/main/
//! │       ├── AndroidManifest.xml
//! │       ├── jniLibs/                          (populated at build time)
//! │       └── kotlin/<package-path>/
//! │           ├── MainActivity.kt
//! │           └── <AppName>Application.kt
//! ├── build.gradle.kts
//! ├── settings.gradle.kts
//! ├── gradle.properties
//! ├── gradlew
//! ├── gradlew.bat
//! └── gradle/wrapper/
//!     ├── gradle-wrapper.jar
//!     └── gradle-wrapper.properties
//! ```
//!
//! The package path under `kotlin/` is `applicationId` with dots
//! converted to slashes: `rs.whisker.examples.helloworld` →
//! `rs/whisker/examples/helloworld/`.

use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use whisker_app_config::AppConfig;

use crate::fingerprint;
use crate::render::render;

// ---- Embedded templates ----------------------------------------------------
//
// Text files go through `{{placeholder}}` substitution. Binary files
// (the gradle wrapper jar) are copied verbatim. `gradlew` is text but
// needs the +x bit on Unix so it lives in its own list.

const APP_BUILD_GRADLE_KTS: &str =
    include_str!("templates/android/app/build.gradle.kts");
const APP_MANIFEST_XML: &str =
    include_str!("templates/android/app/src/main/AndroidManifest.xml");
const MAIN_ACTIVITY_KT: &str =
    include_str!("templates/android/app/src/main/kotlin/MainActivity.kt");
const APPLICATION_KT: &str =
    include_str!("templates/android/app/src/main/kotlin/Application.kt");
const ROOT_BUILD_GRADLE_KTS: &str = include_str!("templates/android/build.gradle.kts");
const SETTINGS_GRADLE_KTS: &str = include_str!("templates/android/settings.gradle.kts");
const GRADLE_PROPERTIES: &str = include_str!("templates/android/gradle.properties");
const GRADLEW: &str = include_str!("templates/android/gradlew");
const GRADLEW_BAT: &str = include_str!("templates/android/gradlew.bat");
const GRADLE_WRAPPER_PROPERTIES: &str =
    include_str!("templates/android/gradle/wrapper/gradle-wrapper.properties");
const GRADLE_WRAPPER_JAR: &[u8] =
    include_bytes!("templates/android/gradle/wrapper/gradle-wrapper.jar");

/// Inputs the Android renderer pulls out of `AppConfig` (+ a few
/// values the cli passes in like the dylib name and the workspace's
/// `native/android/whisker-runtime` location).
///
/// Holding these in a struct rather than a big tuple keeps the
/// fingerprint serialization stable and the template-vars build site
/// easy to read.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AndroidInputs {
    pub app_name: String,
    pub version: String,
    pub build_number: u32,
    pub application_id: String,
    pub min_sdk: u32,
    pub target_sdk: u32,
    /// Crate name with hyphens replaced by underscores — what
    /// `System.loadLibrary` and `keepDebugSymbols` reference.
    pub rust_lib_name: String,
    /// Absolute or `settings.gradle.kts`-relative path to
    /// `<workspace>/native/android/whisker-runtime`. The renderer
    /// writes it verbatim into the `project(":whisker-runtime").projectDir`
    /// call.
    pub whisker_runtime_path: PathBuf,
    /// Absolute path to the dir holding the Lynx AARs (typically
    /// `<workspace>/target/lynx-android`). Registered as a `flatDir`
    /// repo in the generated `settings.gradle.kts` so whisker-runtime's
    /// `api(name="LynxAndroid", ext="aar")` style deps resolve.
    pub whisker_lynx_aar_dir: PathBuf,
    /// Bumped whenever the template *shape* changes (added file,
    /// renamed placeholder, …). The fingerprint mixes this in so
    /// existing `gen/` trees regenerate after an upgrade.
    pub template_version: u32,
}

/// Render the Android project into `out_dir` (typically
/// `<crate_dir>/gen/android`). Returns whether files were actually
/// rewritten — `false` means the cached fingerprint matched and the
/// existing tree was reused. The caller decides what to do with that
/// (log "in sync", skip a downstream sync, …).
pub fn sync(out_dir: &Path, inputs: &AndroidInputs) -> Result<bool> {
    let new_fp = fingerprint::fingerprint(
        serde_json::to_vec(inputs)
            .context("serialize AndroidInputs for fingerprint")?
            .as_slice(),
    );
    let fp_path = out_dir.join(".whisker-fingerprint");
    if let Ok(existing) = std::fs::read_to_string(&fp_path) {
        if existing.trim() == new_fp {
            return Ok(false);
        }
    }

    write_files(out_dir, inputs).context("write Android project files")?;
    std::fs::write(&fp_path, &new_fp)
        .with_context(|| format!("write fingerprint {}", fp_path.display()))?;
    Ok(true)
}

/// Build the `{{var}}` table from `inputs`. Split out so unit tests
/// can assert against the result without going through file I/O.
pub(crate) fn template_vars(inputs: &AndroidInputs) -> HashMap<&'static str, String> {
    let mut v = HashMap::new();
    v.insert("app_name", inputs.app_name.clone());
    v.insert("version", inputs.version.clone());
    v.insert("build_number", inputs.build_number.to_string());
    v.insert("android_application_id", inputs.application_id.clone());
    v.insert("android_application_class", application_class_name(&inputs.app_name));
    v.insert("android_min_sdk", inputs.min_sdk.to_string());
    v.insert("android_target_sdk", inputs.target_sdk.to_string());
    v.insert("android_project_name", project_name(&inputs.app_name));
    v.insert("rust_lib_name", inputs.rust_lib_name.clone());
    v.insert(
        "whisker_runtime_android_path",
        inputs.whisker_runtime_path.display().to_string(),
    );
    v.insert(
        "whisker_lynx_aar_dir",
        inputs.whisker_lynx_aar_dir.display().to_string(),
    );
    v
}

/// Application class. `HelloWorld` → `HelloWorldApplication`. Strips
/// non-identifier characters and ensures the leading char is alpha.
fn application_class_name(app_name: &str) -> String {
    let cleaned: String = app_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if cleaned.is_empty() {
        return "WhiskerApp_Application".into();
    }
    format!("{cleaned}Application")
}

/// `rootProject.name`. Lowercase, hyphenated form of the app name —
/// e.g. `HelloWorld` → `hello-world-android`. Matches the existing
/// example convention (gradle warns on uppercase project names).
fn project_name(app_name: &str) -> String {
    let mut out = String::new();
    for (i, c) in app_name.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('-');
        }
        out.extend(c.to_lowercase());
    }
    if out.is_empty() {
        out.push_str("whisker-app");
    }
    format!("{out}-android")
}

/// Convert `rs.whisker.examples.helloworld` → `rs/whisker/examples/helloworld`.
/// Used to build the on-disk path under `app/src/main/kotlin/`.
fn application_id_to_path(application_id: &str) -> PathBuf {
    application_id
        .split('.')
        .filter(|s| !s.is_empty())
        .fold(PathBuf::new(), |acc, seg| acc.join(seg))
}

fn write_files(out_dir: &Path, inputs: &AndroidInputs) -> Result<()> {
    let vars = template_vars(inputs);

    // Wipe the existing tree, but spare anything we know is a runtime
    // build artifact (so we don't blow away gradle's cache on every
    // sync). Today that means `app/build/`, `.gradle/`, and the
    // `app/src/main/jniLibs/` directory whose bytes are produced by
    // `cargo build` outside the renderer.
    clean_managed_tree(out_dir).context("clean previous gen tree")?;

    let kotlin_pkg = out_dir
        .join("app/src/main/kotlin")
        .join(application_id_to_path(&inputs.application_id));

    let app_class_filename = format!("{}.kt", application_class_name(&inputs.app_name));

    // Text templates.
    let text_files: &[(PathBuf, &str)] = &[
        (out_dir.join("app/build.gradle.kts"), APP_BUILD_GRADLE_KTS),
        (
            out_dir.join("app/src/main/AndroidManifest.xml"),
            APP_MANIFEST_XML,
        ),
        (kotlin_pkg.join("MainActivity.kt"), MAIN_ACTIVITY_KT),
        (kotlin_pkg.join(&app_class_filename), APPLICATION_KT),
        (out_dir.join("build.gradle.kts"), ROOT_BUILD_GRADLE_KTS),
        (out_dir.join("settings.gradle.kts"), SETTINGS_GRADLE_KTS),
        (out_dir.join("gradle.properties"), GRADLE_PROPERTIES),
        (
            out_dir.join("gradle/wrapper/gradle-wrapper.properties"),
            GRADLE_WRAPPER_PROPERTIES,
        ),
    ];
    for (path, template) in text_files {
        let rendered = render(template, &vars)
            .with_context(|| format!("render {}", path.display()))?;
        write_file(path, rendered.as_bytes(), false)?;
    }

    // `gradlew` is shell — needs +x.
    write_file(&out_dir.join("gradlew"), GRADLEW.as_bytes(), true)?;
    write_file(&out_dir.join("gradlew.bat"), GRADLEW_BAT.as_bytes(), false)?;

    // Binary.
    write_file(
        &out_dir.join("gradle/wrapper/gradle-wrapper.jar"),
        GRADLE_WRAPPER_JAR,
        false,
    )?;

    Ok(())
}

/// Delete the previous gen tree but keep `app/build/`, `.gradle/`,
/// and `app/src/main/jniLibs/`. These three are runtime build
/// artifacts; wiping them on every sync forces gradle into a cold
/// rebuild and `cargo build` to re-copy the dylib, which would make
/// the dev loop unbearable.
fn clean_managed_tree(out_dir: &Path) -> Result<()> {
    if !out_dir.exists() {
        return Ok(());
    }
    let keep = ["app/build", ".gradle", "app/src/main/jniLibs"];
    for entry in std::fs::read_dir(out_dir)
        .with_context(|| format!("read_dir {}", out_dir.display()))?
    {
        let entry = entry?;
        let rel = entry
            .path()
            .strip_prefix(out_dir)
            .map(|p| p.to_path_buf())
            .ok();
        if let Some(rel) = rel {
            if keep.iter().any(|k| rel == Path::new(k)) {
                continue;
            }
        }
        // Don't blow away top-level `app/` either — only the files we
        // own under it. Recurse one level.
        if entry.file_name() == "app" && entry.path().is_dir() {
            clean_under_app(&entry.path())?;
            continue;
        }
        // Skip our own fingerprint file — it'll be overwritten in `sync`.
        if entry.file_name() == ".whisker-fingerprint" {
            continue;
        }
        remove_path(&entry.path())?;
    }
    Ok(())
}

fn clean_under_app(app_dir: &Path) -> Result<()> {
    for entry in std::fs::read_dir(app_dir)
        .with_context(|| format!("read_dir {}", app_dir.display()))?
    {
        let entry = entry?;
        // Keep `build/` (gradle's output) and the jniLibs subtree.
        if entry.file_name() == "build" {
            continue;
        }
        if entry.path().is_dir() && entry.file_name() == "src" {
            clean_under_src(&entry.path())?;
            continue;
        }
        remove_path(&entry.path())?;
    }
    Ok(())
}

fn clean_under_src(src_dir: &Path) -> Result<()> {
    for entry in std::fs::read_dir(src_dir)
        .with_context(|| format!("read_dir {}", src_dir.display()))?
    {
        let entry = entry?;
        if entry.path().is_dir() && entry.file_name() == "main" {
            clean_under_main(&entry.path())?;
            continue;
        }
        remove_path(&entry.path())?;
    }
    Ok(())
}

fn clean_under_main(main_dir: &Path) -> Result<()> {
    for entry in std::fs::read_dir(main_dir)
        .with_context(|| format!("read_dir {}", main_dir.display()))?
    {
        let entry = entry?;
        // Keep the jniLibs subtree (dylib drops here).
        if entry.file_name() == "jniLibs" {
            continue;
        }
        remove_path(&entry.path())?;
    }
    Ok(())
}

fn remove_path(p: &Path) -> Result<()> {
    if p.is_dir() {
        std::fs::remove_dir_all(p).with_context(|| format!("rm -rf {}", p.display()))
    } else {
        std::fs::remove_file(p).with_context(|| format!("rm {}", p.display()))
    }
}

fn write_file(path: &Path, bytes: &[u8], executable: bool) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir -p {}", parent.display()))?;
    }
    std::fs::write(path, bytes).with_context(|| format!("write {}", path.display()))?;
    #[cfg(unix)]
    if executable {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)?;
    }
    #[cfg(not(unix))]
    let _ = executable;
    Ok(())
}

/// Pull the Android-relevant subset of `AppConfig` into the renderer
/// input struct. Errors out on required-but-missing fields (an
/// applicationId is mandatory; everything else has a default).
pub fn inputs_from(
    app_config: &AppConfig,
    rust_lib_name: String,
    whisker_runtime_path: PathBuf,
    whisker_lynx_aar_dir: PathBuf,
) -> Result<AndroidInputs> {
    let app_name = app_config
        .name
        .clone()
        .ok_or_else(|| anyhow!("whisker.rs: app.name(\"…\") is required"))?;
    let version = app_config
        .version
        .clone()
        .unwrap_or_else(|| "0.1.0".to_string());
    let build_number = app_config.build_number.unwrap_or(1);
    let application_id = app_config
        .android
        .application_id
        .clone()
        .or_else(|| app_config.bundle_id.clone())
        .ok_or_else(|| anyhow!(
            "whisker.rs: app.android(|a| a.application_id(\"…\")) (or app.bundle_id) is required for Android"
        ))?;
    let min_sdk = app_config.android.min_sdk.unwrap_or(24);
    let target_sdk = app_config.android.target_sdk.unwrap_or(34);
    Ok(AndroidInputs {
        app_name,
        version,
        build_number,
        application_id,
        min_sdk,
        target_sdk,
        rust_lib_name,
        whisker_runtime_path,
        whisker_lynx_aar_dir,
        template_version: 2,
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_tempdir() -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let p = std::env::temp_dir().join(format!("whisker-cng-android-test-{pid}-{n}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn sample_inputs() -> AndroidInputs {
        AndroidInputs {
            app_name: "HelloWorld".into(),
            version: "0.1.0".into(),
            build_number: 1,
            application_id: "rs.whisker.examples.helloworld".into(),
            min_sdk: 24,
            target_sdk: 34,
            rust_lib_name: "hello_world".into(),
            whisker_runtime_path: PathBuf::from(
                "/abs/native/android/whisker-runtime",
            ),
            whisker_lynx_aar_dir: PathBuf::from("/abs/target/lynx-android"),
            template_version: 2,
        }
    }

    #[test]
    fn template_vars_carry_required_keys() {
        let inputs = sample_inputs();
        let vars = template_vars(&inputs);
        assert_eq!(vars["android_application_id"], "rs.whisker.examples.helloworld");
        assert_eq!(vars["android_application_class"], "HelloWorldApplication");
        assert_eq!(vars["android_min_sdk"], "24");
        assert_eq!(vars["android_target_sdk"], "34");
        assert_eq!(vars["rust_lib_name"], "hello_world");
        assert_eq!(vars["build_number"], "1");
        assert_eq!(vars["version"], "0.1.0");
    }

    #[test]
    fn application_class_strips_punctuation() {
        assert_eq!(application_class_name("Hello World"), "HelloWorldApplication");
        assert_eq!(application_class_name("My-App"), "MyAppApplication");
    }

    #[test]
    fn project_name_lowercases_and_appends_android_suffix() {
        assert_eq!(project_name("HelloWorld"), "hello-world-android");
    }

    #[test]
    fn application_id_to_path_splits_on_dots() {
        assert_eq!(
            application_id_to_path("rs.whisker.examples.helloworld"),
            PathBuf::from("rs/whisker/examples/helloworld"),
        );
    }

    #[test]
    fn sync_writes_known_files_to_out_dir() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/android");
        let regenerated = sync(&out, &sample_inputs()).expect("sync");
        assert!(regenerated);

        for expected in [
            "app/build.gradle.kts",
            "app/src/main/AndroidManifest.xml",
            "app/src/main/kotlin/rs/whisker/examples/helloworld/MainActivity.kt",
            "app/src/main/kotlin/rs/whisker/examples/helloworld/HelloWorldApplication.kt",
            "build.gradle.kts",
            "settings.gradle.kts",
            "gradle.properties",
            "gradlew",
            "gradlew.bat",
            "gradle/wrapper/gradle-wrapper.properties",
            "gradle/wrapper/gradle-wrapper.jar",
            ".whisker-fingerprint",
        ] {
            assert!(out.join(expected).exists(), "missing: {expected}");
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn sync_substitutes_placeholders_in_generated_files() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/android");
        sync(&out, &sample_inputs()).unwrap();

        let manifest =
            std::fs::read_to_string(out.join("app/src/main/AndroidManifest.xml")).unwrap();
        assert!(manifest.contains("android:name=\".HelloWorldApplication\""));
        assert!(manifest.contains("android:label=\"HelloWorld\""));
        assert!(!manifest.contains("{{"));

        let main_activity = std::fs::read_to_string(
            out.join("app/src/main/kotlin/rs/whisker/examples/helloworld/MainActivity.kt"),
        )
        .unwrap();
        assert!(main_activity.starts_with("package rs.whisker.examples.helloworld\n"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn sync_is_idempotent_when_fingerprint_matches() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/android");
        let first = sync(&out, &sample_inputs()).unwrap();
        assert!(first);
        let second = sync(&out, &sample_inputs()).unwrap();
        assert!(!second, "second sync should be a no-op");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn sync_regenerates_when_inputs_change() {
        let tmp = unique_tempdir();
        let out = tmp.join("gen/android");
        sync(&out, &sample_inputs()).unwrap();
        let mut next = sample_inputs();
        next.target_sdk = 35;
        let regenerated = sync(&out, &next).unwrap();
        assert!(regenerated);
        let app_gradle =
            std::fs::read_to_string(out.join("app/build.gradle.kts")).unwrap();
        assert!(app_gradle.contains("compileSdk = 35"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn sync_preserves_jnilibs_across_regeneration() {
        // The dylib `cargo build` drops here must survive a sync.
        let tmp = unique_tempdir();
        let out = tmp.join("gen/android");
        sync(&out, &sample_inputs()).unwrap();
        let jni = out.join("app/src/main/jniLibs/arm64-v8a");
        std::fs::create_dir_all(&jni).unwrap();
        let dylib = jni.join("libhello_world.so");
        std::fs::write(&dylib, b"FAKE_DYLIB").unwrap();

        let mut next = sample_inputs();
        next.min_sdk = 25;
        sync(&out, &next).unwrap();
        assert!(dylib.exists(), "dylib was wiped by re-sync");
        assert_eq!(std::fs::read(&dylib).unwrap(), b"FAKE_DYLIB");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn inputs_from_errors_when_application_id_unset() {
        let mut cfg = AppConfig::default();
        cfg.name = Some("X".into());
        let err = inputs_from(&cfg, "x".into(), PathBuf::new(), PathBuf::new()).unwrap_err();
        assert!(err.to_string().contains("application_id"), "got: {err:#}");
    }
}
