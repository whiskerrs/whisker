//! Render the complete Cargo-based macOS Host project under `gen/macos/`.
//!
//! Unlike iOS, macOS does not require an Xcode project to create a native app.
//! The generated project is nevertheless a complete platform project shared by
//! both `whisker run desktop` and `whisker build macos`: it owns the Cargo
//! composition root, bundle metadata, entitlements, and resources.

use anyhow::{Context, Result, anyhow, bail};
use std::path::{Path, PathBuf};
use whisker_config::Config;

use crate::fingerprint;
use crate::render::render;

const CARGO_TOML: &str = include_str!("templates/macos/Cargo.toml");
const MAIN_RS: &str = include_str!("templates/macos/src/main.rs");
const INFO_PLIST: &str = include_str!("templates/macos/Info.plist");
const ENTITLEMENTS: &str = include_str!("templates/macos/Entitlements.plist");

/// Fully resolved inputs for one generated macOS project.
#[derive(Clone, Debug, serde::Serialize)]
pub struct MacosInputs {
    /// Human-readable application name and `.app` bundle name.
    pub app_name: String,
    /// Reverse-DNS bundle identifier.
    pub bundle_id: String,
    /// User-visible semantic version.
    pub version: String,
    /// Monotonic bundle build number.
    pub build_number: u32,
    /// Cargo package name of the generated Host executable.
    pub generated_package: String,
    /// Cargo package name of the user's application crate.
    pub user_package: String,
    /// Absolute path to the user's application crate.
    pub user_crate_path: PathBuf,
    /// Complete Cargo dependency declaration for `whisker-macos`.
    pub whisker_macos_dependency: String,
    /// Complete Cargo dependency declaration for the shared Desktop Host API.
    pub whisker_desktop_dependency: String,
    /// Discovered external element definitions for Desktop.
    pub element_modules: Vec<crate::RustElementModuleInput>,
    /// Minimum supported macOS release placed in `Info.plist`.
    pub minimum_system_version: String,
    /// Bumped whenever the generated project shape changes.
    pub template_version: u32,
}

/// Generates or reuses the complete `gen/macos` project.
pub fn sync(out_dir: &Path, inputs: &MacosInputs) -> Result<bool> {
    validate(inputs)?;
    let bytes = serde_json::to_vec(inputs).context("serialize MacosInputs for fingerprint")?;
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
        &render(CARGO_TOML, &vars).context("render macOS Cargo.toml")?,
    )?;
    write_text(
        &out_dir.join("src/main.rs"),
        &render(MAIN_RS, &vars).context("render macOS main.rs")?,
    )?;
    write_text(
        &out_dir.join("Info.plist"),
        &render(INFO_PLIST, &vars).context("render macOS Info.plist")?,
    )?;
    write_text(&out_dir.join("Entitlements.plist"), ENTITLEMENTS)?;
    std::fs::create_dir_all(out_dir.join("Resources"))
        .with_context(|| format!("create {}/Resources", out_dir.display()))?;
    write_text(&fingerprint_path, &new_fingerprint)?;
    Ok(true)
}

/// Resolves macOS fields from the declarative application config.
pub fn inputs_from(
    app_config: &Config,
    user_package: String,
    user_crate_path: PathBuf,
    whisker_macos_dependency: String,
) -> Result<MacosInputs> {
    let app_name = app_config
        .name
        .clone()
        .ok_or_else(|| anyhow!("whisker.rs: app.name(\"…\") is required for macOS"))?;
    let bundle_id = app_config
        .bundle_id
        .clone()
        .ok_or_else(|| anyhow!("whisker.rs: app.bundle_id(\"…\") is required for macOS"))?;
    let version = app_config
        .version
        .clone()
        .unwrap_or_else(|| "0.1.0".to_string());
    let build_number = app_config.build_number.unwrap_or(1);
    let generated_package = format!("{}-whisker-macos", user_package);
    Ok(MacosInputs {
        app_name,
        bundle_id,
        version,
        build_number,
        generated_package,
        user_package,
        user_crate_path,
        whisker_macos_dependency,
        whisker_desktop_dependency: format!("{:?}", env!("CARGO_PKG_VERSION")),
        element_modules: Vec::new(),
        minimum_system_version: "12.0".to_string(),
        template_version: 8,
    })
}

fn validate(inputs: &MacosInputs) -> Result<()> {
    if inputs.app_name.is_empty() || inputs.app_name.contains(['/', ':']) {
        bail!("macOS app name must be non-empty and contain neither '/' nor ':'");
    }
    if inputs.bundle_id.trim().is_empty() {
        bail!("macOS bundle id must not be empty");
    }
    if inputs.generated_package.trim().is_empty() || inputs.user_package.trim().is_empty() {
        bail!("macOS Cargo package names must not be empty");
    }
    Ok(())
}

fn template_vars(inputs: &MacosInputs) -> std::collections::HashMap<&'static str, String> {
    let mut vars = std::collections::HashMap::new();
    vars.insert("app_name", xml_escape(&inputs.app_name));
    vars.insert("app_title_rust", rust_string(&inputs.app_name));
    vars.insert("bundle_id", xml_escape(&inputs.bundle_id));
    vars.insert("version", xml_escape(&inputs.version));
    vars.insert("build_number", inputs.build_number.to_string());
    vars.insert("generated_package", inputs.generated_package.clone());
    vars.insert("user_package_toml", toml_string(&inputs.user_package));
    vars.insert(
        "user_crate_path_toml",
        toml_string(&inputs.user_crate_path.display().to_string()),
    );
    vars.insert(
        "whisker_macos_dependency",
        inputs.whisker_macos_dependency.clone(),
    );
    vars.insert(
        "whisker_desktop_dependency",
        inputs.whisker_desktop_dependency.clone(),
    );
    vars.insert(
        "element_module_dependencies",
        crate::rust_element_module_dependencies(&inputs.element_modules),
    );
    vars.insert(
        "element_module_config",
        crate::rust_element_module_config(&inputs.element_modules),
    );
    vars.insert(
        "minimum_system_version",
        xml_escape(&inputs.minimum_system_version),
    );
    vars
}

fn toml_string(value: &str) -> String {
    format!("{value:?}")
}

fn rust_string(value: &str) -> String {
    format!("{value:?}")
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn clean_managed_tree(out_dir: &Path) -> Result<()> {
    if !out_dir.exists() {
        return Ok(());
    }
    for entry in
        std::fs::read_dir(out_dir).with_context(|| format!("read {}", out_dir.display()))?
    {
        let path = entry?.path();
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
            "whisker-cng-macos-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn sample() -> MacosInputs {
        MacosInputs {
            app_name: "Hello Mac".into(),
            bundle_id: "rs.whisker.hello".into(),
            version: "1.2.3".into(),
            build_number: 7,
            generated_package: "hello-whisker-macos".into(),
            user_package: "hello".into(),
            user_crate_path: PathBuf::from("/tmp/hello"),
            whisker_macos_dependency: "{ path = \"/tmp/whisker/platforms/macos\" }".into(),
            whisker_desktop_dependency: "{ path = \"/tmp/whisker/platforms/desktop\" }".into(),
            element_modules: Vec::new(),
            minimum_system_version: "12.0".into(),
            template_version: 3,
        }
    }

    #[test]
    fn writes_complete_cargo_host_project_and_reuses_fingerprint() {
        let root = tempdir();
        let out = root.join("gen/macos");
        assert!(sync(&out, &sample()).unwrap());
        for path in [
            "Cargo.toml",
            "src/main.rs",
            "Info.plist",
            "Entitlements.plist",
            "Resources",
            ".whisker-fingerprint",
        ] {
            assert!(out.join(path).exists(), "missing {path}");
        }
        assert!(!sync(&out, &sample()).unwrap());
        let manifest = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
        assert!(manifest.contains("package = \"hello\""));
        assert!(manifest.contains("whisker-macos = { path ="));
        assert!(manifest.contains("hot-reload = [\"whisker-macos/hot-reload\"]"));
        assert!(manifest.contains("[profile.dev.package.\"*\"]\nopt-level = 2"));
        assert!(manifest.contains("[profile.dev.package.\"hello\"]\nopt-level = 0"));
        let main = std::fs::read_to_string(out.join("src/main.rs")).unwrap();
        assert!(main.contains("whisker_app::__whisker_application"));
        assert!(main.contains("whisker_app::__whisker_application_hash"));
        assert!(main.contains("run_with_application_hash"));
        assert!(!main.contains("{{"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn extracts_required_fields_from_config() {
        let mut config = Config::default();
        config
            .name("Desktop")
            .bundle_id("rs.whisker.desktop")
            .version("2.0.0")
            .build_number(9);
        let inputs = inputs_from(
            &config,
            "demo".into(),
            PathBuf::from("/app"),
            "\"0.12\"".into(),
        )
        .unwrap();
        assert_eq!(inputs.app_name, "Desktop");
        assert_eq!(inputs.generated_package, "demo-whisker-macos");
        assert_eq!(inputs.build_number, 9);
    }

    #[test]
    fn generated_host_wires_discovered_desktop_module_definitions() {
        let root = tempdir();
        let out = root.join("gen/macos");
        let mut inputs = sample();
        inputs.element_modules.push(crate::RustElementModuleInput {
            package: "whisker-toggle".into(),
            crate_path: PathBuf::from("/modules/whisker-toggle"),
            host_package: "whisker-toggle-desktop-host".into(),
            host_dependency: crate::RustHostDependency::Path(PathBuf::from(
                "/modules/whisker-toggle/desktop",
            )),
        });
        sync(&out, &inputs).unwrap();
        let manifest = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
        assert!(manifest.contains("whisker-toggle = { package = \"whisker-toggle\""));
        let main = std::fs::read_to_string(out.join("src/main.rs")).unwrap();
        assert!(manifest.contains("whisker-toggle-desktop-host ="));
        assert!(!main.contains("#[path ="));
        assert!(main.contains(
            ".with_element_module(whisker_toggle::__whisker_element_module_definition())"
        ));
        assert!(main.contains(
            ".with_module_definition(whisker_toggle_desktop_host::__whisker_module_definition())"
        ));
        std::fs::remove_dir_all(root).ok();
    }
}
