use super::*;
use serde_json::{Value, json};

const FIXTURES: [&str; 6] = [
    include_str!("fixtures/ios.json"),
    include_str!("fixtures/android.json"),
    include_str!("fixtures/macos.json"),
    include_str!("fixtures/windows.json"),
    include_str!("fixtures/linux.json"),
    include_str!("fixtures/web.json"),
];

fn fixture(index: usize) -> Value {
    serde_json::from_str(FIXTURES[index]).unwrap()
}

fn parse(value: Value) -> ProjectIr {
    serde_json::from_value(value).unwrap()
}

fn invalid(value: Value, message: &str) {
    let error = parse(value).validate_structure().unwrap_err().to_string();
    assert!(
        error.contains(message),
        "{error:?} did not contain {message:?}"
    );
}

#[test]
fn every_platform_preserves_its_declaration_through_json() {
    for fixture in FIXTURES {
        let project: ProjectIr = serde_json::from_str(fixture).unwrap();
        project.validate_structure().unwrap();
        let wire = serde_json::to_string(&project).unwrap();
        let decoded: ProjectIr = serde_json::from_str(&wire).unwrap();
        assert_eq!(project, decoded);
        assert_eq!(wire, serde_json::to_string(&decoded).unwrap());
    }
}

#[test]
fn plist_preserves_dictionary_arrays_data_and_dates() {
    let mut value = fixture(0);
    value["project"]["apple"]["targets"]["share"]["info_plist"]["ExampleValues"] = json!({
        "type": "array", "value": [
            {"type": "dict", "value": {"ratio": {"type": "real", "value": 0.25}}},
            {"type": "data", "value": [0, 1, 127, 255]},
            {"type": "date", "value": "2026-01-01T00:00:00Z"}
        ]
    });
    let project = parse(value.clone());
    assert_eq!(
        serde_json::to_value(project).unwrap()["project"]["apple"]["targets"]["share"]["info_plist"],
        value["project"]["apple"]["targets"]["share"]["info_plist"]
    );
}

#[test]
fn unknown_fields_fail_instead_of_disappearing() {
    for index in 0..FIXTURES.len() {
        let mut value = fixture(index);
        value["project"]["unknown_future_field"] = json!({"enabled": true});
        assert!(serde_json::from_value::<ProjectIr>(value).is_err());
    }
    let mut value = fixture(0);
    value["project"]["apple"]["targets"]["share"]["future_capability"] = json!(true);
    assert!(serde_json::from_value::<ProjectIr>(value).is_err());
}

#[test]
fn all_platforms_reject_missing_entry_products() {
    for index in 0..5 {
        let mut value = fixture(index);
        let project = &mut value["project"];
        if matches!(index, 0 | 2) {
            project["apple"]["application"] = json!("missing");
        } else {
            project["application"] = json!("missing");
        }
        invalid(value, "unknown ID");
    }
    let mut value = fixture(5);
    value["project"]["wasm"]["kind"] = json!("bin");
    invalid(value, "must be a cdylib");
}

#[test]
fn apple_checks_embeds_swift_packages_schemes_and_cycles() {
    let mut value = fixture(0);
    value["project"]["apple"]["targets"]["app"]["embeds"][0]["source"]["target"] = json!("missing");
    invalid(value, "embedded product references unknown ID");

    let mut value = fixture(0);
    value["project"]["apple"]["targets"]["app"]["dependencies"][0]["package"] = json!("missing");
    invalid(value, "Swift package references unknown ID");

    let mut value = fixture(0);
    value["project"]["apple"]["schemes"]["Example"]["run_target"] = json!("missing");
    invalid(value, "scheme Example references unknown ID");

    let mut value = fixture(0);
    value["project"]["apple"]["targets"]["share"]["dependencies"] = json!([
        {"kind": "target", "target": "app", "link": false}
    ]);
    invalid(value, "cyclic project dependencies");
}

#[test]
fn android_distinguishes_feature_packaging_from_compile_dependencies() {
    parse(fixture(1)).validate_structure().unwrap();
    let mut value = fixture(1);
    value["project"]["modules"][":shared"]["dependencies"] = json!([
        {"configuration": "implementation", "source": {"kind": "project", "value": ":app"}}
    ]);
    parse(value).validate_structure().unwrap();

    let mut value = fixture(1);
    value["project"]["modules"][":app"]["kind"]["config"]["dynamic_features"] = json!([":shared"]);
    invalid(value, "not a dynamic feature");

    let mut value = fixture(1);
    value["project"]["modules"][":app"]["dependencies"][0]["source"]["value"] = json!(":missing");
    invalid(value, "Android module references unknown ID");
}

#[test]
fn package_references_and_output_collisions_are_checked() {
    let mut value = fixture(3);
    value["project"]["packages"]["main"]["executables"] = json!(["app", "missing"]);
    invalid(value, "MSIX executable references unknown ID");

    let mut value = fixture(3);
    value["project"]["packages"]["main"]["resources"][0]["destination"] = json!("Example.exe");
    invalid(value, "duplicate destination example.exe");

    let mut value = fixture(4);
    value["project"]["packages"]["flatpak"]["manifest"]["command"]["id"] = json!("missing");
    invalid(value, "Flatpak command references unknown ID");

    let mut value = fixture(4);
    value["project"]["resources"][0]["destination"] = json!("bin");
    invalid(value, "overlapping destinations bin and bin/example");

    let mut value = fixture(5);
    value["project"]["manifest"]["path"] = json!("sw.js");
    invalid(value, "duplicate destination sw.js");
}

#[test]
fn project_paths_are_portable_and_validated_on_the_wire() {
    for path in [
        "",
        "/tmp/file",
        "a/../b",
        "./a",
        "a//b",
        "a/",
        "C:/file",
        "C:\\file",
        "a\\b",
        "a\nb",
    ] {
        assert!(ProjectPath::new(path).is_err(), "accepted {path:?}");
        assert!(serde_json::from_value::<ProjectPath>(json!(path)).is_err());
    }
    let mut value = fixture(0);
    value["project"]["apple"]["files"]["../escape"] =
        json!({"kind": "app_file", "source": "input"});
    assert!(serde_json::from_value::<ProjectIr>(value).is_err());

    let mut value = fixture(0);
    value["project"]["apple"]["files"]["Share/extra.swift"] =
        json!({"kind": "generated", "entry": {"contents": ""}});
    invalid(
        value,
        "overlapping destinations Share and Share/extra.swift",
    );
}

#[test]
fn binary_resources_survive_the_model() {
    let ProjectIr::Windows(project) = parse(fixture(3)) else {
        unreachable!()
    };
    let file = &project.files[&ProjectPath::new("assets/data.bin").unwrap()];
    let ProjectFile::Generated { entry } = file else {
        unreachable!()
    };
    assert_eq!(entry.to_bytes().unwrap(), [0, 1, 2, 255]);
}
