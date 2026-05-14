//! Pre-flight environment check for the thin-rebuild pipeline.
//!
//! Catches the "fat build was for one toolchain, the dev loop is now
//! using another" class of breakage *before* `subsecond::apply_patch`
//! runs and segfaults the device. The check is deliberately small —
//! we only assert what's necessary to keep the same captured rustc
//! invocation viable:
//!
//! 1. The current rustc still supports the target triple the fat
//!    build was for. If a `rustup toolchain` change between the fat
//!    build and the first edit dropped the Android target, we want
//!    a clear error here, not a cryptic linker failure later.
//!
//! Things we deliberately do NOT validate:
//!
//! - **Exact rustc version match.** Patch dylibs survive across
//!   patch-level rustc bumps in practice (subsecond is pretty
//!   tolerant); demanding strict equality would break workflows
//!   where `rustup update` is a frequent occurrence. Major version
//!   regressions WILL be surfaced by the thin rebuild itself
//!   (rustc returns non-zero), so we let that path do the talking.
//!
//! - **Sysroot stability.** Same reasoning — rustc verifies its own
//!   sysroot at compile time. We don't add a redundant probe.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use super::wrapper::CapturedRustcInvocation;

/// Run the pre-flight checks. Returns Ok(()) if it's safe to call
/// `thin_rebuild`; otherwise Err with a message a human can act on.
pub fn validate_environment(
    captured: &CapturedRustcInvocation,
    current_rustc: &Path,
) -> Result<()> {
    if let Some(triple) = extract_target_triple(&captured.args) {
        ensure_target_supported(current_rustc, &triple)?;
    }
    Ok(())
}

/// Pull the value passed to `--target` (or `--target=...`) out of a
/// rustc argv. Pure helper — same shape as `extract_crate_name` in
/// the shim.
pub fn extract_target_triple(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--target" {
            return iter.next().cloned();
        }
        if let Some(rest) = arg.strip_prefix("--target=") {
            return Some(rest.to_string());
        }
    }
    None
}

/// Spawn `rustc --print=target-list` and verify `triple` shows up.
/// Synchronous on purpose — runs once per dev-server boot, not per
/// patch.
pub fn ensure_target_supported(rustc: &Path, triple: &str) -> Result<()> {
    let output = Command::new(rustc)
        .args(["--print=target-list"])
        .output()
        .with_context(|| format!("spawn `{} --print=target-list`", rustc.display()))?;
    if !output.status.success() {
        anyhow::bail!(
            "`{} --print=target-list` exited {}",
            rustc.display(),
            output.status,
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.lines().any(|line| line.trim() == triple) {
        Ok(())
    } else {
        anyhow::bail!(
            "rustc at {} doesn't support target triple `{triple}` \
             — check `rustup target list --installed` and re-run \
             the fat build under the same toolchain",
            rustc.display(),
        )
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn captured_with(args: Vec<String>) -> CapturedRustcInvocation {
        CapturedRustcInvocation {
            crate_name: "demo".into(),
            args,
            timestamp_micros: 0,
        }
    }

    fn rustc_path() -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
    }

    /// One target triple we're sure the test runner's rustc supports
    /// — its own host triple. We discover it dynamically from rustc
    /// itself so the test is portable across CI runners.
    fn host_triple() -> String {
        let out = Command::new(rustc_path())
            .args(["-vV"])
            .output()
            .expect("rustc -vV");
        let stdout = String::from_utf8_lossy(&out.stdout);
        stdout
            .lines()
            .find_map(|l| l.strip_prefix("host: "))
            .map(|s| s.trim().to_string())
            .expect("rustc -vV reports a host:")
    }

    // ----- extract_target_triple --------------------------------------

    #[test]
    fn extract_target_triple_from_separated_form() {
        let args = s(&[
            "--edition=2021",
            "--target",
            "aarch64-apple-darwin",
            "src/lib.rs",
        ]);
        assert_eq!(
            extract_target_triple(&args).as_deref(),
            Some("aarch64-apple-darwin")
        );
    }

    #[test]
    fn extract_target_triple_from_equals_form() {
        let args = s(&["--target=x86_64-unknown-linux-gnu"]);
        assert_eq!(
            extract_target_triple(&args).as_deref(),
            Some("x86_64-unknown-linux-gnu")
        );
    }

    #[test]
    fn extract_target_triple_returns_none_when_absent() {
        let args = s(&["--edition=2021", "src/lib.rs"]);
        assert_eq!(extract_target_triple(&args), None);
    }

    // ----- ensure_target_supported (live rustc spawn) ------------------

    #[test]
    fn ensure_target_supported_accepts_the_host_triple() {
        // Whatever rustc reports as its own host MUST be in the
        // target list — otherwise `rustc -vV` and `rustc
        // --print=target-list` disagree, which would be a rustc
        // bug, not ours.
        let triple = host_triple();
        ensure_target_supported(&rustc_path(), &triple).expect("host triple supported");
    }

    #[test]
    fn ensure_target_supported_rejects_a_made_up_triple() {
        let result = ensure_target_supported(&rustc_path(), "totally-not-a-real-triple-9999");
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("doesn't support target triple"), "got: {msg}",);
    }

    #[test]
    fn ensure_target_supported_surfaces_a_missing_rustc_as_err() {
        let result = ensure_target_supported(
            std::path::Path::new("/no/such/rustc/anywhere"),
            "any-triple",
        );
        assert!(result.is_err());
    }

    // ----- validate_environment ---------------------------------------

    #[test]
    fn validate_passes_when_no_target_triple_in_args() {
        // Captured args without --target → nothing to check, pass.
        let captured = captured_with(s(&["--edition=2021", "src/lib.rs"]));
        validate_environment(&captured, &rustc_path()).expect("ok");
    }

    #[test]
    fn validate_passes_with_the_host_triple() {
        let triple = host_triple();
        let captured = captured_with(s(&["--target", &triple, "src/lib.rs"]));
        validate_environment(&captured, &rustc_path()).expect("ok");
    }

    #[test]
    fn validate_fails_when_target_triple_is_unsupported() {
        let captured = captured_with(s(&["--target", "made-up-arch", "src/lib.rs"]));
        let err = validate_environment(&captured, &rustc_path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("made-up-arch"), "{msg}");
    }
}
