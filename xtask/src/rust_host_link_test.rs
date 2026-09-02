//! Generated Rust Host consumer link tests.
//!
//! Every module with a Web/Desktop Rust Host contribution is linked through
//! the same CNG-generated bootstrap used by applications. This catches a
//! missing common element-schema export or Host module definition before a
//! package reaches users.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use whisker_dev_server::Target;

const PACKAGE: &str = "rust-host-link-test";

pub fn run(root: &Path, host: &str) -> Result<()> {
    let fixture = root.join("tests/rust-host-link-test");
    let target = match host {
        "desktop" => Target::Macos,
        "web" => Target::Web,
        _ => bail!("unknown Rust Host {host:?}; expected desktop or web"),
    };
    let gen_dir = sync_project(root, &fixture, target)?;

    let mut command = Command::new(super::cargo());
    command
        .current_dir(root)
        .env(
            "CARGO_TARGET_DIR",
            root.join("target/xtask/rust-host-link-test").join(host),
        )
        .arg("check")
        .arg("--manifest-path")
        .arg(gen_dir.join("Cargo.toml"));
    if matches!(target, Target::Web) {
        command.args(["--target", "wasm32-unknown-unknown"]);
    }
    super::run(&mut command).with_context(|| format!("link generated {host} Host"))
}

fn sync_project(root: &Path, fixture: &Path, target: Target) -> Result<PathBuf> {
    let manifest_path = fixture.join("Cargo.toml");
    let manifest = whisker_cli::manifest::resolve(Some(&manifest_path))
        .context("resolve the Rust Host link-test app")?;
    ensure!(manifest.package == PACKAGE, "unexpected link-test package");

    let platform_dir = match target {
        Target::Macos => "macos",
        Target::Web => "web",
        _ => unreachable!("Rust Host link test only supports Desktop and Web"),
    };
    let gen_dir = fixture.join("gen").join(platform_dir);
    if gen_dir.exists() {
        fs::remove_dir_all(&gen_dir)
            .with_context(|| format!("remove stale {}", gen_dir.display()))?;
    }

    let sync = whisker_cli::platforms::sync_for_target(
        target,
        &manifest.config,
        &manifest.crate_dir,
        root,
        &manifest.package,
    )
    .with_context(|| format!("generate {} consumer project", host_name(target)))?;
    ensure!(
        sync.regenerated,
        "clean link-test project was not regenerated"
    );
    Ok(sync.gen_dir)
}

fn host_name(target: Target) -> &'static str {
    match target {
        Target::Macos => "Desktop",
        Target::Web => "Web",
        _ => unreachable!("Rust Host link test only supports Desktop and Web"),
    }
}
