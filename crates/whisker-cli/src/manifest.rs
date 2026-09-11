//! Resolve a user crate's `whisker.rs` config + `Cargo.toml` metadata
//! into a [`ResolvedManifest`] the CLI can hand to `whisker-dev-server`.
//!
//! The dev-server itself is manifest-agnostic — it accepts flat
//! parameters (paths, bundle ids, application ids, …) via
//! `whisker_dev_server::Config`. Translating the user's
//! `whisker.rs` result into those flat
//! values is the CLI's job and lives in [`mod@super::run`].
//!
//! ## Discovery
//!
//! [`resolve`] takes an optional explicit `Cargo.toml` path. When
//! `None`, it walks up from `cwd` looking for the first `Cargo.toml`
//! that has a `[package]` section (a `[workspace]`-only manifest at
//! the top of a virtual workspace doesn't count — we need the
//! package node).
//!
//! Once a `Cargo.toml` is found, the sibling `whisker.rs` is the
//! config source. Missing `whisker.rs` is an error: the dev-server
//! needs bundle id, application id, etc. that the file supplies.

use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use whisker_config::Config;

use std::collections::BTreeMap;
use whisker_cng::{GenerationReport, GenerationTarget, PlatformSync};

/// One CLI invocation's worth of resolved user-crate state.
#[derive(Debug)]
pub struct ResolvedManifest {
    /// Directory containing the user crate's `Cargo.toml` and (next
    /// to it) `whisker.rs`. All other paths the CLI builds are
    /// relative to this.
    pub crate_dir: PathBuf,
    /// `[package].name` from `Cargo.toml`. Used by the dev-server as
    /// the cargo `-p` argument, and by `whisker-build` to find the
    /// `lib<package>.so` / `.dylib` artifact.
    pub package: String,
    /// Result of running the user's `whisker.rs`. Owned
    /// (decoded from JSON) so subsequent CLI logic can pattern-match
    /// on optional fields without rerunning the generator.
    pub config: Config,
    pub workspace_root: PathBuf,
    pub projects: BTreeMap<GenerationTarget, PlatformSync>,
}

/// Generate all supported projects and resolve their application metadata.
/// An explicit manifest bypasses current-directory discovery. Targeted commands
/// use [`resolve_for_target`] to generate only the project they need.
pub fn resolve(cargo_toml_override: Option<&Path>) -> Result<ResolvedManifest> {
    resolve_targets(cargo_toml_override, &[])
}

/// Generate the requested platform and resolve the resulting application metadata.
pub fn resolve_for_target(
    cargo_toml_override: Option<&Path>,
    target: whisker_dev_server::Target,
) -> Result<ResolvedManifest> {
    resolve_targets(
        cargo_toml_override,
        &[crate::platforms::generation_target(target)],
    )
}

fn resolve_targets(
    cargo_toml_override: Option<&Path>,
    targets: &[GenerationTarget],
) -> Result<ResolvedManifest> {
    let cargo_toml = application_manifest(cargo_toml_override)?;
    Ok(from_report(whisker_cng::generate(&cargo_toml, targets)?))
}

/// Read metadata from the last successful generation without executing user code.
pub fn read_generated(cargo_toml_override: Option<&Path>) -> Result<ResolvedManifest> {
    let cargo_toml = application_manifest(cargo_toml_override)?;
    let report = cargo_toml
        .parent()
        .unwrap()
        .join("target/.whisker/generation.json");
    let report: GenerationReport = serde_json::from_slice(&std::fs::read(report)?)?;
    anyhow::ensure!(
        report.schema_version == 1 && report.crate_dir == cargo_toml.parent().unwrap(),
        "invalid generation report"
    );
    Ok(from_report(report))
}

fn from_report(report: GenerationReport) -> ResolvedManifest {
    ResolvedManifest {
        crate_dir: report.crate_dir,
        package: report.package,
        config: report.config,
        workspace_root: report.workspace_root,
        projects: report.projects,
    }
}

impl ResolvedManifest {
    /// The generated project for this invocation's build target.
    pub fn project(&self, target: whisker_dev_server::Target) -> Result<PlatformSync> {
        self.projects
            .get(&crate::platforms::generation_target(target))
            .cloned()
            .ok_or_else(|| anyhow!("generator did not produce the requested project"))
    }
}

fn application_manifest(cargo_toml_override: Option<&Path>) -> Result<PathBuf> {
    let cargo_toml = match cargo_toml_override {
        Some(p) => p.to_path_buf(),
        None => {
            let cwd = std::env::current_dir().context("read cwd")?;
            find_package_cargo_toml(&cwd).ok_or_else(|| {
                anyhow!(
                    "no `[package]` Cargo.toml at or above {} — pass `--manifest-path <path>` to point at the user crate",
                    cwd.display(),
                )
            })?
        }
    };
    // Canonicalize: downstream sites (Command::current_dir,
    // find_workspace_root's upward walk) break when crate_dir is
    // relative and the workspace root coincides with the process cwd —
    // PathBuf::pop() then bottoms out at "" which feeds chdir("") and
    // surfaces as posix-spawn ENOENT.
    let cargo_toml = std::fs::canonicalize(&cargo_toml)
        .with_context(|| format!("canonicalize {}", cargo_toml.display()))?;
    parse_package_name(&cargo_toml)?;
    Ok(cargo_toml)
}

/// Walk up from `start` looking for the first Cargo.toml with a
/// `[package]` table. A pure `[workspace]` manifest at the top of a
/// virtual workspace is skipped — we want the user-crate package,
/// not the workspace root.
fn find_package_cargo_toml(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        let cargo = cur.join("Cargo.toml");
        if cargo.is_file() {
            if let Ok(txt) = std::fs::read_to_string(&cargo) {
                if has_package_section(&txt) {
                    return Some(cargo);
                }
            }
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// The applicationId an Android build of this config uses — the
/// explicit `android.application_id`, else the shared `bundle_id`.
/// Single source of truth for `whisker run android`,
/// `whisker credential android`, and `whisker build appbundle|apk`,
/// so the credential lookup key always matches what gradle builds.
pub fn android_application_id(config: &Config) -> Option<String> {
    config
        .android
        .application_id
        .clone()
        .or_else(|| config.bundle_id.clone())
}

/// Cheap test for `[package]` without pulling in a full TOML parser
/// just for the walk-up phase. We do a real `toml::from_str` once
/// we've picked a winning candidate (see `parse_package_name`).
fn has_package_section(toml_text: &str) -> bool {
    toml_text.lines().any(|line| {
        let l = line.trim();
        l == "[package]" || l.starts_with("[package]") || l == "[ package ]"
    })
}

/// Parse `[package].name` from the given Cargo.toml.
fn parse_package_name(cargo_toml: &Path) -> Result<String> {
    let text = std::fs::read_to_string(cargo_toml)
        .with_context(|| format!("read {}", cargo_toml.display()))?;
    let doc: toml::Value =
        toml::from_str(&text).with_context(|| format!("parse {} as TOML", cargo_toml.display()))?;
    let name = doc
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .ok_or_else(|| {
            anyhow!(
                "{} has no [package].name (is this a virtual-workspace Cargo.toml?)",
                cargo_toml.display(),
            )
        })?;
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_tempdir(label: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let p = std::env::temp_dir().join(format!("whisker-cli-manifest-{label}-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn has_package_section_detects_the_table_header() {
        assert!(has_package_section("[package]\nname = \"x\"\n"));
        assert!(has_package_section("\n\n[package]\n"));
        assert!(!has_package_section("[workspace]\nmembers = []\n"));
        assert!(!has_package_section("[package.metadata.foo]\nbar = 1\n"));
    }

    #[test]
    fn find_package_cargo_toml_skips_virtual_workspace_root() {
        let tmp = unique_tempdir("vws");
        std::fs::write(tmp.join("Cargo.toml"), "[workspace]\nmembers = [\"app\"]\n").unwrap();
        let app = tmp.join("app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(
            app.join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.0.0\"\n",
        )
        .unwrap();
        // From inside the member, walker should land on the member's
        // Cargo.toml, not the virtual-workspace one.
        assert_eq!(
            find_package_cargo_toml(&app).as_deref(),
            Some(app.join("Cargo.toml").as_path()),
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn parse_package_name_reads_the_name_field() {
        let tmp = unique_tempdir("name");
        let p = tmp.join("Cargo.toml");
        std::fs::write(
            &p,
            "[package]\nname = \"my-cool-app\"\nversion = \"0.0.0\"\n",
        )
        .unwrap();
        assert_eq!(parse_package_name(&p).unwrap(), "my-cool-app");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
