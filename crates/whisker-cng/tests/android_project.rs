#![cfg(feature = "generate")]
use std::path::PathBuf;
use whisker_cng::plugins::{
    android_application_attributes::AndroidApplicationAttributes,
    android_gradle_dependencies::GradleDependencies, android_gradle_plugins::GradlePlugins,
    android_permissions::AndroidPermissions,
};
use whisker_cng::{Config, ProjectEngine, android::*};
use whisker_plugin::{FileEntry, project::*};

fn application_inputs() -> AndroidInputs {
    let mut config = Config::default();
    config.name("IR & App").bundle_id("test.ir.app");
    inputs_from_with_engine(
        &whisker_cng::Engine::new(),
        &config,
        "ir_app".into(),
        PathBuf::from("../.."),
        "ir-app".into(),
        "0.1.25".into(),
        "0.5.0".into(),
        "https://example.invalid/maven".into(),
    )
    .unwrap()
}
fn base() -> AndroidProjectInputs {
    AndroidProjectInputs {
        project: {
            let result = ProjectEngine::with_android_application(application_inputs())
                .compose(&Config::default(), &ProjectIr::Android(Box::default()))
                .unwrap();
            let ProjectIr::Android(project) = result.project else {
                panic!()
            };
            *project
        },
        app_crate_dir: None,
        cargo_selection: Default::default(),
        template_version: 38,
    }
}
fn text(files: &std::collections::BTreeMap<ProjectPath, FileEntry>, path: &str) -> String {
    String::from_utf8(files[&ProjectPath::new(path).unwrap()].to_bytes().unwrap()).unwrap()
}
fn temp() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let p = std::env::temp_dir().join(format!(
        "whisker-android-project-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn builtins_compose_against_the_new_manifest_and_gradle_scopes() {
    let mut input = base();
    let mut config = Config::default();
    config.plugin::<AndroidPermissions>(|p| {
        p.add("android.permission.CAMERA");
        p.add("android.permission.CAMERA");
    });
    config.plugin::<AndroidApplicationAttributes>(|p| {
        p.set("android:theme", "@style/MyTheme");
    });
    config.plugin::<GradlePlugins>(|p| {
        p.add("example.plugin");
        p.add_raw("alias(libs.plugins.native)");
    });
    config.plugin::<GradleDependencies>(|p| {
        p.add("implementation(platform(\"example:bom:1\"))");
    });
    let result = ProjectEngine::with_android_application(application_inputs())
        .compose(&config, &ProjectIr::Android(Box::default()))
        .unwrap();
    assert_eq!(
        result.steps[0],
        whisker_cng::ProjectStep::Merge {
            plugin: application::ApplicationPlugin::NAME.into(),
        }
    );
    let ProjectIr::Android(project) = result.project else {
        panic!()
    };
    input.project = *project;
    let files = render_project(&input).unwrap();
    let manifest = text(&files, "app/src/main/AndroidManifest.xml");
    assert_eq!(manifest.matches("android.permission.CAMERA").count(), 1);
    assert_eq!(manifest.matches("android:theme=").count(), 1);
    assert!(manifest.contains("@style/MyTheme") && manifest.contains("IR &amp; App"));
    let gradle = text(&files, "app/build.gradle.kts");
    assert!(
        gradle.contains("id(\"example.plugin\")") && gradle.contains("alias(libs.plugins.native)")
    );
    assert!(gradle.contains("implementation(platform(\"example:bom:1\"))"));
}
#[test]
fn extra_modules_variants_and_scoped_files_are_rendered() {
    let mut input = base();
    let mut library = input.project.modules[":app"].clone();
    let AndroidModuleKind::Application(app) = library.kind else {
        panic!()
    };
    let mut build = app.android;
    build.namespace = "test.ir.library".into();
    build.statements.clear();
    build.default_config_statements.clear();
    build.sdk.compile = Some(AndroidCompileSdk::Release {
        api: 35,
        minor: None,
        extension: Some(1),
    });
    build.variants.flavor_dimensions = vec!["environment".into()];
    build.variants.product_flavors.insert(
        "staging".into(),
        AndroidProductFlavor {
            dimension: "environment".into(),
            matching_fallbacks: vec!["demo".into()],
            missing_dimension_strategies: Default::default(),
            values: AndroidVariantValues {
                manifest_placeholders: [("host".into(), "\"example.invalid\"".into())].into(),
                ..Default::default()
            },
            statements: vec![],
        },
    );
    build.default_config.build_config_fields.insert(
        "ENABLED".into(),
        AndroidBuildConfigField {
            type_name: "boolean".into(),
            value: "\"true\"".into(),
        },
    );
    build.default_config.res_values.insert(
        "string".into(),
        [("label".into(), "\"Label\"".into())].into(),
    );
    build.source_sets.clear();
    build.source_sets.insert(
        "main".into(),
        AndroidSourceSet {
            manifest: Some(XmlElement::new("manifest")),
            sources: vec![ProjectPath::new("shared/kotlin").unwrap()],
            ..Default::default()
        },
    );
    library.kind = AndroidModuleKind::Library(build);
    library.directory = ProjectPath::new("native/library").unwrap();
    library.dependencies.clear();
    library.build = GradleBuildScript {
        plugins: vec![GradlePlugin {
            id: "com.android.library".into(),
            version: None,
            alias: None,
            apply: true,
        }],
        ..Default::default()
    };
    input.project.modules.insert(":library".into(), library);
    input
        .project
        .modules
        .get_mut(":app")
        .unwrap()
        .dependencies
        .push(GradleDependency {
            configuration: "implementation".into(),
            source: GradleDependencySource::Project(":library".into()),
        });
    input.project.modules.insert(
        ":tool".into(),
        AndroidModule {
            directory: ProjectPath::new("tools").unwrap(),
            kind: AndroidModuleKind::Jvm,
            build: GradleBuildScript {
                plugins: vec![GradlePlugin {
                    id: "java-library".into(),
                    version: None,
                    alias: None,
                    apply: true,
                }],
                ..Default::default()
            },
            dependencies: vec![],
        },
    );
    let files = render_project(&input).unwrap();
    assert!(
        text(&files, "settings.gradle.kts")
            .contains("project(\":library\").projectDir = file(\"native/library\")")
    );
    let script = text(&files, "native/library/build.gradle.kts");
    assert!(script.contains("compileSdk = 35") && script.contains("compileSdkExtension = 1"));
    assert!(!script.contains("applicationId"));
    assert!(script.contains("buildConfigField(\"boolean\", \"ENABLED\", \"true\")"));
    assert!(script.contains("resValue(\"string\", \"label\", \"Label\")"));

    assert!(
        script.contains("dimension = \"environment\"")
            && script.contains("rootProject.file(\"shared/kotlin\")")
    );
    assert!(!text(&files, "tools/build.gradle.kts").contains("android {"));
    assert!(text(&files, "app/build.gradle.kts").contains("project(\":library\")"));
    assert!(
        files.contains_key(
            &ProjectPath::new("native/library/src/main/AndroidManifest.xml").unwrap()
        )
    );
}
#[test]
fn staged_directory_content_changes_invalidate_fingerprint_and_failures_preserve_output() {
    let dir = temp();
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("assets/file.txt"), "one").unwrap();
    let mut input = base();
    input.app_crate_dir = Some(dir.clone());
    input.project.files.insert(
        ProjectPath::new("app/src/main/assets").unwrap(),
        ProjectFile::AppDirectory {
            source: ProjectPath::new("assets").unwrap(),
        },
    );
    let out = dir.join("gen/android");
    assert!(sync_project(&out, &input).unwrap());
    assert!(!sync_project(&out, &input).unwrap());
    std::fs::write(dir.join("assets/file.txt"), "two").unwrap();
    assert!(sync_project(&out, &input).unwrap());
    assert_eq!(
        std::fs::read_to_string(out.join("app/src/main/assets/file.txt")).unwrap(),
        "two"
    );
    let before = std::fs::read(out.join(".whisker-fingerprint")).unwrap();
    input.project.files.insert(
        ProjectPath::new("app/build.gradle.kts").unwrap(),
        ProjectFile::Generated {
            entry: FileEntry::text("mask structured build"),
        },
    );
    assert!(
        sync_project(&out, &input)
            .unwrap_err()
            .to_string()
            .contains("multiple owners")
    );
    assert_eq!(
        before,
        std::fs::read(out.join(".whisker-fingerprint")).unwrap()
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn unresolved_xml_prefix_and_unsupported_backend_fields_fail_explicitly() {
    let mut input = base();
    let AndroidModuleKind::Application(app) =
        &mut input.project.modules.get_mut(":app").unwrap().kind
    else {
        panic!()
    };
    app.android.sdk.compile = Some(AndroidCompileSdk::Release {
        api: 36,
        minor: Some(1),
        extension: None,
    });
    assert!(format!("{:#}", render_project(&input).unwrap_err()).contains("minor compile"));
    let mut input = base();
    let AndroidModuleKind::Application(app) =
        &mut input.project.modules.get_mut(":app").unwrap().kind
    else {
        panic!()
    };
    app.android
        .source_sets
        .get_mut("main")
        .unwrap()
        .manifest
        .as_mut()
        .unwrap()
        .attributes
        .insert("tools:node".into(), "merge".into());
    assert!(
        render_project(&input)
            .unwrap_err()
            .to_string()
            .contains("unbound XML prefix")
    );
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct EditApplicationConfig;
impl whisker_plugin::PluginConfig for EditApplicationConfig {
    // Alphabetically before the application plugin; the initializer edge must
    // determine the execution order, not registration or name order.
    const NAME: &'static str = "a-edit-application";
}
struct EditApplication {
    remove: bool,
}
impl ProjectPlugin for EditApplication {
    type Config = EditApplicationConfig;
    fn contribute(
        &self,
        context: &ProjectContext,
        _: &Self::Config,
    ) -> anyhow::Result<ProjectUpdate> {
        let ProjectIr::Android(original) = &context.project else {
            unreachable!()
        };
        let mut project = original.clone();
        let module = project
            .modules
            .get_mut(":app")
            .expect("application plugin must run first");
        let AndroidModuleKind::Application(app) = &mut module.kind else {
            panic!()
        };
        app.application_id = Some("test.changed.id".into());
        app.android.sdk.min = Some(AndroidApiLevel::Release(28));
        project
            .files
            .remove(&ProjectPath::new("app/src/main/res/values/colors.xml")?);
        if self.remove {
            project.modules.remove(":app");
        }
        Ok(ProjectUpdate::Replace {
            project: Box::new(ProjectIr::Android(project)),
            reason: "customize the application's native declarations".into(),
        })
    }
}

#[test]
fn application_declarations_can_be_edited_and_final_validation_requires_a_main_product() {
    let empty = ProjectIr::Android(Box::default());
    for remove in [false, true] {
        let mut engine = ProjectEngine::with_android_application(application_inputs());
        engine.register(EditApplication { remove });
        let result = engine.compose(&Config::default(), &empty);
        if remove {
            let error = format!("{:#}", result.unwrap_err());
            assert!(
                error.contains("validate composed project structure"),
                "{error}"
            );
        } else {
            let result = result.unwrap();
            assert!(
                matches!(&result.steps[1], whisker_cng::ProjectStep::Replace { plugin, .. } if plugin == "a-edit-application")
            );
            let ProjectIr::Android(project) = result.project else {
                panic!()
            };
            let files = render_project(&AndroidProjectInputs {
                project: *project,
                app_crate_dir: None,
                cargo_selection: Default::default(),
                template_version: 38,
            })
            .unwrap();
            let script = text(&files, "app/build.gradle.kts");
            assert!(script.contains("applicationId = \"test.changed.id\""));
            assert!(script.contains("minSdk = 28"));
            assert!(
                !files
                    .contains_key(&ProjectPath::new("app/src/main/res/values/colors.xml").unwrap())
            );
        }
    }
    assert_eq!(empty, ProjectIr::Android(Box::default()));
}
