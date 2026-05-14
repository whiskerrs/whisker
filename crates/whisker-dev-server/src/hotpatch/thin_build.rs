//! Thin-rebuild driver — produce a patch dylib from a single
//! captured rustc invocation by editing as few of its args as
//! possible.
//!
//! ## Design principle: "minimal edit, verbatim everything else"
//!
//! Whisker does **not** want to re-derive linker / sysroot / SDK args
//! itself — those are the parts most likely to break across OS
//! versions, NDK upgrades, Xcode releases, glibc CSU layout
//! changes, and so on. Instead we capture cargo+rustc's full
//! invocation in I4g-4 and replay it here, **changing only the
//! handful of args that have to differ for a hot-patch dylib**:
//!
//!   - `--crate-type` is forced to `rlib` so rustc emits an object
//!     file containing every `pub fn`'s mangled symbol (cdylib
//!     would strip them — see I4g-6 pivot).
//!   - `--emit` is forced to `obj` so we get a single `.o` we can
//!     hand to the linker ourselves.
//!   - `--out-dir` is redirected to a session-local cache so the
//!     patch artifact doesn't clobber the original `target/`
//!     output.
//!
//! Everything else — target triple, sysroot, link-args, optimisation
//! level, `cfg` flags, `-L` search paths, `-l` link directives — is
//! preserved verbatim. That is the whole point: rustc + cargo
//! already know how to make the linker happy on this OS / SDK
//! combo, and we lean on that.
//!
//! After rustc emits the `.o`, [`build_link_plan`] (X2b) takes the
//! captured **linker** invocation, drops its object inputs (we have
//! a fresh one), substitutes our `.o` and `-o`, and adds
//! `-undefined dynamic_lookup` (macOS) /
//! `--unresolved-symbols=ignore-all` (Linux) so unresolved symbols
//! are deferred to the host process at `dlopen` time. The result is
//! a `.so` / `.dylib` that re-exports back into the original binary
//! for everything except the patched function bodies — exactly what
//! `subsecond::apply_patch` expects.

use std::path::{Path, PathBuf};

use super::wrapper::CapturedRustcInvocation;

/// What [`build_obj_plan`] returns — captured rustc args, edited so
/// that running `rustc` with them produces a single `.o` containing
/// every `pub fn`'s mangled symbol.
///
/// `output_dir` is the directory rustc will write the object into;
/// the actual filename rustc emits is `<crate_name>.o` (with the
/// usual hyphen → underscore translation). `expected_object` is the
/// absolute path the runner should expect to see after the call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjBuildPlan {
    pub args: Vec<String>,
    pub output_dir: PathBuf,
    pub expected_object: PathBuf,
}

/// Object filename rustc emits for `--emit=obj --crate-type=rlib`:
/// `<crate>.o` with hyphens converted to underscores. (Notably no
/// `lib` prefix and no extension other than `.o` — cdylib's
/// `lib<crate>.dylib` rules don't apply here.)
pub fn object_filename(crate_name: &str) -> String {
    let stem = crate_name.replace('-', "_");
    format!("{stem}.o")
}

/// Edit a captured rustc invocation so that running it produces an
/// object file containing every `pub fn`'s mangled symbol — the
/// input to the linker step in [`build_link_plan`].
///
/// Three changes only:
///
///   - **`--crate-type`** is forced to `rlib`. Object files emitted
///     for an `rlib` crate-type retain mangled `pub fn` symbols
///     (cdylib's symbol-visibility filter wouldn't have run yet,
///     because we stop before linking). `lib` would also work but
///     `rlib` is what cargo itself uses for normal dependency
///     compilation, so we stay closer to the rustc call shape that
///     gets the most testing.
///   - **`--emit`** is forced to a single `obj=<output_dir>/<crate>.o`
///     directive. This skips the link step (no `lib<crate>.rlib`
///     metadata bundle, no `.rmeta`, no codegen-units fan-out into
///     deps) and writes one consolidated object file we can hand
///     directly to the linker.
///   - **`--out-dir`** is redirected so the host's `target/` isn't
///     touched (it's still the rustc-default location for any
///     auxiliary file rustc decides to emit).
///
/// Everything else is preserved verbatim — same target triple,
/// sysroot, sysroot suffix, `-C` flags, `-L`/`-l` directives, cfg
/// gates. This is the same "minimal edit, verbatim everything else"
/// principle as the captured-args replay does for the linker side
/// in [`super::link_plan::build_link_plan`]; the only difference is
/// where we stop in rustc's pipeline (`obj` vs `link`).
pub fn build_obj_plan(
    captured: &CapturedRustcInvocation,
    output_dir: &Path,
) -> ObjBuildPlan {
    let mut args = captured.args.clone();
    set_crate_type(&mut args, "rlib");
    set_out_dir(&mut args, output_dir);
    let object_path = output_dir.join(object_filename(&captured.crate_name));
    set_emit_obj(&mut args, &object_path);
    ObjBuildPlan {
        args,
        output_dir: output_dir.to_path_buf(),
        expected_object: object_path,
    }
}

/// Force `--emit` to exactly one directive: `obj=<path>`. Strips
/// every existing `--emit` (separated, `=`, comma-separated mix)
/// and appends one fresh pair. Same fold-and-add semantics as
/// [`set_crate_type`].
///
/// rustc accepts `--emit obj=<path>` as a single output kind with
/// an explicit destination, which avoids ambiguity when other
/// `--emit` directives would otherwise have asked for `link` or
/// `dep-info` etc. (cargo always passes a comma-separated set:
/// `dep-info,metadata,link`. We collapse the lot to just `obj`.)
pub fn set_emit_obj(args: &mut Vec<String>, object_path: &Path) {
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--emit" && i + 1 < args.len() {
            args.drain(i..=i + 1);
            continue;
        }
        if arg.starts_with("--emit=") {
            args.remove(i);
            continue;
        }
        i += 1;
    }
    args.push("--emit".into());
    args.push(format!("obj={}", object_path.to_string_lossy()));
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

/// Platform-specific cdylib filename for the **host** OS. Matches
/// what rustc itself emits for `--crate-type cdylib`:
///   macOS    → `lib<crate>.dylib`
///   Linux    → `lib<crate>.so`     (Android uses the same convention)
///   Windows  → `<crate>.dll`
///
/// Hyphens in the crate name become underscores (rustc convention).
/// Use [`library_filename_for_os`] when the patch target's OS differs
/// from the host (e.g. cross-compiling for Android from macOS).
pub fn library_filename(crate_name: &str) -> String {
    let stem = crate_name.replace('-', "_");
    if cfg!(target_os = "macos") || cfg!(target_os = "ios") {
        format!("lib{stem}.dylib")
    } else if cfg!(target_os = "windows") {
        format!("{stem}.dll")
    } else {
        format!("lib{stem}.so")
    }
}

/// Cross-platform variant: produce the cdylib filename for the
/// **patch target** OS (which may differ from the host). The hot-
/// patch dylib has to match the on-device shared-library naming
/// convention, not the host's — Android wants `lib<crate>.so` even
/// when the dev session is on macOS.
pub fn library_filename_for_os(crate_name: &str, os: super::link_plan::LinkerOs) -> String {
    use super::link_plan::LinkerOs;
    let stem = crate_name.replace('-', "_");
    match os {
        LinkerOs::Macos => format!("lib{stem}.dylib"),
        LinkerOs::Linux => format!("lib{stem}.so"),
        LinkerOs::Other => format!("{stem}.dll"),
    }
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

    // ----- set_emit_obj ------------------------------------------------

    #[test]
    fn set_emit_obj_replaces_separated_form() {
        let mut args = s(&["--emit", "link", "src/lib.rs"]);
        set_emit_obj(&mut args, Path::new("/p/demo.o"));
        assert_eq!(args, s(&["src/lib.rs", "--emit", "obj=/p/demo.o"]));
    }

    #[test]
    fn set_emit_obj_replaces_equals_form_including_comma_lists() {
        // cargo always passes `--emit=dep-info,metadata,link`; the
        // whole comma-separated lot collapses to a single `obj=…`.
        let mut args = s(&["--emit=dep-info,metadata,link", "src/lib.rs"]);
        set_emit_obj(&mut args, Path::new("/p/demo.o"));
        assert_eq!(args, s(&["src/lib.rs", "--emit", "obj=/p/demo.o"]));
    }

    #[test]
    fn set_emit_obj_collapses_multiple_existing_into_one() {
        let mut args = s(&[
            "--emit", "link",
            "--emit=dep-info,metadata",
            "--emit", "metadata",
            "src/lib.rs",
        ]);
        set_emit_obj(&mut args, Path::new("/p/demo.o"));
        assert_eq!(args, s(&["src/lib.rs", "--emit", "obj=/p/demo.o"]));
    }

    #[test]
    fn set_emit_obj_appends_when_no_existing() {
        let mut args = s(&["src/lib.rs"]);
        set_emit_obj(&mut args, Path::new("/p/demo.o"));
        assert_eq!(args, s(&["src/lib.rs", "--emit", "obj=/p/demo.o"]));
    }

    // ----- object_filename ---------------------------------------------

    #[test]
    fn object_filename_is_crate_dot_o_with_underscores() {
        assert_eq!(object_filename("demo"), "demo.o");
        assert_eq!(object_filename("hello-world"), "hello_world.o");
        assert_eq!(object_filename("a-b-c"), "a_b_c.o");
    }

    // ----- build_obj_plan ----------------------------------------------

    #[test]
    fn obj_plan_forces_rlib_and_obj_emit_and_redirects_out_dir() {
        let captured = captured_with(s(&[
            "--edition=2021",
            "--crate-name", "demo",
            "--crate-type", "lib",
            "--emit=dep-info,metadata,link",
            "--out-dir", "/cargo/target/debug/deps",
            "-C", "opt-level=3",
            "src/lib.rs",
        ]));
        let plan = build_obj_plan(&captured, Path::new("/whisker/objs/x"));
        assert_eq!(
            plan.args,
            s(&[
                "--edition=2021",
                "--crate-name", "demo",
                "-C", "opt-level=3",
                "src/lib.rs",
                "--crate-type", "rlib",
                "--out-dir", "/whisker/objs/x",
                "--emit", "obj=/whisker/objs/x/demo.o",
            ]),
        );
        assert_eq!(plan.output_dir, Path::new("/whisker/objs/x"));
        assert_eq!(plan.expected_object, Path::new("/whisker/objs/x/demo.o"));
    }

    #[test]
    fn obj_plan_picks_object_filename_from_captured_crate_name() {
        // crate_name comes from CapturedRustcInvocation.crate_name,
        // *not* from the --crate-name arg — they're typically equal,
        // but the captured field is what we use, so test that.
        let captured = CapturedRustcInvocation {
            crate_name: "thin-build-fixture".into(),
            args: s(&["src/lib.rs"]),
            timestamp_micros: 0,
        };
        let plan = build_obj_plan(&captured, Path::new("/o"));
        assert_eq!(plan.expected_object, Path::new("/o/thin_build_fixture.o"));
        assert!(
            plan.args.contains(&"obj=/o/thin_build_fixture.o".into()),
            "args: {:?}",
            plan.args,
        );
    }

    #[test]
    fn obj_plan_is_idempotent_on_re_run() {
        let captured = captured_with(s(&["src/lib.rs"]));
        let plan1 = build_obj_plan(&captured, Path::new("/o"));
        let plan2 = build_obj_plan(
            &CapturedRustcInvocation {
                crate_name: captured.crate_name.clone(),
                args: plan1.args.clone(),
                timestamp_micros: 0,
            },
            Path::new("/o"),
        );
        assert_eq!(plan1.args, plan2.args);
    }

    #[test]
    fn obj_plan_preserves_target_triple_and_sysroot_args() {
        // The whole point of "minimal edit" is that target-triple,
        // sysroot, link-args, etc. survive untouched. Regression
        // guard: these specific flags must come through verbatim.
        let captured = captured_with(s(&[
            "--target", "aarch64-linux-android",
            "--sysroot", "/some/ndk/sysroot",
            "-Clinker=lld",
            "-Clink-arg=-fuse-ld=lld",
            "-L", "native=/some/lib",
            "-l", "log",
            "src/lib.rs",
        ]));
        let plan = build_obj_plan(&captured, Path::new("/o"));
        for needle in [
            "--target", "aarch64-linux-android",
            "--sysroot", "/some/ndk/sysroot",
            "-Clinker=lld",
            "-Clink-arg=-fuse-ld=lld",
            "-L", "native=/some/lib",
            "-l", "log",
        ] {
            assert!(
                plan.args.iter().any(|a| a == needle),
                "missing {needle:?} from {:?}",
                plan.args,
            );
        }
    }

}
