#![cfg(feature = "generate")]
use std::{collections::BTreeMap, path::PathBuf};
use whisker_cng::{Config, ProjectEngine, web::*};
use whisker_plugin::{FileEntry, PluginConfig, project::*};
fn root() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let p = std::env::temp_dir().join(format!(
        "whisker-web-ir-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn inputs() -> WebInputs {
    let mut config = Config::default();
    config.name("Web Test").web(|web| {
        web.base_path("/app/");
    });
    inputs_from(
        &config,
        "test-app".into(),
        "/tmp/test-app".into(),
        "\"0.14\"".into(),
    )
    .unwrap()
}
fn base() -> WebProjectInputs {
    let result = ProjectEngine::with_web_application(inputs())
        .compose(&Config::default(), &ProjectIr::Web(Box::default()))
        .unwrap();
    let ProjectIr::Web(project) = result.project else {
        panic!()
    };
    WebProjectInputs {
        project: *project,
        app_crate_dir: None,
        cargo_selection: Default::default(),
        template_version: 16,
    }
}
fn path(v: &str) -> ProjectPath {
    ProjectPath::new(v).unwrap()
}
fn text(files: &BTreeMap<ProjectPath, FileEntry>, name: &str) -> String {
    String::from_utf8(files[&path(name)].to_bytes().unwrap()).unwrap()
}
fn resource(project: &mut WebProjectIr, source: &str, dest: &str, content: &str) {
    project.files.insert(
        path(source),
        ProjectFile::Generated {
            entry: FileEntry::text(content),
        },
    );
    project.resources.push(Resource {
        source: path(source),
        destination: path(dest),
        kind: ResourceKind::File,
    });
}
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Options;
impl PluginConfig for Options {
    const NAME: &'static str = "a-web-feature";
}
struct Feature;
impl ProjectPlugin for Feature {
    type Config = Options;
    fn contribute(&self, ctx: &ProjectContext, _: &Options) -> anyhow::Result<ProjectUpdate> {
        let ProjectIr::Web(original) = &ctx.project else {
            return Ok(ProjectUpdate::Keep);
        };
        assert!(original.wasm.is_some());
        let mut project = original.clone();
        project.title = "Edited <Web> & title".into();
        resource(
            &mut project,
            "input/data.json",
            "data.json",
            "{\"ok\":true}",
        );
        Ok(ProjectUpdate::Replace {
            project: Box::new(ProjectIr::Web(project)),
            reason: "set feature document title and data".into(),
        })
    }
}
#[test]
fn application_and_feature_plugins_determine_the_final_document_and_distribution() {
    let mut engine = ProjectEngine::with_web_application(inputs());
    engine.register(Feature);
    let result = engine
        .compose(&Config::default(), &ProjectIr::Web(Box::default()))
        .unwrap();
    let ProjectIr::Web(project) = result.project else {
        panic!()
    };
    let input = WebProjectInputs {
        project: *project,
        ..base()
    };
    let files = render_project(&input).unwrap();
    let html = text(&files, "index.html");
    assert!(html.contains("<title>Edited &lt;Web&gt; &amp; title</title>"));
    assert!(html.contains("from \"/app/whisker_app.js\""));
    assert!(text(&files, "src/lib.rs").contains("document.title()"));
    let out = root();
    sync_project(&out, &input).unwrap();
    let plan = load_build_plan(&out).unwrap();
    let dist = distribution_files(&out, &plan).unwrap();
    assert_eq!(text(&dist, "data.json"), "{\"ok\":true}");
    assert!(!dist.contains_key(&path("Cargo.toml")));
    assert_eq!(files, render_project(&input).unwrap());
    std::fs::remove_dir_all(out).unwrap();
}
#[test]
fn manifests_workers_and_directory_resources_reach_the_distribution() {
    let mut input = base();
    resource(
        &mut input.project,
        "workers/main.js",
        "sw.js",
        "self.addEventListener('fetch', () => {});",
    );
    input.project.files.insert(
        path("assets/icon.svg"),
        ProjectFile::Generated {
            entry: FileEntry::text("<svg/>"),
        },
    );
    input.project.resources.push(Resource {
        source: path("assets"),
        destination: path("public"),
        kind: ResourceKind::Directory,
    });
    input.project.manifest = Some(WebAppManifest {
        path: path("metadata/app.webmanifest"),
        crossorigin: Some(WebCrossOrigin::UseCredentials),
        properties: [
            ("name".into(), serde_json::json!("PWA")),
            (
                "share_target".into(),
                serde_json::json!({"action":"share","method":"POST"}),
            ),
        ]
        .into(),
    });
    input.project.service_workers.insert(
        "main".into(),
        ServiceWorker {
            script: path("sw.js"),
            scope: Some("/app/".into()),
            update_via_cache: Some(ServiceWorkerUpdateViaCache::None),
            kind: ServiceWorkerKind::Module,
        },
    );
    let out = root();
    sync_project(&out, &input).unwrap();
    let dist = distribution_files(&out, &load_build_plan(&out).unwrap()).unwrap();
    assert_eq!(text(&dist, "public/icon.svg"), "<svg/>");
    assert!(text(&dist, "metadata/app.webmanifest").contains("share_target"));
    let html = text(&dist, "index.html");
    assert!(
        html.contains("href=\"/app/metadata/app.webmanifest\" crossorigin=\"use-credentials\"")
    );
    assert!(html.contains("register(\"/app/sw.js\""));
    assert!(html.contains("\"updateViaCache\":\"none\""));
    input.project.files.remove(&path("workers/main.js"));
    assert!(render_project(&input).is_err());
    std::fs::remove_dir_all(out).unwrap();
}
#[test]
fn html_preserves_text_raw_script_boolean_attributes_and_body_order() {
    let mut input = base();
    let item = |id: &str, tag: &str, content: &str| WebHtmlContribution {
        id: id.into(),
        element: HtmlElement {
            name: tag.into(),
            attributes: BTreeMap::new(),
            children: vec![HtmlNode::Text(content.into())],
        },
    };
    input
        .project
        .body_before
        .push(item("before", "pre", "\n<before>&"));
    input.project.body_after.push(item(
        "after",
        "script",
        "if (1 < 2 && true) console.log('ok');",
    ));
    let html = text(&render_project(&input).unwrap(), "index.html");
    assert!(html.contains("<pre>\n\n&lt;before&gt;&amp;</pre>"));
    assert!(html.contains("if (1 < 2 && true)"));
    assert!(html.contains(" hidden ") && !html.contains("hidden=\""));
    assert!(html.find("&lt;before&gt;").unwrap() < html.find("id=\"whisker-root\"").unwrap());
    input
        .project
        .body_after
        .push(item("bad", "script", "const s='<!--<script>';"));
    assert!(render_project(&input).is_err());
}
#[test]
fn file_bytes_invalidate_cache_and_invalid_plans_leave_prior_output_intact() {
    let out = root();
    let app = root();
    std::fs::write(app.join("data"), "one").unwrap();
    let mut input = base();
    input.app_crate_dir = Some(app.clone());
    input.project.files.insert(
        path("input/data"),
        ProjectFile::AppFile {
            source: path("data"),
        },
    );
    input.project.resources.push(Resource {
        source: path("input/data"),
        destination: path("data"),
        kind: ResourceKind::File,
    });
    assert!(sync_project(&out, &input).unwrap());
    assert!(!sync_project(&out, &input).unwrap());
    std::fs::write(app.join("data"), "two").unwrap();
    assert!(sync_project(&out, &input).unwrap());
    let stamp = std::fs::read(out.join(".whisker-fingerprint")).unwrap();
    input
        .project
        .generated_artifacts
        .insert("unsupported".into(), path("worker.wasm"));
    assert!(sync_project(&out, &input).is_err());
    assert_eq!(
        std::fs::read(out.join(".whisker-fingerprint")).unwrap(),
        stamp
    );
    std::fs::remove_dir_all(out).unwrap();
    std::fs::remove_dir_all(app).unwrap();
}
#[test]
fn backend_rejects_unsupported_headers_and_unowned_build_outputs() {
    let mut input = base();
    input.project.response_headers.push(WebResponseHeaders {
        id: "headers".into(),
        scope: WebResponseScope::All,
        headers: [("x-test".into(), "yes".into())].into(),
    });
    assert!(
        render_project(&input)
            .unwrap_err()
            .to_string()
            .contains("hosting adapter")
    );
    input.project.response_headers.clear();
    input.project.wasm.as_mut().unwrap().target = "other".into();
    assert!(
        render_project(&input)
            .unwrap_err()
            .to_string()
            .contains("target does not match")
    );
    let mut input = base();
    resource(&mut input.project, "file", "snippets/a.js", "bad");
    assert!(
        render_project(&input)
            .unwrap_err()
            .to_string()
            .contains("reserved")
    );
}

#[test]
fn build_plan_rejects_nonadjacent_parent_and_child_outputs() {
    let out = root();
    sync_project(&out, &base()).unwrap();
    let mut plan = load_build_plan(&out).unwrap();
    for name in ["a", "a-b", "a/c"] {
        plan.distribution.insert(path(name), path("index.html"));
    }
    std::fs::write(
        out.join(".whisker/web-build.json"),
        serde_json::to_vec(&plan).unwrap(),
    )
    .unwrap();
    assert!(
        load_build_plan(&out)
            .unwrap_err()
            .to_string()
            .contains("overlapping")
    );
    std::fs::remove_dir_all(out).unwrap();
}
