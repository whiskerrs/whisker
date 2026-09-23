//! Browser build pipeline shared by `whisker build web` and the dev server.
//!
//! This module owns exactly the Web artifacts Whisker consumes: compile the
//! CNG-generated composition crate, run wasm-bindgen, and stage a static
//! directory. It intentionally is not a general-purpose asset bundler.

use crate::{CaptureShims, Profile};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub const TARGET: &str = "wasm32-unknown-unknown";
pub const OUTPUT_STEM: &str = "whisker_app";
const DEVELOPMENT_MARKER: &str = "// __WHISKER_DEVELOPMENT_BOOTSTRAP__";
const DEVELOPMENT_BOOTSTRAP: &str = r#"const protocol = location.protocol === "https:" ? "wss:" : "ws:";
      const connect = () => {
        const socket = new WebSocket(`${protocol}//${location.host}/whisker-dev`);
        socket.binaryType = "arraybuffer";
        socket.addEventListener("open", () => {
          socket.send(JSON.stringify({ kind: "hello", aslr_reference: 0 }));
        });
        socket.addEventListener("message", (event) => {
          if (typeof event.data === "string") {
            const message = JSON.parse(event.data);
            if (message.kind === "reload") location.reload();
            return;
          }
          const frame = new Uint8Array(event.data);
          const headerLength = Number(new DataView(
            frame.buffer,
            frame.byteOffset,
            8,
          ).getBigUint64(0));
          const header = new TextDecoder().decode(frame.subarray(8, 8 + headerLength));
          whisker.__whisker_apply_hot_patch(header, frame.subarray(8 + headerLength));
        });
        socket.addEventListener("close", () => setTimeout(connect, 500));
      };
      connect();"#;

#[derive(Debug, Clone)]
pub struct WebBuild {
    pub project_dir: PathBuf,
    pub target_dir: PathBuf,
    pub dist_dir: PathBuf,
    pub package: String,
    pub profile: Profile,
    pub features: Vec<String>,
    pub capture: Option<CaptureShims>,
    /// Inject the WebSocket patch/reload client into the staged HTML.
    pub development: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebArtifacts {
    /// wasm-ld output before wasm-bindgen rewrites its imports.
    pub raw_wasm: PathBuf,
    pub javascript: PathBuf,
    pub wasm: PathBuf,
    pub index_html: PathBuf,
}

pub fn build(config: &WebBuild) -> Result<WebArtifacts> {
    let raw_wasm = compile(config)?;
    bindgen(config, &raw_wasm)
}

/// Compile the generated composition crate without transforming the result.
/// Hot Reload preprocesses this artifact before calling [`bindgen`].
pub fn compile(config: &WebBuild) -> Result<PathBuf> {
    let plan = whisker_cng::web::load_build_plan(&config.project_dir)?;
    anyhow::ensure!(
        plan.wasm.package == config.package,
        "Web package differs from CNG build plan; regenerate gen/web"
    );
    std::fs::create_dir_all(&config.target_dir)
        .with_context(|| format!("create {}", config.target_dir.display()))?;
    std::fs::create_dir_all(&config.dist_dir)
        .with_context(|| format!("create {}", config.dist_dir.display()))?;

    let mut command = std::process::Command::new("cargo");
    command
        .arg("build")
        .arg("--manifest-path")
        .arg(config.project_dir.join(plan.wasm.manifest.as_str()))
        .args(["--target", TARGET])
        .arg("--target-dir")
        .arg(&config.target_dir)
        .arg("--package")
        .arg(&plan.wasm.package)
        .arg("--lib")
        .current_dir(&config.project_dir);
    if let Some(flag) = config.profile.cargo_flag() {
        command.arg(flag);
    }
    if !plan.wasm.default_features {
        command.arg("--no-default-features");
    }
    let features: Vec<_> = plan.wasm.features.iter().chain(&config.features).collect();
    if !features.is_empty() {
        command
            .arg("--features")
            .arg(features.into_iter().cloned().collect::<Vec<_>>().join(","));
    }
    if let Some(capture) = &config.capture {
        std::fs::create_dir_all(&capture.rustc_cache_dir).with_context(|| {
            format!("create rustc capture {}", capture.rustc_cache_dir.display())
        })?;
        std::fs::create_dir_all(&capture.linker_cache_dir).with_context(|| {
            format!(
                "create linker capture {}",
                capture.linker_cache_dir.display()
            )
        })?;
        command.envs(crate::capture_env_vars_all_crates(capture));
    }
    let compile_step = crate::ui::step(
        crate::ui::OperationKind::Compile,
        format!("{} ({TARGET})", config.package),
    );
    let status = compile_step
        .pipe(&mut command)
        .context("spawn Cargo Web build")?;
    if !status.success() {
        compile_step.fail(status.to_string());
        anyhow::bail!("Cargo Web build exited with {status}");
    }
    compile_step.done("");

    let crate_stem = &plan.wasm.target;
    let raw_wasm = config
        .target_dir
        .join(TARGET)
        .join(config.profile.dir_name())
        .join(format!("{crate_stem}.wasm"));
    if !raw_wasm.is_file() {
        anyhow::bail!(
            "Cargo succeeded but WebAssembly output is missing at {}",
            raw_wasm.display()
        );
    }

    Ok(raw_wasm)
}

/// Run wasm-bindgen and stage the static document around `raw_wasm`.
pub fn bindgen(config: &WebBuild, raw_wasm: &Path) -> Result<WebArtifacts> {
    let package_step = crate::ui::step(crate::ui::OperationKind::Package, "wasm-bindgen");
    match bindgen_inner(config, raw_wasm) {
        Ok(artifacts) => {
            package_step.done("");
            Ok(artifacts)
        }
        Err(error) => {
            package_step.fail("wasm-bindgen failed");
            Err(error)
        }
    }
}

fn bindgen_inner(config: &WebBuild, raw_wasm: &Path) -> Result<WebArtifacts> {
    let plan = whisker_cng::web::load_build_plan(&config.project_dir)?;
    let mut files = whisker_cng::web::distribution_files(&config.project_dir, &plan)?;
    let document = files
        .remove(&plan.document)
        .context("Web document missing from distribution")?;
    let html = String::from_utf8(document.to_bytes()?)?;
    anyhow::ensure!(
        html.contains(DEVELOPMENT_MARKER),
        "Web document is missing the Whisker development marker; regenerate gen/web"
    );
    if config.dist_dir.exists() {
        std::fs::remove_dir_all(&config.dist_dir)
            .with_context(|| format!("clean {}", config.dist_dir.display()))?;
    }
    std::fs::create_dir_all(&config.dist_dir)
        .with_context(|| format!("create {}", config.dist_dir.display()))?;
    let hot_reload = config.capture.is_some();
    let mut bindgen = wasm_bindgen_cli_support::Bindgen::new();
    bindgen
        .input_path(raw_wasm)
        .out_name(OUTPUT_STEM)
        .typescript(false)
        .debug(config.profile == Profile::Debug)
        // Match wasm-bindgen's CLI behavior: `init()` resolves the colocated
        // `_bg.wasm` module when the caller does not pass an explicit URL.
        .omit_default_module_path(false)
        // The fat module is also the reference used to resolve future side
        // modules. Preserve its linker exports and stable symbol/name data;
        // a release/non-hot build remains free to strip them normally.
        .keep_lld_exports(hot_reload)
        .keep_debug(hot_reload)
        .remove_name_section(!hot_reload)
        .remove_producers_section(!hot_reload)
        .demangle(false);
    bindgen
        .web(true)
        .context("configure wasm-bindgen Web target")?;
    bindgen
        .generate(&config.dist_dir)
        .context("run wasm-bindgen for Web Host")?;

    for (path, entry) in files {
        let path = config.dist_dir.join(path.as_str());
        anyhow::ensure!(
            !path.exists(),
            "static resource collides with wasm-bindgen output: {}",
            path.display()
        );
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, entry.to_bytes()?)?;
        #[cfg(unix)]
        if let Some(mode) = entry.mode {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))?;
        }
    }
    let index_html = config.dist_dir.join(plan.document.as_str());
    let bootstrap = if config.development {
        DEVELOPMENT_BOOTSTRAP
    } else {
        ""
    };
    std::fs::write(&index_html, html.replace(DEVELOPMENT_MARKER, bootstrap))
        .with_context(|| format!("stage {}", index_html.display()))?;
    Ok(WebArtifacts {
        raw_wasm: raw_wasm.to_path_buf(),
        javascript: config.dist_dir.join(format!("{OUTPUT_STEM}.js")),
        wasm: config.dist_dir.join(format!("{OUTPUT_STEM}_bg.wasm")),
        index_html,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_names_are_stable_and_do_not_leak_cargo_package_names() {
        assert_eq!(OUTPUT_STEM, "whisker_app");
        assert_eq!(TARGET, "wasm32-unknown-unknown");
    }

    #[test]
    fn development_bootstrap_is_not_part_of_release_html() {
        let source = format!("before {DEVELOPMENT_MARKER} after");
        assert!(!source.replace(DEVELOPMENT_MARKER, "").contains("WebSocket"));
        assert!(
            source
                .replace(DEVELOPMENT_MARKER, DEVELOPMENT_BOOTSTRAP)
                .contains("WebSocket")
        );
    }

    #[test]
    fn bindgen_publishes_declared_resources_and_keeps_development_code_out_of_release() {
        let root =
            std::env::temp_dir().join(format!("whisker-web-distribution-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".whisker")).unwrap();
        let plan = r#"{
            "version":1,
            "wasm":{"manifest":"Cargo.toml","package":"fixture","target":"fixture","kind":"cdylib","features":[],"default_features":true},
            "document":"index.html", "base_path":"/app/",
            "generated_artifacts":{"js":"whisker_app.js","wasm":"whisker_app_bg.wasm"},
            "distribution":{"index.html":"index.html","data/nested.txt":"input.txt","sw.js":"sw.js"}
        }"#;
        std::fs::write(root.join(".whisker/web-build.json"), plan).unwrap();
        std::fs::write(
            root.join("index.html"),
            format!("<script>{DEVELOPMENT_MARKER}</script>"),
        )
        .unwrap();
        std::fs::write(root.join("input.txt"), "published").unwrap();
        std::fs::write(root.join("sw.js"), "// worker").unwrap();
        std::fs::write(root.join("private.txt"), "not distributed").unwrap();
        let wasm = root.join("input.wasm");
        std::fs::write(&wasm, b"\0asm\x01\0\0\0").unwrap();
        let mut config = WebBuild {
            project_dir: root.clone(),
            target_dir: root.join("target"),
            dist_dir: root.join("dist"),
            package: "fixture".into(),
            profile: Profile::Debug,
            features: vec![],
            capture: None,
            development: false,
        };
        let artifacts = bindgen(&config, &wasm).unwrap();
        assert!(artifacts.javascript.is_file() && artifacts.wasm.is_file());
        assert_eq!(
            std::fs::read_to_string(config.dist_dir.join("data/nested.txt")).unwrap(),
            "published"
        );
        assert!(config.dist_dir.join("sw.js").is_file());
        assert!(!config.dist_dir.join("private.txt").exists());
        assert!(
            !std::fs::read_to_string(&artifacts.index_html)
                .unwrap()
                .contains("WebSocket")
        );
        std::fs::write(config.dist_dir.join("stale.txt"), "old").unwrap();
        config.development = true;
        bindgen(&config, &wasm).unwrap();
        assert!(!config.dist_dir.join("stale.txt").exists());
        assert!(
            std::fs::read_to_string(&artifacts.index_html)
                .unwrap()
                .contains("WebSocket")
        );
        std::fs::remove_file(root.join("input.txt")).unwrap();
        assert!(bindgen(&config, &wasm).is_err());
        assert!(
            artifacts.index_html.is_file(),
            "preflight must preserve the old dist on missing input"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
