//! `whisker-rustc-shim` binary's logic.
//!
//! Cargo invokes the binary as:
//!
//! ```text
//! whisker-rustc-shim <rustc-path> <rustc-args...>
//! ```
//!
//! when `RUSTC_WORKSPACE_WRAPPER=whisker-rustc-shim` is set. We do two
//! things, in order:
//!
//! 1. Dump the rustc invocation (full argv + crate name + timestamp)
//!    to JSON at `$WHISKER_RUSTC_CACHE_DIR/<crate>-<microseconds>.json`.
//!    The dev server reads these later to drive thin rebuilds (I4g-5).
//! 2. Spawn the *real* rustc with the original args and exit with the
//!    same status code — to cargo, the wrapper is invisible.
//!
//! If `WHISKER_RUSTC_CACHE_DIR` is unset, step 1 is skipped. That way a
//! stray `RUSTC_WORKSPACE_WRAPPER=whisker-rustc-shim` (left over from a
//! crashed `whisker run`) doesn't break ordinary `cargo build`.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// One captured rustc invocation. The dev-server-side `wrapper`
/// module deserialises one of these per crate to reconstruct the
/// thin-rebuild command line.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct CapturedRustcInvocation {
    /// The crate cargo was building when this invocation fired
    /// (extracted from `--crate-name`). May be empty if rustc was
    /// invoked without `--crate-name`, e.g. for build-script probes.
    pub crate_name: String,
    /// Full argv passed to rustc, **excluding** the rustc binary
    /// path itself (cargo prepends that, but the rest is what we
    /// need to replay).
    pub args: Vec<String>,
    /// When the invocation happened, as microseconds since UNIX
    /// epoch. Used to disambiguate multiple invocations of the same
    /// crate within one fat build.
    pub timestamp_micros: u128,
}

/// Entry point called from `src/bin/whisker_rustc_shim.rs`.
pub fn run() -> Result<()> {
    let mut argv: Vec<String> = std::env::args().collect();
    if argv.len() < 2 {
        anyhow::bail!(
            "whisker-rustc-shim: expected `<wrapper> <rustc-path> [rustc-args...]`, \
             got {} arg(s)",
            argv.len(),
        );
    }
    let _wrapper = argv.remove(0); // own path; not needed
    let real_rustc = argv.remove(0); // path cargo prepended
    let rustc_args = argv; // remainder = real rustc args

    // Capture step (silent if no cache dir).
    if let Some(cache_dir) = std::env::var_os("WHISKER_RUSTC_CACHE_DIR") {
        let cache_dir = PathBuf::from(cache_dir);
        let invocation = capture(&rustc_args)?;
        save_invocation(&cache_dir, &invocation)
            .with_context(|| format!("save to {}", cache_dir.display()))?;
    }

    // Forward to real rustc, transparent exit.
    let status = std::process::Command::new(&real_rustc)
        .args(&rustc_args)
        .status()
        .with_context(|| format!("spawn {real_rustc}"))?;
    std::process::exit(status.code().unwrap_or(1));
}

// ----- Pure helpers (testable) ----------------------------------------------

/// Build a [`CapturedRustcInvocation`] from a rustc argv slice.
/// Pure aside from reading the system clock for the timestamp.
pub fn capture(rustc_args: &[String]) -> Result<CapturedRustcInvocation> {
    Ok(CapturedRustcInvocation {
        crate_name: extract_crate_name(rustc_args).unwrap_or_default(),
        args: rustc_args.to_vec(),
        timestamp_micros: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros())
            .unwrap_or(0),
    })
}

/// Find the value passed to `--crate-name` in a rustc argv slice.
/// rustc's CLI guarantees `--crate-name <name>` (separate args) when
/// cargo invokes it; the equals form (`--crate-name=foo`) isn't used
/// in practice but we handle it defensively.
pub fn extract_crate_name(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--crate-name" {
            return iter.next().cloned();
        }
        if let Some(rest) = arg.strip_prefix("--crate-name=") {
            return Some(rest.to_string());
        }
    }
    None
}

/// Filesystem-safe filename for a captured invocation. Same crate may
/// be compiled multiple times in one fat build (e.g. lib + test + bin
/// targets, build-script vs. main); the timestamp avoids collisions.
pub fn invocation_filename(invocation: &CapturedRustcInvocation) -> String {
    let crate_for_path = if invocation.crate_name.is_empty() {
        "_unknown"
    } else {
        invocation.crate_name.as_str()
    };
    format!(
        "{}-{}.json",
        crate_for_path.replace(['-', '/'], "_"),
        invocation.timestamp_micros,
    )
}

/// Persist `invocation` under `cache_dir/<filename>.json`. Creates
/// `cache_dir` if missing.
pub fn save_invocation(cache_dir: &Path, invocation: &CapturedRustcInvocation) -> Result<()> {
    std::fs::create_dir_all(cache_dir)
        .with_context(|| format!("create {}", cache_dir.display()))?;
    let path = cache_dir.join(invocation_filename(invocation));
    let json = serde_json::to_string_pretty(invocation).context("serialize")?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
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
        let p = std::env::temp_dir().join(format!("whisker-rustc-shim-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    // ----- extract_crate_name ------------------------------------------

    #[test]
    fn extract_crate_name_from_separated_form() {
        let args = s(&[
            "--edition=2021",
            "--crate-name",
            "hello_world",
            "--out-dir",
            "x",
        ]);
        assert_eq!(extract_crate_name(&args).as_deref(), Some("hello_world"));
    }

    #[test]
    fn extract_crate_name_from_equals_form() {
        let args = s(&["--crate-name=foo_bar", "--edition=2021"]);
        assert_eq!(extract_crate_name(&args).as_deref(), Some("foo_bar"));
    }

    #[test]
    fn extract_crate_name_returns_none_when_absent() {
        let args = s(&["--edition=2021", "--out-dir", "x"]);
        assert_eq!(extract_crate_name(&args), None);
    }

    #[test]
    fn extract_crate_name_first_occurrence_wins() {
        // Pathological but well-defined: cargo never sends two, but if
        // it ever did we'd take the first.
        let args = s(&["--crate-name", "first", "--crate-name", "second"]);
        assert_eq!(extract_crate_name(&args).as_deref(), Some("first"));
    }

    // ----- capture -----------------------------------------------------

    #[test]
    fn capture_includes_full_argv_unchanged() {
        let argv = s(&[
            "--crate-name",
            "demo",
            "--edition=2021",
            "src/lib.rs",
            "-C",
            "opt-level=0",
        ]);
        let inv = capture(&argv).unwrap();
        assert_eq!(inv.args, argv);
        assert_eq!(inv.crate_name, "demo");
        assert!(inv.timestamp_micros > 0);
    }

    #[test]
    fn capture_with_no_crate_name_leaves_field_empty() {
        let inv = capture(&s(&["--edition=2021", "src/lib.rs"])).unwrap();
        assert_eq!(inv.crate_name, "");
    }

    // ----- invocation_filename ----------------------------------------

    #[test]
    fn invocation_filename_uses_underscored_crate_name_and_timestamp() {
        let inv = CapturedRustcInvocation {
            crate_name: "hello-world".into(),
            args: vec![],
            timestamp_micros: 1_000_000,
        };
        assert_eq!(invocation_filename(&inv), "hello_world-1000000.json");
    }

    #[test]
    fn invocation_filename_handles_anonymous_crate() {
        let inv = CapturedRustcInvocation {
            crate_name: "".into(),
            args: vec![],
            timestamp_micros: 42,
        };
        assert_eq!(invocation_filename(&inv), "_unknown-42.json");
    }

    #[test]
    fn invocation_filename_strips_path_separators() {
        // Defensive — rustc's --crate-name doesn't contain slashes,
        // but we shouldn't crater if something weird sneaks in.
        let inv = CapturedRustcInvocation {
            crate_name: "weird/name".into(),
            args: vec![],
            timestamp_micros: 7,
        };
        assert_eq!(invocation_filename(&inv), "weird_name-7.json");
    }

    // ----- save_invocation (round-trip on disk) -----------------------

    #[test]
    fn save_invocation_writes_a_readable_json_file() {
        let dir = unique_tempdir();
        let inv = CapturedRustcInvocation {
            crate_name: "x".into(),
            args: s(&["--crate-name", "x", "src/lib.rs"]),
            timestamp_micros: 12345,
        };
        save_invocation(&dir, &inv).expect("save");

        let path = dir.join(invocation_filename(&inv));
        assert!(
            path.is_file(),
            "json file should exist at {}",
            path.display()
        );

        let body = std::fs::read_to_string(&path).unwrap();
        let parsed: CapturedRustcInvocation = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed, inv);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_invocation_creates_the_cache_dir_if_missing() {
        let dir = unique_tempdir().join("nested/does/not/exist");
        assert!(!dir.exists());
        let inv = CapturedRustcInvocation {
            crate_name: "x".into(),
            args: vec![],
            timestamp_micros: 1,
        };
        save_invocation(&dir, &inv).expect("save");
        assert!(dir.is_dir());

        // cleanup
        let mut to_remove = dir;
        for _ in 0..4 {
            to_remove.pop();
        }
        let _ = std::fs::remove_dir_all(&to_remove);
    }
}
