//! Serialize final Web declarations and describe their distribution to the builder.
use super::*;
use crate::project_files::{check_destination, insert, stage_declared};
use anyhow::ensure;
use std::collections::BTreeMap;
use whisker_plugin::{FileEntry, project::*};
const PLAN_PATH: &str = ".whisker/web-build.json";
const OUTPUT_ROOT: &str = ".whisker/web-output";

#[derive(Debug, Clone, serde::Serialize)]
pub struct WebProjectInputs {
    pub project: WebProjectIr,
    pub app_crate_dir: Option<PathBuf>,
    pub cargo_selection: crate::CargoSelection,
    pub template_version: u32,
}
/// Versioned CNG/build handoff. Sources and destinations are project-relative paths.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebBuildPlan {
    pub version: u32,
    pub wasm: RustBuild,
    pub document: ProjectPath,
    pub generated_artifacts: BTreeMap<String, ProjectPath>,
    pub base_path: String,
    /// Distribution destination -> staged source; only these files are published.
    pub distribution: BTreeMap<ProjectPath, ProjectPath>,
}
impl WebBuildPlan {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "unsupported Web build plan version; regenerate gen/web"
        );
        ensure!(
            self.wasm.kind == RustArtifactKind::Cdylib,
            "Web requires a cdylib"
        );
        ensure!(
            self.document.as_str() == "index.html",
            "Web driver requires index.html"
        );
        ensure!(
            self.generated_artifacts
                == [
                    ("js".into(), ProjectPath::new("whisker_app.js")?),
                    ("wasm".into(), ProjectPath::new("whisker_app_bg.wasm")?)
                ]
                .into(),
            "Web driver supports only js=whisker_app.js and wasm=whisker_app_bg.wasm"
        );
        ensure!(
            self.distribution.contains_key(&self.document),
            "Web plan must distribute the document"
        );
        let mut paths = std::collections::BTreeSet::new();
        for path in self
            .distribution
            .keys()
            .chain(self.generated_artifacts.values())
        {
            ensure!(
                paths.insert(path.as_str()),
                "duplicate Web distribution output"
            );
        }
        for path in &paths {
            for (index, _) in path.match_indices('/') {
                ensure!(
                    !paths.contains(&path[..index]),
                    "overlapping Web distribution outputs"
                );
            }
        }
        // wasm-bindgen may also emit snippets/ and worker helper files. It owns
        // this namespace even when a particular build does not use it.
        for path in self.distribution.keys() {
            ensure!(
                !path.as_str().starts_with("snippets/") && path.as_str() != "snippets",
                "snippets is reserved for wasm-bindgen"
            );
        }
        Ok(())
    }
}
pub fn load_build_plan(project_dir: &Path) -> Result<WebBuildPlan> {
    let bytes = std::fs::read(project_dir.join(PLAN_PATH))
        .context("read Web build plan; regenerate gen/web")?;
    let plan: WebBuildPlan = serde_json::from_slice(&bytes)?;
    plan.validate()?;
    Ok(plan)
}
/// Read and validate all static distribution bytes before the builder cleans dist.
pub fn distribution_files(
    project_dir: &Path,
    plan: &WebBuildPlan,
) -> Result<BTreeMap<ProjectPath, FileEntry>> {
    plan.validate()?;
    let declared = plan
        .distribution
        .iter()
        .map(|(dest, source)| {
            (
                dest.clone(),
                ProjectFile::AppFile {
                    source: source.clone(),
                },
            )
        })
        .collect();
    let mut files = BTreeMap::new();
    stage_declared(&mut files, &declared, Some(project_dir))?;
    crate::project_files::validate(&files)?;
    Ok(files)
}
pub fn sync_project(out_dir: &Path, inputs: &WebProjectInputs) -> Result<bool> {
    let files = render_project(inputs)?;
    let fp = crate::fingerprint::fingerprint(&serde_json::to_vec(&(inputs, &files, 1u32))?);
    let stamp = out_dir.join(".whisker-fingerprint");
    if std::fs::read_to_string(&stamp).is_ok_and(|s| s.trim() == fp) {
        return Ok(false);
    }
    for path in files.keys() {
        check_destination(out_dir, &out_dir.join(path.as_str()))?;
    }
    clean_managed_tree(out_dir)?;
    for (path, entry) in files {
        let path = out_dir.join(path.as_str());
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, entry.to_bytes()?)?;
        #[cfg(unix)]
        if let Some(mode) = entry.mode {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))?;
        }
    }
    write_text(&stamp, &fp)?;
    Ok(true)
}
pub fn render_project(inputs: &WebProjectInputs) -> Result<BTreeMap<ProjectPath, FileEntry>> {
    let web = &inputs.project;
    ProjectIr::Web(Box::new(web.clone())).validate_structure()?;
    ensure!(
        web.response_headers.is_empty(),
        "Web response_headers require a hosting adapter; the current static backend cannot apply them"
    );
    let mut files = BTreeMap::new();
    stage_declared(&mut files, &web.files, inputs.app_crate_dir.as_deref())?;
    for path in files.keys() {
        ensure!(
            !matches!(
                path.as_str().split('/').next(),
                Some(".whisker" | "dist" | "target")
            ),
            "reserved Web staging path: {}",
            path.as_str()
        );
    }
    let mut distribution = BTreeMap::new();
    let doc = web.document.as_ref().unwrap();
    insert(&mut files, doc.clone(), FileEntry::text(document(web)?))?;
    distribution.insert(doc.clone(), doc.clone());
    for resource in &web.resources {
        match resource.kind {
            ResourceKind::File => {
                ensure!(
                    files.contains_key(&resource.source),
                    "missing staged Web resource {}",
                    resource.source.as_str()
                );
                ensure!(
                    distribution
                        .insert(resource.destination.clone(), resource.source.clone())
                        .is_none(),
                    "duplicate distribution resource"
                );
            }
            ResourceKind::Directory => {
                let prefix = format!("{}/", resource.source.as_str());
                let mut found = false;
                for source in files.keys().filter(|p| p.as_str().starts_with(&prefix)) {
                    let path = ProjectPath::new(format!(
                        "{}/{}",
                        resource.destination.as_str(),
                        &source.as_str()[prefix.len()..]
                    ))?;
                    ensure!(
                        distribution.insert(path, source.clone()).is_none(),
                        "duplicate distribution resource"
                    );
                    found = true;
                }
                ensure!(
                    found,
                    "missing or empty staged Web resource directory {}",
                    resource.source.as_str()
                );
            }
        }
    }
    if let Some(manifest) = &web.manifest {
        let source = ProjectPath::new(format!("{OUTPUT_ROOT}/{}", manifest.path.as_str()))?;
        insert(
            &mut files,
            source.clone(),
            FileEntry::text(serde_json::to_string_pretty(&manifest.properties)?),
        )?;
        distribution.insert(manifest.path.clone(), source);
    }
    for worker in web.service_workers.values() {
        ensure!(
            distribution.contains_key(&worker.script),
            "service worker file is not present in the distribution: {}",
            worker.script.as_str()
        );
    }
    let wasm = web.wasm.as_ref().unwrap();
    let cargo_file = files
        .get(&wasm.manifest)
        .context("Web Cargo manifest must be staged")?;
    let cargo: toml::Value = toml::from_str(std::str::from_utf8(&cargo_file.to_bytes()?)?)?;
    ensure!(
        cargo
            .get("package")
            .and_then(|v| v.get("name"))
            .and_then(|v| v.as_str())
            == Some(&wasm.package),
        "Web Cargo package does not match wasm declaration"
    );
    let default_target = wasm.package.replace('-', "_");
    let target = cargo
        .get("lib")
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or(&default_target);
    ensure!(
        target == wasm.target
            && !target.is_empty()
            && target
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "Web Cargo library target does not match wasm declaration"
    );
    let plan = WebBuildPlan {
        version: 1,
        wasm: wasm.clone(),
        document: doc.clone(),
        generated_artifacts: web.generated_artifacts.clone(),
        base_path: web.base_path.clone(),
        distribution,
    };
    plan.validate()?;
    insert(
        &mut files,
        ProjectPath::new(PLAN_PATH)?,
        FileEntry::text(serde_json::to_string_pretty(&plan)?),
    )?;
    crate::project_files::validate(&files)?;
    Ok(files)
}
fn attrs(attributes: &BTreeMap<String, Option<String>>) -> Result<String> {
    ensure!(
        attributes.values().flatten().all(|v| !v.contains('\0')),
        "HTML attribute contains a null character"
    );
    Ok(attributes
        .iter()
        .map(|(name, value)| match value {
            Some(value) => format!(" {name}=\"{}\"", html_escape(value)),
            None => format!(" {name}"),
        })
        .collect())
}
fn element(e: &HtmlElement) -> Result<String> {
    let name = e.name.to_ascii_lowercase();
    ensure!(
        !matches!(
            name.as_str(),
            "html" | "head" | "body" | "plaintext" | "noscript" | "svg" | "math"
        ),
        "unsupported HTML element context: {name}"
    );
    let mut out = format!("<{}{}>", e.name, attrs(&e.attributes)?);
    if [
        "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
        "source", "track", "wbr",
    ]
    .contains(&name.as_str())
    {
        return Ok(out);
    }
    let raw = matches!(
        name.as_str(),
        "script" | "style" | "xmp" | "iframe" | "noembed" | "noframes"
    );
    let mut children = String::new();
    for child in &e.children {
        match child {
            HtmlNode::Text(text) => {
                ensure!(!text.contains('\0'), "HTML text contains a null character");
                children.push_str(&if raw { text.clone() } else { html_escape(text) });
            }
            HtmlNode::Element(child) => {
                ensure!(
                    !raw && name != "textarea",
                    "text-only HTML element has element children"
                );
                children.push_str(&element(child)?);
            }
        }
    }
    if raw {
        ensure!(
            !children.to_ascii_lowercase().contains(&format!("</{name}")),
            "raw-text closing tag in HTML"
        );
        ensure!(
            name != "script" || !children.contains("<!--"),
            "inline script HTML comment syntax is unsupported; use an external script"
        );
    }
    // The HTML parser strips one initial LF in these elements.
    if matches!(name.as_str(), "pre" | "textarea" | "listing") && children.starts_with('\n') {
        out.push('\n');
    }
    out.push_str(&children);
    out.push_str(&format!("</{}>", e.name));
    Ok(out)
}
fn document(web: &WebProjectIr) -> Result<String> {
    ensure!(
        !web.title.contains('\0') && !web.lang.contains('\0'),
        "Web title/lang contains a null character"
    );
    let mut out = format!(
        "<!doctype html>\n<html lang=\"{}\"{}>\n<head>\n",
        html_escape(&web.lang),
        attrs(&web.html_attributes)?
    );
    // Keep charset declarations early even when the document title is long.
    for item in &web.head {
        out.push_str(&element(&item.element)?);
        out.push('\n');
    }
    out.push_str(&format!("<title>{}</title>\n", html_escape(&web.title)));
    if let Some(manifest) = &web.manifest {
        out.push_str(&format!(
            "<link rel=\"manifest\" href=\"{}\"{}>\n",
            html_escape(&web.output_url(&manifest.path)?),
            match manifest.crossorigin {
                None => "",
                Some(WebCrossOrigin::Anonymous) => " crossorigin=\"anonymous\"",
                Some(WebCrossOrigin::UseCredentials) => " crossorigin=\"use-credentials\"",
            }
        ));
    }
    out.push_str("</head>\n<body>\n");
    for item in web
        .body_before
        .iter()
        .chain(&web.body)
        .chain(&web.body_after)
    {
        out.push_str(&element(&item.element)?);
        out.push('\n');
    }
    if !web.service_workers.is_empty() {
        out.push_str("<script>\nif ('serviceWorker' in navigator) {\n");
        for worker in web.service_workers.values() {
            let mut options = serde_json::Map::new();
            options.insert(
                "type".into(),
                serde_json::json!(match worker.kind {
                    ServiceWorkerKind::Classic => "classic",
                    ServiceWorkerKind::Module => "module",
                }),
            );
            if let Some(scope) = &worker.scope {
                options.insert("scope".into(), scope.clone().into());
            }
            if let Some(cache) = worker.update_via_cache {
                options.insert(
                    "updateViaCache".into(),
                    serde_json::json!(match cache {
                        ServiceWorkerUpdateViaCache::All => "all",
                        ServiceWorkerUpdateViaCache::Imports => "imports",
                        ServiceWorkerUpdateViaCache::None => "none",
                    }),
                );
            }
            out.push_str(&format!("navigator.serviceWorker.register({}, {}).catch(error => console.error('Service worker registration failed', error));\n", serde_json::to_string(&web.output_url(&worker.script)?)?, serde_json::to_string(&options)?));
        }
        out.push_str("}\n</script>\n");
    }
    out.push_str("</body>\n</html>\n");
    Ok(out)
}
