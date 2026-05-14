//! Fat-build runner + captured-args loader.
//!
//! The other half of the rustc/linker hijack started in I4g-4a.
//! `tuft-rustc-shim` writes a JSON file per rustc invocation into a
//! cache dir; this module:
//!
//! 1. Spawns the *fat build* — a normal cargo build with
//!    `RUSTC_WORKSPACE_WRAPPER=tuft-rustc-shim` set, so the cache
//!    fills up.
//! 2. Loads those JSON files back into a `HashMap<String,
//!    CapturedRustcInvocation>` keyed by crate name, picking the
//!    most recent timestamp when a crate was rebuilt mid-session.
//! 3. (Future, I4g-5) hands the captured args to a thin-rebuild
//!    driver that only recompiles the changed crate and re-links.
//!
//! `CapturedRustcInvocation` is currently *defined* here, not in
//! tuft-cli, so that the shim binary doesn't need to pull in the
//! whole dev-server dep tree (tokio / axum / notify / object). The
//! shim has its own copy of the struct shape; serde keeps the wire
//! format compatible. A future cleanup will extract a tiny
//! `tuft-hotpatch-types` crate and dedupe both sides — see TODO.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Target;

/// Mirrors `tuft_cli::rustc_shim::CapturedRustcInvocation` exactly.
/// Kept duplicated (rather than imported) so the shim binary stays
/// dep-light. JSON wire format is what binds them — both sides go
/// through serde, so a field rename in one without the other will
/// trip the deserialize step at run time and emit a clear error.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct CapturedRustcInvocation {
    pub crate_name: String,
    pub args: Vec<String>,
    pub timestamp_micros: u128,
}

/// Spawn a `cargo` build for the given target with `RUSTC_WORKSPACE_WRAPPER`
/// pointed at `shim_path` and `TUFT_RUSTC_CACHE_DIR` pointed at `cache_dir`.
/// Inherits stdout/stderr so cargo's progress is visible. After the build
/// completes successfully, [`load_captured_args`] can read the cache.
///
/// `target` is currently a hint only; the cargo command we run is the
/// host build (`cargo build -p <pkg>`). I4g-5 will switch to the
/// platform-specific xtask invocations once thin rebuild is wired up.
pub fn run_fat_build(
    workspace_root: &Path,
    package: &str,
    _target: Target,
    shim_path: &Path,
    cache_dir: &Path,
) -> Result<()> {
    std::fs::create_dir_all(cache_dir)
        .with_context(|| format!("create cache dir {}", cache_dir.display()))?;
    let status = Command::new("cargo")
        .args(["build", "-p", package])
        .current_dir(workspace_root)
        .env("RUSTC_WORKSPACE_WRAPPER", shim_path)
        .env("TUFT_RUSTC_CACHE_DIR", cache_dir)
        .status()
        .context("spawn cargo for fat build")?;
    if !status.success() {
        anyhow::bail!("fat build failed: cargo exited {status}");
    }
    Ok(())
}

/// Walk `cache_dir`, deserialise every `*.json` produced by
/// `tuft-rustc-shim`, and collapse duplicates per crate by keeping
/// the most-recent timestamp. Empty / unparseable files are skipped
/// with a warning rather than aborting the whole load — a partial
/// fat build shouldn't take the dev loop down.
pub fn load_captured_args(
    cache_dir: &Path,
) -> Result<HashMap<String, CapturedRustcInvocation>> {
    let mut by_crate: HashMap<String, CapturedRustcInvocation> = HashMap::new();
    if !cache_dir.is_dir() {
        return Ok(by_crate); // empty cache is fine, just nothing to do
    }
    for entry in std::fs::read_dir(cache_dir)
        .with_context(|| format!("read_dir {}", cache_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let body = match std::fs::read_to_string(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("[tuft-dev] skip {}: {e}", path.display());
                continue;
            }
        };
        let inv: CapturedRustcInvocation = match serde_json::from_str(&body) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("[tuft-dev] skip {}: malformed json: {e}", path.display());
                continue;
            }
        };
        keep_newest(&mut by_crate, inv);
    }
    Ok(by_crate)
}

/// Pure helper for the load loop's "keep most-recent per crate"
/// decision. Pulled out so unit tests don't have to write JSON to
/// disk to exercise the merge.
pub fn keep_newest(
    map: &mut HashMap<String, CapturedRustcInvocation>,
    inv: CapturedRustcInvocation,
) {
    match map.get(&inv.crate_name) {
        Some(prev) if prev.timestamp_micros >= inv.timestamp_micros => {
            // already have a newer or equal-timestamp entry; ignore.
        }
        _ => {
            map.insert(inv.crate_name.clone(), inv);
        }
    }
}

/// Convenience: best-effort default cache dir under the workspace's
/// `target/.tuft/rustc-args/`. Created on demand.
pub fn default_cache_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join("target/.tuft/rustc-args")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn unique_tempdir() -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let p = std::env::temp_dir().join(format!("tuft-wrapper-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn write_invocation(dir: &Path, inv: &CapturedRustcInvocation) {
        let name = format!(
            "{}-{}.json",
            inv.crate_name.replace(['-', '/'], "_"),
            inv.timestamp_micros,
        );
        let body = serde_json::to_string_pretty(inv).unwrap();
        std::fs::write(dir.join(name), body).unwrap();
    }

    // ----- load_captured_args ------------------------------------------

    #[test]
    fn load_captured_args_returns_empty_for_missing_cache_dir() {
        let map = load_captured_args(Path::new("/nope/does/not/exist")).unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn load_captured_args_returns_one_entry_per_crate_for_distinct_crates() {
        let dir = unique_tempdir();
        write_invocation(
            &dir,
            &CapturedRustcInvocation {
                crate_name: "foo".into(),
                args: s(&["--crate-name", "foo", "src/lib.rs"]),
                timestamp_micros: 100,
            },
        );
        write_invocation(
            &dir,
            &CapturedRustcInvocation {
                crate_name: "bar".into(),
                args: s(&["--crate-name", "bar", "src/lib.rs"]),
                timestamp_micros: 200,
            },
        );

        let map = load_captured_args(&dir).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map["foo"].args, s(&["--crate-name", "foo", "src/lib.rs"]));
        assert_eq!(map["bar"].args, s(&["--crate-name", "bar", "src/lib.rs"]));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_captured_args_keeps_the_most_recent_invocation_per_crate() {
        let dir = unique_tempdir();
        // Older invocation: shorter args.
        write_invocation(
            &dir,
            &CapturedRustcInvocation {
                crate_name: "foo".into(),
                args: s(&["--old-args"]),
                timestamp_micros: 100,
            },
        );
        // Newer invocation: longer args.
        write_invocation(
            &dir,
            &CapturedRustcInvocation {
                crate_name: "foo".into(),
                args: s(&["--newer-args", "--more"]),
                timestamp_micros: 200,
            },
        );

        let map = load_captured_args(&dir).unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map["foo"].timestamp_micros, 200);
        assert_eq!(map["foo"].args, s(&["--newer-args", "--more"]));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_captured_args_skips_non_json_files() {
        let dir = unique_tempdir();
        std::fs::write(dir.join("README.md"), "not json").unwrap();
        write_invocation(
            &dir,
            &CapturedRustcInvocation {
                crate_name: "foo".into(),
                args: vec![],
                timestamp_micros: 1,
            },
        );

        let map = load_captured_args(&dir).unwrap();
        assert_eq!(map.len(), 1);
        assert!(map.contains_key("foo"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_captured_args_skips_malformed_json_with_a_warning() {
        let dir = unique_tempdir();
        std::fs::write(dir.join("garbage.json"), "{ not valid json").unwrap();
        write_invocation(
            &dir,
            &CapturedRustcInvocation {
                crate_name: "good".into(),
                args: vec![],
                timestamp_micros: 1,
            },
        );

        let map = load_captured_args(&dir).unwrap();
        assert_eq!(map.len(), 1);
        assert!(map.contains_key("good"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ----- keep_newest --------------------------------------------------

    #[test]
    fn keep_newest_inserts_into_empty_map() {
        let mut m = HashMap::new();
        keep_newest(
            &mut m,
            CapturedRustcInvocation {
                crate_name: "x".into(),
                args: vec![],
                timestamp_micros: 1,
            },
        );
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn keep_newest_replaces_when_timestamp_strictly_newer() {
        let mut m = HashMap::new();
        m.insert(
            "x".into(),
            CapturedRustcInvocation {
                crate_name: "x".into(),
                args: s(&["old"]),
                timestamp_micros: 5,
            },
        );
        keep_newest(
            &mut m,
            CapturedRustcInvocation {
                crate_name: "x".into(),
                args: s(&["new"]),
                timestamp_micros: 10,
            },
        );
        assert_eq!(m["x"].args, s(&["new"]));
    }

    #[test]
    fn keep_newest_does_not_replace_with_equal_or_older_timestamp() {
        let mut m = HashMap::new();
        m.insert(
            "x".into(),
            CapturedRustcInvocation {
                crate_name: "x".into(),
                args: s(&["incumbent"]),
                timestamp_micros: 10,
            },
        );
        keep_newest(
            &mut m,
            CapturedRustcInvocation {
                crate_name: "x".into(),
                args: s(&["equal"]),
                timestamp_micros: 10,
            },
        );
        keep_newest(
            &mut m,
            CapturedRustcInvocation {
                crate_name: "x".into(),
                args: s(&["older"]),
                timestamp_micros: 1,
            },
        );
        assert_eq!(m["x"].args, s(&["incumbent"]));
    }

    // ----- default_cache_dir -------------------------------------------

    #[test]
    fn default_cache_dir_lives_under_target_dot_tuft() {
        let p = default_cache_dir(Path::new("/tmp/ws"));
        assert!(p.ends_with("target/.tuft/rustc-args"));
    }

    // ----- run_fat_build (integration: runs a real cargo) ---------------
    //
    // Smoke-test only: spawn `cargo --version` instead of a real
    // build to keep the test fast. The real round-trip
    // (build → JSON files appear → load_captured_args returns them)
    // is exercised in I4g-5's integration test against a fixture
    // crate.

    #[test]
    fn run_fat_build_creates_the_cache_dir_even_if_build_fails() {
        // Point the wrapper at /bin/true so cargo doesn't actually
        // compile anything; we just want to know `run_fat_build`
        // creates the cache dir and surfaces a non-zero exit as Err.
        let dir = unique_tempdir();
        let cache = dir.join("nested/cache");
        // Bogus workspace_root means cargo build will fail; that's
        // the path we want to assert on.
        let bad_workspace = unique_tempdir();
        let res = run_fat_build(
            &bad_workspace,
            "no-such-package",
            Target::Host,
            Path::new("/bin/true"),
            &cache,
        );
        assert!(res.is_err(), "build of nonexistent pkg should error");
        assert!(cache.is_dir(), "cache dir should be created up front");

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&bad_workspace);
    }
}
