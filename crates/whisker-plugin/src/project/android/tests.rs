use super::*;
use crate::project::{ProjectIr, XmlNode};
use serde_json::{Value, json};

fn json_fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/android.json")).unwrap()
}

fn project(value: Value) -> AndroidProjectIr {
    let ProjectIr::Android(project) = serde_json::from_value(value).unwrap() else {
        unreachable!()
    };
    *project
}

fn check(project: AndroidProjectIr) -> anyhow::Result<()> {
    ProjectIr::Android(Box::new(project)).validate_structure()
}

fn app(project: &mut AndroidProjectIr) -> &mut AndroidApplication {
    let AndroidModuleKind::Application(app) = &mut project.modules.get_mut(":app").unwrap().kind
    else {
        unreachable!()
    };
    app
}

fn expanded_fixture() -> Value {
    let mut value = json_fixture();
    let modules = &mut value["project"]["modules"];
    modules[":jvm"] = json!({"directory":"jvm", "kind":{"kind":"jvm"}});
    modules[":custom"] = json!({"directory":"custom", "kind":{"kind":"custom"},
        "build":{"plugins":[{"id":"com.android.kotlin.multiplatform.library","apply":true}],
        "statements":["kotlin { androidLibrary { namespace = \"com.example.kmp\" } }"]}});
    modules[":external"] = json!({"directory":"external", "kind":{"kind":"external","config":{"build_file":"external/custom.gradle.kts"}}});
    modules[":pack"] = json!({"directory":"pack", "kind":{"kind":"asset_pack","config":{
        "pack_name":"photos", "delivery":"fast-follow", "assets":["pack/src/main/assets"]}}});
    modules[":tests"] = json!({"directory":"tests", "kind":{"kind":"test","config":{
        "target":":app", "android":{"namespace":"com.example.tests"}}}});
    modules[":app"]["kind"]["config"]["asset_packs"] = json!([":pack"]);
    modules[":app"]["dependencies"] = json!([
        {"configuration":"implementation","source":{"kind":"project","value":":external"}},
        {"configuration":"testImplementation","source":{"kind":"project","value":":jvm"}},
        {"configuration":"implementation","source":{"kind":"project","value":":custom"}}
    ]);
    let android = &mut modules[":app"]["kind"]["config"]["android"];
    android.as_object_mut().unwrap().remove("sdk");
    android["variants"] = json!({
        "flavor_dimensions":["tier"],
        "product_flavors":{"free":{"dimension":"tier","missing_dimension_strategies":{"store":["play","direct"]}}},
        "build_types":{"staging":{"matching_fallbacks":["debug","release"],"statements":["isDebuggable = true"]}}
    });
    android["source_sets"]["main"]["java_resources"] = json!(["app/src/main/resources"]);
    android["source_sets"]["main"]["aidl"] = json!(["app/src/main/aidl"]);
    android["source_sets"]["main"]["shaders"] = json!(["app/src/main/shaders"]);
    android["source_sets"]["main"]["baseline_profiles"] = json!(["app/src/main/baselineProfiles"]);
    value["project"]["settings"]["android_sdk"] = json!({
        "compile":{"kind":"release","api":36,"minor":1,"extension":2},
        "min":{"kind":"release","value":24},
        "target":{"kind":"preview","value":"FuturePreview"}
    });
    value["project"]["settings"]["plugin_management"]["plugins"] =
        json!({"com.android.settings":"9.2.0"});
    value["project"]["settings"]["plugins"] = json!([{"id":"com.android.settings","apply":true}]);
    value["project"]["root_build"] = json!({
        "imports":["java.util.Properties"], "buildscript":["repositories { mavenCentral() }"],
        "plugins":[{"id":"com.android.application","alias":"libs.plugins.android.application","apply":false}]
    });
    value
}

#[test]
fn all_roles_inherited_sdk_and_scopes_round_trip() {
    let project = project(expanded_fixture());
    check(project.clone()).unwrap();
    let wire = serde_json::to_value(&project).unwrap();
    let decoded: AndroidProjectIr = serde_json::from_value(wire).unwrap();
    assert_eq!(decoded, project);
    let AndroidModuleKind::Application(app) = &project.modules[":app"].kind else {
        unreachable!()
    };
    assert_eq!(app.android.sdk, AndroidSdk::default());
    let source = &app.android.source_sets["main"];
    assert_ne!(source.android_resources, source.java_resources);
    assert_eq!(project.settings.plugins[0].id, "com.android.settings");
    assert_eq!(
        project.root_build.plugins[0].alias.as_deref(),
        Some("libs.plugins.android.application")
    );
}

#[test]
fn sdk_supports_release_preview_addon_and_absence() {
    for compile in [
        json!({"kind":"release","api":36,"minor":1,"extension":4}),
        json!({"kind":"preview","codename":"FuturePreview"}),
        json!({"kind":"add_on","api":35,"vendor":"Vendor","name":"Addon"}),
        Value::Null,
    ] {
        let mut value = json_fixture();
        value["project"]["modules"][":app"]["kind"]["config"]["android"]["sdk"]["compile"] =
            compile;
        let project = project(value);
        check(project.clone()).unwrap();
        assert_eq!(
            project,
            serde_json::from_str(&serde_json::to_string(&project).unwrap()).unwrap()
        );
    }
}

#[test]
fn disjoint_configuration_edges_are_not_rejected_as_a_module_cycle() {
    let mut value = json_fixture();
    let modules = &mut value["project"]["modules"];
    for (id, dir, config, target) in [
        (":a", "a", "debugImplementation", ":b"),
        (":b", "b", "releaseImplementation", ":a"),
    ] {
        modules[id] = json!({"directory":dir,"kind":{"kind":"library","config":{"namespace":format!("com.example.{dir}")}},
            "dependencies":[{"configuration":config,"source":{"kind":"project","value":target}}]});
    }
    let mut project = project(value);
    check(project.clone()).unwrap();
    project.modules.get_mut(":a").unwrap().dependencies[0].source =
        GradleDependencySource::Project(":missing".into());
    assert!(
        check(project)
            .unwrap_err()
            .to_string()
            .contains("unknown ID")
    );
}

#[test]
fn role_relationships_and_external_ownership_are_validated() {
    let mut value = expanded_fixture();
    value["project"]["modules"][":photos"]["kind"]["config"]["base"] = json!(":shared");
    assert!(check(project(value)).is_err());
    let mut value = expanded_fixture();
    value["project"]["modules"][":tests"]["kind"]["config"]["target"] = json!(":pack");
    assert!(
        check(project(value))
            .unwrap_err()
            .to_string()
            .contains("test module")
    );
    let mut value = expanded_fixture();
    value["project"]["modules"][":app"]["kind"]["config"]["asset_packs"] = json!([":shared"]);
    assert!(
        check(project(value))
            .unwrap_err()
            .to_string()
            .contains("not an asset pack")
    );
    let mut value = expanded_fixture();
    value["project"]["modules"][":external"]["build"] =
        json!({"statements":["println(\"ignored?\")"]});
    assert!(
        check(project(value))
            .unwrap_err()
            .to_string()
            .contains("external module")
    );
    let mut value = expanded_fixture();
    value["project"]["modules"][":app"]["kind"]["config"]["android"]["variants"]["product_flavors"]
        ["free"]["dimension"] = json!("missing");
    assert!(
        check(project(value))
            .unwrap_err()
            .to_string()
            .contains("undeclared dimension")
    );
}

fn plugin(id: &str, version: &str) -> GradlePlugin {
    GradlePlugin {
        id: id.into(),
        version: Some(version.into()),
        alias: None,
        apply: true,
    }
}

#[test]
fn plugin_merge_retains_order_is_idempotent_and_rejects_conflicts_atomically() {
    let mut script = GradleBuildScript {
        plugins: vec![plugin("z.base", "1")],
        ..Default::default()
    };
    let contribution = GradleBuildScript {
        plugins: vec![plugin("a.feature", "2")],
        ..Default::default()
    };
    script.merge_from(&contribution).unwrap();
    script.merge_from(&contribution).unwrap();
    assert_eq!(
        script
            .plugins
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["z.base", "a.feature"]
    );
    let before = script.clone();
    let conflict = GradleBuildScript {
        imports: vec!["unexpected.Import".into()],
        plugins: vec![plugin("z.base", "3")],
        ..Default::default()
    };
    let error = script.merge_from(&conflict).unwrap_err().to_string();
    assert!(error.contains("plugins[z.base]"));
    assert_eq!(before, script);
}

#[test]
fn repository_and_sdk_conflicts_are_transactional() {
    let repository = |expression: &str| GradleRepository {
        id: "company".into(),
        expression: expression.into(),
    };
    let mut settings = GradleSettings::default();
    settings
        .plugin_management
        .repositories
        .push(repository("maven { url = uri(\"https://one.example\") }"));
    let mut patch = GradleSettings::default();
    patch.imports.push("unexpected.Import".into());
    patch
        .plugin_management
        .repositories
        .push(repository("maven { url = uri(\"https://two.example\") }"));
    let before = settings.clone();
    assert!(
        settings
            .merge_from(&patch)
            .unwrap_err()
            .to_string()
            .contains("repositories[company]")
    );
    assert_eq!(settings, before);

    let mut sdk = AndroidSdk {
        compile: Some(AndroidCompileSdk::Release {
            api: 36,
            minor: Some(1),
            extension: None,
        }),
        ..Default::default()
    };
    sdk.merge_from(&AndroidSdk {
        compile: Some(AndroidCompileSdk::Release {
            api: 36,
            minor: None,
            extension: Some(2),
        }),
        ..Default::default()
    })
    .unwrap();
    let before = sdk.clone();
    assert!(
        sdk.merge_from(&AndroidSdk {
            compile: Some(AndroidCompileSdk::Preview {
                codename: "FuturePreview".into()
            }),
            ..Default::default()
        })
        .is_err()
    );
    assert_eq!(sdk, before);
}

#[test]
fn named_variants_merge_values_but_not_conflicting_values_or_priorities() {
    let mut variants: AndroidVariants = serde_json::from_value(json!({
        "flavor_dimensions":["tier","store"],
        "product_flavors":{"free":{"dimension":"tier","values":{"manifest_placeholders":{"host":"\"one.example\""}}}}
    })).unwrap();
    let patch: AndroidVariants = serde_json::from_value(json!({
        "product_flavors":{"free":{"dimension":"tier","values":{"res_values":{"string":{"title":"\"Example\""}}}}}
    })).unwrap();
    variants.merge_from(&patch).unwrap();
    variants.merge_from(&patch).unwrap();
    assert_eq!(variants.product_flavors.len(), 1);
    assert_eq!(
        variants.product_flavors["free"]
            .values
            .manifest_placeholders
            .len(),
        1
    );
    assert_eq!(
        variants.product_flavors["free"].values.res_values["string"].len(),
        1
    );
    let before = variants.clone();
    let bad: AndroidVariants=serde_json::from_value(json!({"product_flavors":{"free":{"dimension":"tier","values":{"manifest_placeholders":{"host":"\"two.example\""}}}}})).unwrap();
    assert!(
        variants
            .merge_from(&bad)
            .unwrap_err()
            .to_string()
            .contains("manifest_placeholders[host]")
    );
    assert_eq!(variants, before);
    assert!(
        variants
            .merge_from(&AndroidVariants {
                flavor_dimensions: vec!["store".into(), "tier".into()],
                ..Default::default()
            })
            .is_err()
    );
    assert_eq!(variants, before);
}

#[test]
fn project_merge_rolls_back_prior_module_changes_on_a_late_conflict() {
    let mut original = project(expanded_fixture());
    let before = original.clone();
    let mut patch = original.clone();
    patch
        .modules
        .get_mut(":app")
        .unwrap()
        .build
        .imports
        .push("new.Import".into());
    patch.modules.get_mut(":tests").unwrap().directory = ProjectPath::new("different").unwrap();
    assert!(original.merge_from(&patch).is_err());
    assert_eq!(original, before);
    original.merge_from(&before).unwrap();
    assert_eq!(original, before);
}

fn selector(name: &str, identity: Option<&str>) -> AndroidManifestSelector {
    AndroidManifestSelector {
        name: name.into(),
        attributes: identity
            .map(|value| BTreeMap::from([("android:name".into(), value.into())]))
            .unwrap_or_default(),
    }
}

fn activity_path() -> Vec<AndroidManifestSelector> {
    vec![
        selector("application", None),
        selector("activity", Some(".MainActivity")),
    ]
}

fn edit(
    path: Vec<AndroidManifestSelector>,
    key: &str,
    value: AndroidManifestAttributeEdit,
) -> AndroidManifestEdit {
    AndroidManifestEdit::Upsert {
        path,
        attributes: BTreeMap::from([(key.into(), value)]),
        append: vec![],
    }
}

#[test]
fn manifest_selector_edits_are_idempotent_and_override_is_explicit() {
    let mut project = project(json_fixture());
    let source = app(&mut project)
        .android
        .source_sets
        .get_mut("main")
        .unwrap();
    let set = edit(
        activity_path(),
        "android:launchMode",
        AndroidManifestAttributeEdit::Set("singleTop".into()),
    );
    source.edit_manifest(std::slice::from_ref(&set)).unwrap();
    let before = source.clone();
    source.edit_manifest(&[set]).unwrap();
    assert_eq!(*source, before);
    let conflict = edit(
        activity_path(),
        "android:launchMode",
        AndroidManifestAttributeEdit::Set("singleTask".into()),
    );
    let add_permission = AndroidManifestEdit::Upsert {
        path: vec![selector(
            "uses-permission",
            Some("android.permission.CAMERA"),
        )],
        attributes: BTreeMap::new(),
        append: vec![],
    };
    assert!(source.edit_manifest(&[add_permission, conflict]).is_err());
    assert_eq!(*source, before);
    source
        .edit_manifest(&[edit(
            activity_path(),
            "android:launchMode",
            AndroidManifestAttributeEdit::Override("singleTask".into()),
        )])
        .unwrap();
    assert_ne!(*source, before);
    source
        .edit_manifest(&[edit(
            activity_path(),
            "android:launchMode",
            AndroidManifestAttributeEdit::Remove,
        )])
        .unwrap();
    source
        .edit_manifest(&[AndroidManifestEdit::Remove {
            path: activity_path(),
        }])
        .unwrap();
    let before = source.clone();
    source
        .edit_manifest(&[AndroidManifestEdit::Remove {
            path: activity_path(),
        }])
        .unwrap();
    assert_eq!(*source, before);
}

#[test]
fn manifest_edits_reject_ambiguity_identity_changes_and_root_removal() {
    let mut source = AndroidSourceSet::default();
    assert!(
        source
            .edit_manifest(&[AndroidManifestEdit::Remove { path: vec![] }])
            .is_err()
    );
    source
        .edit_manifest(&[AndroidManifestEdit::Remove {
            path: activity_path(),
        }])
        .unwrap();
    assert_eq!(source, AndroidSourceSet::default());
    let activity = XmlNode::Element(XmlElement {
        name: "activity".into(),
        attributes: BTreeMap::from([("android:name".into(), ".MainActivity".into())]),
        children: vec![],
    });
    source.manifest = Some(XmlElement {
        name: "manifest".into(),
        attributes: BTreeMap::new(),
        children: vec![XmlNode::Element(XmlElement {
            name: "application".into(),
            attributes: BTreeMap::new(),
            children: vec![activity.clone(), activity],
        })],
    });
    let before = source.clone();
    let error = source
        .edit_manifest(&[edit(
            activity_path(),
            "android:label",
            AndroidManifestAttributeEdit::Set("Example".into()),
        )])
        .unwrap_err();
    assert!(format!("{error:#}").contains("ambiguous"));
    assert!(
        source
            .edit_manifest(&[AndroidManifestEdit::Remove { path: vec![] }])
            .is_err()
    );
    assert_eq!(source, before);
    assert!(
        source
            .edit_manifest(&[edit(
                activity_path(),
                "android:name",
                AndroidManifestAttributeEdit::Override(".Other".into())
            )])
            .is_err()
    );
    assert_eq!(source, before);
}

#[test]
fn manifest_root_order_and_distinct_intent_filters_are_preserved() {
    let mut source = AndroidSourceSet::default();
    for permission in ["android.permission.CAMERA", "android.permission.INTERNET"] {
        source
            .edit_manifest(&[
                AndroidManifestEdit::Upsert {
                    path: vec![selector("application", None)],
                    attributes: BTreeMap::new(),
                    append: vec![],
                },
                AndroidManifestEdit::Upsert {
                    path: vec![selector("uses-permission", Some(permission))],
                    attributes: BTreeMap::new(),
                    append: vec![],
                },
            ])
            .unwrap();
    }
    let root = source.manifest.as_ref().unwrap();
    assert!(matches!(root.children.last().unwrap(),XmlNode::Element(e) if e.name=="application"));
    let filter = |action: &str| {
        XmlNode::Element(XmlElement {
            name: "intent-filter".into(),
            attributes: BTreeMap::new(),
            children: vec![XmlNode::Element(XmlElement {
                name: "action".into(),
                attributes: BTreeMap::from([("android:name".into(), action.into())]),
                children: vec![],
            })],
        })
    };
    let patch = AndroidManifestEdit::Upsert {
        path: activity_path(),
        attributes: BTreeMap::new(),
        append: vec![
            filter("android.intent.action.SEND"),
            filter("android.intent.action.VIEW"),
        ],
    };
    source.edit_manifest(std::slice::from_ref(&patch)).unwrap();
    let before = source.clone();
    source.edit_manifest(&[patch]).unwrap();
    assert_eq!(source, before);
    let root = source.manifest.as_ref().unwrap();
    let XmlNode::Element(application) = root.children.last().unwrap() else {
        unreachable!()
    };
    let XmlNode::Element(activity) = &application.children[0] else {
        unreachable!()
    };
    assert_eq!(activity.children.len(), 2);
}

#[test]
fn empty_composition_input_accepts_application_and_partial_contributions() {
    let complete = project(json_fixture());
    let mut empty = AndroidProjectIr::default();
    assert!(check(empty.clone()).is_err());
    let roundtrip: AndroidProjectIr =
        serde_json::from_slice(&serde_json::to_vec(&empty).unwrap()).unwrap();
    assert_eq!(roundtrip, empty);
    empty.merge_from(&complete).unwrap();
    assert_eq!(empty, complete);
    empty
        .merge_from(&AndroidProjectIr {
            properties: [("plugin.flag".into(), "true".into())].into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(empty.application, complete.application);
    check(empty.clone()).unwrap();
    let before = empty.clone();
    assert!(
        empty
            .merge_from(&AndroidProjectIr {
                application: ":other".into(),
                ..Default::default()
            })
            .is_err()
    );
    assert_eq!(empty, before);

    let mut partial = AndroidProjectIr {
        properties: [("shared".into(), "one".into())].into(),
        ..Default::default()
    };
    let before = partial.clone();
    let mut conflict = complete;
    conflict.properties.insert("shared".into(), "two".into());
    assert!(partial.merge_from(&conflict).is_err());
    assert_eq!(
        partial, before,
        "failed merge must not retain the application ID"
    );
}
