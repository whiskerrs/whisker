//! End-to-end check on `IosProjectIr.pbxproj_ops` → rendered
//! `project.pbxproj` (RFC #164 B-direction PR 4).
//!
//! The renderer uses template injection — no pbxproj parsing.
//! Plugin-contributed ops produce additions to:
//!   - PBXBuildFile + PBXFileReference sections (file declared)
//!   - PBXSourcesBuildPhase / PBXResourcesBuildPhase /
//!     PBXFrameworksBuildPhase files lists (built into target)
//!   - "Whisker Plugin Files" PBXGroup (visible in navigator)
//!   - target Debug + Release XCBuildConfiguration buildSettings
//!     (SetBuildSetting)

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use whisker_cng::plugins::ios_pbxproj_ops::IosPbxprojOps;
use whisker_config::Config;

fn unique_tempdir() -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let p = std::env::temp_dir().join(format!("whisker-cng-pbxproj-e2e-{pid}-{n}"));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn base_app() -> Config {
    let mut a = Config::default();
    a.name("HelloWorld")
        .bundle_id("rs.whisker.examples.helloWorld");
    a
}

fn sync_and_read_pbxproj(app: &Config) -> (PathBuf, String) {
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
    let pbxproj =
        std::fs::read_to_string(out.join(format!("{}.xcodeproj/project.pbxproj", inputs.scheme)))
            .unwrap();
    (tmp, pbxproj)
}

// ============================================================================
// AddResource — the Firebase GoogleService-Info.plist case
// ============================================================================

#[test]
fn add_resource_emits_pbx_build_file_and_file_reference() {
    let mut app = base_app();
    app.plugin::<IosPbxprojOps>(|c| {
        c.add_resource("GoogleService-Info.plist");
    });
    let (tmp, pbxproj) = sync_and_read_pbxproj(&app);
    assert!(
        pbxproj.contains("/* GoogleService-Info.plist in Resources */"),
        "PBXBuildFile entry missing",
    );
    assert!(
        pbxproj.contains("isa = PBXFileReference; lastKnownFileType = text.plist.xml")
            && pbxproj.contains("path = \"GoogleService-Info.plist\""),
        "PBXFileReference entry missing",
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn add_resource_appears_in_resources_build_phase_files_list() {
    let mut app = base_app();
    app.plugin::<IosPbxprojOps>(|c| {
        c.add_resource("GoogleService-Info.plist");
    });
    let (tmp, pbxproj) = sync_and_read_pbxproj(&app);
    let phase_open = pbxproj
        .find("isa = PBXResourcesBuildPhase;")
        .expect("PBXResourcesBuildPhase block");
    let phase_close = pbxproj[phase_open..]
        .find("\n\t\t};")
        .map(|i| phase_open + i)
        .unwrap();
    let inside_phase = &pbxproj[phase_open..phase_close];
    assert!(
        inside_phase.contains("GoogleService-Info.plist in Resources"),
        "Resources phase missing the file: {inside_phase}",
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn add_resource_appears_in_whisker_plugin_files_group() {
    let mut app = base_app();
    app.plugin::<IosPbxprojOps>(|c| {
        c.add_resource("GoogleService-Info.plist");
    });
    let (tmp, pbxproj) = sync_and_read_pbxproj(&app);
    let group_open = pbxproj.find("name = \"Whisker Plugin Files\"").unwrap();
    // Walk backwards to children = (
    let children_marker = pbxproj[..group_open].rfind("children = (").unwrap();
    let group_section = &pbxproj[children_marker..group_open];
    assert!(
        group_section.contains("GoogleService-Info.plist"),
        "navigator group missing the file: {group_section}",
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

// ============================================================================
// AddSource
// ============================================================================

#[test]
fn add_source_lands_in_sources_build_phase() {
    let mut app = base_app();
    app.plugin::<IosPbxprojOps>(|c| {
        c.add_source("Sources/PluginContrib.swift");
    });
    let (tmp, pbxproj) = sync_and_read_pbxproj(&app);
    let sources_open = pbxproj
        .find("isa = PBXSourcesBuildPhase;")
        .expect("PBXSourcesBuildPhase block");
    let sources_close = pbxproj[sources_open..]
        .find("\n\t\t};")
        .map(|i| sources_open + i)
        .unwrap();
    let inside = &pbxproj[sources_open..sources_close];
    assert!(
        inside.contains("Sources/PluginContrib.swift in Sources"),
        "Sources phase missing the file: {inside}",
    );
    // PBXFileReference's lastKnownFileType correctly identified.
    assert!(pbxproj.contains("lastKnownFileType = sourcecode.swift"));
    let _ = std::fs::remove_dir_all(&tmp);
}

// ============================================================================
// LinkSystemFramework
// ============================================================================

#[test]
fn link_system_framework_emits_sdkroot_file_ref_and_appears_in_frameworks_phase() {
    let mut app = base_app();
    app.plugin::<IosPbxprojOps>(|c| {
        c.link_system_framework("AVFoundation.framework");
    });
    let (tmp, pbxproj) = sync_and_read_pbxproj(&app);
    // File reference uses sourceTree = SDKROOT and the canonical
    // System/Library/Frameworks path.
    assert!(
        pbxproj.contains("path = \"System/Library/Frameworks/AVFoundation.framework\"")
            && pbxproj.contains("sourceTree = SDKROOT"),
        "framework file ref shape wrong",
    );
    // Frameworks phase carries the new entry.
    let frameworks_open = pbxproj.find("isa = PBXFrameworksBuildPhase;").unwrap();
    let frameworks_close = pbxproj[frameworks_open..]
        .find("\n\t\t};")
        .map(|i| frameworks_open + i)
        .unwrap();
    let inside = &pbxproj[frameworks_open..frameworks_close];
    assert!(
        inside.contains("AVFoundation.framework in Frameworks"),
        "Frameworks phase missing: {inside}",
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

// ============================================================================
// SetBuildSetting
// ============================================================================

#[test]
fn set_build_setting_appears_in_both_debug_and_release_target_configs() {
    let mut app = base_app();
    app.plugin::<IosPbxprojOps>(|c| {
        c.set_build_setting("OTHER_LDFLAGS", "$(inherited) -ObjC");
    });
    let (tmp, pbxproj) = sync_and_read_pbxproj(&app);
    // Each target config block's buildSettings should now end with
    // the new entry. We look for it appearing twice — once Debug,
    // once Release.
    let occurrences = pbxproj
        .matches("OTHER_LDFLAGS = \"$(inherited) -ObjC\"")
        .count();
    assert_eq!(
        occurrences, 2,
        "expected the build setting in both Debug + Release, found {occurrences}",
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn set_build_setting_escapes_quotes_in_value() {
    // Some compiler defines need quotes inside the value, e.g.
    // `-DFOO="bar"` becomes `-DFOO=\"bar\"` in the pbxproj.
    // Without escaping, the plain `"` would terminate the
    // surrounding OpenStep plist string and break the parser.
    let mut app = base_app();
    app.plugin::<IosPbxprojOps>(|c| {
        c.set_build_setting("GCC_PREPROCESSOR_DEFINITIONS", "FOO=\"bar baz\"");
    });
    let (tmp, pbxproj) = sync_and_read_pbxproj(&app);
    // The escaped form ends up inside the quoted literal.
    assert!(
        pbxproj.contains("GCC_PREPROCESSOR_DEFINITIONS = \"FOO=\\\"bar baz\\\"\""),
        "value not escaped: {pbxproj}",
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

// ============================================================================
// Determinism — same input → same UUIDs across renders
// ============================================================================

#[test]
fn rendering_twice_with_same_input_yields_byte_identical_pbxproj() {
    let mut app = base_app();
    app.plugin::<IosPbxprojOps>(|c| {
        c.add_resource("GoogleService-Info.plist")
            .link_system_framework("AVFoundation.framework");
    });
    let (tmp_a, pbxproj_a) = sync_and_read_pbxproj(&app);
    let (tmp_b, pbxproj_b) = sync_and_read_pbxproj(&app);
    assert_eq!(pbxproj_a, pbxproj_b);
    let _ = std::fs::remove_dir_all(&tmp_a);
    let _ = std::fs::remove_dir_all(&tmp_b);
}

// ============================================================================
// No-op when no plugin contributes
// ============================================================================

#[test]
fn baseline_pbxproj_is_intact_when_no_plugin_declared() {
    let app = base_app();
    let (tmp, pbxproj) = sync_and_read_pbxproj(&app);
    // Baseline still present.
    assert!(pbxproj.contains("PRODUCT_BUNDLE_IDENTIFIER = \"rs.whisker.examples.helloWorld\""));
    assert!(pbxproj.contains("AppDelegate.swift"));
    // PBXResourcesBuildPhase exists (it's always emitted), but
    // its files list is empty.
    let resources_open = pbxproj.find("isa = PBXResourcesBuildPhase;").unwrap();
    let resources_close = pbxproj[resources_open..]
        .find("\n\t\t};")
        .map(|i| resources_open + i)
        .unwrap();
    let inside = &pbxproj[resources_open..resources_close];
    // No entries beyond the empty files = () marker.
    assert!(!inside.contains("in Resources */,"), "{inside}");
    // No SetBuildSetting noise either.
    assert!(!pbxproj.contains("/* extra-pbxproj */"));
    let _ = std::fs::remove_dir_all(&tmp);
}

// ============================================================================
// Realistic: Firebase iOS scenario
// ============================================================================

#[test]
fn firebase_ios_scenario_emits_resource_registration_and_objc_flag() {
    // A `whisker-firebase` plugin (Phase 4 dogfood) would:
    //   1. Drop GoogleService-Info.plist via extra_files
    //   2. Register it as a resource so Xcode bundles it
    //   3. Some Firebase SDKs require -ObjC linker flag for
    //      categories on NSObject subclasses
    let mut app = base_app();
    app.plugin::<IosPbxprojOps>(|c| {
        c.add_resource("GoogleService-Info.plist")
            .set_build_setting("OTHER_LDFLAGS", "$(inherited) -ObjC");
    });
    let (tmp, pbxproj) = sync_and_read_pbxproj(&app);
    assert!(pbxproj.contains("GoogleService-Info.plist in Resources"));
    assert!(
        pbxproj
            .matches("OTHER_LDFLAGS = \"$(inherited) -ObjC\"")
            .count()
            == 2,
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
