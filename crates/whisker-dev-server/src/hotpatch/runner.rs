//! Spawn-side companions to [`super::thin_build::build_obj_plan`]
//! and [`super::link_plan::build_link_plan`].
//!
//! Two stages, one helper that wires them together:
//!
//!   - [`run_obj_plan`] — invoke rustc with an [`ObjBuildPlan`] and
//!     return the paths of the emitted object files.
//!   - [`run_link_plan`] — invoke the linker driver with a
//!     [`LinkPlan`] and return the path of the produced
//!     `.so` / `.dylib`.
//!   - [`thin_rebuild_obj`] — composes both: build the obj plan,
//!     spawn rustc, build the link plan, spawn the linker, return
//!     the dylib path.
//!
//! All three inherit stdout/stderr so compile / link errors land
//! in the dev-server's terminal instead of being swallowed.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::link_plan::{LinkPlan, LinkerOs, build_link_plan};
use super::thin_build::{ObjBuildPlan, build_obj_plan};
use super::wrapper::CapturedRustcInvocation;

/// Marker error: rustc was spawned fine and exited 1 — i.e. it
/// *rejected the code* and printed its own diagnostics (stderr is
/// inherited, so they're already on the dev terminal). The change
/// loop downcasts to this to distinguish "the user's edit doesn't
/// compile" (a full reload cargo build would fail identically after a
/// much longer wait — don't bother) from infrastructure failures
/// like a missing capture cache or a broken link line (where a cold
/// rebuild genuinely can recover).
///
/// Exit codes other than 1 (ICE = 101, killed by signal) stay on the
/// generic error path: they don't prove anything about the code.
#[derive(Debug)]
pub struct RustcRejectedCode;

impl std::fmt::Display for RustcRejectedCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "rustc rejected the code (exit 1); diagnostics above")
    }
}

impl std::error::Error for RustcRejectedCode {}

/// Spawn rustc with `plan.args` from `cwd`. On success, returns the
/// codegen-unit objects it wrote into `plan.object_dir`, sorted.
pub async fn run_obj_plan(
    plan: &ObjBuildPlan,
    rustc_path: &Path,
    cwd: &Path,
) -> Result<Vec<PathBuf>> {
    // rustc names every unit object with a per-invocation suffix, so
    // the previous build's objects would be linked beside the new ones.
    match std::fs::remove_dir_all(&plan.object_dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(e).with_context(|| format!("clear {}", plan.object_dir.display()));
        }
    }
    std::fs::create_dir_all(&plan.object_dir)
        .with_context(|| format!("create out dir {}", plan.object_dir.display()))?;
    // `plan.envs` replays the `CARGO_*` / `OUT_DIR` vars captured
    // during the fat build — without them any `env!("CARGO_PKG_*")`
    // in the user's code fails to compile under this raw (cargo-less)
    // rustc spawn.
    let status = tokio::process::Command::new(rustc_path)
        .args(&plan.args)
        .envs(&plan.envs)
        .current_dir(cwd)
        .status()
        .await
        .with_context(|| format!("spawn {}", rustc_path.display()))?;
    if !status.success() {
        if status.code() == Some(1) {
            return Err(anyhow::Error::new(RustcRejectedCode));
        }
        anyhow::bail!(
            "rustc exited {} during obj rebuild (out_dir={})",
            status,
            plan.object_dir.display(),
        );
    }
    let objects = codegen_unit_objects(&plan.object_dir)?;
    if objects.is_empty() {
        anyhow::bail!(
            "rustc succeeded but wrote no objects into `{}`",
            plan.object_dir.display(),
        );
    }
    Ok(objects)
}

fn codegen_unit_objects(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut objects = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let path = entry
            .with_context(|| format!("read {}", dir.display()))?
            .path();
        // A crate that fits in one unit also gets a merged `<crate>.o`
        // holding the same symbols as its `.rcgu.o`.
        let is_unit = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".rcgu.o"));
        if is_unit {
            objects.push(path);
        }
    }
    objects.sort();
    Ok(objects)
}

/// Spawn the linker driver with `plan.args` from `cwd`. On success,
/// returns the path of the produced shared object (= `plan.output`).
///
/// `linker_path` is typically the same `cc`/`clang` rustc would use.
/// On macOS, `xcrun -f clang` resolves to the active toolchain's
/// driver. On Linux/Android, the NDK ships a per-API-level wrapper
/// (e.g. `aarch64-linux-android21-clang`).
pub async fn run_link_plan(plan: &LinkPlan, linker_path: &Path, cwd: &Path) -> Result<PathBuf> {
    if let Some(parent) = plan.output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create out dir {}", parent.display()))?;
    }
    // Capture stderr so a failed link surfaces *why* (e.g. unresolved
    // symbols, missing libraries) instead of just "exit 1". stdout is
    // inherited so progress / warnings remain visible.
    let out = tokio::process::Command::new(linker_path)
        .args(&plan.args)
        .current_dir(cwd)
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .with_context(|| format!("spawn {}", linker_path.display()))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!(
            "linker `{}` exited {} during patch link (output={})\n\
             argv: {:?}\n\
             stderr:\n{}",
            linker_path.display(),
            out.status,
            plan.output.display(),
            plan.args,
            stderr.trim_end(),
        );
    }
    if !plan.output.is_file() {
        anyhow::bail!(
            "linker succeeded but `{}` was not produced",
            plan.output.display(),
        );
    }
    Ok(plan.output.clone())
}

/// Compose [`run_obj_plan`] and [`run_link_plan`] into the full
/// hot-patch rebuild.
///
/// Inputs are the **captured** rustc + linker calls from the fat
/// build, plus where the patch should land and which OS the patch
/// is going to run on. Returns the final `.so`/`.dylib` path that
/// can be packaged into a `JumpTable` and sent to the device.
///
/// This function is the "happy path" — `Patcher` calls it directly
/// when neither captured call is missing and the target is supported.
///
/// `aslr_stub` is an optional pre-built jump-stub object
/// ([`crate::hotpatch::create_undefined_symbol_stub`]). When `Some`,
/// it gets linked into the patch dylib alongside the freshly rebuilt
/// objects, supplying every host symbol the patch references as a
/// hardcoded runtime-address trampoline. When `None`, the patch is
/// linked with `--unresolved-symbols=ignore-all` only — fine for
/// host-only fixtures where the test process satisfies the patch via
/// `dynamic_lookup`.
#[allow(clippy::too_many_arguments)]
pub async fn thin_rebuild_obj(
    captured_rustc: &CapturedRustcInvocation,
    captured_linker_args: &[String],
    output_dir: &Path,
    output_dylib: &Path,
    rustc_path: &Path,
    linker_path: &Path,
    cwd: &Path,
    target_os: LinkerOs,
    aslr_stub: Option<&Path>,
) -> Result<PathBuf> {
    let obj_plan = build_obj_plan(captured_rustc, output_dir);
    let objects = run_obj_plan(&obj_plan, rustc_path, cwd).await?;
    let extras: Vec<PathBuf> = aslr_stub.map(|p| vec![p.to_path_buf()]).unwrap_or_default();
    let link_plan = build_link_plan(
        captured_linker_args,
        &objects,
        output_dylib,
        target_os,
        &extras,
        &[],
    );
    run_link_plan(&link_plan, linker_path, cwd).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn unique_tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let p = std::env::temp_dir().join(format!("whisker-runner-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    // ----- run_obj_plan ------------------------------------------------

    #[tokio::test]
    async fn run_obj_plan_reports_rejected_code_when_rustc_exits_1() {
        // Plan that will fail because the source file doesn't exist —
        // rustc runs, prints a diagnostic, and exits 1, which is the
        // same shape as a compile error in real user code. The change
        // loop relies on downcasting to `RustcRejectedCode` here to
        // skip the pointless full reload fallback.
        let dir = unique_tempdir();
        let plan = ObjBuildPlan {
            envs: Default::default(),
            args: s(&[
                "--edition=2021",
                "--crate-name",
                "demo",
                "--crate-type",
                "rlib",
                "--out-dir",
                dir.join("objects").to_string_lossy().as_ref(),
                "/nope/this/source/does/not/exist.rs",
                "--emit",
                "obj",
            ]),
            object_dir: dir.join("objects"),
        };
        let res = run_obj_plan(&plan, Path::new("rustc"), &dir).await;
        let err = res.unwrap_err();
        assert!(
            err.downcast_ref::<RustcRejectedCode>().is_some(),
            "exit 1 should downcast to RustcRejectedCode: {err:#}",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_obj_plan_compile_error_downcasts_through_context() {
        // Same as above but with a real source file containing a type
        // error, and with a `.context(...)` layer like the Patcher
        // adds — downcast_ref must still find the marker through the
        // context chain.
        let dir = unique_tempdir();
        let src = dir.join("bad.rs");
        std::fs::write(&src, "pub fn broken() -> u32 { \"not a number\" }\n").unwrap();
        let plan = ObjBuildPlan {
            envs: Default::default(),
            args: s(&[
                "--edition=2021",
                "--crate-name",
                "bad",
                "--crate-type",
                "rlib",
                "--out-dir",
                dir.join("objects").to_string_lossy().as_ref(),
                src.to_string_lossy().as_ref(),
                "--emit",
                "obj",
            ]),
            object_dir: dir.join("objects"),
        };
        let res = run_obj_plan(&plan, Path::new("rustc"), &dir)
            .await
            .context("rustc --emit=obj for thin patch");
        let err = res.unwrap_err();
        assert!(
            err.downcast_ref::<RustcRejectedCode>().is_some(),
            "compile error should downcast through context: {err:#}",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_obj_plan_errors_when_rustc_binary_doesnt_exist() {
        let dir = unique_tempdir();
        let plan = ObjBuildPlan {
            envs: Default::default(),
            args: vec![],
            object_dir: dir.join("objects"),
        };
        let res = run_obj_plan(&plan, Path::new("/nope/no-such-rustc"), &dir).await;
        assert!(res.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_obj_plan_returns_only_this_builds_unit_objects() {
        let dir = unique_tempdir();
        let src = dir.join("lib.rs");
        std::fs::write(
            &src,
            "pub mod a { pub fn one() -> u32 { 1 } }\npub mod b { pub fn two() -> u32 { 2 } }\n",
        )
        .unwrap();
        let captured = CapturedRustcInvocation {
            crate_name: "units".into(),
            args: s(&[
                "--edition=2021",
                "--crate-name",
                "units",
                src.to_string_lossy().as_ref(),
            ]),
            envs: Default::default(),
            timestamp_micros: 0,
        };
        let plan = build_obj_plan(&captured, &dir);

        let first = run_obj_plan(&plan, Path::new("rustc"), &dir).await.unwrap();
        assert!(first.len() > 1, "one object per module: {first:?}");
        assert!(
            first
                .iter()
                .all(|p| p.to_string_lossy().ends_with(".rcgu.o")),
            "{first:?}",
        );

        // rustc renames every unit object on each run; a second build
        // must not hand the first build's objects to the linker too.
        let second = run_obj_plan(&plan, Path::new("rustc"), &dir).await.unwrap();
        assert_eq!(second.len(), first.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_obj_plan_skips_the_merged_object_of_a_single_unit_crate() {
        let dir = unique_tempdir();
        let src = dir.join("lib.rs");
        std::fs::write(&src, "pub fn one() -> u32 { 1 }\n").unwrap();
        let captured = CapturedRustcInvocation {
            crate_name: "single".into(),
            args: s(&[
                "--edition=2021",
                "--crate-name",
                "single",
                src.to_string_lossy().as_ref(),
            ]),
            envs: Default::default(),
            timestamp_micros: 0,
        };
        let plan = build_obj_plan(&captured, &dir);
        let objects = run_obj_plan(&plan, Path::new("rustc"), &dir).await.unwrap();
        assert_eq!(objects.len(), 1, "{objects:?}");
        assert!(plan.object_dir.join("single.o").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ----- run_link_plan -----------------------------------------------

    #[tokio::test]
    async fn run_link_plan_creates_output_parent_dir() {
        // The linker call will fail (we use /usr/bin/true so it
        // returns success without writing anything), then run_link_plan
        // surfaces "succeeded but file not produced". The test's
        // load-bearing claim is "parent dir was created up front
        // even though the call ultimately failed".
        let dir = unique_tempdir();
        let nested_out = dir.join("nested/sub").join("libfoo.dylib");
        let plan = LinkPlan {
            args: vec![],
            output: nested_out.clone(),
        };
        let res = run_link_plan(&plan, Path::new("/usr/bin/true"), &dir).await;
        assert!(res.is_err(), "true returns success but writes no file");
        let parent = nested_out.parent().unwrap();
        assert!(parent.is_dir(), "parent should have been created");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn run_link_plan_surfaces_linker_nonzero_exit() {
        // /usr/bin/false exits 1 — we want a clear error, not a
        // "file not found" misdirection.
        let dir = unique_tempdir();
        let plan = LinkPlan {
            args: vec![],
            output: dir.join("libfoo.dylib"),
        };
        let res = run_link_plan(&plan, Path::new("/usr/bin/false"), &dir).await;
        let err = res.unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("linker"), "msg: {msg}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
