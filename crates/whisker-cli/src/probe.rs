//! Evaluate `whisker.rs` without compiling the application library.
//!
//! Cargo-registered configuration binaries provide their own `main`. Files
//! without a binary target use the legacy `configure(&mut Config)` adapter.
//! Both run in a generated package with only configuration and discovered
//! plugin dependencies. Results are cached under `target/.whisker/` until the
//! configuration or application manifest changes.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use whisker_cng::{DiscoveredPlugin, ProjectDependencyGraph};
use whisker_config::Config;

/// Run the probe and return the parsed config. Caches via mtime so
/// the second call (and later) returns instantly until `whisker.rs`
/// changes.
///
/// `crate_name` is used to name the probe binary (so the temp `target/`
/// doesn't collide if the user happens to also have a probe-shaped
/// crate of their own).
pub fn run(whisker_rs: &Path, crate_dir: &Path, crate_name: &str) -> Result<Config> {
    let user_manifest = crate_dir.join("Cargo.toml");
    let cache = crate_dir.join("target/.whisker/config-cache.json");
    if cache_is_fresh(&cache, &[whisker_rs, user_manifest.as_path()]) {
        let json = std::fs::read_to_string(&cache)
            .with_context(|| format!("read cache {}", cache.display()))?;
        return serde_json::from_str(&json)
            .with_context(|| format!("parse cached config {}", cache.display()));
    }
    // The probe needs every plugin crate as a dependency to name its
    // `Plugin` impl; see `render_plugin_dep_lines` for how they're
    // spelled.
    let plugins = ProjectDependencyGraph::resolve(&user_manifest, crate_name)
        .with_context(|| format!("resolve Whisker dependencies for `{crate_name}`"))?
        .cng_plugins;

    // Carry over the user crate's own `[patch.crates-io]` table, if
    // any — see `read_patch_crates_io_table`'s doc comment for why.
    let patch_table = read_patch_crates_io_table(&user_manifest)?;

    let has_main = is_config_binary(&user_manifest, whisker_rs)?;
    let probe_dir = crate_dir.join("target/.whisker/config-probe");
    write_probe_project(
        &probe_dir,
        whisker_rs,
        crate_name,
        &plugins,
        patch_table.as_deref(),
        has_main,
    )?;
    let json = run_cargo_probe(&probe_dir, crate_name)?;
    if let Some(parent) = cache.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    std::fs::write(&cache, &json).with_context(|| format!("write cache {}", cache.display()))?;
    serde_json::from_str(&json).with_context(|| "parse probe stdout as Config JSON")
}

/// The user crate's own `[patch.crates-io]` table, if any, re-rendered
/// as a standalone TOML snippet ready to append to the probe's
/// `Cargo.toml`.
///
/// Without it, an app that patches its `whisker-*` deps to a local
/// checkout still gets the probe's own `whisker-config` from
/// crates.io, so the probe drifts behind the app's real dependency
/// graph and rejects any `whisker.rs` API added since the last
/// release.
fn read_patch_crates_io_table(user_manifest: &Path) -> Result<Option<String>> {
    let contents = std::fs::read_to_string(user_manifest)
        .with_context(|| format!("read {}", user_manifest.display()))?;
    let doc: toml::Value = contents
        .parse()
        .with_context(|| format!("parse {} as TOML", user_manifest.display()))?;
    let Some(patch) = doc.get("patch") else {
        return Ok(None);
    };
    let mut table = toml::value::Table::new();
    table.insert("patch".to_string(), patch.clone());
    let rendered = toml::to_string(&table).context("serialize [patch] table")?;
    Ok(Some(rendered))
}

fn is_config_binary(user_manifest: &Path, whisker_rs: &Path) -> Result<bool> {
    let contents = std::fs::read_to_string(user_manifest)
        .with_context(|| format!("read {}", user_manifest.display()))?;
    let doc: toml::Value = contents.parse().context("parse application manifest")?;
    let config_path = whisker_rs
        .canonicalize()
        .context("resolve whisker.rs path")?;
    let crate_dir = user_manifest.parent().context("manifest has no parent")?;
    Ok(doc
        .get("bin")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|bin| bin.get("path").and_then(toml::Value::as_str))
        .any(|path| crate_dir.join(path).canonicalize().ok().as_ref() == Some(&config_path)))
}

fn cache_is_fresh(cache: &Path, sources: &[&Path]) -> bool {
    let Ok(cache_mtime) = std::fs::metadata(cache).and_then(|m| m.modified()) else {
        return false;
    };
    // Comparing `SystemTime` directly rather than via
    // `duration_since`, which errors on pre-`UNIX_EPOCH` clock skew.
    for source in sources {
        let Ok(src_mtime) = std::fs::metadata(source).and_then(|m| m.modified()) else {
            // Source missing → conservative: regenerate.
            return false;
        };
        if src_mtime > cache_mtime {
            return false;
        }
    }
    true
}

/// Write the probe manifest and, for legacy configurations, its entry point.
fn write_probe_project(
    probe_dir: &Path,
    whisker_rs: &Path,
    crate_name: &str,
    plugins: &[DiscoveredPlugin],
    patch_table: Option<&str>,
    has_main: bool,
) -> Result<()> {
    let src_dir = probe_dir.join("src");
    std::fs::create_dir_all(&src_dir).with_context(|| format!("mkdir {}", src_dir.display()))?;

    let probe_crate_name = format!("__whisker_config_probe_{}", crate_name.replace('-', "_"));
    let plugin_dep_lines = render_plugin_dep_lines(plugins);
    let bin_path = if has_main {
        whisker_rs
            .canonicalize()
            .context("resolve whisker.rs path")?
    } else {
        PathBuf::from("src/main.rs")
    };
    let cargo_toml = format!(
        r#"# Auto-generated by whisker-cli (do not edit).
[package]
name = "{probe_crate_name}"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
whisker-config = {whisker_config_dep}
serde_json = "1"
{plugin_dep_lines}
[[bin]]
name = "{probe_crate_name}"
path = {bin_path}

# Avoid leaking workspace inheritance: every published-config field
# the parent workspace sets (rust-version, license, …) would have to
# match here. Stand-alone keeps the probe immune to workspace churn.
[workspace]

{patch_table}
"#,
        bin_path = toml::Value::String(bin_path.to_string_lossy().into_owned()),
        whisker_config_dep = whisker_config_dep_spec(),
        patch_table = patch_table.unwrap_or_default(),
    );
    std::fs::write(probe_dir.join("Cargo.toml"), cargo_toml)
        .with_context(|| format!("write {}/Cargo.toml", probe_dir.display()))?;

    if !has_main {
        let main_rs = format!(
            r#"include!({whisker_rs:?});

fn main() {{
    let mut cfg = whisker_config::Config::default();
    configure(&mut cfg);
    serde_json::to_writer(std::io::stdout().lock(), &cfg).expect("serialize Config");
}}
"#,
            whisker_rs = whisker_rs.canonicalize()?.to_string_lossy(),
        );
        std::fs::write(src_dir.join("main.rs"), main_rs)
            .with_context(|| format!("write {}/src/main.rs", src_dir.display()))?;
    }

    Ok(())
}

fn run_cargo_probe(probe_dir: &Path, _crate_name: &str) -> Result<String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(&cargo)
        .arg("run")
        .arg("--quiet")
        .arg("--release")
        .arg("--manifest-path")
        .arg(probe_dir.join("Cargo.toml"))
        .output()
        .with_context(|| format!("spawn cargo run for probe at {}", probe_dir.display()))?;
    if !out.status.success() {
        anyhow::bail!(
            "config probe build/run failed (exit {})\nstderr:\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr),
        );
    }
    String::from_utf8(out.stdout).context("probe stdout not valid UTF-8")
}

/// Format each discovered Whisker CNG plugin crate as a probe
/// `[dependencies]` line:
///
/// ```toml
/// whisker-audio = { path = "...", default-features = false }
/// ```
///
/// `default-features = false` is critical — plugin crates by
/// convention put their heavyweight runtime behind a `runtime`
/// feature so the probe build stays cheap. The probe only needs
/// the `cng` module exposing `Plugin` + `Config` types.
fn render_plugin_dep_lines(plugins: &[DiscoveredPlugin]) -> String {
    if plugins.is_empty() {
        return String::new();
    }
    // One crate can ship several plugins; list it once.
    let mut seen = std::collections::BTreeSet::new();
    let mut out = String::new();
    for p in plugins {
        if !seen.insert(p.source_crate.as_str()) {
            continue;
        }
        out.push_str(&format!(
            "{} = {{ path = \"{}\", default-features = false }}\n",
            p.source_crate,
            p.source_manifest_dir.display(),
        ));
    }
    out
}

/// The `whisker-config` dependency spec the probe's `Cargo.toml`
/// should use.
///
/// Two cases, distinguished by whether the local source dir exists:
///
///   * **In-workspace development** (this `whisker-cli` was built from
///     a checkout of the Whisker monorepo): point the probe at the
///     local `crates/whisker-config` source via `path` so edits to
///     `whisker-config` are picked up without a publish/version bump.
///   * **External users** (this `whisker-cli` was installed from
///     crates.io): the local path doesn't exist, so depend on the
///     published `whisker-config` whose version matches this
///     `whisker-cli` build. `whisker-config` shares the workspace
///     version with `whisker-cli`, so `CARGO_PKG_VERSION` is the
///     correct, in-lockstep version to request from crates.io.
fn whisker_config_dep_spec() -> String {
    match in_workspace_config_path() {
        Some(path) => format!("{{ path = {:?} }}", path.display().to_string()),
        None => format!("\"{}\"", env!("CARGO_PKG_VERSION")),
    }
}

/// The local `crates/whisker-config` source dir, if this `whisker-cli`
/// was built from a monorepo checkout. `CARGO_MANIFEST_DIR` is baked
/// at compile time: in-workspace it's `<workspace>/crates/whisker-cli`
/// (sibling `whisker-config` exists); installed from crates.io it's
/// the registry `src/.../whisker-cli-<v>` dir (no sibling
/// `whisker-config` dir), so this returns `None`.
fn in_workspace_config_path() -> Option<PathBuf> {
    let cli_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let app_config = cli_dir.parent()?.join("whisker-config");
    app_config.is_dir().then_some(app_config)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestApp(PathBuf);

    impl TestApp {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("whisker-config-bin-test-{}", std::process::id()));
            std::fs::create_dir_all(path.join("src")).unwrap();
            Self(path)
        }

        fn write(&self, path: &str, contents: &str) {
            std::fs::write(self.0.join(path), contents).unwrap();
        }

        fn evaluate(&self) -> Config {
            let _ = std::fs::remove_file(self.0.join("target/.whisker/config-cache.json"));
            run(&self.0.join("whisker.rs"), &self.0, "config-test-app").unwrap()
        }

        fn cargo(&self, args: &[&str]) -> std::process::Output {
            Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
                .args(args)
                .current_dir(&self.0)
                .env("CARGO_NET_OFFLINE", "true")
                .output()
                .unwrap()
        }
    }

    impl Drop for TestApp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn configuration_binary_preserves_probe_isolation_and_cargo_add_flow() {
        let app = TestApp::new();
        let manifest = format!(
            r#"[package]
name = "config-test-app"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
whisker-config = {}
"#,
            whisker_config_dep_spec(),
        );
        app.write("Cargo.toml", &manifest);
        app.write(
            "src/lib.rs",
            "compile_error!(\"application must not be built\");",
        );
        app.write(
            "whisker.rs",
            "pub fn configure(app: &mut whisker_config::Config) { app.name(\"Legacy\"); }",
        );
        assert_eq!(app.evaluate().name.as_deref(), Some("Legacy"));

        let manifest = format!(
            r#"{manifest}
[[bin]]
name = "custom-config-name"
path = "./whisker.rs"
required-features = ["whisker-config"]
test = false
bench = false
[features]
whisker-config = []
"#,
        );
        app.write("Cargo.toml", &manifest);
        app.write(
            "whisker.rs",
            r#"//! Application configuration.
fn main() {
    whisker_config::run(|app| {
        app.name("Main").bundle_id("test.config");
        app.android(|android| { android.min_sdk(24); });
    });
}
"#,
        );
        let config = app.evaluate();
        assert_eq!(config.name.as_deref(), Some("Main"));
        assert_eq!(config.android.min_sdk, Some(24));

        let gated = app.cargo(&["run", "--bin", "custom-config-name"]);
        assert!(!gated.status.success());
        assert!(String::from_utf8_lossy(&gated.stderr).contains("requires the features"));

        let asset_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/whisker-asset")
            .canonicalize()
            .unwrap();
        let added = app.cargo(&[
            "add",
            "whisker-asset",
            "--path",
            asset_path.to_str().unwrap(),
        ]);
        assert!(
            added.status.success(),
            "{}",
            String::from_utf8_lossy(&added.stderr)
        );
        app.write(
            "whisker.rs",
            r#"fn main() {
    whisker_config::run(|app| {
        app.name("Assets");
        app.plugin::<whisker_asset::WhiskerAsset>(|assets| { assets.dir("assets"); });
    });
}
"#,
        );
        let config = app.evaluate();
        assert_eq!(config.plugins["whisker-asset"]["dirs"][0], "assets");

        app.write("src/lib.rs", "pub fn application() {}");
        let direct = app.cargo(&[
            "run",
            "--quiet",
            "--bin",
            "custom-config-name",
            "--features",
            "whisker-config",
        ]);
        assert!(
            direct.status.success(),
            "{}",
            String::from_utf8_lossy(&direct.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&direct.stdout).unwrap(),
            serde_json::to_value(config).unwrap(),
        );

        app.write(
            "whisker.rs",
            "compile_error!(\"config must be skipped\"); fn main() {}",
        );
        let default_build = app.cargo(&["build", "--quiet"]);
        assert!(
            default_build.status.success(),
            "{}",
            String::from_utf8_lossy(&default_build.stderr)
        );
        let library_build = app.cargo(&[
            "rustc",
            "--quiet",
            "--crate-type",
            "cdylib",
            "--",
            "-C",
            "debuginfo=0",
        ]);
        assert!(
            library_build.status.success(),
            "{}",
            String::from_utf8_lossy(&library_build.stderr)
        );
    }
}
