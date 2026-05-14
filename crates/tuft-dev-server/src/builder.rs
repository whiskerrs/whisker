//! Tier 2 cold rebuild: spawn cargo / xtask to produce a fresh
//! artifact for the active [`Target`].
//!
//! For Tier 2 (`HotPatchMode::Tier2ColdRebuild`) this module just
//! shells out to cargo / xtask and produces a fresh artifact. When
//! Tier 1 is active, the same build doubles as the **fat build**
//! that captures rustc + linker invocations for the hot-patch
//! pipeline — the dev loop calls [`Builder::with_capture`] with
//! the shim paths and cache dirs before the initial build, and
//! cargo runs the shims transparently via env vars (no command
//! line change).

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::Target;

/// Shim wiring that turns a [`Builder::build`] into a Tier 1 fat
/// build. All paths are absolute; the dev-server creates the cache
/// dirs on demand. `real_linker` is what the linker shim forwards
/// to (typically the same `cc`/`clang` cargo would have used).
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

/// Builds the artifact appropriate for `target`. For host targets
/// that's a plain `cargo build -p`; for device targets we lean on
/// the existing `xtask` orchestration so we don't duplicate the
/// NDK / xcframework dance.
pub struct Builder {
    workspace_root: PathBuf,
    package: String,
    target: Target,
    /// Cargo features forwarded to whichever compilation step
    /// actually compiles the user crate. For Tier 2 development
    /// builds the dev loop turns on `tuft/hot-reload`.
    features: Vec<String>,
    /// `Some` → set the rustc + linker shim envs on every spawn so
    /// the resulting cargo invocation is also a fat build. `None`
    /// for plain Tier 2.
    capture: Option<CaptureShims>,
}

impl Builder {
    pub fn new(workspace_root: PathBuf, package: String, target: Target) -> Self {
        Self {
            workspace_root,
            package,
            target,
            features: Vec::new(),
            capture: None,
        }
    }

    pub fn with_features(mut self, features: Vec<String>) -> Self {
        self.features = features;
        self
    }

    /// Install both shims into every spawn. When set, the build
    /// fills the configured cache dirs with rustc + linker JSON
    /// captures while otherwise producing the same artifact a
    /// plain `cargo build` would. Pre-existing `RUSTFLAGS` are
    /// preserved (the `-C linker=…` is prepended).
    pub fn with_capture(mut self, capture: CaptureShims) -> Self {
        self.capture = Some(capture);
        self
    }

    /// Run the build. Inherits stdout/stderr so cargo's own progress
    /// output is visible in the dev server's terminal.
    pub async fn build(&self) -> Result<()> {
        let plan = self.plan();
        let mut cmd = Command::new(&plan.program);
        cmd.args(&plan.args).current_dir(&self.workspace_root);

        if let Some(c) = &self.capture {
            std::fs::create_dir_all(&c.rustc_cache_dir).with_context(|| {
                format!(
                    "create rustc cache dir {}",
                    c.rustc_cache_dir.display(),
                )
            })?;
            std::fs::create_dir_all(&c.linker_cache_dir).with_context(|| {
                format!(
                    "create linker cache dir {}",
                    c.linker_cache_dir.display(),
                )
            })?;
            for (k, v) in capture_env_vars(c) {
                cmd.env(k, v);
            }
        }

        let status = cmd
            .status()
            .await
            .with_context(|| format!("spawn {}", plan.program))?;
        if !status.success() {
            anyhow::bail!("{} exited {}", plan.program, status);
        }
        Ok(())
    }

    /// Pure-function side of [`build`]: derive the (program, args)
    /// pair from `target` + `package` + `features`. Factored out so
    /// unit tests don't have to actually run cargo.
    pub fn plan(&self) -> BuildPlan {
        plan_for(&self.package, self.target, &self.features)
    }

    /// Whether this builder is currently configured for a fat build.
    pub fn captures_shims(&self) -> bool {
        self.capture.is_some()
    }
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
            "TUFT_RUSTC_CACHE_DIR".into(),
            c.rustc_cache_dir.to_string_lossy().into(),
        ),
        (
            "TUFT_LINKER_CACHE_DIR".into(),
            c.linker_cache_dir.to_string_lossy().into(),
        ),
        (
            "TUFT_REAL_LINKER".into(),
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
    let save_temps =
        format!("-Csave-temps=y -Cdebug-assertions=on {export_dynamic}");
    let save_temps = save_temps.as_str();
    match c.target_triple.as_deref() {
        Some(triple) => {
            out.push((target_linker_env_var(triple), shim));
            let prior =
                std::env::var(target_rustflags_env_var(triple)).unwrap_or_default();
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

/// What command the dev loop will spawn to produce a fresh artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlan {
    pub program: String,
    pub args: Vec<String>,
}

fn plan_for(package: &str, target: Target, features: &[String]) -> BuildPlan {
    match target {
        Target::Host => {
            let mut args = vec!["build".into(), "-p".into(), package.to_string()];
            push_features(&mut args, features);
            BuildPlan { program: "cargo".into(), args }
        }
        Target::Android => {
            // Reuse the existing xtask orchestration:
            //   cargo xtask android build-example -p <package>
            // Feature plumbing into the cdylib is I4f; for now we
            // just smuggle the requested features into a flag the
            // xtask is going to learn to consume.
            let mut args = vec![
                "xtask".into(),
                "android".into(),
                "build-example".into(),
                "-p".into(),
                package.to_string(),
            ];
            push_features(&mut args, features);
            BuildPlan { program: "cargo".into(), args }
        }
        Target::IosSimulator => {
            // Likewise:
            //   cargo xtask ios build-xcframework -p <package>
            // The actual Simulator install + launch happens in the
            // installer; we only need a fresh xcframework here.
            let mut args = vec![
                "xtask".into(),
                "ios".into(),
                "build-xcframework".into(),
                "-p".into(),
                package.to_string(),
            ];
            push_features(&mut args, features);
            BuildPlan { program: "cargo".into(), args }
        }
    }
}

fn push_features(args: &mut Vec<String>, features: &[String]) {
    for f in features {
        args.push("--features".into());
        args.push(f.clone());
    }
}

// Cheap helper so the installer module doesn't need its own
// path-resolution logic.
pub(crate) fn android_apk_path(workspace_root: &Path, package: &str) -> PathBuf {
    workspace_root
        .join("examples")
        .join(package)
        .join("android/app/build/outputs/apk/debug/app-debug.apk")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn b(target: Target) -> Builder {
        Builder::new(
            PathBuf::from("/tmp/ws"),
            "hello-world".into(),
            target,
        )
    }

    #[test]
    fn host_plan_calls_cargo_build_for_the_package() {
        let p = b(Target::Host).plan();
        assert_eq!(p.program, "cargo");
        assert_eq!(p.args, ["build", "-p", "hello-world"]);
    }

    #[test]
    fn android_plan_invokes_xtask_build_example() {
        let p = b(Target::Android).plan();
        assert_eq!(p.program, "cargo");
        assert_eq!(
            p.args,
            ["xtask", "android", "build-example", "-p", "hello-world"],
        );
    }

    #[test]
    fn ios_plan_invokes_xtask_build_xcframework() {
        let p = b(Target::IosSimulator).plan();
        assert_eq!(p.program, "cargo");
        assert_eq!(
            p.args,
            ["xtask", "ios", "build-xcframework", "-p", "hello-world"],
        );
    }

    #[test]
    fn features_are_forwarded_repeated_per_feature() {
        let p = b(Target::Host)
            .with_features(vec!["tuft/hot-reload".into(), "extra".into()])
            .plan();
        assert_eq!(
            p.args,
            [
                "build",
                "-p",
                "hello-world",
                "--features",
                "tuft/hot-reload",
                "--features",
                "extra",
            ],
        );
    }

    #[test]
    fn android_apk_path_is_under_examples_with_debug_suffix() {
        let p = android_apk_path(Path::new("/tmp/ws"), "hello-world");
        assert!(p.to_string_lossy().ends_with(
            "/examples/hello-world/android/app/build/outputs/apk/debug/app-debug.apk"
        ));
    }

    // ----- capture_env_vars + with_capture -----------------------------

    fn sample_capture() -> CaptureShims {
        CaptureShims {
            rustc_shim: PathBuf::from("/bin/tuft-rustc-shim"),
            linker_shim: PathBuf::from("/bin/tuft-linker-shim"),
            rustc_cache_dir: PathBuf::from("/cache/rustc"),
            linker_cache_dir: PathBuf::from("/cache/linker"),
            real_linker: PathBuf::from("/usr/bin/cc"),
            target_triple: None,
        }
    }

    fn sample_capture_for(triple: &str) -> CaptureShims {
        CaptureShims {
            target_triple: Some(triple.into()),
            ..sample_capture()
        }
    }

    fn env_map(c: &CaptureShims) -> std::collections::HashMap<String, String> {
        capture_env_vars(c).into_iter().collect()
    }

    #[test]
    fn capture_env_vars_sets_both_shim_envs_and_cache_dirs() {
        let c = sample_capture();
        let m = env_map(&c);
        assert_eq!(m["RUSTC_WORKSPACE_WRAPPER"], "/bin/tuft-rustc-shim");
        assert_eq!(m["TUFT_RUSTC_CACHE_DIR"], "/cache/rustc");
        assert_eq!(m["TUFT_LINKER_CACHE_DIR"], "/cache/linker");
        assert_eq!(m["TUFT_REAL_LINKER"], "/usr/bin/cc");
    }

    #[test]
    fn capture_env_vars_includes_dash_c_linker_in_rustflags() {
        // RUSTFLAGS isn't easy to mutate across threads in tests
        // (env is process-wide), so just check the produced value
        // contains the linker flag — the prior-value preservation is
        // tested separately by a serial test below.
        let c = sample_capture();
        let m = env_map(&c);
        assert!(
            m["RUSTFLAGS"].contains("-Clinker=/bin/tuft-linker-shim"),
            "RUSTFLAGS missing -Clinker: {}",
            m["RUSTFLAGS"],
        );
    }

    #[test]
    fn capture_env_vars_preserves_existing_rustflags_when_set() {
        // Process-wide env is risky to mutate in parallel tests, so
        // synchronise on a mutex to keep this single test serial
        // with respect to itself; it's the only test that sets
        // RUSTFLAGS.
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        let _g = LOCK.lock().unwrap();

        let prev = std::env::var_os("RUSTFLAGS");
        std::env::set_var("RUSTFLAGS", "--cfg=existing_flag");
        let m = env_map(&sample_capture());
        match prev {
            Some(p) => std::env::set_var("RUSTFLAGS", p),
            None => std::env::remove_var("RUSTFLAGS"),
        }

        assert!(
            m["RUSTFLAGS"].starts_with("--cfg=existing_flag "),
            "prior flag not preserved: {}",
            m["RUSTFLAGS"],
        );
        assert!(m["RUSTFLAGS"].contains("-Clinker=/bin/tuft-linker-shim"));
    }

    #[test]
    fn with_capture_marks_builder_as_capturing() {
        let plain = b(Target::Host);
        assert!(!plain.captures_shims());
        let wrapped = b(Target::Host).with_capture(sample_capture());
        assert!(wrapped.captures_shims());
    }

    // ----- target_linker_env_var ---------------------------------------

    #[test]
    fn target_linker_env_var_uppercases_and_underscores_the_triple() {
        assert_eq!(
            target_linker_env_var("aarch64-linux-android"),
            "CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER",
        );
        assert_eq!(
            target_linker_env_var("x86_64-linux-android"),
            "CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER",
        );
        assert_eq!(
            target_linker_env_var("armv7-linux-androideabi"),
            "CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER",
        );
        assert_eq!(
            target_linker_env_var("aarch64-apple-darwin"),
            "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER",
        );
    }

    #[test]
    fn target_specific_capture_uses_cargo_target_linker_not_rustflags() {
        // The key piece: with a target_triple set, we DO NOT put
        // `-Clinker` into RUSTFLAGS — that would apply to host
        // build scripts too and break cross-compilation.
        let m = env_map(&sample_capture_for("aarch64-linux-android"));
        assert_eq!(
            m["CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"],
            "/bin/tuft-linker-shim",
        );
        assert!(
            !m.contains_key("RUSTFLAGS"),
            "RUSTFLAGS should not be set when target_triple is Some: {:?}",
            m,
        );
    }

    #[test]
    fn target_specific_capture_sets_save_temps_in_target_rustflags() {
        // The captured linker invocation references rustc's temp
        // dir; without -Csave-temps it gets cleaned up before we
        // can replay. Verify the right env var carries it.
        let m = env_map(&sample_capture_for("aarch64-linux-android"));
        assert!(
            m["CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS"].contains("save-temps=y"),
            "expected save-temps in target RUSTFLAGS, got {:?}",
            m,
        );
    }

    #[test]
    fn host_only_capture_combines_clinker_and_save_temps_in_rustflags() {
        // No target_triple → both go into the global RUSTFLAGS,
        // which is fine for host-only (no cross-build scripts to
        // break).
        let m = env_map(&sample_capture());
        assert!(m["RUSTFLAGS"].contains("-Clinker="));
        assert!(m["RUSTFLAGS"].contains("save-temps=y"));
    }

    #[test]
    fn target_rustflags_env_var_uppercases_like_the_linker_one() {
        assert_eq!(
            target_rustflags_env_var("aarch64-linux-android"),
            "CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS",
        );
        assert_eq!(
            target_rustflags_env_var("armv7-linux-androideabi"),
            "CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_RUSTFLAGS",
        );
    }

    #[test]
    fn target_specific_capture_still_sets_the_shared_envs() {
        // RUSTC_WORKSPACE_WRAPPER and the cache dirs are common to
        // both forms.
        let m = env_map(&sample_capture_for("aarch64-linux-android"));
        assert_eq!(m["RUSTC_WORKSPACE_WRAPPER"], "/bin/tuft-rustc-shim");
        assert_eq!(m["TUFT_RUSTC_CACHE_DIR"], "/cache/rustc");
        assert_eq!(m["TUFT_LINKER_CACHE_DIR"], "/cache/linker");
        assert_eq!(m["TUFT_REAL_LINKER"], "/usr/bin/cc");
    }
}
