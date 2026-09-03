//! Render the Cargo browser Host project consumed by Whisker's Web builder.

use anyhow::{Context, Result, anyhow, bail};
use std::path::{Path, PathBuf};
use whisker_config::Config;

use crate::fingerprint;
use crate::render::render;

const CARGO_TOML: &str = include_str!("templates/web/Cargo.toml.template");
const LIB_RS: &str = include_str!("templates/web/src/lib.rs");
const INDEX_HTML: &str = include_str!("templates/web/index.html");

/// Fully resolved inputs for one generated Web project.
#[derive(Clone, Debug, serde::Serialize)]
pub struct WebInputs {
    /// Browser document title.
    pub app_name: String,
    /// Static document background configured in `whisker.rs` (`#RRGGBB`).
    pub background: String,
    /// Cargo package name of the generated WASM composition root.
    pub generated_package: String,
    /// Cargo package name of the user's application crate.
    pub user_package: String,
    /// Absolute path to the user's application crate.
    pub user_crate_path: PathBuf,
    /// Complete Cargo dependency declaration for `whisker-web`.
    pub whisker_web_dependency: String,
    /// Discovered external element definitions for Web.
    pub element_modules: Vec<crate::RustElementModuleInput>,
    /// Bumped whenever the generated project shape changes.
    pub template_version: u32,
}

/// Resolves browser project fields from application config and Cargo metadata.
pub fn inputs_from(
    app_config: &Config,
    user_package: String,
    user_crate_path: PathBuf,
    whisker_web_dependency: String,
) -> Result<WebInputs> {
    let app_name = app_config
        .name
        .clone()
        .ok_or_else(|| anyhow!("whisker.rs: app.name(\"…\") is required for Web"))?;
    let background = crate::background::AppBackground::resolve(app_config)?;
    Ok(WebInputs {
        app_name,
        background: background.hex().to_string(),
        generated_package: format!("{user_package}-whisker-web"),
        user_package,
        user_crate_path,
        whisker_web_dependency,
        element_modules: Vec::new(),
        template_version: 12,
    })
}

/// Generates or reuses the complete `gen/web` project.
pub fn sync(out_dir: &Path, inputs: &WebInputs) -> Result<bool> {
    validate(inputs)?;
    let bytes = serde_json::to_vec(inputs).context("serialize WebInputs for fingerprint")?;
    let new_fingerprint = fingerprint::fingerprint(&bytes);
    let fingerprint_path = out_dir.join(".whisker-fingerprint");
    if std::fs::read_to_string(&fingerprint_path).is_ok_and(|value| value.trim() == new_fingerprint)
    {
        return Ok(false);
    }

    clean_managed_tree(out_dir)?;
    let vars = template_vars(inputs);
    write_text(
        &out_dir.join("Cargo.toml"),
        &render(CARGO_TOML, &vars).context("render Web Cargo.toml")?,
    )?;
    write_text(
        &out_dir.join("src/lib.rs"),
        &render(LIB_RS, &vars).context("render Web lib.rs")?,
    )?;
    write_text(
        &out_dir.join("index.html"),
        &render(INDEX_HTML, &vars).context("render Web index.html")?,
    )?;
    write_text(&fingerprint_path, &new_fingerprint)?;
    Ok(true)
}

fn validate(inputs: &WebInputs) -> Result<()> {
    if inputs.app_name.trim().is_empty() {
        bail!("Web app name must not be empty");
    }
    if inputs.generated_package.trim().is_empty() || inputs.user_package.trim().is_empty() {
        bail!("Web Cargo package names must not be empty");
    }
    crate::background::AppBackground::parse(&inputs.background)
        .context("validate Web application background")?;
    Ok(())
}

fn template_vars(inputs: &WebInputs) -> std::collections::HashMap<&'static str, String> {
    let mut vars = std::collections::HashMap::new();
    vars.insert("app_name_html", html_escape(&inputs.app_name));
    vars.insert("background_css", inputs.background.clone());
    vars.insert("app_title_rust", format!("{:?}", inputs.app_name));
    vars.insert("generated_package", inputs.generated_package.clone());
    vars.insert("user_package_toml", format!("{:?}", inputs.user_package));
    vars.insert(
        "user_crate_path_toml",
        format!("{:?}", inputs.user_crate_path.display().to_string()),
    );
    vars.insert(
        "whisker_web_dependency",
        inputs.whisker_web_dependency.clone(),
    );
    vars.insert(
        "element_module_dependencies",
        crate::rust_element_module_dependencies(&inputs.element_modules),
    );
    vars.insert(
        "element_module_config",
        crate::rust_element_module_config(&inputs.element_modules),
    );
    vars
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn clean_managed_tree(out_dir: &Path) -> Result<()> {
    if !out_dir.exists() {
        return Ok(());
    }
    for entry in
        std::fs::read_dir(out_dir).with_context(|| format!("read {}", out_dir.display()))?
    {
        let path = entry?.path();
        if path.file_name().is_some_and(|name| name == "dist") {
            continue;
        }
        if path.is_dir() {
            std::fs::remove_dir_all(&path)
                .with_context(|| format!("remove generated directory {}", path.display()))?;
        } else {
            std::fs::remove_file(&path)
                .with_context(|| format!("remove generated file {}", path.display()))?;
        }
    }
    Ok(())
}

fn write_text(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    std::fs::write(path, contents).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn tempdir() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "whisker-cng-web-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn sample() -> WebInputs {
        WebInputs {
            app_name: "Hello Web".into(),
            background: "#FFFFFF".into(),
            generated_package: "hello-whisker-web".into(),
            user_package: "hello".into(),
            user_crate_path: PathBuf::from("/tmp/hello"),
            whisker_web_dependency: "{ path = \"/tmp/whisker/platforms/web\" }".into(),
            element_modules: Vec::new(),
            template_version: 12,
        }
    }

    #[test]
    fn writes_complete_web_project_and_reuses_fingerprint() {
        let root = tempdir();
        let out = root.join("gen/web");
        assert!(sync(&out, &sample()).unwrap());
        for path in [
            "Cargo.toml",
            "src/lib.rs",
            "index.html",
            ".whisker-fingerprint",
        ] {
            assert!(out.join(path).exists(), "missing {path}");
        }
        assert!(!out.join("Trunk.toml").exists());
        assert!(!sync(&out, &sample()).unwrap());
        let manifest = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
        assert!(manifest.contains("package = \"hello\""));
        assert!(manifest.contains("whisker-web"));
        let html = std::fs::read_to_string(out.join("index.html")).unwrap();
        assert!(html.contains("<title>Hello Web</title>"));
        assert!(html.contains("background: #FFFFFF"));
        assert!(html.contains("import init, * as whisker"));
        assert!(html.contains("from \"/whisker_app.js\""));
        assert!(html.contains("__WHISKER_DEVELOPMENT_BOOTSTRAP__"));
        assert!(!html.contains("new WebSocket"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn inputs_and_generated_document_use_static_background() {
        let mut config = Config::default();
        config.name("Background").background("#101018");
        let inputs = inputs_from(
            &config,
            "background".into(),
            PathBuf::from("/tmp/background"),
            "\"0.12\"".into(),
        )
        .unwrap();
        assert_eq!(inputs.background, "#101018");

        let root = tempdir();
        let out = root.join("gen/web");
        sync(&out, &inputs).unwrap();
        let html = std::fs::read_to_string(out.join("index.html")).unwrap();
        assert!(html.contains("background: #101018"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn generated_host_wires_discovered_web_module_definitions() {
        let root = tempdir();
        let out = root.join("gen/web");
        let mut inputs = sample();
        inputs.element_modules.push(crate::RustElementModuleInput {
            package: "whisker-example".into(),
            crate_path: PathBuf::from("/modules/whisker-example"),
            host_package: "whisker-example-web".into(),
            host_dependency: crate::RustHostDependency::Path(PathBuf::from(
                "/modules/whisker-example/web",
            )),
        });
        sync(&out, &inputs).unwrap();
        let manifest = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
        assert!(manifest.contains("whisker-example = { package = \"whisker-example\""));
        let source = std::fs::read_to_string(out.join("src/lib.rs")).unwrap();
        assert!(manifest.contains("whisker-example-web ="));
        assert!(!source.contains("#[path ="));
        assert!(source.contains(".with_module("));
        assert!(source.contains("whisker_example_web::__whisker_module_definition()"));
        std::fs::remove_dir_all(root).ok();
    }
}
