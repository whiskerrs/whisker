//! Standard browser Host, document, and Cargo integration declarations.
use super::*;
use std::collections::BTreeMap;
use whisker_plugin::{FileEntry, PluginConfig, project::*};

/// Standard Web application plugin, initialized with resolved app/module inputs.
pub struct ApplicationPlugin {
    inputs: WebInputs,
}
/// App settings are supplied through WebInputs, not a second override map.
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationPluginConfig {}
impl PluginConfig for ApplicationPluginConfig {
    const NAME: &'static str = ApplicationPlugin::NAME;
}
impl ApplicationPlugin {
    /// Reserved identity of the standard Web initializer.
    pub const NAME: &'static str = "whisker-web-application";
    pub fn new(inputs: WebInputs) -> Self {
        Self { inputs }
    }
}
impl ProjectPlugin for ApplicationPlugin {
    type Config = ApplicationPluginConfig;
    fn contribute(&self, ctx: &ProjectContext, _: &Self::Config) -> Result<ProjectUpdate> {
        if !matches!(ctx.project, ProjectIr::Web(_)) {
            return Ok(ProjectUpdate::Keep);
        }
        Ok(ProjectUpdate::Merge {
            project: Box::new(ProjectIr::Web(Box::new(declarations(&self.inputs)?))),
        })
    }
}
fn element(
    id: &str,
    name: &str,
    attributes: &[(&str, Option<&str>)],
    text: Option<String>,
) -> WebHtmlContribution {
    WebHtmlContribution {
        id: id.into(),
        element: HtmlElement {
            name: name.into(),
            attributes: attributes
                .iter()
                .map(|(k, v)| ((*k).into(), v.map(str::to_owned)))
                .collect(),
            children: text.into_iter().map(HtmlNode::Text).collect(),
        },
    }
}
fn declarations(inputs: &WebInputs) -> Result<WebProjectIr> {
    validate(inputs)?;
    let vars = template_vars(inputs);
    let mut head = vec![
        element("charset", "meta", &[("charset", Some("utf-8"))], None),
        element(
            "viewport",
            "meta",
            &[
                ("name", Some("viewport")),
                ("content", Some("width=device-width, initial-scale=1")),
            ],
            None,
        ),
        element(
            "router-base",
            "meta",
            &[
                ("name", Some("whisker-base-path")),
                ("content", Some(&inputs.base_path)),
            ],
            None,
        ),
        element(
            "background",
            "style",
            &[],
            Some(format!(
                "html, body, #whisker-root {{ width: 100%; height: 100%; margin: 0; background: {}; }}",
                inputs.background
            )),
        ),
    ];
    if let Some(url) = &inputs.favicon_data_url {
        head.push(element(
            "favicon",
            "link",
            &[("rel", Some("icon")), ("href", Some(url))],
            None,
        ));
    }
    let mut files = BTreeMap::new();
    for (path, template) in [("Cargo.toml", CARGO_TOML), ("src/lib.rs", LIB_RS)] {
        files.insert(
            ProjectPath::new(path)?,
            ProjectFile::Generated {
                entry: FileEntry::text(render(template, &vars)?),
            },
        );
    }
    Ok(WebProjectIr {
        wasm: Some(RustBuild {
            manifest: ProjectPath::new("Cargo.toml")?,
            package: inputs.generated_package.clone(),
            target: inputs.generated_package.replace('-', "_"),
            kind: RustArtifactKind::Cdylib,
            features: vec![],
            default_features: true,
        }),
        document: Some(ProjectPath::new("index.html")?),
        generated_artifacts: [
            ("js".into(), ProjectPath::new("whisker_app.js")?),
            ("wasm".into(), ProjectPath::new("whisker_app_bg.wasm")?),
        ]
        .into(),
        title: inputs.app_name.clone(),
        lang: "en".into(),
        base_path: inputs.base_path.clone(),
        head,
        body: vec![
            element("mount", "div", &[("id", Some("whisker-root"))], None),
            element(
                "development-error",
                "pre",
                &[("id", Some("whisker-dev-error")), ("hidden", None)],
                None,
            ),
            element(
                "bootstrap",
                "script",
                &[("type", Some("module"))],
                Some(render(
                    include_str!("../templates/web/bootstrap.js"),
                    &vars,
                )?),
            ),
        ],
        files,
        ..Default::default()
    })
}
impl crate::ProjectEngine {
    /// Compose the browser application first, then dependency project plugins.
    pub fn with_web_application(inputs: WebInputs) -> Self {
        let mut engine = Self::with_initializer(ApplicationPlugin::new(inputs));
        engine.register_mobile_builtins();
        engine
    }
}
