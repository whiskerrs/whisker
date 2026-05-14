//! Resolve the on-disk paths of `whisker-rustc-shim` and
//! `whisker-linker-shim` for the dev session.
//!
//! Both shims live in the workspace's `whisker-cli` package, so they're
//! built into the cargo target dir alongside the main `whisker` binary.
//! The dev-server needs absolute paths to set them as
//! `RUSTC_WORKSPACE_WRAPPER` and `-C linker=…`.
//!
//! Resolution order:
//!
//!   1. Compute the expected paths under `<target>/debug/`.
//!      `target` defaults to `<workspace>/target` but `CARGO_TARGET_DIR`
//!      env wins (production usage; CI commonly redirects).
//!   2. If both exist, return them as-is.
//!   3. Otherwise spawn `cargo build -p whisker-cli --bin whisker-rustc-shim
//!      --bin whisker-linker-shim` from the workspace, then re-check.
//!      A build failure surfaces as `Err(_)` — the dev session simply
//!      cannot run Tier 1 without these binaries.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Absolute paths to both shim binaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShimPaths {
    pub rustc_shim: PathBuf,
    pub linker_shim: PathBuf,
}

/// Compute the expected shim paths without touching the filesystem.
/// Pure function — used both in tests and as the first half of
/// [`resolve_shim_paths`].
pub fn expected_shim_paths(workspace_root: &Path) -> ShimPaths {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root.join("target"));
    let bin = |name: &str| target_dir.join("debug").join(exe_name(name));
    ShimPaths {
        rustc_shim: bin("whisker-rustc-shim"),
        linker_shim: bin("whisker-linker-shim"),
    }
}

/// Pure helper: append `.exe` on Windows. Pulled out so tests don't
/// have to fork on cfg.
pub fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Resolve both shim paths. Build them with `cargo build` if the
/// binaries aren't on disk yet. Returns absolute paths suitable for
/// passing to `RUSTC_WORKSPACE_WRAPPER` and `-C linker=…`.
///
/// `workspace_root` is the root the build is spawned from; cargo
/// finds the right `whisker-cli` package via the workspace `members`
/// declaration.
pub fn resolve_shim_paths(workspace_root: &Path) -> Result<ShimPaths> {
    let paths = expected_shim_paths(workspace_root);
    if paths.rustc_shim.is_file() && paths.linker_shim.is_file() {
        return Ok(paths);
    }
    build_shims(workspace_root).context("build whisker-cli shim binaries")?;
    let paths = expected_shim_paths(workspace_root);
    anyhow::ensure!(
        paths.rustc_shim.is_file(),
        "expected `{}` to exist after `cargo build` of the shims",
        paths.rustc_shim.display(),
    );
    anyhow::ensure!(
        paths.linker_shim.is_file(),
        "expected `{}` to exist after `cargo build` of the shims",
        paths.linker_shim.display(),
    );
    Ok(paths)
}

fn build_shims(workspace_root: &Path) -> Result<()> {
    eprintln!(
        "[whisker-dev-server] building shim binaries (`cargo build -p whisker-cli --bin whisker-rustc-shim --bin whisker-linker-shim`)…"
    );
    let status = std::process::Command::new("cargo")
        .args([
            "build",
            "-p",
            "whisker-cli",
            "--bin",
            "whisker-rustc-shim",
            "--bin",
            "whisker-linker-shim",
        ])
        .current_dir(workspace_root)
        .status()
        .context("spawn cargo")?;
    anyhow::ensure!(status.success(), "cargo exited {status}");
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_name_appends_dot_exe_on_windows_otherwise_passes_through() {
        let name = exe_name("foo");
        if cfg!(windows) {
            assert_eq!(name, "foo.exe");
        } else {
            assert_eq!(name, "foo");
        }
    }

    #[test]
    fn expected_paths_default_to_workspace_target_debug() {
        // We deliberately don't try to clear CARGO_TARGET_DIR
        // because env mutations in tests are racy. Instead, accept
        // both shapes (`<ws>/target/debug/...` or
        // `<CARGO_TARGET_DIR>/debug/...`) and just sanity-check the
        // basename + suffix.
        let p = expected_shim_paths(Path::new("/tmp/ws"));
        let rustc_basename = p.rustc_shim.file_name().and_then(|n| n.to_str()).unwrap();
        let linker_basename = p.linker_shim.file_name().and_then(|n| n.to_str()).unwrap();
        assert_eq!(rustc_basename, exe_name("whisker-rustc-shim"));
        assert_eq!(linker_basename, exe_name("whisker-linker-shim"));
        assert!(
            p.rustc_shim.parent().unwrap().ends_with("debug"),
            "expected …/debug/, got {}",
            p.rustc_shim.display(),
        );
        assert!(p.linker_shim.parent().unwrap().ends_with("debug"));
    }

    #[test]
    fn resolve_returns_existing_paths_without_rebuilding() {
        // Set up a fake target dir with the two binaries already
        // present, point CARGO_TARGET_DIR at it, and verify that
        // resolve_shim_paths doesn't try to invoke cargo. We can't
        // really observe "did not invoke cargo" from inside the test
        // — but we can observe that the call returns Ok cheaply
        // (no panic, no hang) and the returned paths point at the
        // files we just created.
        let dir = unique_tempdir();
        let target = dir.join("target");
        std::fs::create_dir_all(target.join("debug")).unwrap();
        let rustc = target.join("debug").join(exe_name("whisker-rustc-shim"));
        let linker = target.join("debug").join(exe_name("whisker-linker-shim"));
        std::fs::write(&rustc, b"#!/bin/sh\nexit 0\n").unwrap();
        std::fs::write(&linker, b"#!/bin/sh\nexit 0\n").unwrap();

        // Use CARGO_TARGET_DIR so we don't depend on the workspace
        // layout the tests run from.
        let prev = std::env::var_os("CARGO_TARGET_DIR");
        std::env::set_var("CARGO_TARGET_DIR", &target);
        let result = resolve_shim_paths(&dir);
        match prev {
            Some(p) => std::env::set_var("CARGO_TARGET_DIR", p),
            None => std::env::remove_var("CARGO_TARGET_DIR"),
        }

        let paths = result.expect("resolve");
        assert_eq!(paths.rustc_shim, rustc);
        assert_eq!(paths.linker_shim, linker);

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn unique_tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let p = std::env::temp_dir().join(format!("whisker-shim-paths-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
