#![cfg(feature = "generate")]

#[path = "fixtures/project_support.rs"]
mod support;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use support::Fixture;
use whisker_cng::{Config, ProjectEngine, ProjectStep};
use whisker_plugin::{PluginConfig, project::*};

fn fixture(platform: &str) -> ProjectIr {
    let source = match platform {
        "ios" => include_str!("../../whisker-plugin/src/project/fixtures/ios.json"),
        "android" => include_str!("../../whisker-plugin/src/project/fixtures/android.json"),
        "macos" => include_str!("../../whisker-plugin/src/project/fixtures/macos.json"),
        "windows" => include_str!("../../whisker-plugin/src/project/fixtures/windows.json"),
        "linux" => include_str!("../../whisker-plugin/src/project/fixtures/linux.json"),
        "web" => include_str!("../../whisker-plugin/src/project/fixtures/web.json"),
        _ => panic!("unknown fixture"),
    };
    serde_json::from_str(source).unwrap()
}

fn engine(subprocess: bool) -> ProjectEngine {
    let mut e = ProjectEngine::new().with_app_crate_dir(PathBuf::from("app-root"));
    if subprocess {
        e.register_subprocess(
            "fixture-project",
            env!("CARGO_BIN_EXE_whisker-cng-fixture-project-plugin"),
        );
    } else {
        e.register(Fixture);
    }
    e
}

#[derive(Default, Serialize, Deserialize)]
struct FirstConfig;
impl PluginConfig for FirstConfig {
    const NAME: &'static str = "first";
}
struct First {
    update: ProjectUpdate,
    before: &'static [&'static str],
    after: &'static [&'static str],
}
impl ProjectPlugin for First {
    type Config = FirstConfig;
    fn before(&self) -> &'static [&'static str] {
        self.before
    }
    fn after(&self) -> &'static [&'static str] {
        self.after
    }
    fn contribute(&self, _: &ProjectContext, _: &FirstConfig) -> Result<ProjectUpdate> {
        Ok(self.update.clone())
    }
}
fn first(update: ProjectUpdate) -> First {
    First {
        update,
        before: &["fixture-project"],
        after: &[],
    }
}
fn merge(project: ProjectIr) -> ProjectUpdate {
    ProjectUpdate::Merge {
        project: Box::new(project),
    }
}
fn replace(project: ProjectIr, reason: &str) -> ProjectUpdate {
    ProjectUpdate::Replace {
        project: Box::new(project),
        reason: reason.into(),
    }
}

#[test]
fn six_platforms_roundtrip_through_typed_and_subprocess_plugins() {
    for platform in ["ios", "android", "macos", "windows", "linux", "web"] {
        let base = fixture(platform);
        let mut cfg = Config::default();
        cfg.project_plugin::<Fixture>(|c| {
            c.expected = Some(ProjectContext {
                project: base.clone(),
                app_crate_dir: Some("app-root".into()),
            });
            c.update = Some(merge(base.clone()));
        });
        let typed = engine(false).compose(&cfg, &base).unwrap();
        let process = engine(true).compose(&cfg, &base).unwrap();
        assert_eq!(typed, process);
        assert_eq!(process.project, base);
        assert_eq!(
            process.steps,
            [ProjectStep::Merge {
                plugin: "fixture-project".into()
            }]
        );
    }
}

#[test]
fn prior_fields_large_payload_and_app_root_survive_subprocess_then_later_plugin() {
    let base = fixture("android");
    let mut before = base.clone();
    let ProjectIr::Android(ir) = &mut before else {
        unreachable!()
    };
    ir.properties
        .insert("first.property".into(), "a".repeat(200_000));
    let mut next = before.clone();
    let ProjectIr::Android(ir) = &mut next else {
        unreachable!()
    };
    ir.properties
        .insert("subprocess.property".into(), "b".repeat(200_000));
    let mut cfg = Config::default();
    cfg.project_plugin::<Fixture>(|c| {
        c.expected = Some(ProjectContext {
            project: before.clone(),
            app_crate_dir: Some("app-root".into()),
        });
        c.update = Some(merge(next.clone()));
    });
    for subprocess in [false, true] {
        let mut e = engine(subprocess);
        e.register(first(merge(before.clone())));
        e.register(After {
            expected: next.clone(),
        });
        let result = e.compose(&cfg, &base).unwrap();
        assert_eq!(result.project, next);
        assert!(
            matches!(&result.steps[..], [ProjectStep::Merge {plugin: a}, ProjectStep::Merge {plugin: b}, ProjectStep::Keep {plugin: c}] if a == "first" && b == "fixture-project" && c == "after")
        );
    }
}
#[derive(Default, Serialize, Deserialize)]
struct AfterConfig;
impl PluginConfig for AfterConfig {
    const NAME: &'static str = "after";
}
struct After {
    expected: ProjectIr,
}
impl ProjectPlugin for After {
    type Config = AfterConfig;
    fn after(&self) -> &'static [&'static str] {
        &["fixture-project"]
    }
    fn contribute(&self, context: &ProjectContext, _: &AfterConfig) -> Result<ProjectUpdate> {
        assert_eq!(context.project, self.expected);
        Ok(ProjectUpdate::Keep)
    }
}

#[test]
fn conflicting_writers_are_rejected_and_foundation_is_unchanged() {
    let base = fixture("android");
    let original = base.clone();
    let mut before = base.clone();
    let ProjectIr::Android(ir) = &mut before else {
        unreachable!()
    };
    ir.properties.insert("shared".into(), "one".into());
    let mut conflict = base.clone();
    let ProjectIr::Android(ir) = &mut conflict else {
        unreachable!()
    };
    ir.properties.insert("shared".into(), "two".into());
    let mut cfg = Config::default();
    cfg.project_plugin::<Fixture>(|c| c.update = Some(merge(conflict)));
    for subprocess in [false, true] {
        let mut e = engine(subprocess);
        e.register(first(merge(before.clone())));
        let error = format!("{:#}", e.compose(&cfg, &base).unwrap_err());
        for part in [
            "fixture-project",
            "first",
            "properties",
            "shared",
            "conflicting",
        ] {
            assert!(error.contains(part), "{error}");
        }
        assert_eq!(base, original);
    }
}

#[test]
fn explicit_replacement_preserves_intended_edits_and_records_reason() {
    let base = fixture("web");
    let mut replacement = base.clone();
    let ProjectIr::Web(web) = &mut replacement else {
        unreachable!()
    };
    web.title = "Plugin title".into();
    web.head.clear();
    let mut cfg = Config::default();
    cfg.name("Original app title");
    cfg.project_plugin::<Fixture>(|c| {
        c.update = Some(replace(replacement.clone(), "replace document metadata"))
    });
    for subprocess in [false, true] {
        let result = engine(subprocess).compose(&cfg, &base).unwrap();
        assert_eq!(result.project, replacement);
        assert_eq!(
            result.steps,
            [ProjectStep::Replace {
                plugin: "fixture-project".into(),
                reason: "replace document metadata".into()
            }]
        );
    }
}

#[test]
fn replacement_requires_reason_and_cannot_switch_platforms() {
    let base = fixture("web");
    for (update, expected) in [
        (replace(base.clone(), " "), "reason"),
        (replace(fixture("android"), "switch"), "platform"),
        (merge(fixture("android")), "platform"),
    ] {
        let mut cfg = Config::default();
        cfg.project_plugin::<Fixture>(|c| c.update = Some(update));
        for subprocess in [false, true] {
            let error = format!("{:#}", engine(subprocess).compose(&cfg, &base).unwrap_err());
            assert!(error.contains(expected), "{error}");
        }
    }
}

#[test]
fn references_may_be_completed_by_later_plugins_but_must_exist_at_end() {
    for platform in ["ios", "android"] {
        let complete = fixture(platform);
        let mut base = complete.clone();
        match &mut base {
            ProjectIr::Ios(ir) => {
                ir.apple.targets.remove("share").unwrap();
            }
            ProjectIr::Android(ir) => {
                ir.modules.remove(":shared").unwrap();
            }
            _ => unreachable!(),
        }
        assert!(base.validate_structure().is_err());
        assert!(
            ProjectEngine::new()
                .compose(&Config::default(), &base)
                .is_err()
        );
        for subprocess in [false, true] {
            let mut e = engine(subprocess);
            e.register(first(merge(base.clone())));
            let mut cfg = Config::default();
            cfg.project_plugin::<Fixture>(|c| c.update = Some(merge(complete.clone())));
            assert_eq!(e.compose(&cfg, &base).unwrap().project, complete);
        }
    }
}

#[test]
fn ordering_errors_and_unregistered_config_fail_before_contributions() {
    let base = fixture("web");
    let invalid_update = replace(base.clone(), ""); // Must never reach application.
    for (before, after, expected) in [
        (&["missing"][..], &[][..], "no plugin"),
        (&["first"][..], &[][..], "itself"),
        (&["fixture-project"][..], &["fixture-project"][..], "cycle"),
    ] {
        let mut e = engine(false);
        e.register(First {
            update: invalid_update.clone(),
            before,
            after,
        });
        assert!(
            format!("{:#}", e.compose(&Config::default(), &base).unwrap_err()).contains(expected)
        );
    }
    let mut e = engine(false);
    e.register(Fixture);
    assert!(
        e.compose(&Config::default(), &base)
            .unwrap_err()
            .to_string()
            .contains("same name")
    );
    let mut cfg = Config::default();
    cfg.project_plugin::<Fixture>(|_| {});
    assert!(
        ProjectEngine::new()
            .compose(&cfg, &base)
            .unwrap_err()
            .to_string()
            .contains("not registered")
    );
}

#[test]
fn default_config_and_validation_are_identical_in_both_modes() {
    for subprocess in [false, true] {
        let e = engine(subprocess);
        let base = fixture("web");
        assert_eq!(
            e.compose(&Config::default(), &base).unwrap().steps,
            [ProjectStep::Keep {
                plugin: "fixture-project".into()
            }]
        );
        let mut cfg = Config::default();
        cfg.project_plugin::<Fixture>(|c| c.reject = true);
        assert!(e.compose(&cfg, &base).is_err());
        cfg.project_plugin::<Fixture>(|_| {}); // last configuration wins
        assert!(e.compose(&cfg, &base).is_ok());
        cfg.plugins.insert(
            "fixture-project".into(),
            serde_json::json!({"reject":"not-a-bool"}),
        );
        assert!(e.compose(&cfg, &base).is_err());
    }
}

#[test]
fn incompatible_legacy_and_wrong_identity_binaries_fail_preflight() {
    for (name, binary, expected) in [
        (
            "incompatible",
            env!("CARGO_BIN_EXE_whisker-cng-fixture-project-incompatible"),
            "unsupported project plugin protocol/schema",
        ),
        (
            "fixture-echo-plugin",
            env!("CARGO_BIN_EXE_whisker-cng-fixture-echo-plugin"),
            "exited",
        ),
        (
            "wrong-name",
            env!("CARGO_BIN_EXE_whisker-cng-fixture-project-plugin"),
            "identity mismatch",
        ),
    ] {
        let mut e = ProjectEngine::new();
        e.register(first(replace(fixture("web"), ""))); // If run, would fail for a different reason.
        e.register_subprocess(name, binary);
        let error = format!(
            "{:#}",
            e.compose(&Config::default(), &fixture("web")).unwrap_err()
        );
        assert!(
            error.contains("preflight") && error.contains(expected),
            "{error}"
        );
    }
}

#[test]
fn contribution_responses_are_checked_even_after_successful_preflight() {
    for (fault, expected) in [
        ("schema", "unsupported"),
        ("descriptor", "changed its descriptor"),
        ("phase", "instead of a contribution"),
        ("unversioned", "protocol"),
    ] {
        let mut cfg = Config::default();
        cfg.plugins.insert(
            "fixture-project".into(),
            serde_json::json!({"__wire_fault": fault}),
        );
        let error = format!(
            "{:#}",
            engine(true).compose(&cfg, &fixture("web")).unwrap_err()
        );
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn initializer_uses_normal_updates_and_precedes_typed_and_subprocess_plugins() {
    let empty = ProjectIr::Android(Box::default());
    let complete = fixture("android");
    for subprocess in [false, true] {
        // "fixture-project" sorts after "first"; use the fixture as initializer
        // to prove that this ordering is independent of plugin names.
        let mut e = ProjectEngine::with_initializer(Fixture);
        e.register(First {
            update: ProjectUpdate::Keep,
            before: &[],
            after: &[],
        });
        let mut config = Config::default();
        config.project_plugin::<Fixture>(|c| {
            c.expected = Some(ProjectContext {
                project: empty.clone(),
                app_crate_dir: None,
            });
            c.update = Some(merge(complete.clone()));
        });
        let result = e.compose(&config, &empty).unwrap();
        assert_eq!(result.project, complete);
        assert_eq!(
            result.steps[0],
            ProjectStep::Merge {
                plugin: "fixture-project".into()
            }
        );

        // No application data in the engine's input. The following plugin must
        // see the initializer's contribution, in either invocation mode.
        let mut e = ProjectEngine::with_initializer(First {
            update: merge(complete.clone()),
            before: &[],
            after: &[],
        });
        if subprocess {
            e.register_subprocess(
                "fixture-project",
                env!("CARGO_BIN_EXE_whisker-cng-fixture-project-plugin"),
            );
        } else {
            e.register(Fixture);
        }
        config.project_plugin::<Fixture>(|c| {
            c.expected = Some(ProjectContext {
                project: complete.clone(),
                app_crate_dir: None,
            });
        });
        let result = e.compose(&config, &empty).unwrap();
        assert_eq!(result.project, complete);
        assert_eq!(
            result.steps[0],
            ProjectStep::Merge {
                plugin: "first".into()
            }
        );
    }
    assert!(
        ProjectEngine::new()
            .compose(&Config::default(), &empty)
            .is_err()
    );
    assert_eq!(empty, ProjectIr::Android(Box::default()));
}

#[test]
fn initializer_ordering_and_preflight_errors_prevent_all_contributions() {
    let empty = ProjectIr::Android(Box::default());
    for first_before in [&["fixture-project"][..], &[][..]] {
        let mut e = ProjectEngine::with_initializer(Fixture);
        e.register(First {
            update: ProjectUpdate::Keep,
            before: first_before,
            after: &[],
        });
        let mut config = Config::default();
        config.project_plugin::<Fixture>(|c| c.reject = true);
        if first_before.is_empty() {
            e.register_subprocess(
                "incompatible",
                env!("CARGO_BIN_EXE_whisker-cng-fixture-project-incompatible"),
            );
        }
        let error = format!("{:#}", e.compose(&config, &empty).unwrap_err());
        assert!(
            error.contains(if first_before.is_empty() {
                "preflight"
            } else {
                "ordering cycle"
            }),
            "{error}"
        );
    }
    let mut e = ProjectEngine::with_initializer(First {
        update: merge(fixture("android")),
        before: &[],
        after: &["fixture-project"],
    });
    e.register(Fixture);
    assert!(
        e.compose(&Config::default(), &empty)
            .unwrap_err()
            .to_string()
            .contains("cycle")
    );
    let mut e = ProjectEngine::with_initializer(Fixture);
    e.register(Fixture);
    assert!(
        e.compose(&Config::default(), &empty)
            .unwrap_err()
            .to_string()
            .contains("same name")
    );
}
