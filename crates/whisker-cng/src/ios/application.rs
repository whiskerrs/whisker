//! Standard iOS application declarations, including native Host and Cargo integration.
use super::*;
use whisker_plugin::project::*;

/// Standard application plugin. Inputs are resolved from app configuration and,
/// when present, legacy plugin contributions before project composition begins.
pub struct ApplicationPlugin {
    inputs: IosInputs,
}
/// Application settings are supplied once through IosInputs.
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationPluginConfig {}
impl whisker_plugin::PluginConfig for ApplicationPluginConfig {
    const NAME: &'static str = ApplicationPlugin::NAME;
}
impl ApplicationPlugin {
    /// Stable ordering identity, reserved by the standard generator.
    pub const NAME: &'static str = "whisker-ios-application";
    /// Capture the application's resolved settings and module graph.
    pub fn new(inputs: IosInputs) -> Self {
        Self { inputs }
    }
}
impl ProjectPlugin for ApplicationPlugin {
    type Config = ApplicationPluginConfig;
    fn contribute(&self, context: &ProjectContext, _: &Self::Config) -> Result<ProjectUpdate> {
        if !matches!(context.project, ProjectIr::Ios(_)) {
            return Ok(ProjectUpdate::Keep);
        }
        Ok(ProjectUpdate::Merge {
            project: Box::new(ProjectIr::Ios(declarations(&self.inputs)?)),
        })
    }
}
pub(super) fn settings(entries: &[(&str, &str)]) -> BTreeMap<String, AppleBuildSetting> {
    entries
        .iter()
        .map(|(k, v)| (k.to_string(), AppleBuildSetting::String(v.to_string())))
        .collect()
}
pub(super) fn legacy_value(value: &PlistValue) -> PropertyListValue {
    match value {
        PlistValue::String(v) => PropertyListValue::String(v.clone()),
        PlistValue::Boolean(v) => PropertyListValue::Boolean(*v),
        PlistValue::Integer(v) => PropertyListValue::Integer(*v),
        PlistValue::Real(v) => PropertyListValue::Real(*v),
        PlistValue::Array(v) => PropertyListValue::Array(v.iter().map(legacy_value).collect()),
        PlistValue::Dict(v) => PropertyListValue::Dict(
            v.iter()
                .map(|(k, v)| (k.clone(), legacy_value(v)))
                .collect(),
        ),
    }
}
pub(super) fn apply_ops(target: &mut AppleTarget, ops: &[PbxprojOp]) -> Result<()> {
    for op in ops {
        let path = |p: &Path| -> Result<ProjectPath> {
            crate::render::validate_extra_file_path(p)?;
            ProjectPath::new(p.to_str().context("non-UTF-8 plugin path")?)
        };
        match op {
            PbxprojOp::AddSource { path: p } => {
                let v = AppleSource {
                    path: AppleBuildPath::Project(path(p)?),
                    platform_filters: vec![],
                    compiler_flags: vec![],
                };
                if !target.sources.contains(&v) {
                    target.sources.push(v);
                }
            }
            PbxprojOp::AddResource { path: p } => {
                let v = AppleResource::Process { path: path(p)? };
                if !target.resources.contains(&v) {
                    target.resources.push(v);
                }
            }
            PbxprojOp::AddResourceFolder { path: p } => {
                let v = AppleResource::Copy {
                    resource: Resource {
                        source: path(p)?,
                        destination: ProjectPath::new(
                            p.file_name()
                                .and_then(|v| v.to_str())
                                .context("resource folder name")?,
                        )?,
                        kind: ResourceKind::Directory,
                    },
                };
                if !target.resources.contains(&v) {
                    target.resources.push(v);
                }
            }
            PbxprojOp::LinkSystemFramework { name } => {
                let v = AppleDependency::SystemFramework {
                    name: name.clone(),
                    weak: false,
                    platform_filters: vec![],
                };
                if !target.dependencies.contains(&v) {
                    target.dependencies.push(v);
                }
            }
            PbxprojOp::SetBuildSetting { key, value } => {
                target
                    .build_settings
                    .insert(key.clone(), AppleBuildSetting::String(value.clone()));
            }
        }
    }
    Ok(())
}
fn declarations(inputs: &IosInputs) -> Result<IosProjectIr> {
    crate::background::AppBackground::parse(&inputs.background)?;
    let vars = template_vars(inputs);
    let mut info: BTreeMap<String, PropertyListValue> = [
        ("CFBundleDevelopmentRegion", "$(DEVELOPMENT_LANGUAGE)"),
        ("CFBundleDisplayName", &inputs.app_name),
        ("CFBundleExecutable", "$(EXECUTABLE_NAME)"),
        ("CFBundleIdentifier", "$(PRODUCT_BUNDLE_IDENTIFIER)"),
        ("CFBundleInfoDictionaryVersion", "6.0"),
        ("CFBundleName", "$(PRODUCT_NAME)"),
        ("CFBundlePackageType", "APPL"),
        ("CFBundleShortVersionString", &inputs.version),
        ("CFBundleVersion", &inputs.build_number.to_string()),
        ("UILaunchStoryboardName", "LaunchScreen"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), PropertyListValue::String(v.into())))
    .collect();
    info.insert(
        "UIApplicationSceneManifest".into(),
        PropertyListValue::Dict(
            [(
                "UIApplicationSupportsMultipleScenes".into(),
                PropertyListValue::Boolean(false),
            )]
            .into(),
        ),
    );
    info.extend(
        inputs
            .extra_info_plist
            .iter()
            .filter(|(k, _)| {
                k.as_str() != "UILaunchScreen" && k.as_str() != "UILaunchStoryboardName"
            })
            .map(|(k, v)| (k.clone(), legacy_value(v))),
    );
    let output = AppleBuildPath::Expression {
        expression: "$(BUILT_PRODUCTS_DIR)/Frameworks/WhiskerDriver.framework".into(),
    };
    let mut script_vars = vars.clone();
    for k in ["whisker_workspace_root", "whisker_user_package"] {
        script_vars.insert(
            k,
            vars[k]
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('$', "\\$")
                .replace('`', "\\`"),
        );
    }
    let mut target = AppleTarget {
        product_name: inputs.scheme.clone(),
        product_type: Some("com.apple.product-type.application".into()),
        info_plist: info,
        sources: vec![AppleSource {
            path: AppleBuildPath::Project(ProjectPath::new("Sources/AppDelegate.swift")?),
            platform_filters: vec![],
            compiler_flags: vec![],
        }],
        resources: [
            "Resources/Assets.xcassets",
            "Resources/LaunchScreen.storyboard",
        ]
        .map(|p| AppleResource::Process {
            path: ProjectPath::new(p).unwrap(),
        })
        .into(),
        build_settings: settings(&[
            ("CODE_SIGN_IDENTITY", "iPhone Developer"),
            ("INFOPLIST_FILE", "Info.plist"),
            ("FRAMEWORK_SEARCH_PATHS", "$(BUILT_PRODUCTS_DIR)/Frameworks"),
            (
                "LD_RUNPATH_SEARCH_PATHS",
                "$(inherited) @executable_path/Frameworks",
            ),
            ("OTHER_LDFLAGS", "$(inherited) -framework WhiskerDriver"),
            ("PRODUCT_BUNDLE_IDENTIFIER", &inputs.bundle_id),
            ("PRODUCT_NAME", "$(TARGET_NAME)"),
            ("SDKROOT", "iphoneos"),
            ("TARGETED_DEVICE_FAMILY", "1,2"),
        ]),
        dependencies: vec![
            AppleDependency::SwiftProduct {
                package: "whisker_modules".into(),
                product: "WhiskerModules".into(),
                weak: false,
                platform_filters: vec![],
            },
            AppleDependency::BuildOutput {
                path: output.clone(),
                weak: false,
                platform_filters: vec![],
            },
        ],
        embeds: vec![AppleEmbed {
            source: AppleEmbedSource::BuildOutput { path: output },
            destination: ProjectPath::new("Frameworks")?,
            code_sign_on_copy: true,
            remove_headers_on_copy: true,
            platform_filters: vec![],
        }],
        scripts: vec![AppleBuildScript {
            name: "Whisker Build Rust App".into(),
            position: AppleScriptPosition::BeforeSources,
            shell: "/bin/bash".into(),
            script: render(include_str!("../templates/ios/build-rust.sh"), &script_vars)?,
            inputs: vec![],
            outputs: vec![AppleBuildPath::Expression {
                expression:
                    "$(BUILT_PRODUCTS_DIR)/Frameworks/WhiskerDriver.framework/WhiskerDriver".into(),
            }],
            input_file_lists: vec![],
            output_file_lists: vec![],
            based_on_dependency_analysis: Some(false),
        }],
        ..Default::default()
    };
    apply_ops(&mut target, &inputs.pbxproj_ops)?;
    let mut apple = AppleProjectIr {
        application: "app".into(),
        targets: [("app".into(), target)].into(),
        build_settings: settings(&[
            ("ALWAYS_SEARCH_USER_PATHS", "NO"),
            ("CLANG_ENABLE_MODULES", "YES"),
            ("CLANG_ENABLE_OBJC_ARC", "YES"),
            ("COPY_PHASE_STRIP", "NO"),
            ("GCC_C_LANGUAGE_STANDARD", "gnu11"),
            ("IPHONEOS_DEPLOYMENT_TARGET", &inputs.deployment_target),
            ("SDKROOT", "iphoneos"),
            ("SWIFT_VERSION", "5.9"),
        ]),
        configurations: [
            (
                "Debug".into(),
                AppleBuildConfiguration {
                    settings: settings(&[
                        ("DEBUG_INFORMATION_FORMAT", "dwarf"),
                        ("ENABLE_TESTABILITY", "YES"),
                        ("GCC_OPTIMIZATION_LEVEL", "0"),
                        ("ONLY_ACTIVE_ARCH", "YES"),
                        ("SWIFT_ACTIVE_COMPILATION_CONDITIONS", "DEBUG"),
                        ("SWIFT_OPTIMIZATION_LEVEL", "-Onone"),
                    ]),
                    ..Default::default()
                },
            ),
            (
                "Release".into(),
                AppleBuildConfiguration {
                    settings: settings(&[
                        ("DEBUG_INFORMATION_FORMAT", "dwarf-with-dsym"),
                        ("ENABLE_NS_ASSERTIONS", "NO"),
                        ("SWIFT_COMPILATION_MODE", "wholemodule"),
                        ("SWIFT_OPTIMIZATION_LEVEL", "-O"),
                    ]),
                    ..Default::default()
                },
            ),
        ]
        .into(),
        default_configuration: Some("Debug".into()),
        swift_packages: [(
            "whisker_modules".into(),
            SwiftPackage::Local {
                path: ProjectPath::new("whisker_modules")?,
            },
        )]
        .into(),
        schemes: [(
            inputs.scheme.clone(),
            AppleScheme {
                build_targets: vec!["app".into()],
                run_target: Some("app".into()),
                test_targets: vec![],
                run_configuration: "Debug".into(),
                test_configuration: Some("Debug".into()),
                profile_configuration: Some("Release".into()),
                analyze_configuration: Some("Debug".into()),
                archive_configuration: "Release".into(),
                action_options: Default::default(),
                build_for: Default::default(),
                test_plans: vec![],
                default_test_plan: None,
            },
        )]
        .into(),
        ..Default::default()
    };
    apple
        .configurations
        .get_mut("Debug")
        .unwrap()
        .settings
        .insert(
            "GCC_PREPROCESSOR_DEFINITIONS".into(),
            AppleBuildSetting::List(vec!["DEBUG=1".into(), "$(inherited)".into()]),
        );
    for (path, template) in [
        ("Sources/AppDelegate.swift", APP_DELEGATE_SWIFT),
        (
            "Resources/LaunchScreen.storyboard",
            LAUNCH_SCREEN_STORYBOARD,
        ),
        ("Resources/Assets.xcassets/Contents.json", ASSET_CATALOG),
        (
            "Resources/Assets.xcassets/WhiskerBackground.colorset/Contents.json",
            BACKGROUND_COLORSET,
        ),
    ] {
        apple.files.insert(
            ProjectPath::new(path)?,
            ProjectFile::Generated {
                entry: FileEntry::text(render(template, &vars)?),
            },
        );
    }
    for (path, entry) in crate::ios_modules::module_files(&inputs.modules, &inputs.workspace_root) {
        apple.files.insert(path, ProjectFile::Generated { entry });
    }
    for (path, entry) in &inputs.extra_files {
        crate::render::validate_extra_file_path(path)?;
        apple.files.insert(
            ProjectPath::new(path.to_str().context("non-UTF-8 plugin path")?)?,
            ProjectFile::Generated {
                entry: entry.clone(),
            },
        );
    }
    Ok(IosProjectIr { apple })
}
