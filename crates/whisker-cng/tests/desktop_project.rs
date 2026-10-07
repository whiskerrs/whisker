#![cfg(feature = "generate")]
use std::{collections::BTreeMap, path::PathBuf};
use whisker_cng::{Config, GenerationTarget as Target, ProjectEngine, desktop::*};
use whisker_plugin::{FileEntry, project::*};
struct App(PathBuf);
impl App {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "whisker-desktop-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root.canonicalize().unwrap())
    }
    fn inputs(&self, target: Target) -> DesktopInputs {
        let mut config = Config::default();
        config
            .name("Example 日本語")
            .bundle_id("org.example.App")
            .version("1.2.3")
            .build_number(7)
            .background("#102030");
        inputs_from(
            &config,
            target,
            "demo".into(),
            self.0.clone(),
            "\"0.14\"".into(),
        )
        .unwrap()
    }
    fn project(&self, target: Target) -> DesktopProjectInputs {
        self.compose(self.inputs(target))
    }
    fn compose(&self, inputs: DesktopInputs) -> DesktopProjectInputs {
        let initial = inputs.empty_project().unwrap();
        let selection = inputs.cargo_selection.clone();
        let engine = if inputs.platform == Target::Windows {
            ProjectEngine::with_windows_application(inputs)
        } else {
            ProjectEngine::with_linux_application(inputs)
        };
        let result = engine.compose(&Config::default(), &initial).unwrap();
        DesktopProjectInputs {
            project: result.project,
            app_crate_dir: Some(self.0.clone()),
            cargo_selection: selection,
        }
    }
}
impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn path(s: &str) -> ProjectPath {
    ProjectPath::new(s).unwrap()
}
fn text(files: &BTreeMap<ProjectPath, FileEntry>, key: &str) -> String {
    String::from_utf8(files[&path(key)].to_bytes().unwrap()).unwrap()
}
#[test]
fn application_features_and_metadata_are_lowered_for_both_platforms() {
    let app = App::new();
    for target in [Target::Windows, Target::Linux] {
        let mut inputs = app.inputs(target);
        inputs.cargo_selection.features = vec!["auth".into()];
        inputs.cargo_selection.no_default_features = true;
        let project = app.compose(inputs);
        let files = render_project(&project).unwrap();
        let manifest: toml::Value = text(&files, "Cargo.toml").parse().unwrap();
        assert_eq!(
            manifest["dependencies"]["whisker-app"]["default-features"].as_bool(),
            Some(false)
        );
        assert_eq!(
            manifest["dependencies"]["whisker-app"]["features"][0].as_str(),
            Some("auth")
        );
        let main = text(&files, "src/main.rs");
        assert!(main.contains(".with_background_rgb(16, 32, 48)"));
        assert!(main.contains("run_with_application_hash"));
        let out = app.0.join(target.as_str());
        assert!(sync_project(&out, &project).unwrap());
        assert!(!sync_project(&out, &project).unwrap());
        let plan = load_build_plan(&out).unwrap();
        assert_eq!(plan.platform, target);
        if target == Target::Windows {
            assert!(text(&files, "build.rs").contains("compile_for"));
            assert!(text(&files, ".whisker-windows-0.rc").contains("FILEVERSION 1,2,3,7"));
            assert!(text(&files, ".whisker/windows/0.manifest").contains("1.2.3.7"));
            assert_eq!(
                manifest["build-dependencies"]["embed-resource"].as_str(),
                Some("3.0")
            );
        } else {
            let metadata = distribution_files(&out, &plan).unwrap();
            let desktop = text(&metadata, "share/applications/org.example.App.desktop");
            assert!(desktop.contains("Name=Example\\s日本語"));
            assert!(desktop.contains("Exec=demo-whisker-linux\n"));
            assert!(!files.contains_key(&path("build.rs")));
        }
    }
}
#[test]
fn linux_serializes_actions_lists_xml_and_dbus_without_shell_rewriting() {
    let app = App::new();
    let mut inputs = app.project(Target::Linux);
    let ProjectIr::Linux(l) = &mut inputs.project else {
        panic!()
    };
    let entry = l.desktop_entries.values_mut().next().unwrap();
    entry.entries.insert(
        "Actions".into(),
        DesktopEntryValue::List(vec!["compose".into()]),
    );
    entry.entries.insert(
        "MimeType".into(),
        DesktopEntryValue::List(vec!["text/plain".into(), "x-test/a;b".into()]),
    );
    entry.actions.insert(
        "compose".into(),
        [
            (
                "Name".into(),
                DesktopEntryValue::String("New\nmessage".into()),
            ),
            (
                "Exec".into(),
                DesktopEntryValue::String("demo --title \"hello world\" %U".into()),
            ),
        ]
        .into(),
    );
    let mut xml = XmlElement::new("mime-info");
    xml.attributes.insert(
        "xmlns".into(),
        "http://www.freedesktop.org/standards/shared-mime-info".into(),
    );
    xml.children.push(XmlNode::Text("<&>".into()));
    l.mime_packages
        .insert(path("share/mime/packages/org.example.App.xml"), xml);
    let mut component = XmlElement::new("component");
    let mut description = XmlElement::new("name");
    description
        .attributes
        .insert("xml:lang".into(), "ja".into());
    description.children.push(XmlNode::Text("日本語".into()));
    component.children.push(XmlNode::Element(description));
    l.metainfo.insert(
        path("share/metainfo/org.example.App.metainfo.xml"),
        component,
    );
    l.dbus_services.insert(
        path("share/dbus-1/services/org.example.App.service"),
        DbusService {
            entries: [
                ("Name".into(), "org.example.App".into()),
                ("Exec".into(), "/usr/bin/demo --service".into()),
            ]
            .into(),
        },
    );
    let out = app.0.join("linux");
    sync_project(&out, &inputs).unwrap();
    let files = distribution_files(&out, &load_build_plan(&out).unwrap()).unwrap();
    let desktop = text(&files, "share/applications/org.example.App.desktop");
    assert!(desktop.contains("MimeType=text/plain;x-test/a\\;b;\n"));
    assert!(desktop.contains("[Desktop Action compose]"));
    assert!(desktop.contains("Exec=demo\\s--title\\s\"hello\\sworld\"\\s%U"));
    assert!(text(&files, "share/mime/packages/org.example.App.xml").contains("&lt;&amp;&gt;"));
    assert!(
        text(&files, "share/dbus-1/services/org.example.App.service")
            .contains("Exec=/usr/bin/demo --service\n")
    );
}
#[test]
fn input_bytes_modes_stale_files_and_failed_preflight() {
    let app = App::new();
    let mut inputs = app.project(Target::Linux);
    std::fs::write(app.0.join("source"), "one").unwrap();
    let ProjectIr::Linux(l) = &mut inputs.project else {
        panic!()
    };
    l.files.insert(
        path("data.txt"),
        ProjectFile::AppFile {
            source: path("source"),
        },
    );
    l.resources.push(Resource {
        source: path("data.txt"),
        destination: path("share/demo/data.txt"),
        kind: ResourceKind::File,
    });
    let out = app.0.join("linux");
    assert!(sync_project(&out, &inputs).unwrap());
    std::fs::write(out.join("stale"), "old").unwrap();
    std::fs::write(app.0.join("source"), "two").unwrap();
    assert!(sync_project(&out, &inputs).unwrap());
    assert!(!out.join("stale").exists());
    assert_eq!(
        std::fs::read_to_string(out.join("data.txt")).unwrap(),
        "two"
    );
    std::fs::remove_file(app.0.join("source")).unwrap();
    assert!(sync_project(&out, &inputs).is_err());
    assert_eq!(
        std::fs::read_to_string(out.join("data.txt")).unwrap(),
        "two"
    );
}
#[test]
fn unsupported_packaging_build_scripts_and_expanded_path_collisions_fail() {
    let app = App::new();
    let mut inputs = app.project(Target::Windows);
    let ProjectIr::Windows(w) = &mut inputs.project else {
        panic!()
    };
    w.files.insert(
        path("build.rs"),
        ProjectFile::Generated {
            entry: FileEntry::text("fn main() {}"),
        },
    );
    assert!(
        render_project(&inputs)
            .unwrap_err()
            .to_string()
            .contains("owns build.rs")
    );
    let ProjectIr::Windows(w) = &mut inputs.project else {
        panic!()
    };
    w.files.remove(&path("build.rs"));
    w.packages.insert(
        "store".into(),
        MsixPackage {
            manifest: XmlElement::new("Package"),
            executables: vec!["app".into()],
            resources: vec![],
        },
    );
    assert!(
        render_project(&inputs)
            .unwrap_err()
            .to_string()
            .contains("MSIX")
    );
    let mut inputs = app.project(Target::Windows);
    std::fs::create_dir(app.0.join("assets")).unwrap();
    std::fs::write(app.0.join("assets/CON.txt"), "bad Windows name").unwrap();
    let ProjectIr::Windows(w) = &mut inputs.project else {
        panic!()
    };
    w.files.insert(
        path("assets"),
        ProjectFile::AppDirectory {
            source: path("assets"),
        },
    );
    assert!(
        render_project(&inputs)
            .unwrap_err()
            .to_string()
            .contains("reserved Windows")
    );
    let mut reserved = app.project(Target::Windows);
    let ProjectIr::Windows(w) = &mut reserved.project else {
        panic!()
    };
    w.files.insert(
        path("TARGET/input"),
        ProjectFile::Generated {
            entry: FileEntry::text("reserved"),
        },
    );
    assert!(
        render_project(&reserved)
            .unwrap_err()
            .to_string()
            .contains("reserved desktop staging")
    );
    let mut inputs = app.project(Target::Linux);
    let ProjectIr::Linux(l) = &mut inputs.project else {
        panic!()
    };
    l.packages.insert(
        "deb".into(),
        LinuxPackage::Recipe {
            format: "deb".into(),
            path: path("debian/control"),
        },
    );
    assert!(
        render_project(&inputs)
            .unwrap_err()
            .to_string()
            .contains("package backends")
    );
}
#[test]
fn empty_identities_initialize_but_cannot_silently_replace_an_application() {
    let app = App::new();
    for target in [Target::Windows, Target::Linux] {
        let mut empty = app.inputs(target).empty_project().unwrap();
        assert!(empty.validate_structure().is_err());
        let initialized = app.project(target).project;
        empty.merge_from(&initialized).unwrap();
        assert_eq!(empty, initialized);
        let mut changed = initialized.clone();
        match &mut changed {
            ProjectIr::Windows(w) => w.application = "other".into(),
            ProjectIr::Linux(l) => l.app_id = "other".into(),
            _ => unreachable!(),
        }
        assert!(empty.merge_from(&changed).is_err());
        assert_eq!(empty, initialized);
    }
}

#[test]
#[ignore = "requires rc.exe or llvm-rc; set WHISKER_TEST_RC"]
fn windows_resource_compiler_accepts_manifest_icon_and_unicode_version_info() {
    let app = App::new();
    let mut inputs = app.inputs(Target::Windows);
    let mut png = std::io::Cursor::new(vec![]);
    image::RgbaImage::from_pixel(256, 256, image::Rgba([20, 30, 40, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    inputs.app_icon_png = Some(png.into_inner());
    let project = app.compose(inputs);
    let out = app.0.join("windows");
    sync_project(&out, &project).unwrap();
    let output = std::process::Command::new(
        std::env::var_os("WHISKER_TEST_RC").expect("set WHISKER_TEST_RC to rc.exe or llvm-rc"),
    )
    .args(["/FO", "resources.res", "/I", ".", ".whisker-windows-0.rc"])
    .current_dir(&out)
    .output()
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(std::fs::metadata(out.join("resources.res")).unwrap().len() > 256);
}
