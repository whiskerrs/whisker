use super::*;
use std::time::SystemTime;

// Diagnostic from both failed attempts of the v0.13.3 release.
const THROTTLED: &str = "the remote server responded with an error (status 429 Too Many Requests): You have published too many updates to existing crates in a short period of time. Please try again after Fri, 04 Sep 2026 18:40:23 GMT and see https://crates.io/docs/rate-limits for more details.";

#[test]
fn honors_registry_reset_and_ignores_duplicate_diagnostics() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_788_547_185);
    assert_eq!(
        publish::retry_delay(THROTTLED, now),
        Some(Duration::from_secs(48))
    );
    assert_eq!(
        publish::retry_delay(&format!("{THROTTLED}\n{THROTTLED}"), now),
        Some(Duration::from_secs(48))
    );
}

#[test]
fn stale_or_unparseable_reset_has_bounded_fallback() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_788_547_224);
    assert_eq!(
        publish::retry_delay(THROTTLED, now),
        Some(Duration::from_secs(120))
    );
    let invalid = THROTTLED.replace("Fri, 04 Sep 2026 18:40:23 GMT", "invalid GMT");
    assert_eq!(
        publish::retry_delay(&invalid, now),
        Some(Duration::from_secs(120))
    );
}

#[test]
fn other_failures_are_not_retried() {
    for error in [
        "error[E0432]: unresolved import",
        "crates.io: 403 Forbidden",
        "GitHub: status 429 Too Many Requests",
        "failed to publish crate version 0.4.29",
    ] {
        assert_eq!(publish::retry_delay(error, SystemTime::now()), None);
    }
}

#[cfg(unix)]
#[test]
fn subprocess_failure_after_throttling_does_not_reuse_old_diagnostics() {
    for (message, status, retryable) in
        [(THROTTLED, "1", true), ("compilation failed", "101", false)]
    {
        let (success, diagnostics) = publish::cargo_publish(
            Command::new("sh")
                .args([
                    "-c",
                    "printf '%s\\n' \"$MESSAGE\" >&2; exit \"$EXIT_STATUS\"",
                ])
                .env("MESSAGE", message)
                .env("EXIT_STATUS", status),
        )
        .unwrap();
        assert!(!success);
        assert_eq!(
            publish::retry_delay(&diagnostics, SystemTime::now()).is_some(),
            retryable
        );
    }
}

#[test]
fn registry_check_uses_exact_versions_and_rejects_yanked_or_invalid_data() {
    let index = "{\"vers\":\"0.13.3\",\"yanked\":false}\n{\"vers\":\"0.13.4\",\"yanked\":true}\n";
    assert!(publish::index_contains(index, "0.13.3").unwrap());
    assert!(!publish::index_contains(index, "0.13.5").unwrap());
    assert!(publish::index_contains(index, "0.13.4").is_err());
    assert!(publish::index_contains("<html>unavailable</html>", "0.13.3").is_err());
    for (name, path) in [
        ("a", "1/a"),
        ("AB", "2/ab"),
        ("abc", "3/a/abc"),
        ("whisker", "wh/is/whisker"),
    ] {
        assert_eq!(publish::index_path(name).unwrap(), path);
    }
    assert!(publish::index_path("../private").is_err());
}

#[test]
fn versions_cannot_inject_paths_shell_or_workflow_outputs() {
    for invalid in [
        "v1.2.3",
        "01.2.3",
        "1.2.3\nsdk=9.9.9",
        "$(id)",
        "1.2.3/../../main",
        "1.2.3-rc.1",
    ] {
        assert!(version(invalid).is_err());
    }
    assert_eq!(version("1.2.3").unwrap().to_string(), "1.2.3");
}

#[test]
fn updates_renamed_and_target_path_dependencies_but_keeps_registry_dependencies() {
    let manifest = r#"[workspace.dependencies]
whisker = { path = "crates/whisker", version = "0.13.3", features = ["hot-reload"] }
registry = { package = "whisker", version = "0.13.3" }
[target.'cfg(unix)'.dependencies.alias]
package = "whisker"
path = "../whisker"
version = "0.13.3"
[dev-dependencies]
whisker = { path = "../whisker" }
"#;
    let updated = prepare::update_test_manifest(
        manifest,
        &BTreeMap::from([("whisker".to_owned(), "0.13.4".to_owned())]),
    );
    let doc: DocumentMut = updated.parse().unwrap();
    assert_eq!(
        doc["workspace"]["dependencies"]["whisker"]["version"].as_str(),
        Some("0.13.4")
    );
    assert_eq!(
        doc["target"]["cfg(unix)"]["dependencies"]["alias"]["version"].as_str(),
        Some("0.13.4")
    );
    assert_eq!(
        doc["workspace"]["dependencies"]["registry"]["version"].as_str(),
        Some("0.13.3")
    );
    assert!(doc["dev-dependencies"]["whisker"].get("version").is_none());
    assert!(updated.contains("features = [\"hot-reload\"]"));
}

fn fixture() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    write(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = ['crates/whisker', 'crates/whisker-subsecond', 'consumer']\nresolver = '2'\n[workspace.package]\nversion = '0.13.3'\nedition = '2021'\n[workspace.dependencies]\nwhisker = { path = 'crates/whisker', version = '0.13.3' }\n",
    );
    write(
        root,
        "crates/whisker/Cargo.toml",
        "[package]\nname = 'whisker'\nversion.workspace = true\nedition.workspace = true\n",
    );
    write(
        root,
        "crates/whisker-subsecond/Cargo.toml",
        "[package]\nname = 'whisker-subsecond'\nversion = '0.7.12'\nedition = '2021'\n",
    );
    write(
        root,
        "consumer/Cargo.toml",
        "[package]\nname = 'consumer'\nversion = '0.0.0'\nedition = '2021'\npublish = false\n[dependencies]\nwhisker = { path = '../crates/whisker', version = '0.13.3' }\n",
    );
    for path in [
        "crates/whisker/src/lib.rs",
        "crates/whisker-subsecond/src/lib.rs",
        "consumer/src/lib.rs",
    ] {
        write(root, path, "");
    }
    write(
        root,
        CLI_PATH,
        "const WHISKER_SDK_VERSION: &str = \"0.1.21\";\nconst WHISKER_GRADLE_PLUGIN_VERSION: &str = \"0.5.0\";\n",
    );
    write(
        root,
        IOS_PATH,
        "pub const WHISKER_IOS_SPM_VERSION: &str = \"0.1.13\";\n",
    );
    write(
        root,
        "packages/test/Package.swift",
        ".package(url: \"https://github.com/whiskerrs/whisker.git\", exact: \"0.1.13\")\n",
    );
    git(root, &["init", "-b", "main"]).unwrap();
    git(root, &["config", "user.name", "Release Test"]).unwrap();
    git(root, &["config", "user.email", "release@example.invalid"]).unwrap();
    git(root, &["add", "."]).unwrap();
    git(root, &["commit", "-m", "fixture"]).unwrap();
    directory
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn fixture_plan(root: &Path) -> ReleasePlan {
    ReleasePlan {
        version: "0.13.4".into(),
        source: git(root, &["rev-parse", "HEAD"]).unwrap().trim().into(),
        sdk: Some("0.1.22".into()),
        gradle: None,
        ios: Some("0.1.14".into()),
        subsecond: None,
        crates: BTreeMap::new(),
    }
}

#[test]
fn prepared_workspace_resolves_with_inheritance_fork_and_all_native_pins() {
    let directory = fixture();
    let root = directory.path();
    let mut plan = fixture_plan(root);
    prepare::stamp_versions(root, &plan).unwrap();
    plan.crates = published_packages(root).unwrap();
    assert_eq!(
        plan.crates,
        BTreeMap::from([
            ("whisker".into(), "0.13.4".into()),
            ("whisker-subsecond".into(), "0.7.12".into())
        ])
    );
    let crate_manifest = fs::read_to_string(root.join("crates/whisker/Cargo.toml")).unwrap();
    assert!(crate_manifest.contains("version.workspace = true"));
    assert_eq!(native::pin(root, "sdk").unwrap(), "0.1.22");
    assert_eq!(native::pin(root, "gradle").unwrap(), "0.5.0");
    native::check_swift_pins(root, "0.1.14").unwrap();
    write(root, &plan.notes_path(), "Release notes");
    plan.validate_checkout(root).unwrap();
    plan.crates.insert("whisker".into(), "9.9.9".into());
    assert!(plan.validate_checkout(root).is_err());
}

#[test]
fn independently_bumps_the_fork_and_rejects_version_reuse() {
    let directory = fixture();
    let root = directory.path();
    let mut plan = fixture_plan(root);
    plan.subsecond = Some("0.7.13".into());
    prepare::stamp_versions(root, &plan).unwrap();
    assert_eq!(
        published_packages(root).unwrap()["whisker-subsecond"],
        "0.7.13"
    );
    assert!(prepare::stamp_versions(root, &plan).is_err());
}

#[test]
fn mismatched_swift_module_pin_prevents_release() {
    let directory = fixture();
    write(
        directory.path(),
        "packages/another/Package.swift",
        ".package(url: \"https://github.com/whiskerrs/whisker.git\", exact: \"0.1.12\")\n",
    );
    assert!(native::check_swift_pins(directory.path(), "0.1.13").is_err());
}

#[test]
fn immutable_tags_resume_same_commit_but_never_move() {
    let directory = fixture();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare"]).unwrap();
    let root = directory.path();
    git(
        root,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    )
    .unwrap();
    ensure_tag(root, "sdk-v0.1.22", true).unwrap();
    ensure_tag(root, "sdk-v0.1.22", true).unwrap();
    git(root, &["commit", "--allow-empty", "-m", "later change"]).unwrap();
    assert!(ensure_tag(root, "sdk-v0.1.22", false).is_err());
    assert!(ensure_tag(root, "sdk-v0.1.22", true).is_err());
}

#[test]
fn verifies_sdk_artifacts_and_both_gradle_plugin_markers() {
    let sdk = native::artifact_urls("sdk", "0.1.22").unwrap();
    assert_eq!(sdk.len(), 6);
    assert!(
        sdk.iter()
            .any(|url| url.ends_with("whisker-runtime-android-0.1.22.aar"))
    );
    let plugin = native::artifact_urls("gradle", "0.5.1").unwrap();
    assert!(
        plugin
            .iter()
            .any(|url| url.contains("/rs/whisker/rs.whisker.gradle.plugin/"))
    );
    assert!(
        plugin
            .iter()
            .any(|url| url.contains("/rs/whisker/gradle/rs.whisker.gradle.gradle.plugin/"))
    );
}
