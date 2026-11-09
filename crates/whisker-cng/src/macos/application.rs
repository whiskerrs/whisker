//! Standard macOS application declarations, including native Host and Cargo integration.
use super::*;
use crate::ios::application::settings;
use whisker_plugin::{FileEntry, PluginConfig, project::*};

pub struct ApplicationPlugin {
    inputs: MacosInputs,
}
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationPluginConfig {}
impl PluginConfig for ApplicationPluginConfig {
    const NAME: &'static str = ApplicationPlugin::NAME;
}
impl ApplicationPlugin {
    pub const NAME: &'static str = "whisker-macos-application";
    pub fn new(inputs: MacosInputs) -> Self {
        Self { inputs }
    }
}
impl ProjectPlugin for ApplicationPlugin {
    type Config = ApplicationPluginConfig;
    fn contribute(&self, ctx: &ProjectContext, _: &Self::Config) -> Result<ProjectUpdate> {
        if !matches!(ctx.project, ProjectIr::Macos(_)) {
            return Ok(ProjectUpdate::Keep);
        }
        Ok(ProjectUpdate::Merge {
            project: Box::new(ProjectIr::Macos(declarations(&self.inputs)?)),
        })
    }
}
fn declarations(inputs: &MacosInputs) -> Result<MacosProjectIr> {
    validate(inputs)?;
    let vars = template_vars(inputs);
    let mut info: BTreeMap<String, PropertyListValue> = [
        ("CFBundleDevelopmentRegion", "$(DEVELOPMENT_LANGUAGE)"),
        ("CFBundleDisplayName", inputs.app_name.as_str()),
        ("CFBundleExecutable", "$(EXECUTABLE_NAME)"),
        ("CFBundleIdentifier", "$(PRODUCT_BUNDLE_IDENTIFIER)"),
        ("CFBundleInfoDictionaryVersion", "6.0"),
        ("CFBundleName", "$(PRODUCT_NAME)"),
        ("CFBundlePackageType", "APPL"),
        ("CFBundleShortVersionString", inputs.version.as_str()),
        ("LSMinimumSystemVersion", "$(MACOSX_DEPLOYMENT_TARGET)"),
        ("NSPrincipalClass", "NSApplication"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), PropertyListValue::String(v.into())))
    .collect();
    info.insert(
        "CFBundleVersion".into(),
        PropertyListValue::String(inputs.build_number.to_string()),
    );
    info.insert(
        "NSHighResolutionCapable".into(),
        PropertyListValue::Boolean(true),
    );
    let driver = AppleBuildPath::Expression {
        expression: format!("$(BUILT_PRODUCTS_DIR)/{DRIVER_LIBRARY}"),
    };
    let mut target = AppleTarget {
        product_name: inputs.app_name.clone(),
        product_type: Some("com.apple.product-type.application".into()),
        info_plist: info,
        sources: vec![AppleSource {
            path: AppleBuildPath::Project(ProjectPath::new("Sources/AppDelegate.swift")?),
            platform_filters: vec![],
            compiler_flags: vec![],
        }],
        resources: vec![AppleResource::Process {
            path: ProjectPath::new("Resources/Assets.xcassets")?,
        }],
        build_settings: settings(&[
            ("CODE_SIGN_IDENTITY", "-"),
            ("COMBINE_HIDPI_IMAGES", "YES"),
            ("LIBRARY_SEARCH_PATHS", "$(inherited) $(BUILT_PRODUCTS_DIR)"),
            (
                "LD_RUNPATH_SEARCH_PATHS",
                "$(inherited) @executable_path/../Frameworks",
            ),
            ("PRODUCT_BUNDLE_IDENTIFIER", &inputs.bundle_id),
            ("PRODUCT_NAME", "$(TARGET_NAME)"),
            ("SDKROOT", "macosx"),
        ]),
        dependencies: vec![
            AppleDependency::SwiftProduct {
                package: "whisker_modules".into(),
                product: "WhiskerModules".into(),
                weak: false,
                platform_filters: vec![],
            },
            AppleDependency::BuildOutput {
                path: driver.clone(),
                weak: false,
                platform_filters: vec![],
            },
        ],
        embeds: vec![AppleEmbed {
            source: AppleEmbedSource::BuildOutput { path: driver },
            destination: ProjectPath::new("Frameworks")?,
            code_sign_on_copy: true,
            remove_headers_on_copy: false,
            platform_filters: vec![],
        }],
        scripts: vec![AppleBuildScript {
            name: "Whisker Build Rust App".into(),
            position: AppleScriptPosition::BeforeSources,
            shell: "/bin/bash".into(),
            script: render(include_str!("../templates/ios/build-rust.sh"), &vars)?,
            inputs: vec![],
            outputs: vec![AppleBuildPath::Expression {
                expression: format!("$(BUILT_PRODUCTS_DIR)/{DRIVER_LIBRARY}"),
            }],
            input_file_lists: vec![],
            output_file_lists: vec![],
            based_on_dependency_analysis: Some(false),
        }],
        ..Default::default()
    };
    let mut files = ProjectFiles::new();
    if let Some(png) = &inputs.app_icon_png {
        for (name, png) in icon::render(png)? {
            files.insert(
                ProjectPath::new(format!("Resources/AppIcon.iconset/{name}"))?,
                ProjectFile::Generated {
                    entry: FileEntry::binary(&png),
                },
            );
        }
        target.resources.push(AppleResource::Process {
            path: ProjectPath::new("Resources/AppIcon.iconset")?,
        });
        target.info_plist.insert(
            "CFBundleIconFile".into(),
            PropertyListValue::String("AppIcon".into()),
        );
    }
    for (path, template) in [
        ("Sources/AppDelegate.swift", APP_DELEGATE_SWIFT),
        (
            "Resources/Assets.xcassets/Contents.json",
            crate::ios::ASSET_CATALOG,
        ),
        (
            "Resources/Assets.xcassets/WhiskerBackground.colorset/Contents.json",
            crate::ios::BACKGROUND_COLORSET,
        ),
    ] {
        files.insert(
            ProjectPath::new(path)?,
            ProjectFile::Generated {
                entry: FileEntry::text(render(template, &vars)?),
            },
        );
    }
    for (path, entry) in crate::ios_modules::module_files(
        &inputs.modules,
        &inputs.workspace_root,
        crate::modules::ModulePlatform::Macos,
    ) {
        files.insert(path, ProjectFile::Generated { entry });
    }
    let mut debug = settings(&[
        ("DEBUG_INFORMATION_FORMAT", "dwarf"),
        ("ENABLE_TESTABILITY", "YES"),
        ("GCC_OPTIMIZATION_LEVEL", "0"),
        ("ONLY_ACTIVE_ARCH", "YES"),
        ("SWIFT_ACTIVE_COMPILATION_CONDITIONS", "DEBUG"),
        ("SWIFT_OPTIMIZATION_LEVEL", "-Onone"),
    ]);
    debug.insert(
        "GCC_PREPROCESSOR_DEFINITIONS".into(),
        AppleBuildSetting::List(vec!["DEBUG=1".into(), "$(inherited)".into()]),
    );
    Ok(MacosProjectIr {
        apple: AppleProjectIr {
            application: "app".into(),
            targets: [("app".into(), target)].into(),
            build_settings: settings(&[
                ("ALWAYS_SEARCH_USER_PATHS", "NO"),
                ("CLANG_ENABLE_MODULES", "YES"),
                ("CLANG_ENABLE_OBJC_ARC", "YES"),
                ("COPY_PHASE_STRIP", "NO"),
                ("GCC_C_LANGUAGE_STANDARD", "gnu11"),
                ("MACOSX_DEPLOYMENT_TARGET", &inputs.minimum_system_version),
                ("SDKROOT", "macosx"),
                ("SWIFT_VERSION", "5.9"),
            ]),
            configurations: [
                (
                    "Debug".into(),
                    AppleBuildConfiguration {
                        settings: debug,
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
                inputs.app_name.clone(),
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
            files,
        },
    })
}
impl crate::ProjectEngine {
    pub fn with_macos_application(inputs: MacosInputs) -> Self {
        let mut engine = Self::with_initializer(ApplicationPlugin::new(inputs));
        engine.register_mobile_builtins();
        engine
    }
}
