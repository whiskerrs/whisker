//! Tier 1 fat-build capture shim wiring.
//!
//! When the dev-server runs a Tier 2 cold rebuild for hot-reload, it
//! transparently elevates that build into a **fat build**: cargo
//! still produces the same artifact, but the rustc and linker
//! invocations get intercepted by [`whisker-rustc-shim`] and
//! [`whisker-linker-shim`] respectively, which dump their argv to
//! JSON files under the configured cache dirs. The Tier 1 thin
//! rebuild later replays those argvs to produce a patch dylib.
//!
//! The setup is just env vars (cargo's RUSTC_WORKSPACE_WRAPPER +
//! per-target linker overrides). [`capture_env_vars`] computes the
//! map; callers merge it into their `Command`.
//!
//! Moved here from `whisker-dev-server::builder` so `whisker-cli`'s
//! `whisker run` path (which lives outside dev-server in Phase 3+)
//! can also drive fat builds when it wants Tier 1 ready.

use std::path::PathBuf;

/// Shim wiring that turns a plain cargo invocation into a Tier 1
/// fat build. All paths are absolute; the dev-server creates the
/// cache dirs on demand. `real_linker` is what the linker shim
/// forwards to (typically the same `cc`/`clang` cargo would have
/// used).
///
/// `target_triple` is the **Rust target triple** the user code will
/// compile for. When set, the linker shim is installed only for
/// that triple via cargo's `CARGO_TARGET_<UPPER>_LINKER` env var —
/// host-only artifacts (build scripts, proc-macros) keep their
/// default linker. When `None`, the shim is installed globally via
/// `RUSTFLAGS=-Clinker=…` (fine for host-only Tier 1 setups).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureShims {
    pub rustc_shim: PathBuf,
    pub linker_shim: PathBuf,
    pub rustc_cache_dir: PathBuf,
    pub linker_cache_dir: PathBuf,
    pub real_linker: PathBuf,
    pub target_triple: Option<String>,
}

/// Compute the env vars that turn a plain `cargo` invocation into a
/// fat build that captures rustc + linker args. Caller is expected
/// to merge these into a `Command` (test helper / production code
/// share this function).
///
/// When `c.target_triple` is `Some(t)`, the linker shim is installed
/// **only** for that triple via
/// `CARGO_TARGET_<TRIPLE_UPPER>_LINKER=<shim>` — cargo's own
/// mechanism for per-target linker selection. This is the critical
/// piece for cross-compilation: build scripts and proc-macros, which
/// are compiled for the **host** triple, keep their default host
/// linker, so they don't get redirected at the NDK / cross linker.
///
/// When `c.target_triple` is `None`, the shim is installed via
/// `RUSTFLAGS=-Clinker=…` (the global form). Pre-existing
/// `RUSTFLAGS` in the dev-server's env are preserved.
pub fn capture_env_vars(c: &CaptureShims) -> Vec<(String, String)> {
    let mut out = vec![
        (
            "RUSTC_WORKSPACE_WRAPPER".into(),
            c.rustc_shim.to_string_lossy().into(),
        ),
        (
            "WHISKER_RUSTC_CACHE_DIR".into(),
            c.rustc_cache_dir.to_string_lossy().into(),
        ),
        (
            "WHISKER_LINKER_CACHE_DIR".into(),
            c.linker_cache_dir.to_string_lossy().into(),
        ),
        (
            "WHISKER_REAL_LINKER".into(),
            c.real_linker.to_string_lossy().into(),
        ),
    ];

    let shim = c.linker_shim.to_string_lossy().to_string();
    // Three flags every fat build needs for Tier 1 to work:
    //
    // `-Csave-temps=y` keeps rustc's temp dir (containing the
    // version script and bridge-static archive the linker args
    // reference) on disk after the fat build finishes — without it,
    // rustc deletes everything in `/var/folders/.../rustc*/` on
    // exit and the captured linker invocation becomes unreplayable.
    //
    // `-Clink-arg=-Wl,--export-dynamic` (Linux/Android) /
    // `-Clink-arg=-Wl,-export_dynamic` (macOS) exports every
    // symbol from the original cdylib into its dynamic-symbol
    // table. The patch dylib references std::fmt, alloc, etc.
    // via undefined refs and resolves them against the loaded
    // process at `dlopen` time — but cdylib's default symbol
    // visibility hides those internal-to-the-library symbols.
    // Without --export-dynamic, `dlopen` on the patch fails with
    // "cannot locate symbol _ZN4core3fmt3num...". The cost is a
    // slightly larger .so (the dynamic symbol table grows);
    // acceptable for dev builds.
    //
    // `-Cdebug-assertions=on` toggles the only `cfg!(debug_assertions)`
    // branch in `subsecond::HotFn::try_call` — in release builds
    // without this, subsecond compiles to `self.inner.call_it(args)`
    // and skips the JumpTable entirely (apply_patch becomes a no-op
    // from the caller's perspective). Tier 1 dev builds want the
    // JumpTable lookup but otherwise keep release-level optimization;
    // this flag flips the cfg without dropping to the dev profile.
    //
    // Pick the export-dynamic flag spelling for the *target* triple,
    // not the host — Apple linkers take `-export_dynamic`; GNU / lld
    // take `--export-dynamic`. Default to the GNU form when
    // target_triple is None (host-only setups land here).
    let export_dynamic = match c.target_triple.as_deref() {
        Some(t) if t.contains("apple") => "-Clink-arg=-Wl,-export_dynamic",
        _ => "-Clink-arg=-Wl,--export-dynamic",
    };
    let save_temps = format!("-Csave-temps=y -Cdebug-assertions=on {export_dynamic}");
    let save_temps = save_temps.as_str();
    match c.target_triple.as_deref() {
        Some(triple) => {
            out.push((target_linker_env_var(triple), shim));
            let prior = std::env::var(target_rustflags_env_var(triple)).unwrap_or_default();
            let mut rustflags = String::new();
            if !prior.is_empty() {
                rustflags.push_str(&prior);
                rustflags.push(' ');
            }
            rustflags.push_str(save_temps);
            out.push((target_rustflags_env_var(triple), rustflags));
        }
        None => {
            let prior = std::env::var("RUSTFLAGS").unwrap_or_default();
            let mut rustflags = String::new();
            if !prior.is_empty() {
                rustflags.push_str(&prior);
                rustflags.push(' ');
            }
            rustflags.push_str(&format!("-Clinker={shim} {save_temps}"));
            out.push(("RUSTFLAGS".into(), rustflags));
        }
    }
    out
}

/// Same uppercasing rule as [`target_linker_env_var`] but for the
/// `…_RUSTFLAGS` variant. Cargo applies these flags only when
/// building for the given triple, so they don't break host build
/// scripts.
pub fn target_rustflags_env_var(triple: &str) -> String {
    let mut s = String::with_capacity(triple.len() + 24);
    s.push_str("CARGO_TARGET_");
    for ch in triple.chars() {
        if ch.is_ascii_alphanumeric() {
            s.push(ch.to_ascii_uppercase());
        } else {
            s.push('_');
        }
    }
    s.push_str("_RUSTFLAGS");
    s
}

/// Translate a Rust target triple to the cargo env var that selects
/// its linker. Cargo's rule: uppercase the triple and replace
/// non-alphanumerics with `_`, then prepend `CARGO_TARGET_` and
/// append `_LINKER`.
///
/// e.g. `aarch64-linux-android` → `CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER`.
pub fn target_linker_env_var(triple: &str) -> String {
    let mut s = String::with_capacity(triple.len() + 22);
    s.push_str("CARGO_TARGET_");
    for ch in triple.chars() {
        if ch.is_ascii_alphanumeric() {
            s.push(ch.to_ascii_uppercase());
        } else {
            s.push('_');
        }
    }
    s.push_str("_LINKER");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shim_for_triple(triple: Option<&str>) -> CaptureShims {
        CaptureShims {
            rustc_shim: PathBuf::from("/tmp/rustc-shim"),
            linker_shim: PathBuf::from("/tmp/linker-shim"),
            rustc_cache_dir: PathBuf::from("/tmp/rustc-cache"),
            linker_cache_dir: PathBuf::from("/tmp/linker-cache"),
            real_linker: PathBuf::from("/usr/bin/cc"),
            target_triple: triple.map(String::from),
        }
    }

    #[test]
    fn target_linker_env_var_uppercases_and_replaces_separators() {
        assert_eq!(
            target_linker_env_var("aarch64-linux-android"),
            "CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER",
        );
    }

    #[test]
    fn target_rustflags_env_var_matches_cargo_convention() {
        assert_eq!(
            target_rustflags_env_var("aarch64-apple-ios-sim"),
            "CARGO_TARGET_AARCH64_APPLE_IOS_SIM_RUSTFLAGS",
        );
    }

    #[test]
    fn capture_env_vars_emits_workspace_wrapper_and_cache_dirs() {
        let vars = capture_env_vars(&shim_for_triple(Some("aarch64-linux-android")));
        let names: std::collections::HashSet<&str> = vars.iter().map(|(k, _)| k.as_str()).collect();
        assert!(names.contains("RUSTC_WORKSPACE_WRAPPER"));
        assert!(names.contains("WHISKER_RUSTC_CACHE_DIR"));
        assert!(names.contains("WHISKER_LINKER_CACHE_DIR"));
        assert!(names.contains("WHISKER_REAL_LINKER"));
        assert!(names.contains("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"));
        assert!(names.contains("CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS"));
    }

    #[test]
    fn capture_env_vars_picks_apple_export_dynamic_for_ios_triples() {
        let vars = capture_env_vars(&shim_for_triple(Some("aarch64-apple-ios-sim")));
        let rustflags = vars
            .iter()
            .find(|(k, _)| k == "CARGO_TARGET_AARCH64_APPLE_IOS_SIM_RUSTFLAGS")
            .map(|(_, v)| v.as_str())
            .unwrap();
        assert!(rustflags.contains("-Wl,-export_dynamic"));
        assert!(!rustflags.contains("-Wl,--export-dynamic"));
    }

    #[test]
    fn capture_env_vars_picks_gnu_export_dynamic_for_android_triples() {
        let vars = capture_env_vars(&shim_for_triple(Some("aarch64-linux-android")));
        let rustflags = vars
            .iter()
            .find(|(k, _)| k == "CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS")
            .map(|(_, v)| v.as_str())
            .unwrap();
        assert!(rustflags.contains("-Wl,--export-dynamic"));
    }

    #[test]
    fn capture_env_vars_no_triple_falls_back_to_global_rustflags() {
        let vars = capture_env_vars(&shim_for_triple(None));
        let names: std::collections::HashSet<&str> = vars.iter().map(|(k, _)| k.as_str()).collect();
        assert!(names.contains("RUSTFLAGS"));
        // Per-target keys should not appear.
        assert!(!names.iter().any(|k| k.contains("CARGO_TARGET_")));
    }
}
