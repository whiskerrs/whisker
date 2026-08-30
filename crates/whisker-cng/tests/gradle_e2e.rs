//! End-to-end check on the gradle IR pass-through: built-in plugin →
//! engine → `inputs_from` → template substitution → rendered
//! `app/build.gradle.kts`. The manifest-side equivalents
//! (Info.plist / permissions / meta-data) live in
//! `tests/builtins_e2e.rs`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use whisker_cng::plugins::android_gradle_dependencies::GradleDependencies;
use whisker_cng::plugins::android_gradle_plugins::GradlePlugins;
use whisker_config::Config;

fn unique_tempdir() -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let p = std::env::temp_dir().join(format!("whisker-cng-gradle-e2e-{pid}-{n}"));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn base_android_app() -> Config {
    let mut a = Config::default();
    a.name("HelloWorld").android(|x| {
        x.application_id("rs.whisker.examples.helloworld");
    });
    a
}

fn sync_and_read_gradle(app: &Config) -> String {
    let inputs = whisker_cng::android::inputs_from(
        app,
        "hello_world".into(),
        PathBuf::from("../.."),
        "hello-world".into(),
        "0.1.0".into(),
        "0.1.0".into(),
        "https://whiskerrs.github.io/whisker/maven".into(),
    )
    .unwrap();
    let tmp = unique_tempdir();
    let out = tmp.join("gen/android");
    whisker_cng::android::sync(&out, &inputs).unwrap();
    let gradle = std::fs::read_to_string(out.join("app/build.gradle.kts")).unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
    gradle
}

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
    let plugins_open = gradle.find("plugins {").unwrap();
    let plugins_close = gradle[plugins_open..].find("\n}").unwrap() + plugins_open;
    let inside_plugins = &gradle[plugins_open..plugins_close];
    assert!(
        inside_plugins.contains("com.google.gms.google-services"),
        "must be inside plugins block: {inside_plugins}",
    );
}

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
    let mut app = base_android_app();
    app.plugin::<GradleDependencies>(|c| {
        c.add("kapt(\"androidx.room:room-compiler:2.6.0\")")
            .add("runtimeOnly(\"com.example:plugin:1.0\")");
    });
    let gradle = sync_and_read_gradle(&app);
    assert!(gradle.contains("kapt(\"androidx.room:room-compiler:2.6.0\")"));
    assert!(gradle.contains("runtimeOnly(\"com.example:plugin:1.0\")"));
}

#[test]
fn gradle_baseline_contains_only_the_builtin_plugin_when_no_user_plugin_is_declared() {
    let app = base_android_app();
    let gradle = sync_and_read_gradle(&app);
    assert!(gradle.contains("id(\"com.android.application\")"));
    assert!(gradle.contains("id(\"rs.whisker.gradle\")"));
    assert!(gradle.contains("whisker-runtime-android"));
    assert!(!gradle.contains("lynx"));
    assert!(!gradle.contains("com.google.gms.google-services"));
    assert!(!gradle.contains("firebase-analytics"));
}

#[test]
fn gradle_firebase_scenario_reaches_the_rendered_file() {
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
