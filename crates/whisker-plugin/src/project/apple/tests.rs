use super::*;
use serde_json::{Value, json};
fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/ios.json")).unwrap()
}
fn project(v: Value) -> AppleProjectIr {
    serde_json::from_value(v["project"]["apple"].clone()).unwrap()
}

#[test]
fn apple_layers_generated_paths_embeds_schemes_and_aggregate_round_trip() {
    let mut v = fixture();
    let a = &mut v["project"]["apple"];
    a["configurations"] = json!({"Debug":{"xcconfig":"Config/Debug.xcconfig"},"Release":{}});
    a["default_configuration"] = json!("Debug");
    a["build_settings"] = json!({"OTHER_LDFLAGS":["$(inherited)","-framework","UIKit"]});
    a["targets"]["generate"] = json!({"product_name":"Generate","kind":"aggregate","scripts":[{"name":"generate","position":"before_sources","shell":"/bin/sh","script":"generate","outputs":[{"expression":"$(DERIVED_FILE_DIR)/Generated.swift"}],"input_file_lists":["Config/Inputs.xcfilelist"],"based_on_dependency_analysis":true}]});
    let app = &mut a["targets"]["app"];
    app["dependencies"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"target","target":"generate","link":false}));
    app["sources"].as_array_mut().unwrap().push(json!({"path":{"expression":"$(DERIVED_FILE_DIR)/Generated.swift"},"platform_filters":["ios"]}));
    app["headers"] = json!([{"path":"Headers/API.h","visibility":"public"}]);
    app["embeds"].as_array_mut().unwrap().push(json!({"source":{"kind":"file","path":"Vendor/SDK.xcframework"},"destination":"Frameworks","code_sign_on_copy":true,"remove_headers_on_copy":true}));
    a["schemes"]["Example"]["test_plans"] = json!(["Tests/Smoke.xctestplan"]);
    a["schemes"]["Example"]["default_test_plan"] = json!("Tests/Smoke.xctestplan");
    let p = project(v);
    validate_apple(&p).unwrap();
    assert_eq!(
        p,
        serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap()
    );
    let mut invalid = p.clone();
    invalid.targets.get_mut("generate").unwrap().product_type = Some("anything".into());
    assert!(
        validate_apple(&invalid)
            .unwrap_err()
            .to_string()
            .contains("aggregate")
    );
    let mut invalid = p.clone();
    invalid
        .schemes
        .get_mut("Example")
        .unwrap()
        .test_configuration = Some("Typo".into());
    assert!(
        validate_apple(&invalid)
            .unwrap_err()
            .to_string()
            .contains("configuration")
    );
}
#[test]
fn apple_filtered_edges_do_not_create_false_union_cycles() {
    let mut v = fixture();
    let t = &mut v["project"]["apple"]["targets"];
    t["app"]["embeds"] = json!([]);
    t["app"]["dependencies"] =
        json!([{"kind":"target","target":"share","link":false,"platform_filters":["ios"]}]);
    t["share"]["dependencies"] =
        json!([{"kind":"target","target":"app","link":false,"platform_filters":["macos"]}]);
    validate_apple(&project(v.clone())).unwrap();
    v["project"]["apple"]["targets"]["share"]["dependencies"][0]["platform_filters"] =
        json!(["ios"]);
    assert!(
        validate_apple(&project(v))
            .unwrap_err()
            .to_string()
            .contains("cyclic")
    );
}
#[test]
fn apple_merge_nested_plists_is_atomic_and_preserves_ordered_flags() {
    let mut a = project(fixture());
    let mut b = a.clone();
    b.targets.get_mut("app").unwrap().info_plist.insert(
        "Nested".into(),
        PropertyListValue::Dict(BTreeMap::from([(
            "a".into(),
            PropertyListValue::Boolean(true),
        )])),
    );
    a.merge_from(&b).unwrap();
    a.merge_from(&b).unwrap();
    let mut c = b.clone();
    c.targets.get_mut("app").unwrap().info_plist.insert(
        "Nested".into(),
        PropertyListValue::Dict(BTreeMap::from([(
            "b".into(),
            PropertyListValue::Boolean(false),
        )])),
    );
    a.merge_from(&c).unwrap();
    let PropertyListValue::Dict(dict) = &a.targets["app"].info_plist["Nested"] else {
        panic!()
    };
    assert_eq!(dict.len(), 2);
    a.build_settings.insert(
        "FLAGS".into(),
        AppleBuildSetting::List(vec!["-x".into(), "a".into()]),
    );
    let before = a.clone();
    let mut bad = a.clone();
    bad.targets
        .get_mut("app")
        .unwrap()
        .info_plist
        .insert("Added".into(), PropertyListValue::Boolean(true));
    bad.build_settings.insert(
        "FLAGS".into(),
        AppleBuildSetting::List(vec!["a".into(), "-x".into()]),
    );
    assert!(
        a.merge_from(&bad)
            .unwrap_err()
            .to_string()
            .contains("FLAGS")
    );
    assert_eq!(a, before);
    let mut plist = PropertyListValue::Array(vec![PropertyListValue::String("one".into())]);
    assert!(
        plist
            .merge_from(&PropertyListValue::Array(vec![PropertyListValue::String(
                "two".into()
            )]))
            .is_err()
    );
}
#[test]
fn macos_bundle_root_files_remain_separate_from_resources() {
    let v: Value = serde_json::from_str(include_str!("../fixtures/macos.json")).unwrap();
    let mut p = project(v);
    p.targets.get_mut("app").unwrap().bundle_files.push(serde_json::from_value(json!({"source":"LaunchAgent.plist","destination":"Contents/Library/LaunchAgents/com.example.agent.plist","kind":"file"})).unwrap());
    validate_apple(&p).unwrap();
    let mut b = p.clone();
    b.targets.get_mut("app").unwrap().bundle_files[0].source =
        ProjectPath::new("Different.plist").unwrap();
    assert!(p.merge_from(&b).is_err());
}

#[test]
fn scheme_action_scopes_and_privacy_plists_compose_without_flattening() {
    let mut value = fixture();
    value["project"]["apple"]["schemes"]["Example"]["action_options"] = json!({
        "run":{"arguments":["--tag","one","--tag","two"],"environment":{"MODE":"local"}},
        "test":{"environment":{"MODE":"test"},"pre_actions":[{"name":"prepare","script":"echo prepare","environment_target":"app"}]}
    });
    value["project"]["apple"]["schemes"]["Example"]["build_for"] =
        json!({"app":{"archiving":false,"testing":true}});
    value["project"]["apple"]["targets"]["app"]["resource_plists"] = json!({"PrivacyInfo.xcprivacy":{"type":"dict","value":{"NSPrivacyTracking":{"type":"boolean","value":false}}}});
    let mut p = project(value);
    validate_apple(&p).unwrap();
    let mut extra = p.clone();
    extra
        .schemes
        .get_mut("Example")
        .unwrap()
        .action_options
        .get_mut(&AppleSchemeAction::Run)
        .unwrap()
        .environment
        .insert("EXTRA".into(), "1".into());
    p.merge_from(&extra).unwrap();
    p.merge_from(&extra).unwrap();
    assert_eq!(
        p.schemes["Example"].action_options[&AppleSchemeAction::Run]
            .arguments
            .len(),
        4
    );
    assert_eq!(
        p.schemes["Example"].action_options[&AppleSchemeAction::Test].environment["MODE"],
        "test"
    );
    assert_eq!(
        p,
        serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap()
    );
    let before = p.clone();
    let mut bad = p.clone();
    bad.schemes
        .get_mut("Example")
        .unwrap()
        .action_options
        .get_mut(&AppleSchemeAction::Run)
        .unwrap()
        .arguments
        .reverse();
    assert!(p.merge_from(&bad).is_err());
    assert_eq!(p, before);
    let mut bad = p;
    bad.schemes
        .get_mut("Example")
        .unwrap()
        .action_options
        .get_mut(&AppleSchemeAction::Test)
        .unwrap()
        .pre_actions[0]
        .environment_target = Some("missing".into());
    assert!(
        validate_apple(&bad)
            .unwrap_err()
            .to_string()
            .contains("environment")
    );
}

#[test]
fn empty_apple_project_merges_application_and_build_outputs() {
    let complete: crate::project::ProjectIr =
        serde_json::from_str(include_str!("../fixtures/ios.json")).unwrap();
    let crate::project::ProjectIr::Ios(mut complete) = complete else {
        unreachable!()
    };
    let mut empty = IosProjectIr::default();
    assert!(
        crate::project::ProjectIr::Ios(empty.clone())
            .validate_structure()
            .is_err()
    );
    let path = AppleBuildPath::Expression {
        expression: "$(BUILT_PRODUCTS_DIR)/Driver.framework".into(),
    };
    let app = complete
        .apple
        .targets
        .get_mut(&complete.apple.application)
        .unwrap();
    app.dependencies.push(AppleDependency::BuildOutput {
        path: path.clone(),
        weak: false,
        platform_filters: vec![],
    });
    app.embeds.push(AppleEmbed {
        source: AppleEmbedSource::BuildOutput { path },
        destination: crate::project::ProjectPath::new("Frameworks").unwrap(),
        code_sign_on_copy: true,
        remove_headers_on_copy: true,
        platform_filters: vec![],
    });
    empty.merge_from(&complete).unwrap();
    empty.merge_from(&IosProjectIr::default()).unwrap();
    assert_eq!(empty, complete);
    let encoded = serde_json::to_vec(&empty).unwrap();
    assert_eq!(
        serde_json::from_slice::<IosProjectIr>(&encoded).unwrap(),
        complete
    );
    crate::project::ProjectIr::Ios(empty.clone())
        .validate_structure()
        .unwrap();
    let mut conflict = IosProjectIr::default();
    conflict.apple.application = "another".into();
    assert!(empty.merge_from(&conflict).is_err());
    assert_eq!(empty, complete);
}
