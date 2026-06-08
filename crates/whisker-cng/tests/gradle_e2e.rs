//! End-to-end check on the gradle IR pass-through (RFC #164
//! B-direction PR 2): built-in plugin → engine → `inputs_from` →
//! template substitution → rendered `app/build.gradle.kts`.
//!
//! Complements:
//!   - `crates/whisker-cng/src/plugins/android_gradle_*.rs`
//!     unit tests, which only check IR-level mutations
//!   - `tests/builtins_e2e.rs` which covers Info.plist /
//!     permissions / meta-data — those land in the manifest, not
//!     gradle
//!
//! Together with PR 1's IR-canonical refactor, this means
//! Firebase / Google Maps / Crashlytics integration is now
//! expressible as a single `app.plugin::<…>(|c| …)` block without
//! forking `whisker-cng`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use whisker_app_config::AppConfig;
use whisker_cng::plugins::android_gradle_dependencies::GradleDependencies;
use whisker_cng::plugins::android_gradle_plugins::GradlePlugins;

fn unique_tempdir() -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let p = std::env::temp_dir().join(format!("whisker-cng-gradle-e2e-{pid}-{n}"));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn base_android_app() -> AppConfig {
    let mut a = AppConfig::default();
    a.name("HelloWorld").android(|x| {
        x.application_id("rs.whisker.examples.helloworld");
    });
    a
}

fn sync_and_read_gradle(app: &AppConfig) -> String {
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
    let gradle = std::fs::read_to_string(out.join("app/build.gradle.kts")).unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
    gradle
}

// ============================================================================
// Gradle plugins block
// ============================================================================

#[test]
fn gradle_bare_plugin_id_is_wrapped_in_id_call() {
    let mut app = base_android_app();
    app.plugin::<GradlePlugins>(|c| {
        c.add("com.google.gms.google-services");
    });
    let gradle = sync_and_read_gradle(&app);
    assert!(
        gradle.contains("id(\"com.google.gms.google-services\")"),
        "{gradle}",
    );
}

#[test]
fn gradle_raw_id_line_passes_through_verbatim() {
    let mut app = base_android_app();
    app.plugin::<GradlePlugins>(|c| {
        c.add_raw("id(\"com.android.dynamic-feature\") version \"8.5.0\"");
    });
    let gradle = sync_and_read_gradle(&app);
    assert!(
        gradle.contains("id(\"com.android.dynamic-feature\") version \"8.5.0\""),
        "{gradle}",
    );
}

#[test]
fn gradle_version_catalog_alias_passes_through_verbatim() {
    // Version catalog form — the renderer recognises `(` as a
    // "this is already DSL" marker and doesn't double-wrap.
    let mut app = base_android_app();
    app.plugin::<GradlePlugins>(|c| {
        c.add_raw("alias(libs.plugins.kotlin.android)");
    });
    let gradle = sync_and_read_gradle(&app);
    assert!(
        gradle.contains("alias(libs.plugins.kotlin.android)"),
        "{gradle}",
    );
    // Specifically, the renderer must NOT have wrapped it as
    // `id("alias(libs.plugins.kotlin.android)")`.
    assert!(
        !gradle.contains("id(\"alias("),
        "renderer wrapped a DSL call: {gradle}",
    );
}

#[test]
fn gradle_plugin_entry_lands_inside_the_plugins_block() {
    let mut app = base_android_app();
    app.plugin::<GradlePlugins>(|c| {
        c.add("com.google.gms.google-services");
    });
    let gradle = sync_and_read_gradle(&app);
    // Find the plugins { } block and check our entry is inside.
    let plugins_open = gradle.find("plugins {").unwrap();
    let plugins_close = gradle[plugins_open..].find("\n}").unwrap() + plugins_open;
    let inside_plugins = &gradle[plugins_open..plugins_close];
    assert!(
        inside_plugins.contains("com.google.gms.google-services"),
        "must be inside plugins block: {inside_plugins}",
    );
}

// ============================================================================
// Gradle dependencies block
// ============================================================================

#[test]
fn gradle_dependency_line_emitted_verbatim() {
    let mut app = base_android_app();
    app.plugin::<GradleDependencies>(|c| {
        c.add("implementation(\"com.google.firebase:firebase-analytics:21.5.0\")");
    });
    let gradle = sync_and_read_gradle(&app);
    assert!(
        gradle.contains("implementation(\"com.google.firebase:firebase-analytics:21.5.0\")"),
        "{gradle}",
    );
}

#[test]
fn gradle_dependencies_land_inside_the_dependencies_block() {
    let mut app = base_android_app();
    app.plugin::<GradleDependencies>(|c| {
        c.add("implementation(\"com.example:lib:1.0\")");
    });
    let gradle = sync_and_read_gradle(&app);
    let deps_open = gradle.find("dependencies {").unwrap();
    let deps_close = gradle[deps_open..].find("\n}").unwrap() + deps_open;
    let inside_deps = &gradle[deps_open..deps_close];
    assert!(
        inside_deps.contains("com.example:lib:1.0"),
        "must be inside dependencies block: {inside_deps}",
    );
}

#[test]
fn gradle_dependencies_preserve_insertion_order() {
    let mut app = base_android_app();
    app.plugin::<GradleDependencies>(|c| {
        c.add("implementation(\"com.example:a:1.0\")")
            .add("implementation(\"com.example:b:1.0\")")
            .add("implementation(\"com.example:c:1.0\")");
    });
    let gradle = sync_and_read_gradle(&app);
    let a = gradle.find("com.example:a").unwrap();
    let b = gradle.find("com.example:b").unwrap();
    let c = gradle.find("com.example:c").unwrap();
    assert!(a < b && b < c, "ordering broken: {a} {b} {c}");
}

#[test]
fn gradle_supports_non_implementation_configurations() {
    // Real-world: Firebase needs kapt + classpath stuff in
    // various configurations. The raw-line approach must let
    // users emit any of them.
    let mut app = base_android_app();
    app.plugin::<GradleDependencies>(|c| {
        c.add("kapt(\"androidx.room:room-compiler:2.6.0\")")
            .add("runtimeOnly(\"com.example:plugin:1.0\")");
    });
    let gradle = sync_and_read_gradle(&app);
    assert!(gradle.contains("kapt(\"androidx.room:room-compiler:2.6.0\")"));
    assert!(gradle.contains("runtimeOnly(\"com.example:plugin:1.0\")"));
}

// ============================================================================
// No-op when no plugin declared
// ============================================================================

#[test]
fn gradle_baseline_unchanged_when_no_plugin_declared() {
    let app = base_android_app();
    let gradle = sync_and_read_gradle(&app);
    // Baseline plugins / deps must still be there.
    assert!(gradle.contains("id(\"com.android.application\")"));
    assert!(gradle.contains("id(\"rs.whisker.gradle\")"));
    assert!(gradle.contains("implementation(\"rs.whisker:whisker-runtime-android:"));
    // Nothing from any plugin.
    assert!(!gradle.contains("com.google.gms.google-services"));
    assert!(!gradle.contains("firebase-analytics"));
}

// ============================================================================
// Realistic scenario: Firebase
// ============================================================================

#[test]
fn gradle_firebase_scenario_reaches_the_rendered_file() {
    // Recreate the exact pattern a `whisker-firebase` plugin
    // (Phase 4 dogfood) would produce.
    let mut app = base_android_app();
    app.plugin::<GradlePlugins>(|c| {
        c.add("com.google.gms.google-services");
    });
    app.plugin::<GradleDependencies>(|c| {
        c.add("implementation(platform(\"com.google.firebase:firebase-bom:33.1.0\"))")
            .add("implementation(\"com.google.firebase:firebase-analytics\")");
    });
    let gradle = sync_and_read_gradle(&app);
    assert!(gradle.contains("id(\"com.google.gms.google-services\")"));
    assert!(gradle.contains("platform(\"com.google.firebase:firebase-bom:33.1.0\")"));
    assert!(gradle.contains("implementation(\"com.google.firebase:firebase-analytics\")"));
}
