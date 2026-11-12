//! Thin-rebuild driver — produce a patch dylib from a single
//! captured rustc invocation by editing as few of its args as
//! possible.
//!
//! ## Design principle: "minimal edit, verbatim everything else"
//!
//! Whisker does **not** want to re-derive linker / sysroot / SDK args
//! itself — those are the parts most likely to break across OS
//! versions, NDK upgrades, Xcode releases, glibc CSU layout
//! changes, and so on. Instead the shims capture cargo+rustc's full
//! invocation and this module replays it, **changing only the
//! handful of args that have to differ for a hot-patch dylib**:
//!
//!   - `--crate-type` is forced to `rlib` so rustc emits an object
//!     file containing every `pub fn`'s mangled symbol, which a
//!     cdylib would strip.
//!   - `--emit` is forced to `obj` so we get object files we can
//!     hand to the linker ourselves, one per codegen unit.
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
//! After rustc emits the objects, [`super::link_plan::build_link_plan`] takes the
//! captured **linker** invocation, drops its object inputs (we have
//! fresh ones), substitutes our objects and `-o`, and adds
//! `-undefined dynamic_lookup` (macOS) /
//! `--unresolved-symbols=ignore-all` (Linux) so unresolved symbols
//! are deferred to the host process at `dlopen` time. The result is
//! a `.so` / `.dylib` that re-exports back into the original binary
//! for everything except the patched function bodies — exactly what
//! `subsecond::apply_patch` expects.

use std::path::{Path, PathBuf};

use super::wrapper::CapturedRustcInvocation;

/// What [`build_obj_plan`] returns — captured rustc args, edited so
/// that running `rustc` with them produces object files containing
/// every `pub fn`'s mangled symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjBuildPlan {
    pub args: Vec<String>,
    /// Environment to overlay on the rustc spawn — the `CARGO_*` /
    /// `OUT_DIR` vars captured during the fat build, so `env!()`
    /// invocations in the user's code still resolve under the raw
    /// (cargo-less) thin rebuild.
    pub envs: std::collections::BTreeMap<String, String>,
    /// Directory rustc writes this crate's codegen-unit objects into.
    /// The runner empties it before every build.
    pub object_dir: PathBuf,
}

/// Codegen-unit count for thin rebuilds — rustc's own default for
/// incremental builds.
const CODEGEN_UNITS: &str = "256";

/// Edit a captured rustc invocation so that running it produces
/// object files containing every `pub fn`'s mangled symbol — the
/// input to the linker step in [`super::link_plan::build_link_plan`].
///
/// The changes that shape the output:
///
///   - **`--crate-type`** is forced to `rlib`. Object files emitted
///     for an `rlib` crate-type retain mangled `pub fn` symbols
///     (cdylib's symbol-visibility filter wouldn't have run yet,
///     because we stop before linking). `lib` would also work but
///     `rlib` is what cargo itself uses for normal dependency
///     compilation, so we stay closer to the rustc call shape that
///     gets the most testing.
///   - **`--emit`** is forced to a bare `obj`. This skips the link
///     step (no `lib<crate>.rlib` metadata bundle, no `.rmeta`) and
///     leaves one object per codegen unit in `--out-dir`.
///   - **`--out-dir`** is redirected to
///     `<output_dir>/objects/<crate>` so the host's `target/` isn't
///     touched and each crate's objects stay apart.
///   - **`-C codegen-units`** is forced, so an edit re-runs LLVM only
///     for the units it touches.
///
/// Everything else is preserved verbatim — same target triple,
/// sysroot, sysroot suffix, `-C` flags, `-L`/`-l` directives, cfg
/// gates. This is the same "minimal edit, verbatim everything else"
/// principle as the captured-args replay does for the linker side
/// in [`super::link_plan::build_link_plan`]; the only difference is
/// where we stop in rustc's pipeline (`obj` vs `link`).
pub fn build_obj_plan(captured: &CapturedRustcInvocation, output_dir: &Path) -> ObjBuildPlan {
    let mut args = captured.args.clone();
    set_crate_type(&mut args, "rlib");
    let object_dir = output_dir
        .join("objects")
        .join(captured.crate_name.replace('-', "_"));
    set_out_dir(&mut args, &object_dir);
    set_emit_obj(&mut args);
    // Without an explicit count rustc collapses `--emit obj` to one
    // unit, and any edit then re-runs LLVM over the whole crate.
    set_codegen_units(&mut args, CODEGEN_UNITS);
    // The fat build needs `-Csave-temps` to keep its link inputs; here
    // it would only write two bitcode files beside every object.
    strip_save_temps(&mut args);
    // cargo's captured args carry `--json=artifacts,…` /
    // `--error-format=json` for its own parsing. Nothing reads them
    // here, and left in they print a `{"$message_type":"artifact",…}`
    // line into the dev terminal per patch.
    strip_json_flags(&mut args);
    // Reuse rustc's incremental cache across thin rebuilds.
    set_incremental(&mut args, &output_dir.join("incremental"));
    // The fat build captured `-Copt-level=3`, where LLVM passes are
    // ~88% of rustc's wall time. Patched code only has to run, not run
    // fast, so opt-level=0 trades a slower function body for a much
    // faster compile. It also disables intra-crate inlining, keeping
    // the patch's call graph aligned with the host's exported symbols
    // instead of emitting UND references to inlined-away functions.
    override_opt_level(&mut args, "0");
    ObjBuildPlan {
        args,
        envs: captured.envs.clone(),
        object_dir,
    }
}

/// Force `-C codegen-units=<count>`, stripping any existing directive
/// in either the split or the combined form.
fn set_codegen_units(args: &mut Vec<String>, count: &str) {
    strip_codegen_option(args, "codegen-units");
    args.push("-C".into());
    args.push(format!("codegen-units={count}"));
}

/// Remove every `-C save-temps[=…]` directive.
fn strip_save_temps(args: &mut Vec<String>) {
    strip_codegen_option(args, "save-temps");
}

/// Remove every `-C <name>` / `-C <name>=…` directive, in the split
/// (`-C x`), combined (`-Cx`) and long (`--codegen=x`) forms.
fn strip_codegen_option(args: &mut Vec<String>, name: &str) {
    let is_option = |value: &str| {
        value
            .strip_prefix(name)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('='))
    };
    let mut i = 0;
    while i < args.len() {
        if (args[i] == "-C" || args[i] == "--codegen")
            && i + 1 < args.len()
            && is_option(&args[i + 1])
        {
            args.drain(i..i + 2);
            continue;
        }
        let combined = args[i]
            .strip_prefix("--codegen=")
            .or_else(|| args[i].strip_prefix("-C"));
        if combined.is_some_and(is_option) {
            args.remove(i);
            continue;
        }
        i += 1;
    }
}

/// Force `-Copt-level=<level>`. Strips any existing opt-level
/// directive (single-arg `-Copt-level=N`, split form `-C opt-level=N`,
/// and the shorthand `-O` = opt-level=2).
fn override_opt_level(args: &mut Vec<String>, level: &str) {
    let mut i = 0;
    while i < args.len() {
        if (args[i] == "-C" || args[i] == "--codegen")
            && i + 1 < args.len()
            && args[i + 1].starts_with("opt-level=")
        {
            args.drain(i..i + 2);
            continue;
        }
        if args[i].starts_with("-Copt-level=") || args[i].starts_with("--codegen=opt-level=") {
            args.remove(i);
            continue;
        }
        if args[i] == "-O" {
            args.remove(i);
            continue;
        }
        i += 1;
    }
    args.push("-C".into());
    args.push(format!("opt-level={level}"));
}

/// Force `-C incremental=<dir>` to point at our patch cache dir.
/// Strips any existing `-C incremental=...` rustc dropped in (cargo
/// usually doesn't pass one when building a `dylib`/`cdylib`, but
/// we strip defensively in case the captured args ever carry one).
fn set_incremental(args: &mut Vec<String>, incremental_dir: &Path) {
    let mut i = 0;
    while i < args.len() {
        if (args[i] == "-C" || args[i] == "--codegen")
            && i + 1 < args.len()
            && args[i + 1].starts_with("incremental=")
        {
            args.drain(i..=i + 1);
            continue;
        }
        // Combined form: `-Cincremental=...`
        if args[i].starts_with("-Cincremental=") || args[i].starts_with("--codegen=incremental=") {
            args.remove(i);
            continue;
        }
        i += 1;
    }
    args.push("-C".into());
    args.push(format!("incremental={}", incremental_dir.display()));
}

/// Remove `--json=…` and `--error-format=json` from the captured
/// rustc args so the thin rebuild emits plain human text instead of
/// the structured JSON channel cargo would have parsed. Both forms
/// (separated and `=`) are stripped; we don't restore a default
/// because rustc's default is already human output.
fn strip_json_flags(args: &mut Vec<String>) {
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        // `--json <list>` or `--error-format <fmt>` (separated forms)
        if (arg == "--json" || arg == "--error-format") && i + 1 < args.len() {
            args.drain(i..=i + 1);
            continue;
        }
        // `--json=...` or `--error-format=...` (equals forms)
        if arg.starts_with("--json=") || arg.starts_with("--error-format=") {
            args.remove(i);
            continue;
        }
        i += 1;
    }
}

/// Force `--emit` to exactly one directive: `obj`. Strips every
/// existing `--emit` (separated, `=`, comma-separated mix) and
/// appends one fresh pair. Same fold-and-add semantics as
/// [`set_crate_type`].
///
/// The directive carries no `=<path>`: rustc ignores an explicit
/// path once the crate has more than one codegen unit.
pub fn set_emit_obj(args: &mut Vec<String>) {
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
    args.push("obj".into());
}

/// Force every `--crate-type` arg to a single value (`new_kind`).
/// rustc allows the flag to repeat (one binary can be multiple
/// crate-types in one invocation); a hot-patch always wants exactly
/// one. The fold-and-add behaviour is:
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
            envs: Default::default(),
            timestamp_micros: 0,
        }
    }

    #[test]
    fn build_obj_plan_carries_captured_envs_through() {
        let mut captured = captured_with(s(&["--edition=2021", "src/lib.rs"]));
        captured
            .envs
            .insert("CARGO_PKG_VERSION".to_string(), "0.7.0".to_string());
        let plan = build_obj_plan(&captured, Path::new("/tmp/patches"));
        assert_eq!(plan.envs, captured.envs);
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
            "--crate-type",
            "rlib",
            "--crate-type",
            "dylib",
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
        set_emit_obj(&mut args);
        assert_eq!(args, s(&["src/lib.rs", "--emit", "obj"]));
    }

    #[test]
    fn set_emit_obj_replaces_equals_form_including_comma_lists() {
        // cargo always passes `--emit=dep-info,metadata,link`; the
        // whole comma-separated lot collapses to a single `obj`.
        let mut args = s(&["--emit=dep-info,metadata,link", "src/lib.rs"]);
        set_emit_obj(&mut args);
        assert_eq!(args, s(&["src/lib.rs", "--emit", "obj"]));
    }

    #[test]
    fn set_emit_obj_collapses_multiple_existing_into_one() {
        let mut args = s(&[
            "--emit",
            "link",
            "--emit=dep-info,metadata",
            "--emit",
            "metadata",
            "src/lib.rs",
        ]);
        set_emit_obj(&mut args);
        assert_eq!(args, s(&["src/lib.rs", "--emit", "obj"]));
    }

    #[test]
    fn set_emit_obj_appends_when_no_existing() {
        let mut args = s(&["src/lib.rs"]);
        set_emit_obj(&mut args);
        assert_eq!(args, s(&["src/lib.rs", "--emit", "obj"]));
    }

    // ----- build_obj_plan ----------------------------------------------

    #[test]
    fn obj_plan_forces_rlib_and_obj_emit_and_redirects_out_dir() {
        let captured = captured_with(s(&[
            "--edition=2021",
            "--crate-name",
            "demo",
            "--crate-type",
            "lib",
            "--emit=dep-info,metadata,link",
            "--out-dir",
            "/cargo/target/debug/deps",
            "-C",
            "opt-level=3",
            "src/lib.rs",
        ]));
        let plan = build_obj_plan(&captured, Path::new("/whisker/objs/x"));
        assert_eq!(
            plan.args,
            s(&[
                "--edition=2021",
                "--crate-name",
                "demo",
                "src/lib.rs",
                "--crate-type",
                "rlib",
                "--out-dir",
                "/whisker/objs/x/objects/demo",
                "--emit",
                "obj",
                "-C",
                "codegen-units=256",
                "-C",
                "incremental=/whisker/objs/x/incremental",
                "-C",
                "opt-level=0",
            ]),
        );
        assert_eq!(plan.object_dir, Path::new("/whisker/objs/x/objects/demo"));
    }

    #[test]
    fn obj_plan_picks_object_dir_from_captured_crate_name() {
        // crate_name comes from CapturedRustcInvocation.crate_name,
        // *not* from the --crate-name arg — they're typically equal,
        // but the captured field is what we use, so test that.
        let captured = CapturedRustcInvocation {
            crate_name: "thin-build-fixture".into(),
            args: s(&["src/lib.rs"]),
            envs: Default::default(),
            timestamp_micros: 0,
        };
        let plan = build_obj_plan(&captured, Path::new("/o"));
        assert_eq!(plan.object_dir, Path::new("/o/objects/thin_build_fixture"));
    }

    #[test]
    fn obj_plan_overrides_captured_codegen_units() {
        for captured_form in [
            s(&["-C", "codegen-units=1", "src/lib.rs"]),
            s(&["-Ccodegen-units=1", "src/lib.rs"]),
            s(&["--codegen=codegen-units=16", "src/lib.rs"]),
        ] {
            let plan = build_obj_plan(&captured_with(captured_form), Path::new("/o"));
            let units: Vec<&String> = plan
                .args
                .iter()
                .filter(|a| a.contains("codegen-units"))
                .collect();
            assert_eq!(units, ["codegen-units=256"], "args: {:?}", plan.args);
        }
    }

    #[test]
    fn obj_plan_drops_save_temps_but_keeps_other_codegen_flags() {
        let captured = captured_with(s(&[
            "-Csave-temps=y",
            "-C",
            "save-temps",
            "-Cdebug-assertions=on",
            "src/lib.rs",
        ]));
        let plan = build_obj_plan(&captured, Path::new("/o"));
        assert!(
            !plan.args.iter().any(|a| a.contains("save-temps")),
            "args: {:?}",
            plan.args,
        );
        assert!(plan.args.contains(&"-Cdebug-assertions=on".to_string()));
    }

    #[test]
    fn obj_plan_is_idempotent_on_re_run() {
        let captured = captured_with(s(&["src/lib.rs"]));
        let plan1 = build_obj_plan(&captured, Path::new("/o"));
        let plan2 = build_obj_plan(
            &CapturedRustcInvocation {
                crate_name: captured.crate_name.clone(),
                args: plan1.args.clone(),
                envs: Default::default(),
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
            "--target",
            "aarch64-linux-android",
            "--sysroot",
            "/some/ndk/sysroot",
            "-Clinker=lld",
            "-Clink-arg=-fuse-ld=lld",
            "-L",
            "native=/some/lib",
            "-l",
            "log",
            "src/lib.rs",
        ]));
        let plan = build_obj_plan(&captured, Path::new("/o"));
        for needle in [
            "--target",
            "aarch64-linux-android",
            "--sysroot",
            "/some/ndk/sysroot",
            "-Clinker=lld",
            "-Clink-arg=-fuse-ld=lld",
            "-L",
            "native=/some/lib",
            "-l",
            "log",
        ] {
            assert!(
                plan.args.iter().any(|a| a == needle),
                "missing {needle:?} from {:?}",
                plan.args,
            );
        }
    }
}
