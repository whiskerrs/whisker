//! Thin-rebuild driver — produce a patch dylib from a single
//! captured rustc invocation by editing as few of its args as
//! possible.
//!
//! ## Design principle: "minimal edit, verbatim everything else"
//!
//! Tuft does **not** want to re-derive linker / sysroot / SDK args
//! itself — those are the parts most likely to break across OS
//! versions, NDK upgrades, Xcode releases, glibc CSU layout
//! changes, and so on. Instead we capture cargo+rustc's full
//! invocation in I4g-4 and replay it here, **changing only the
//! handful of args that have to differ for a hot-patch dylib**:
//!
//!   - `--crate-type` is forced to `cdylib`. The original may have
//!     been `rlib` (a static archive of intermediate metadata) or
//!     `bin` / whatever — a hot-patch needs to be a relocatable
//!     shared library the device runtime can `dlopen`.
//!   - `--out-dir` is redirected to a session-local cache so the
//!     patch artifact doesn't clobber the original `target/`
//!     output.
//!
//! Everything else — target triple, sysroot, link-args, optimisation
//! level, `cfg` flags, `-L` search paths, `-l` link directives — is
//! preserved verbatim. That is the whole point: rustc + cargo
//! already know how to make the linker happy on this OS / SDK
//! combo, and we lean on that.

use std::path::{Path, PathBuf};

use super::wrapper::CapturedRustcInvocation;

/// What the dev-server will spawn to produce a patch dylib.
/// Pure-data; the runner side reads this and `Command::new("rustc")`
/// against it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThinRebuildPlan {
    /// Final argv to pass to rustc (the binary itself isn't in
    /// here; the runner picks the same rustc cargo would have).
    pub args: Vec<String>,
    /// Where the patch dylib will land (computed for the caller's
    /// convenience; equal to `<out_dir>/lib<crate>.{so,dylib}`).
    pub output_dir: PathBuf,
}

/// Build a [`ThinRebuildPlan`] from a captured invocation by
/// editing only the args that have to change.
///
/// `output_dir` is where the resulting patch dylib should land —
/// typically `target/.tuft/patches/<session-id>/`. The directory
/// must exist (or the runner must create it) before rustc is
/// invoked; this function only assembles arguments.
pub fn build_thin_rebuild_plan(
    captured: &CapturedRustcInvocation,
    output_dir: &Path,
) -> ThinRebuildPlan {
    let mut args = captured.args.clone();
    set_crate_type(&mut args, "cdylib");
    set_out_dir(&mut args, output_dir);
    ThinRebuildPlan {
        args,
        output_dir: output_dir.to_path_buf(),
    }
}

/// Force every `--crate-type` arg to a single value (`new_kind`).
/// rustc allows the flag to repeat (one binary can be multiple
/// crate-types in one invocation); for a hot-patch we always want
/// exactly one — `cdylib`. The fold-and-add behaviour is:
///
///   - every existing `--crate-type X` (separate or `=` form) is
///     stripped;
///   - one fresh `--crate-type <new_kind>` pair is appended at the
///     end.
///
/// This is more idempotent than "rewrite in place" — the result
/// is always a single contiguous pair regardless of how many
/// the input had.
pub fn set_crate_type(args: &mut Vec<String>, new_kind: &str) {
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--crate-type" && i + 1 < args.len() {
            args.drain(i..=i + 1);
            continue;
        }
        if arg.starts_with("--crate-type=") {
            args.remove(i);
            continue;
        }
        i += 1;
    }
    args.push("--crate-type".into());
    args.push(new_kind.into());
}

/// Redirect rustc's output directory. Same fold-and-add semantics
/// as [`set_crate_type`]: strip every existing form, append one
/// fresh pair. Handles `--out-dir <DIR>`, `--out-dir=<DIR>`, and
/// the `-o <PATH>` short form (rare in cargo invocations but
/// possible — we drop it because `--out-dir` wins for `--crate-type
/// cdylib`).
pub fn set_out_dir(args: &mut Vec<String>, dir: &Path) {
    let dir_str = dir.to_string_lossy().to_string();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if (arg == "--out-dir" || arg == "-o") && i + 1 < args.len() {
            args.drain(i..=i + 1);
            continue;
        }
        if arg.starts_with("--out-dir=") {
            args.remove(i);
            continue;
        }
        i += 1;
    }
    args.push("--out-dir".into());
    args.push(dir_str);
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

    // ----- set_crate_type ----------------------------------------------

    #[test]
    fn set_crate_type_replaces_a_single_existing_separated_pair() {
        let mut args = s(&["--edition=2021", "--crate-type", "rlib", "src/lib.rs"]);
        set_crate_type(&mut args, "cdylib");
        assert_eq!(
            args,
            s(&["--edition=2021", "src/lib.rs", "--crate-type", "cdylib"]),
        );
    }

    #[test]
    fn set_crate_type_replaces_the_equals_form() {
        let mut args = s(&["--crate-type=rlib", "--edition=2021"]);
        set_crate_type(&mut args, "cdylib");
        assert_eq!(args, s(&["--edition=2021", "--crate-type", "cdylib"]));
    }

    #[test]
    fn set_crate_type_collapses_multiple_existing_into_one() {
        // rustc allows `--crate-type rlib --crate-type cdylib` to
        // produce both at once. For a hot-patch we want exactly
        // one, regardless of how many came in.
        let mut args = s(&[
            "--crate-type", "rlib",
            "--crate-type", "dylib",
            "--crate-type=staticlib",
            "src/lib.rs",
        ]);
        set_crate_type(&mut args, "cdylib");
        assert_eq!(args, s(&["src/lib.rs", "--crate-type", "cdylib"]));
    }

    #[test]
    fn set_crate_type_appends_when_no_existing() {
        let mut args = s(&["--edition=2021", "src/lib.rs"]);
        set_crate_type(&mut args, "cdylib");
        assert_eq!(
            args,
            s(&["--edition=2021", "src/lib.rs", "--crate-type", "cdylib"]),
        );
    }

    // ----- set_out_dir -------------------------------------------------

    #[test]
    fn set_out_dir_replaces_separated_form() {
        let mut args = s(&["--out-dir", "/old/path", "src/lib.rs"]);
        set_out_dir(&mut args, Path::new("/new/path"));
        assert_eq!(args, s(&["src/lib.rs", "--out-dir", "/new/path"]));
    }

    #[test]
    fn set_out_dir_replaces_equals_form() {
        let mut args = s(&["--out-dir=/old/path", "src/lib.rs"]);
        set_out_dir(&mut args, Path::new("/new/path"));
        assert_eq!(args, s(&["src/lib.rs", "--out-dir", "/new/path"]));
    }

    #[test]
    fn set_out_dir_replaces_the_short_o_form() {
        let mut args = s(&["-o", "/old/file.rlib", "src/lib.rs"]);
        set_out_dir(&mut args, Path::new("/new/path"));
        assert_eq!(args, s(&["src/lib.rs", "--out-dir", "/new/path"]));
    }

    #[test]
    fn set_out_dir_appends_when_no_existing() {
        let mut args = s(&["src/lib.rs"]);
        set_out_dir(&mut args, Path::new("/new/path"));
        assert_eq!(args, s(&["src/lib.rs", "--out-dir", "/new/path"]));
    }

    // ----- build_thin_rebuild_plan -------------------------------------

    #[test]
    fn plan_preserves_arbitrary_other_args_verbatim() {
        // Unrelated args must come through untouched: target triple,
        // sysroot, -C flags, -L search paths, -l link directives,
        // cfg flags, --check-cfg, etc. Tuft does not interpret them.
        let captured = captured_with(s(&[
            "--edition=2021",
            "--crate-name", "demo",
            "--crate-type", "rlib",
            "--target", "aarch64-apple-darwin",
            "-C", "opt-level=3",
            "-C", "embed-bitcode=no",
            "-L", "dependency=/some/path",
            "-l", "iconv",
            "--cfg", "feature=\"alpha\"",
            "--check-cfg", "cfg(docsrs,test)",
            "--out-dir", "/cargo/target/debug/deps",
            "src/lib.rs",
        ]));
        let plan = build_thin_rebuild_plan(&captured, Path::new("/tuft/patches/x"));

        // crate-type was rewritten, out-dir was rewritten, all other
        // args are preserved in original order.
        assert_eq!(
            plan.args,
            s(&[
                "--edition=2021",
                "--crate-name", "demo",
                "--target", "aarch64-apple-darwin",
                "-C", "opt-level=3",
                "-C", "embed-bitcode=no",
                "-L", "dependency=/some/path",
                "-l", "iconv",
                "--cfg", "feature=\"alpha\"",
                "--check-cfg", "cfg(docsrs,test)",
                "src/lib.rs",
                "--crate-type", "cdylib",
                "--out-dir", "/tuft/patches/x",
            ]),
        );
        assert_eq!(plan.output_dir, Path::new("/tuft/patches/x"));
    }

    #[test]
    fn plan_handles_an_input_with_no_out_dir_or_crate_type() {
        let captured = captured_with(s(&["--edition=2021", "src/lib.rs"]));
        let plan = build_thin_rebuild_plan(&captured, Path::new("/tmp/p"));
        assert_eq!(
            plan.args,
            s(&[
                "--edition=2021",
                "src/lib.rs",
                "--crate-type", "cdylib",
                "--out-dir", "/tmp/p",
            ]),
        );
    }

    #[test]
    fn plan_is_idempotent_on_a_hot_patch_re_run() {
        // Running build_thin_rebuild_plan on its own output should
        // produce the same args (no duplication of --crate-type or
        // --out-dir).
        let captured = captured_with(s(&["src/lib.rs"]));
        let plan1 = build_thin_rebuild_plan(&captured, Path::new("/p"));
        let plan2 = build_thin_rebuild_plan(
            &CapturedRustcInvocation {
                crate_name: captured.crate_name.clone(),
                args: plan1.args.clone(),
                timestamp_micros: 0,
            },
            Path::new("/p"),
        );
        assert_eq!(plan1.args, plan2.args);
    }
}
