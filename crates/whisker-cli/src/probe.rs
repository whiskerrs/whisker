//! Run a tiny probe binary that includes the user's `whisker.rs` and
//! emits the resulting `Config` as JSON on stdout.
//!
//! ## Why a probe binary?
//!
//! `whisker.rs` is regular Rust source — there's no parser we could
//! point at the file. The user's `configure` function can do
//! arbitrary computation (env lookups, conditional fields, etc.), so
//! the only way to know what it *means* is to execute it.
//!
//! The probe is a one-file `cargo run` that:
//!   1. `include!("path/to/whisker.rs")` — splices the user's
//!      `configure` fn into our tiny `main`.
//!   2. Calls `configure(&mut Config::default())`.
//!   3. Serializes the populated config to JSON and prints it.
//!
//! Compile cost is single-digit seconds the first time (just
//! `whisker-config` + `serde_json`), and the result is cached
//! under `target/.whisker/config-cache.json`. The cache is
//! invalidated by mtime: re-running the probe is a no-op unless
//! `whisker.rs` was touched since the cache write.
//!
//! ## Why not include the umbrella `whisker` crate?
//!
//! `whisker-config` is intentionally a small, dependency-light
//! crate so the probe build is cheap. Pulling in `whisker`
//! (umbrella) would also pull `whisker-runtime`, `whisker-driver`,
//! Lynx headers, etc. — turning the probe into a multi-minute build.
//! The user crate's `whisker.rs` therefore writes
//! `use whisker_config::Config` directly, not `use
//! whisker::Config`.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use whisker_cng::{DiscoveredPlugin, discover_plugins};
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
    // Discover the user app's Whisker CNG plugin deps so the probe
    // can import each plugin's `Plugin` impl by name. Each gets
    // added to the probe's Cargo.toml with `default-features = false`
    // so the runtime-heavy parts of the plugin crate don't get
    // built (the convention is plugin crates gate their runtime
    // behind a `runtime` feature; the probe only needs the `cng`
    // module).
    let plugins = discover_plugins(&user_manifest, crate_name)
        .with_context(|| format!("discover Whisker CNG plugins for `{crate_name}`"))?;

    let probe_dir = crate_dir.join("target/.whisker/config-probe");
    write_probe_project(&probe_dir, whisker_rs, crate_name, &plugins)?;
    let json = run_cargo_probe(&probe_dir, crate_name)?;
    if let Some(parent) = cache.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    std::fs::write(&cache, &json).with_context(|| format!("write cache {}", cache.display()))?;
    serde_json::from_str(&json).with_context(|| "parse probe stdout as Config JSON")
}

fn cache_is_fresh(cache: &Path, sources: &[&Path]) -> bool {
    let Ok(cache_mtime) = std::fs::metadata(cache).and_then(|m| m.modified()) else {
        return false;
    };
    // Invalidate the cache when ANY of the watched sources is
    // newer than the cache. Today that's `whisker.rs` plus the
    // user crate's `Cargo.toml` (so adding/removing a plugin dep
    // forces a probe rebuild). `SystemTime` already implements
    // `PartialOrd` — direct comparison covers both the happy path
    // and the post-`UNIX_EPOCH` clock-skew edge case that
    // `duration_since` would `Err` on.
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

/// Write the probe's `Cargo.toml` + `src/main.rs`. Both files are
/// idempotent: we rewrite them on every cache-miss so an out-of-tree
/// edit (or a probe-dir delete) self-heals.
fn write_probe_project(
    probe_dir: &Path,
    whisker_rs: &Path,
    crate_name: &str,
    plugins: &[DiscoveredPlugin],
) -> Result<()> {
    let src_dir = probe_dir.join("src");
    std::fs::create_dir_all(&src_dir).with_context(|| format!("mkdir {}", src_dir.display()))?;

    let probe_crate_name = format!("__whisker_config_probe_{}", crate_name.replace('-', "_"));
    let plugin_dep_lines = render_plugin_dep_lines(plugins);
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
path = "src/main.rs"

# Avoid leaking workspace inheritance: every published-config field
# the parent workspace sets (rust-version, license, …) would have to
# match here. Stand-alone keeps the probe immune to workspace churn.
[workspace]
"#,
        whisker_config_dep = whisker_config_dep_spec(),
    );
    std::fs::write(probe_dir.join("Cargo.toml"), cargo_toml)
        .with_context(|| format!("write {}/Cargo.toml", probe_dir.display()))?;

    // `include!` takes an absolute path; we use display() so the path
    // is a normal forward-slashed string (Windows isn't supported by
    // the dev server, so display() is safe).
    let main_rs = format!(
        r#"// Auto-generated by whisker-cli (do not edit).
//
// This probe binary splices the user's `whisker.rs` into a `main`
// that prints the resulting `Config` as JSON on stdout. The
// host shell (`whisker run`) parses that JSON and projects the
// fields it needs into a flat `whisker_dev_server::Config`.

include!({whisker_rs:?});

fn main() {{
    let mut cfg = whisker_config::Config::default();
    configure(&mut cfg);
    let stdout = std::io::stdout();
    serde_json::to_writer(stdout.lock(), &cfg).expect("serialize Config");
}}
"#,
        whisker_rs = whisker_rs.to_string_lossy(),
    );
    std::fs::write(src_dir.join("main.rs"), main_rs)
        .with_context(|| format!("write {}/src/main.rs", src_dir.display()))?;

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
    // Dedup by source_crate name — multiple plugins can ship from
    // one crate and we only need to list that crate once.
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
