use super::*;
use tests::{fixture, write};

fn workspace() -> tempfile::TempDir {
    let directory = fixture();
    let root = directory.path();
    let mut manifest = read_toml(&root.join("Cargo.toml")).unwrap();
    for member in [
        "crates/whisker-runtime",
        "crates/whisker-cli",
        "crates/whisker-cng",
        "packages/router",
        "packages/router/web",
        "packages/untouched",
    ] {
        manifest["workspace"]["members"]
            .as_array_mut()
            .unwrap()
            .push(member);
    }
    write(root, "Cargo.toml", &manifest.to_string());
    for (path, name, dependency) in [
        ("crates/whisker-runtime", "whisker-runtime", ""),
        ("crates/whisker-cng", "whisker-cng", ""),
        (
            "crates/whisker-cli",
            "whisker-cli",
            "[dependencies]\nwhisker.workspace = true\n",
        ),
        (
            "packages/router",
            "router",
            "[dependencies]\nwhisker.workspace = true\n",
        ),
        (
            "packages/router/web",
            "router-web",
            "[dependencies]\nwhisker.workspace = true\n",
        ),
        (
            "packages/untouched",
            "untouched",
            "[dependencies]\nwhisker.workspace = true\n",
        ),
    ] {
        let include = if name == "router" {
            "include = ['Cargo.toml', 'src/**', 'web/Cargo.toml', 'web/src/**', 'Package.swift']\n"
        } else {
            ""
        };
        write(
            root,
            &format!("{path}/Cargo.toml"),
            &format!(
                "[package]\nname = '{name}'\nversion.workspace = true\nedition.workspace = true\n{include}{dependency}"
            ),
        );
        write(root, &format!("{path}/src/lib.rs"), "");
    }
    write(
        root,
        "packages/router/Package.swift",
        ".package(url: \"https://github.com/whiskerrs/whisker.git\", exact: \"0.1.13\")\n",
    );
    crate::run(
        Command::new(crate::cargo())
            .current_dir(root)
            .args(["generate-lockfile", "--offline"]),
    )
    .unwrap();
    let mut previous = plan(root);
    previous.selective = None;
    previous.crates = published_packages(root).unwrap();
    write(
        root,
        PLAN_PATH,
        &serde_json::to_string_pretty(&previous).unwrap(),
    );
    commit(root);
    directory
}

fn commit(root: &Path) {
    git(root, &["add", "."]).unwrap();
    git(root, &["commit", "-m", "fixture changes"]).unwrap();
}

fn plan(root: &Path) -> ReleasePlan {
    let source = git(root, &["rev-parse", "HEAD"]).unwrap().trim().to_owned();
    ReleasePlan {
        version: "0.13.3".into(),
        source: source.clone(),
        sdk: None,
        gradle: None,
        ios: None,
        subsecond: None,
        crates: BTreeMap::new(),
        selective: Some(selection::SelectivePlan {
            id: "20260908.1".into(),
            baseline: source,
            packages: BTreeMap::new(),
            publish: Default::default(),
            reasons: BTreeMap::new(),
            contents: BTreeMap::new(),
            lockfile: String::new(),
        }),
    }
}

#[test]
fn packages_only_release_preserves_core_and_unrelated_package_requirements() {
    let directory = workspace();
    let root = directory.path();
    let mut plan = plan(root);
    write(
        root,
        "packages/router/web/src/lib.rs",
        "pub fn changed() {}\n",
    );
    commit(root);
    prepare::prepare_selection(root, &mut plan, None).unwrap();
    assert_eq!(
        plan.publishing(),
        BTreeMap::from([
            ("router".into(), "0.13.4".into()),
            ("router-web".into(), "0.13.4".into())
        ])
    );
    assert_eq!(plan.crates["whisker"], "0.13.3");
    assert_eq!(plan.crates["untouched"], "0.13.3");
    let manifest = read_toml(&root.join("packages/untouched/Cargo.toml")).unwrap();
    assert_eq!(
        manifest["dependencies"]["whisker"]["version"].as_str(),
        Some("0.13.3")
    );
    assert_eq!(manifest["package"]["version"].as_str(), Some("0.13.3"));
    write(root, &plan.notes_path(), "notes");
    plan.validate_checkout(root).unwrap();
}

#[test]
fn core_patch_pins_core_without_republishing_packages() {
    let directory = workspace();
    let root = directory.path();
    let mut plan = plan(root);
    plan.version = "0.13.4".into();
    write(
        root,
        "crates/whisker-runtime/src/lib.rs",
        "pub fn changed() {}\n",
    );
    commit(root);
    prepare::prepare_selection(root, &mut plan, Some("0.13.4")).unwrap();
    assert_eq!(
        plan.publishing().keys().cloned().collect::<Vec<_>>(),
        ["whisker", "whisker-cli", "whisker-cng", "whisker-runtime"]
    );
    let workspace = read_toml(&root.join("Cargo.toml")).unwrap();
    assert_eq!(
        workspace["workspace"]["dependencies"]["whisker"]["version"].as_str(),
        Some("=0.13.4")
    );
    let package = read_toml(&root.join("packages/router/Cargo.toml")).unwrap();
    assert_eq!(
        package["dependencies"]["whisker"]["version"].as_str(),
        Some("0.13.3")
    );
}

#[test]
fn packages_only_release_after_a_core_release_keeps_core_exact_pins() {
    let directory = workspace();
    let root = directory.path();
    let mut first = plan(root);
    first.version = "0.13.4".into();
    prepare::prepare_selection(root, &mut first, Some("0.13.4")).unwrap();
    write(
        root,
        PLAN_PATH,
        &serde_json::to_string_pretty(&first).unwrap(),
    );
    write(root, &first.notes_path(), "notes");
    commit(root);
    let mut second = plan(root);
    second.version = first.version;
    second.selective.as_mut().unwrap().id = "20260908.2".into();
    write(root, "packages/router/src/lib.rs", "pub fn changed() {}\n");
    commit(root);
    prepare::prepare_selection(root, &mut second, None).unwrap();
    assert_eq!(
        second.publishing().keys().cloned().collect::<Vec<_>>(),
        ["router", "router-web"]
    );
    let workspace = read_toml(&root.join("Cargo.toml")).unwrap();
    assert_eq!(
        workspace["workspace"]["dependencies"]["whisker"]["version"].as_str(),
        Some("=0.13.4")
    );
    assert_eq!(second.crates["whisker"], "0.13.4");
    write(root, &second.notes_path(), "notes");
    second.validate_checkout(root).unwrap();
}

#[test]
fn detaching_preserves_commented_renamed_target_dependencies_and_additive_features() {
    let directory = workspace();
    let root = directory.path();
    let mut workspace = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    workspace.push_str("\nalias = { package = 'whisker', path = 'crates/whisker', version = '0.13.3', default-features = false, features = ['a'] }\n");
    write(root, "Cargo.toml", &workspace);
    let core = fs::read_to_string(root.join("crates/whisker/Cargo.toml")).unwrap();
    write(
        root,
        "crates/whisker/Cargo.toml",
        &format!("{core}\n[features]\na = []\nb = []\n"),
    );
    write(
        root,
        "packages/untouched/Cargo.toml",
        "[package]\nname = 'untouched'\nversion.workspace = true\nedition.workspace = true\n[target.'cfg(unix)'.dependencies]\n# A dependency with a retained comment.\nalias = { workspace = true, features = ['b'], optional = true }\n",
    );
    commit(root);
    let before = selection::Inventory::read(root)
        .unwrap()
        .contents(root)
        .unwrap();
    selection::detach_packages(root).unwrap();
    assert_eq!(
        before,
        selection::Inventory::read(root)
            .unwrap()
            .contents(root)
            .unwrap()
    );
    let manifest = read_toml(&root.join("packages/untouched/Cargo.toml")).unwrap();
    let dependency = &manifest["target"]["cfg(unix)"]["dependencies"]["alias"];
    assert_eq!(dependency["path"].as_str(), Some("../../crates/whisker"));
    assert_eq!(dependency["package"].as_str(), Some("whisker"));
    assert_eq!(dependency["default-features"].as_bool(), Some(false));
    assert_eq!(dependency["optional"].as_bool(), Some(true));
    assert_eq!(
        dependency["features"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
}

#[test]
fn native_pin_changes_require_core_and_select_packages_whose_swift_manifests_change() {
    let directory = workspace();
    let root = directory.path();
    let mut selected = plan(root);
    selected.sdk = Some("0.1.22".into());
    assert!(
        prepare::prepare_selection(root, &mut selected, None)
            .unwrap_err()
            .to_string()
            .contains("core has changes")
    );
    selected.version = "0.13.4".into();
    selected.ios = Some("0.1.14".into());
    prepare::prepare_selection(root, &mut selected, Some("0.13.4")).unwrap();
    assert!(selected.publishing().contains_key("whisker-cli"));
    assert!(selected.publishing().contains_key("whisker-cng"));
    assert!(selected.publishing().contains_key("router"));
    assert!(!selected.publishing().contains_key("untouched"));
    write(root, &selected.notes_path(), "notes");
    selected.validate_checkout(root).unwrap();
}

#[test]
fn publication_selection_cannot_omit_updated_crates_or_include_unchanged_crates() {
    let directory = workspace();
    let root = directory.path();
    let mut selected = plan(root);
    selected
        .selective
        .as_mut()
        .unwrap()
        .packages
        .insert("router".into(), "patch".into());
    prepare::prepare_selection(root, &mut selected, None).unwrap();
    write(root, &selected.notes_path(), "notes");
    let mut missing = selected.clone();
    missing
        .selective
        .as_mut()
        .unwrap()
        .publish
        .remove("router-web");
    assert!(
        missing
            .validate_checkout(root)
            .unwrap_err()
            .to_string()
            .contains("publication selection")
    );
    selected
        .selective
        .as_mut()
        .unwrap()
        .publish
        .insert("untouched".into());
    assert!(
        selected
            .validate_checkout(root)
            .unwrap_err()
            .to_string()
            .contains("publication selection")
    );
}

#[test]
fn overrides_reject_unknown_groups_version_reuse_and_unaligned_group_members() {
    for (group, request) in [
        ("missing", "patch"),
        ("core", "patch"),
        ("router", "0.13.3"),
        ("router", "0.12.0"),
    ] {
        let directory = workspace();
        let root = directory.path();
        let current = selection::Inventory::read(root).unwrap();
        assert!(
            current
                .select(
                    &current,
                    &Default::default(),
                    None,
                    None,
                    &BTreeMap::from([(group.into(), request.into())])
                )
                .is_err()
        );
    }
    let directory = workspace();
    let root = directory.path();
    let mut current = selection::Inventory::read(root).unwrap();
    let baseline = selection::Inventory::read(root).unwrap();
    current.packages.get_mut("router-web").unwrap().version = "0.1.0".into();
    assert!(
        current
            .select(&baseline, &Default::default(), None, None, &BTreeMap::new())
            .unwrap_err()
            .to_string()
            .contains("must share a version")
    );
}

#[test]
fn incompatible_core_release_expands_to_dependents_and_keeps_one_package_group_version() {
    let directory = workspace();
    let root = directory.path();
    let mut plan = plan(root);
    plan.version = "0.14.0".into();
    plan.selective
        .as_mut()
        .unwrap()
        .packages
        .insert("router".into(), "minor".into());
    prepare::prepare_selection(root, &mut plan, Some("0.14.0")).unwrap();
    assert_eq!(plan.crates["router"], "0.14.0");
    assert_eq!(plan.crates["router-web"], "0.14.0");
    assert_eq!(plan.crates["untouched"], "0.13.4");
    let package = read_toml(&root.join("packages/untouched/Cargo.toml")).unwrap();
    assert_eq!(
        package["dependencies"]["whisker"]["version"].as_str(),
        Some("0.14.0")
    );
    assert!(!plan.publishing().contains_key("whisker-subsecond"));
}

#[test]
fn core_and_fork_changes_require_explicit_versions() {
    for path in [
        "crates/whisker-runtime/src/lib.rs",
        "crates/whisker-subsecond/src/lib.rs",
    ] {
        let directory = workspace();
        let root = directory.path();
        let mut plan = plan(root);
        write(root, path, "pub fn changed() {}\n");
        commit(root);
        let error = prepare::prepare_selection(root, &mut plan, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("specify"), "{error}");
    }
}

#[test]
fn excluded_tests_do_not_trigger_publication_but_packaged_native_files_do() {
    let directory = workspace();
    let root = directory.path();
    let mut plan = plan(root);
    write(
        root,
        "packages/router/tests/test.rs",
        "#[test] fn test() {}\n",
    );
    commit(root);
    let inventory = selection::Inventory::read(root).unwrap();
    let baseline =
        selection::Baseline::new(root, &plan.selective.as_ref().unwrap().baseline).unwrap();
    assert_eq!(
        inventory.contents(root).unwrap(),
        selection::Inventory::read(baseline.path())
            .unwrap()
            .contents(baseline.path())
            .unwrap()
    );
    write(
        root,
        "packages/router/Package.swift",
        "// changed native manifest\n",
    );
    commit(root);
    prepare::prepare_selection(root, &mut plan, None).unwrap();
    assert!(plan.publishing().contains_key("router"));
}

#[test]
fn preparation_fingerprints_reject_late_selected_and_unselected_changes_and_lockfile_edits() {
    let directory = workspace();
    let root = directory.path();
    let mut plan = plan(root);
    plan.selective
        .as_mut()
        .unwrap()
        .packages
        .insert("router".into(), "0.15.0".into());
    prepare::prepare_selection(root, &mut plan, None).unwrap();
    write(root, &plan.notes_path(), "notes");
    plan.validate_checkout(root).unwrap();
    for path in [
        "packages/router/src/lib.rs",
        "packages/untouched/src/lib.rs",
        "Cargo.lock",
    ] {
        let previous = fs::read_to_string(root.join(path)).unwrap();
        write(root, path, &format!("{previous}\n// unexpected change\n"));
        assert!(plan.validate_checkout(root).is_err(), "{path}");
        write(root, path, &previous);
    }
}

#[test]
fn selective_identifiers_are_safe_and_legacy_plans_keep_their_publication_set_and_tag() {
    for id in [
        "../../main",
        "20260908.0",
        "20260908.1\nrelease=true",
        "20260908.$(id)",
        "0.13.9",
    ] {
        assert!(selection::validate_id(id).is_err());
    }
    selection::validate_id("20260908.12").unwrap();
    let mut legacy: ReleasePlan = serde_json::from_str(r#"{"version":"0.13.3","source":"abc","sdk":null,"gradle":null,"ios":null,"subsecond":null,"crates":{"whisker":"0.13.3","router":"0.13.3"}}"#).unwrap();
    assert_eq!(legacy.publishing(), legacy.crates);
    assert_eq!(legacy.tag(), "whisker-v0.13.3");
    assert_eq!(legacy.notes_path(), "releases/0.13.3.md");
    let directory = workspace();
    legacy.selective = plan(directory.path()).selective;
    legacy
        .selective
        .as_mut()
        .unwrap()
        .publish
        .insert("router".into());
    assert_eq!(legacy.publishing().len(), 1);
    assert_eq!(legacy.tag(), "whisker-release-20260908.1");
}
