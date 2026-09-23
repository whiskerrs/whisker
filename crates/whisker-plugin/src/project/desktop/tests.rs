use super::*;
use serde_json::{Value, json};
fn windows() -> WindowsProjectIr {
    let v: Value = serde_json::from_str(include_str!("../fixtures/windows.json")).unwrap();
    serde_json::from_value(v["project"].clone()).unwrap()
}
fn linux() -> LinuxProjectIr {
    let v: Value = serde_json::from_str(include_str!("../fixtures/linux.json")).unwrap();
    serde_json::from_value(v["project"].clone()).unwrap()
}
fn resource(path: &str) -> Resource {
    Resource {
        source: ProjectPath::new("source").unwrap(),
        destination: ProjectPath::new(path).unwrap(),
        kind: crate::project::ResourceKind::File,
    }
}
#[test]
fn windows_names_and_case_collisions_are_platform_specific() {
    for path in [
        "example.EXE",
        "DATA.BIN",
        "Example.exe/child",
        "NUL.txt",
        "a/LPT¹.log",
        "folder./x",
        "file?.txt",
    ] {
        let mut p = windows();
        p.resources.push(resource(path));
        assert!(validate_windows(&p).is_err(), "accepted {path}");
    }
    let mut l = linux();
    l.resources
        .extend([resource("data/Name"), resource("data/name")]);
    validate_linux(&l).unwrap();
    let mut w = windows();
    w.resources.push(resource("日本語/名前.txt"));
    validate_windows(&w).unwrap();
}
#[test]
fn independent_msix_packages_and_fixed_version_fields_round_trip() {
    let mut w = windows();
    let mut auxiliary = w.packages["main"].clone();
    auxiliary.executables.clear();
    w.packages.insert("resources".into(), auxiliary);
    let v = w
        .executables
        .get_mut("app")
        .unwrap()
        .version_info
        .as_mut()
        .unwrap();
    v.flags_mask = Some(0x3f);
    v.flags = Some(1);
    v.file_os = Some(0x40004);
    v.file_type = Some(1);
    validate_windows(&w).unwrap();
    assert_eq!(
        w,
        serde_json::from_str(&serde_json::to_string(&w).unwrap()).unwrap()
    );
    let mut bad = w.clone();
    bad.packages
        .get_mut("main")
        .unwrap()
        .resources
        .push(resource("appxmanifest.XML"));
    assert!(validate_windows(&bad).is_err());
    let mut bad = w.clone();
    bad.packages.get_mut("main").unwrap().executables.clear();
    assert!(
        validate_windows(&bad)
            .unwrap_err()
            .to_string()
            .contains("application executable")
    );
    let mut bad = w;
    bad.executables
        .get_mut("app")
        .unwrap()
        .version_info
        .as_mut()
        .unwrap()
        .strings
        .insert("040904B0".into(), BTreeMap::new());
    assert!(
        validate_windows(&bad)
            .unwrap_err()
            .to_string()
            .contains("translation")
    );
}
#[test]
fn windows_localized_values_compose_and_late_conflict_rolls_back() {
    let mut a = windows();
    let mut b = a.clone();
    b.executables
        .get_mut("app")
        .unwrap()
        .version_info
        .as_mut()
        .unwrap()
        .strings
        .insert(
            "041104b0".into(),
            BTreeMap::from([("ProductName".into(), "サンプル".into())]),
        );
    a.merge_from(&b).unwrap();
    a.merge_from(&b).unwrap();
    assert_eq!(
        a.executables["app"]
            .version_info
            .as_ref()
            .unwrap()
            .strings
            .len(),
        2
    );
    let before = a.clone();
    let mut bad = a.clone();
    bad.resources.push(resource("extra"));
    bad.packages
        .get_mut("main")
        .unwrap()
        .manifest
        .attributes
        .insert("changed".into(), "true".into());
    assert!(a.merge_from(&bad).is_err());
    assert_eq!(a, before);
}
#[test]
fn flatpak_native_properties_commands_and_fragment_modules_round_trip() {
    let mut p = linux();
    let LinuxPackage::Flatpak { manifest } = p.packages.get_mut("flatpak").unwrap() else {
        panic!()
    };
    manifest.command = FlatpakCommand::Native {
        command: ProjectPath::new("wrapper").unwrap(),
    };
    manifest
        .properties
        .insert("sdk-extensions".into(), json!(["org.example.Extension"]));
    manifest.modules.push(FlatpakModule::File(
        ProjectPath::new("packaging/deps.yaml").unwrap(),
    ));
    validate_linux(&p).unwrap();
    assert_eq!(
        p,
        serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap()
    );
    let mut bad = p;
    let LinuxPackage::Flatpak { manifest } = bad.packages.get_mut("flatpak").unwrap() else {
        panic!()
    };
    manifest
        .properties
        .insert("command".into(), json!("hidden"));
    assert!(
        validate_linux(&bad)
            .unwrap_err()
            .to_string()
            .contains("reserved")
    );
}
#[test]
fn desktop_lists_actions_and_keyed_merges_are_explicit() {
    let mut p = linux();
    let entry = p.desktop_entries.values_mut().next().unwrap();
    entry
        .append_list("MimeType", &["application/x-a;b".into()])
        .unwrap();
    assert_eq!(
        entry.entries["MimeType"],
        DesktopEntryValue::List(vec![
            "x-scheme-handler/example".into(),
            "application/x-a;b".into()
        ])
    );
    assert!(entry.append_list("Name", &["bad".into()]).is_err());
    validate_linux(&p).unwrap();
    let mut bad = p.clone();
    bad.desktop_entries
        .values_mut()
        .next()
        .unwrap()
        .append_list("Actions", &["Missing".into()])
        .unwrap();
    assert!(
        validate_linux(&bad)
            .unwrap_err()
            .to_string()
            .contains("desktop action")
    );
    let mut b = p.clone();
    b.desktop_entries
        .values_mut()
        .next()
        .unwrap()
        .entries
        .insert(
            "Name[fr]".into(),
            DesktopEntryValue::String("Exemple".into()),
        );
    p.merge_from(&b).unwrap();
    p.merge_from(&b).unwrap();
    let before = p.clone();
    let mut bad = p.clone();
    bad.desktop_entries
        .values_mut()
        .next()
        .unwrap()
        .entries
        .insert("Name".into(), DesktopEntryValue::String("Conflict".into()));
    assert!(p.merge_from(&bad).is_err());
    assert_eq!(p, before);
}
#[test]
fn linux_xml_roots_and_flatpak_module_identity_are_checked() {
    let mut bad = linux();
    bad.mime_packages.values_mut().next().unwrap().name = "wrong".into();
    assert!(validate_linux(&bad).is_err());
    let mut p = linux();
    let mut b = p.clone();
    let LinuxPackage::Flatpak { manifest } = b.packages.get_mut("flatpak").unwrap() else {
        panic!()
    };
    let FlatpakModule::Inline(module) = &mut manifest.modules[0] else {
        panic!()
    };
    module.insert("buildsystem".into(), json!("meson"));
    let before = p.clone();
    assert!(p.merge_from(&b).is_err());
    assert_eq!(p, before);
}
