//! Whisker's Android application policy, expressed as project declarations.
use super::*;
use whisker_plugin::project::*;

/// Whisker's standard Android application contribution.
///
/// Resolves native declarations from application inputs and returns Merge, just
/// like a library plugin. Register with ProjectEngine::with_initializer to run
/// before feature plugins. Inputs may include resolved legacy contributions;
/// they are never reapplied after project plugins run.
pub struct ApplicationPlugin {
    inputs: AndroidInputs,
}

/// The application's settings come from AndroidInputs; no second override set
/// is accepted through Config.plugins.
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationPluginConfig {}

impl whisker_plugin::PluginConfig for ApplicationPluginConfig {
    const NAME: &'static str = ApplicationPlugin::NAME;
}

impl ApplicationPlugin {
    /// Stable identity used in ordering constraints and composition reports.
    pub const NAME: &'static str = "whisker-android-application";

    /// Capture resolved application settings and native toolchain inputs.
    pub fn new(inputs: AndroidInputs) -> Self {
        Self { inputs }
    }
}

impl ProjectPlugin for ApplicationPlugin {
    type Config = ApplicationPluginConfig;

    fn contribute(&self, context: &ProjectContext, _: &Self::Config) -> Result<ProjectUpdate> {
        if !matches!(context.project, ProjectIr::Android(_)) {
            return Ok(ProjectUpdate::Keep);
        }
        Ok(ProjectUpdate::Merge {
            project: Box::new(ProjectIr::Android(Box::new(declarations(&self.inputs)?))),
        })
    }
}

/// Build the declarations owned by the standard application plugin.
pub(super) fn declarations(inputs: &AndroidInputs) -> Result<AndroidProjectIr> {
    crate::background::AppBackground::parse(&inputs.background)?;
    anyhow::ensure!(
        inputs.application_id.split('.').all(|part| {
            !part.is_empty()
                && part.chars().enumerate().all(|(i, c)| {
                    c == '_' || c.is_ascii_alphabetic() || i > 0 && c.is_ascii_digit()
                })
        }),
        "Android application ID must contain valid package identifiers"
    );

    let mut vars = template_vars(inputs);
    // These substitutions occur inside Kotlin string literals in the Host
    // snippets; model-owned IDs/metadata are rendered separately below.
    for key in [
        "whisker_workspace_path",
        "whisker_user_package",
        "whisker_gradle_plugin_version",
        "rust_lib_name",
    ] {
        let quoted = super::project_render::quote(&vars[key]);
        vars.insert(key, quoted[1..quoted.len() - 1].to_owned());
    }

    let repositories = |portal: bool| {
        let mut repos = vec![GradleRepository {
            id: "whisker".into(),
            expression: format!(
                "maven {{ url = uri({}) }}",
                super::project_render::quote(&inputs.whisker_maven_url)
            ),
        }];
        if portal {
            repos.push(GradleRepository {
                id: "gradlePluginPortal".into(),
                expression: "gradlePluginPortal()".into(),
            });
        }
        repos.extend(["google", "mavenCentral"].map(|id| GradleRepository {
            id: id.into(),
            expression: format!("{id}()"),
        }));
        repos
    };
    let mut build = AndroidBuild {
        namespace: inputs.application_id.clone(),
        sdk: AndroidSdk {
            compile: Some(AndroidCompileSdk::Release {
                api: inputs.target_sdk,
                minor: None,
                extension: None,
            }),
            min: Some(AndroidApiLevel::Release(inputs.min_sdk)),
            target: Some(AndroidApiLevel::Release(inputs.target_sdk)),
        },
        default_config: Default::default(),
        default_config_statements: vec![format!(
            "versionCode = {}\nversionName = {}",
            inputs.build_number,
            super::project_render::quote(&inputs.version)
        )],
        variants: Default::default(),
        source_sets: Default::default(),
        statements: vec![render(
            include_str!("../templates/android/host-android.gradle.kts"),
            &vars,
        )?],
    };
    // Signing local variables and build types share lexical scope. Keep this
    // native policy together, after the structured variants in the renderer.
    for name in ["debug", "release"] {
        build.variants.build_types.insert(
            name.into(),
            AndroidBuildType {
                statements: vec!["isMinifyEnabled = false".into()],
                ..Default::default()
            },
        );
    }
    build.statements.push("buildTypes {\n    getByName(\"release\") {\n        if (whiskerKeystore != null) { signingConfig = signingConfigs.getByName(\"whiskerRelease\") }\n    }\n}".into());
    build.source_sets.insert(
        "main".into(),
        AndroidSourceSet {
            manifest: Some(manifest(inputs)),
            sources: vec![
                ProjectPath::new("app/src/main/java")?,
                ProjectPath::new("app/src/main/kotlin")?,
            ],
            ..Default::default()
        },
    );
    let mut module = AndroidModule {
        directory: ProjectPath::new("app")?,
        kind: AndroidModuleKind::Application(AndroidApplication {
            android: build,
            application_id: Some(inputs.application_id.clone()),
            dynamic_features: vec![],
            asset_packs: vec![],
        }),
        build: GradleBuildScript {
            plugins: [
                "com.android.application",
                "org.jetbrains.kotlin.android",
                "rs.whisker.gradle",
            ]
            .map(|id| plugin(id, None, true))
            .into(),
            ..Default::default()
        },
        dependencies: vec![GradleDependency {
            configuration: "implementation".into(),
            source: GradleDependencySource::Maven("androidx.activity:activity:1.8.2".into()),
        }],
    };
    for entry in &inputs.extra_gradle_plugins {
        add_plugin(&mut module.build, entry);
    }
    module.build.statements.push(
        "kotlin {\n    compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) }\n}".into(),
    );
    module.build.statements.push(format!("dependencies {{\n    if (rootProject.findProject(\":whisker-runtime\") != null) {{\n        implementation(project(\":whisker-runtime\"))\n    }} else {{\n        implementation({})\n    }}\n{}\n}}", super::project_render::quote(&format!("rs.whisker:whisker-runtime-android:{}", inputs.whisker_sdk_version)), render_extra_gradle_dependencies(&inputs.extra_gradle_dependencies)));
    let mut project = AndroidProjectIr {
        application: ":app".into(),
        modules: BTreeMap::from([(":app".into(), module)]),
        settings: GradleSettings {
            plugin_management: GradlePluginManagement {
                statements: vec![render(
                    include_str!("../templates/android/local-plugin-management.gradle.kts"),
                    &vars,
                )?],
                repositories: repositories(true),
                plugins: ["rs.whisker", "rs.whisker.gradle"]
                    .map(|id| (id.into(), inputs.whisker_gradle_plugin_version.clone()))
                    .into(),
                ..Default::default()
            },
            plugins: vec![plugin("rs.whisker", None, true)],
            dependency_resolution: GradleDependencyResolution {
                repositories_mode: Some(GradleRepositoriesMode::FailOnProjectRepos),
                repositories: repositories(false),
                ..Default::default()
            },
            statements: vec![
                format!(
                    "rootProject.name = {}",
                    super::project_render::quote(&project_name(&inputs.app_name))
                ),
                render(
                    include_str!("../templates/android/host-settings.gradle.kts"),
                    &vars,
                )?,
            ],
            ..Default::default()
        },
        root_build: GradleBuildScript {
            plugins: vec![
                plugin("com.android.application", Some("8.10.1"), false),
                plugin("com.android.library", Some("8.10.1"), false),
                plugin("org.jetbrains.kotlin.android", Some("2.4.20"), false),
            ],
            statements: vec![include_str!("../templates/android/host-root.gradle.kts").into()],
            ..Default::default()
        },
        properties: GRADLE_PROPERTIES
            .lines()
            .filter_map(|s| s.split_once('='))
            .map(|(k, v)| (k.into(), v.into()))
            .collect(),
        files: BTreeMap::new(),
    };
    let activity = format!(
        "app/src/main/kotlin/{}/MainActivity.kt",
        inputs.application_id.replace('.', "/")
    );
    for (path, template) in [
        (activity.as_str(), MAIN_ACTIVITY_KT),
        ("app/src/main/res/values/colors.xml", COLORS_XML),
        ("app/src/main/res/values/styles.xml", STYLES_XML),
        ("app/src/main/res/values-v31/styles.xml", STYLES_V31_XML),
        (
            "gradle/wrapper/gradle-wrapper.properties",
            GRADLE_WRAPPER_PROPERTIES,
        ),
    ] {
        project.files.insert(
            ProjectPath::new(path)?,
            ProjectFile::Generated {
                entry: FileEntry::text(render(template, &vars)?),
            },
        );
    }
    let mut wrapper = FileEntry::text(GRADLEW);
    wrapper.mode = Some(0o755);
    for (path, entry) in [
        ("gradlew", wrapper),
        ("gradlew.bat", FileEntry::text(GRADLEW_BAT)),
        (
            "gradle/wrapper/gradle-wrapper.jar",
            FileEntry::binary(GRADLE_WRAPPER_JAR),
        ),
    ] {
        project
            .files
            .insert(ProjectPath::new(path)?, ProjectFile::Generated { entry });
    }
    for (path, entry) in &inputs.extra_files {
        crate::render::validate_extra_file_path(path)?;
        project.files.insert(
            ProjectPath::new(path.to_string_lossy())?,
            ProjectFile::Generated {
                entry: entry.clone(),
            },
        );
    }
    Ok(project)
}

pub(super) fn plugin(id: &str, version: Option<&str>, apply: bool) -> GradlePlugin {
    GradlePlugin {
        id: id.into(),
        version: version.map(Into::into),
        alias: None,
        apply,
    }
}
pub(crate) fn add_plugin(build: &mut GradleBuildScript, entry: &str) {
    if entry.contains('(') {
        build.plugin_statements.push(entry.into());
    } else if !build.plugins.iter().any(|p| p.id == entry) {
        build.plugins.push(plugin(entry, None, true));
    }
}
pub(crate) fn element(name: &str, attrs: &[(&str, &str)], children: Vec<XmlElement>) -> XmlElement {
    XmlElement {
        name: name.into(),
        attributes: attrs
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect(),
        children: children.into_iter().map(XmlNode::Element).collect(),
    }
}
fn manifest(i: &AndroidInputs) -> XmlElement {
    let mut activity = element(
        "activity",
        &[
            ("android:name", ".MainActivity"),
            ("android:exported", "true"),
            ("android:windowSoftInputMode", "adjustResize"),
        ],
        vec![element(
            "intent-filter",
            &[],
            vec![
                element(
                    "action",
                    &[("android:name", "android.intent.action.MAIN")],
                    vec![],
                ),
                element(
                    "category",
                    &[("android:name", "android.intent.category.LAUNCHER")],
                    vec![],
                ),
            ],
        )],
    );
    if !i.main_activity_url_schemes.is_empty() {
        activity
            .attributes
            .insert("android:launchMode".into(), "singleTask".into());
        let mut children = vec![
            element(
                "action",
                &[("android:name", "android.intent.action.VIEW")],
                vec![],
            ),
            element(
                "category",
                &[("android:name", "android.intent.category.DEFAULT")],
                vec![],
            ),
            element(
                "category",
                &[("android:name", "android.intent.category.BROWSABLE")],
                vec![],
            ),
        ];
        children.extend(
            i.main_activity_url_schemes
                .iter()
                .map(|s| element("data", &[("android:scheme", s)], vec![])),
        );
        activity
            .children
            .push(XmlNode::Element(element("intent-filter", &[], children)));
    }
    let mut app = element(
        "application",
        &[
            ("android:label", &i.app_name),
            ("android:usesCleartextTraffic", "true"),
            (
                "android:theme",
                i.android_theme.as_deref().unwrap_or("@style/Theme.Whisker"),
            ),
        ],
        vec![activity],
    );
    for a in &i.extra_application_attributes {
        app.attributes.insert(a.name.clone(), a.value.clone());
    }
    for m in &i.extra_meta_data {
        app.children.push(XmlNode::Element(element(
            "meta-data",
            &[("android:name", &m.name), ("android:value", &m.value)],
            vec![],
        )));
    }
    let mut root = element(
        "manifest",
        &[(
            "xmlns:android",
            "http://schemas.android.com/apk/res/android",
        )],
        vec![],
    );
    for permission in i
        .extra_permissions
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
    {
        root.children.push(XmlNode::Element(element(
            "uses-permission",
            &[("android:name", permission)],
            vec![],
        )));
    }
    root.children.push(XmlNode::Element(app));
    root
}
