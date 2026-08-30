//! hot reload fat-build capture shim wiring.
//!
//! When the dev-server runs a full reload for hot-reload, it
//! transparently elevates that build into a **fat build**: cargo
//! still produces the same artifact, but the rustc and linker
//! invocations get intercepted by [`whisker-rustc-shim`] and
//! [`whisker-linker-shim`] respectively, which dump their argv to
//! JSON files under the configured cache dirs. The hot reload thin
//! rebuild later replays those argvs to produce a patch dylib.
//!
//! The setup is just env vars (cargo's RUSTC_WORKSPACE_WRAPPER +
//! per-target linker overrides). [`capture_env_vars`] computes the
//! map; callers merge it into their `Command`.

use std::path::PathBuf;

/// Shim wiring that turns a plain cargo invocation into a hot reload
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
/// `RUSTFLAGS=-Clinker=…` (fine for host-only hot reload setups).
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
    capture_env_vars_for_triple(c, c.target_triple.as_deref())
}

/// Capture a generated Host project and all of its path dependencies.
///
/// Cargo's `RUSTC_WORKSPACE_WRAPPER` only sees members of the generated
/// workspace. Desktop applications keep user code as a normal path
/// dependency, so their fat build uses `RUSTC_WRAPPER` while retaining the
/// same linker and codegen flags.
pub fn capture_env_vars_all_crates(c: &CaptureShims) -> Vec<(String, String)> {
    capture_env_vars(c)
        .into_iter()
        .map(|(key, value)| {
            if key == "RUSTC_WORKSPACE_WRAPPER" {
                ("RUSTC_WRAPPER".to_string(), value)
            } else {
                (key, value)
            }
        })
        .collect()
}

/// Like [`capture_env_vars`] but applies the linker shim + rustflags
/// to `triple_override` instead of `c.target_triple`. Multi-triple
/// builds (iOS emits dylibs for device + intel-sim + arm64-sim) need
/// the capture envelope on *every* slice, and
/// `CaptureShims::target_triple` carries one slot: calling
/// [`capture_env_vars`] instead leaves the other slices without the
/// rustflags below, so hot-reload dispatch on them silently keeps
/// running the old code.
pub fn capture_env_vars_for_triple(
    c: &CaptureShims,
    triple_override: Option<&str>,
) -> Vec<(String, String)> {
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
    // Three flags every fat build needs for hot reload to work:
    //
    // `-Csave-temps=y` keeps rustc's temp dir — holding the version
    // script and bridge-static archive the captured linker argv
    // references — alive past the build, without which that argv is
    // unreplayable.
    //
    // `-Wl,--export-dynamic` puts the cdylib's internal symbols in its
    // dynamic-symbol table. The patch dylib resolves `std::fmt`,
    // `alloc`, … against the loaded process at `dlopen` time, which
    // cdylib's default visibility would hide. Costs a larger .so.
    //
    // `-Cdebug-assertions=on` selects the `cfg!(debug_assertions)`
    // branch of `subsecond::HotFn::try_call`; the other branch calls
    // `self.inner.call_it(args)` directly and never consults the
    // JumpTable, making `apply_patch` a no-op.
    //
    // The spelling follows the *target* linker: Apple takes
    // `-export_dynamic`, GNU / lld `--export-dynamic`.
    let export_dynamic = match triple_override {
        Some(t) if t.contains("apple") => "-Clink-arg=-Wl,-export_dynamic",
        _ => "-Clink-arg=-Wl,--export-dynamic",
    };
    // `-Copt-level=0` must match the thin patch's opt-level (forced to 0
    // by `hotpatch::thin_build::override_opt_level`). Optimized, the host
    // inlines each `#[component]`'s `__hot::call(move ||{…})` dispatch
    // closure, so the `HotFunction::call_it` / `call_as_ptr` symbol the
    // JumpTable is keyed on is missing or mangled differently from the
    // patch's — lookups miss and the component keeps running old code.
    let save_temps = format!("-Csave-temps=y -Cdebug-assertions=on -Copt-level=0 {export_dynamic}");
    let save_temps = save_temps.as_str();
    match triple_override {
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
    fn all_crates_capture_replaces_only_the_workspace_wrapper() {
        let vars = capture_env_vars_all_crates(&shim_for_triple(Some("aarch64-apple-darwin")));
        let names: std::collections::HashSet<&str> = vars.iter().map(|(k, _)| k.as_str()).collect();
        assert!(names.contains("RUSTC_WRAPPER"));
        assert!(!names.contains("RUSTC_WORKSPACE_WRAPPER"));
        assert!(names.contains("WHISKER_RUSTC_CACHE_DIR"));
        assert!(names.contains("CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER"));
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
        assert!(!names.iter().any(|k| k.contains("CARGO_TARGET_")));
    }
}
