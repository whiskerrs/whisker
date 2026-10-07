//! Standard Cargo Host and macOS bundle declarations.
use super::*;
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
        let inputs = &self.inputs;
        validate(inputs)?;
        let mut target = AppleTarget {
            product_name: inputs.app_name.clone(),
            product_type: Some("com.apple.product-type.application".into()),
            rust: Some(RustBuild {
                manifest: ProjectPath::new("Cargo.toml")?,
                package: inputs.generated_package.clone(),
                target: inputs.generated_package.clone(),
                kind: RustArtifactKind::Bin,
                features: vec![],
                default_features: true,
            }),
            ..Default::default()
        };
        for (key, value) in [
            ("CFBundleDevelopmentRegion", "en"),
            ("CFBundleDisplayName", &inputs.app_name),
            ("CFBundleExecutable", &inputs.generated_package),
            ("CFBundleIdentifier", &inputs.bundle_id),
            ("CFBundleInfoDictionaryVersion", "6.0"),
            ("CFBundleName", &inputs.app_name),
            ("CFBundlePackageType", "APPL"),
            ("CFBundleShortVersionString", &inputs.version),
            ("CFBundleVersion", &inputs.build_number.to_string()),
            ("LSMinimumSystemVersion", &inputs.minimum_system_version),
            ("NSPrincipalClass", "NSApplication"),
        ] {
            target
                .info_plist
                .insert(key.into(), PropertyListValue::String(value.into()));
        }
        target.info_plist.insert(
            "NSHighResolutionCapable".into(),
            PropertyListValue::Boolean(true),
        );
        let vars = template_vars(inputs);
        let mut files = ProjectFiles::new();
        for (path, template) in [("Cargo.toml", CARGO_TOML), ("src/main.rs", MAIN_RS)] {
            files.insert(
                ProjectPath::new(path)?,
                ProjectFile::Generated {
                    entry: FileEntry::text(render(template, &vars)?),
                },
            );
        }
        if let Some(png) = &inputs.app_icon_png {
            for (name, png) in icon::render(png)? {
                files.insert(
                    ProjectPath::new(format!("AppIcon.iconset/{name}"))?,
                    ProjectFile::Generated {
                        entry: FileEntry::binary(&png),
                    },
                );
            }
            target.resources.push(AppleResource::Process {
                path: ProjectPath::new("AppIcon.iconset")?,
            });
            target.info_plist.insert(
                "CFBundleIconFile".into(),
                PropertyListValue::String("AppIcon.icns".into()),
            );
        }
        Ok(ProjectUpdate::Merge {
            project: Box::new(ProjectIr::Macos(MacosProjectIr {
                apple: AppleProjectIr {
                    application: "app".into(),
                    targets: [("app".into(), target)].into(),
                    files,
                    ..Default::default()
                },
            })),
        })
    }
}
impl crate::ProjectEngine {
    pub fn with_macos_application(inputs: MacosInputs) -> Self {
        let mut engine = Self::with_initializer(ApplicationPlugin::new(inputs));
        engine.register_mobile_builtins();
        engine
    }
}
