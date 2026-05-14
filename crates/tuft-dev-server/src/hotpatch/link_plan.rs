//! Build the linker invocation for a hot-patch dylib by editing
//! the captured fat-build linker call (see I4g-X1
//! `tuft-linker-shim`) as little as possible.
//!
//! ## Why we don't construct linker args from scratch
//!
//! cargo + rustc + the platform's clang/gcc driver assemble a
//! large, fragile argv: sysroot, target triple, `-arch`, NDK
//! search paths, framework directories, OS-version-min flags,
//! `-Wl,…` directives, sometimes a custom `-fuse-ld=…`. These
//! shift across:
//!
//!   - macOS major versions (sysroot path, `-platform_version`)
//!   - Xcode releases (`-isysroot`, framework dirs)
//!   - Android NDK r24/r25/r26 (CRT layout, libc++.a, libunwind.a)
//!   - glibc CSU layout (crt1.o vs Scrt1.o, libc_nonshared.a)
//!   - rustc's choice of linker driver (cc, clang, lld)
//!
//! Re-deriving any of these is a long-tail of papercuts. So:
//! capture the fat-build linker invocation verbatim (X1) and edit
//! only the parts a hot-patch must change. Same principle as
//! `build_obj_plan` does for rustc.
//!
//! ## What we change
//!
//!   1. **Drop object/archive inputs** (`.o`, `.rlib`, `.a`,
//!      `.so`, `.dylib`). The fat build linked the entire
//!      workspace; the patch only needs the freshly-rebuilt object.
//!   2. **Drop `-o <path>`** and the existing `-shared` (we
//!      re-add both).
//!   3. **Drop `-undefined <action>`** (the separated macOS form)
//!      so we can deterministically set `dynamic_lookup`.
//!   4. **Drop `--version-script=<path>` and `--no-undefined-version`.**
//!      The fat build's version-script enumerates thousands of
//!      Rust-mangled symbols (rustc auto-generates one for both
//!      `dylib` and `cdylib`). Re-applying it to a patch dylib
//!      that only defines the one changed function makes the
//!      linker emit `version script assignment ... failed:
//!      symbol not defined` for every absent symbol — fatal under
//!      `--no-undefined-version`. The patch dylib's default
//!      visibility (everything global) is the right behaviour:
//!      `subsecond::apply_patch` reads the patch's `.dynsym`
//!      looking for the changed function's mangled name.
//!   5. **Append**:
//!       - `-shared`
//!       - OS-specific "unresolved is fine" directive:
//!           - macOS: `-Wl,-undefined,dynamic_lookup`
//!           - Linux/Android: `-Wl,--unresolved-symbols=ignore-all`
//!       - on Linux/Android, the host dylib as a link input
//!         (when supplied). This adds a `DT_NEEDED` entry for
//!         the host so the Android dynamic linker resolves the
//!         patch's undefined Rust symbols against the host's
//!         already-loaded `libhello_world.so` (which
//!         `System.loadLibrary` placed into the app's classloader
//!         namespace with `RTLD_LOCAL`). Without this, the patch's
//!         `dlopen` fails with `cannot locate symbol _ZN4core3fmt…`
//!         even though the host has the symbol in `.dynsym`.
//!       - the new object path
//!       - `-o <output>`
//!
//! Everything else — `-isysroot`, `-arch`, `-target`, `-L`,
//! `-l`, `-rpath`, `-Wl,…`, `-F`, `-framework`, `-fuse-ld=…`,
//! `-mmacosx-version-min=…` — is preserved verbatim.
//!
//! The "unresolved is fine" directive is the load-bearing trick
//! that makes hot-patch dylibs small and fast: every symbol the
//! patch references but doesn't define (e.g. an unmodified
//! `tuft::println`) is left as an undefined-symbol marker, and
//! `subsecond::apply_patch` resolves it against the *original*
//! binary's symbol table at swap-in time. So the patch dylib
//! holds only the changed function bodies, not their entire
//! transitive call graph.

use std::path::{Path, PathBuf};

/// Result of [`build_link_plan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkPlan {
    /// Final argv to pass to the linker driver (cc / clang / etc.).
    pub args: Vec<String>,
    /// The path the linker will write — equal to `output` passed in.
    /// Surfaced separately so the runner can sanity-check existence
    /// after the spawn returns.
    pub output: PathBuf,
}

/// Which OS-specific "unresolved-is-fine" directive to emit.
/// Android uses the same lld defaults as Linux for our purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkerOs {
    /// macOS host or iOS Simulator.
    Macos,
    /// Linux host or Android device.
    Linux,
    /// Windows host. Currently unsupported — we still strip the
    /// captured args but won't emit a useful directive (PE/COFF
    /// hot-patch isn't on the I4g roadmap).
    Other,
}

/// Re-shape a captured linker invocation into the link step of a
/// hot-patch build. See module docs for the rationale.
///
/// `host_dylib` is the path to the original `.so`/`.dylib` the
/// device loaded — used on Linux/Android to add a `DT_NEEDED` entry
/// so the patch's undefined Rust symbols resolve against the host
/// at `dlopen` time. Pass `None` on Linux when the host is the main
/// executable (no separate `.so`) or on macOS, where
/// `-Wl,-undefined,dynamic_lookup` already routes symbol resolution
/// through every loaded image.
pub fn build_link_plan(
    captured_linker_args: &[String],
    new_object: &Path,
    output: &Path,
    target_os: LinkerOs,
    host_dylib: Option<&Path>,
) -> LinkPlan {
    let mut args = filter_captured_linker_args(captured_linker_args);

    if !args.iter().any(|a| a == "-shared") {
        args.push("-shared".into());
    }
    match target_os {
        LinkerOs::Macos => {
            args.push("-Wl,-undefined,dynamic_lookup".into());
        }
        LinkerOs::Linux => {
            args.push("-Wl,--unresolved-symbols=ignore-all".into());
        }
        LinkerOs::Other => {}
    }

    // DT_NEEDED → host shared object (Android/Linux only). See module
    // docs §5. `-Wl,--no-as-needed` is the linker default for
    // positional `.so` inputs on most lld versions, but spelled
    // explicitly here so a captured `--as-needed` arg upstream can't
    // demote it to an unused-and-dropped entry.
    if matches!(target_os, LinkerOs::Linux) {
        if let Some(host) = host_dylib {
            args.push("-Wl,--no-as-needed".into());
            args.push(host.to_string_lossy().into());
            args.push("-Wl,--as-needed".into());
        }
    }

    args.push(new_object.to_string_lossy().into());
    args.push("-o".into());
    args.push(output.to_string_lossy().into());

    LinkPlan {
        args,
        output: output.to_path_buf(),
    }
}

/// Pick the [`LinkerOs`] that matches the host we're building on.
/// `cfg!`-based — at runtime we know which compiled-in branch ran.
/// For cross-target hot-patch (e.g. macOS host → Android device),
/// callers should pass the target OS explicitly rather than rely on
/// this convenience.
pub fn linker_os_for_host() -> LinkerOs {
    if cfg!(target_os = "macos") || cfg!(target_os = "ios") {
        LinkerOs::Macos
    } else if cfg!(target_os = "linux") || cfg!(target_os = "android") {
        LinkerOs::Linux
    } else {
        LinkerOs::Other
    }
}

/// Strip the captured args of every flag we want to override
/// deterministically: object/archive inputs, the existing `-o`,
/// `-shared`, and the separated `-undefined <action>` pair. Other
/// flags pass through unmodified.
fn filter_captured_linker_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];

        // -o <path>: drop both.
        if arg == "-o" && i + 1 < args.len() {
            i += 2;
            continue;
        }
        // -shared: re-added later.
        if arg == "-shared" {
            i += 1;
            continue;
        }
        // -undefined <action>: re-added later in the macOS branch.
        if arg == "-undefined" && i + 1 < args.len() {
            i += 2;
            continue;
        }
        // Bare object / archive input.
        if is_object_or_archive_input(arg) {
            i += 1;
            continue;
        }
        // Wholesale -Wl,-undefined,dynamic_lookup (the comma form
        // we'll re-add). Drop the existing one so we don't end up
        // with two on Macos.
        if arg.starts_with("-Wl,-undefined,") {
            i += 1;
            continue;
        }
        // Wholesale -Wl,--unresolved-symbols= (Linux equivalent).
        if arg.starts_with("-Wl,--unresolved-symbols=") {
            i += 1;
            continue;
        }
        // Drop fat-build version-scripts — see module docs §4.
        // Both the `=` form (what rustc + our cargo_build.rs emit)
        // and the separated `--version-script <path>` form (defensive;
        // some clang drivers normalize one to the other).
        if arg.starts_with("-Wl,--version-script=")
            || arg.starts_with("--version-script=")
        {
            i += 1;
            continue;
        }
        if (arg == "-Wl,--version-script" || arg == "--version-script")
            && i + 1 < args.len()
        {
            i += 2;
            continue;
        }
        // --no-undefined-version turns the "version-script lists a
        // symbol not defined" warning into a hard error. We dropped
        // the version-script anyway, but a stray --no-undefined-version
        // is now meaningless and could become a future foot-gun if a
        // future capture path reintroduces a version-script.
        if arg == "-Wl,--no-undefined-version" || arg == "--no-undefined-version" {
            i += 1;
            continue;
        }

        out.push(arg.clone());
        i += 1;
    }
    out
}

/// Heuristic: a non-flag arg whose extension is a recognised object
/// or archive format. We deliberately don't treat `-l<name>` or
/// `-L<dir>` as object inputs (they're flags, hence start with `-`).
fn is_object_or_archive_input(arg: &str) -> bool {
    if arg.starts_with('-') {
        return false;
    }
    let ext = Path::new(arg)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    matches!(
        ext.as_deref(),
        Some("o" | "rlib" | "a" | "so" | "dylib" | "obj" | "lib"),
    )
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

    // ----- filter_captured_linker_args ---------------------------------

    #[test]
    fn filter_drops_object_inputs() {
        let kept = filter_captured_linker_args(&s(&[
            "-O3",
            "/tmp/foo.o",
            "/tmp/bar.rlib",
            "/tmp/libstd.a",
            "-l", "iconv",
        ]));
        assert_eq!(kept, s(&["-O3", "-l", "iconv"]));
    }

    #[test]
    fn filter_drops_dynamic_libraries_too() {
        // Captured fat-build linker may have re-linked an existing
        // .so/.dylib; we drop those for the same reason as static
        // archives — their symbols come back via dynamic_lookup.
        let kept = filter_captured_linker_args(&s(&[
            "/tmp/libfoo.so",
            "/tmp/libbar.dylib",
            "-shared",
        ]));
        // -shared also dropped (we re-add later).
        assert!(kept.is_empty(), "expected empty, got {kept:?}");
    }

    #[test]
    fn filter_keeps_search_path_and_link_flags() {
        let kept = filter_captured_linker_args(&s(&[
            "-L", "/sdk/lib",
            "-L/different/dir",
            "-lcurl",
            "-l", "z",
            "-Wl,-rpath,/some/path",
            "-isysroot", "/Applications/Xcode.app/.../MacOSX.sdk",
            "-arch", "arm64",
            "-target", "arm64-apple-macosx14.0.0",
            "-fuse-ld=lld",
            "-mmacosx-version-min=11.0",
        ]));
        assert_eq!(
            kept,
            s(&[
                "-L", "/sdk/lib",
                "-L/different/dir",
                "-lcurl",
                "-l", "z",
                "-Wl,-rpath,/some/path",
                "-isysroot", "/Applications/Xcode.app/.../MacOSX.sdk",
                "-arch", "arm64",
                "-target", "arm64-apple-macosx14.0.0",
                "-fuse-ld=lld",
                "-mmacosx-version-min=11.0",
            ]),
        );
    }

    #[test]
    fn filter_drops_existing_output_path() {
        let kept = filter_captured_linker_args(&s(&[
            "-shared", "-o", "/old/libfoo.dylib", "/tmp/foo.o",
        ]));
        assert!(kept.is_empty(), "got {kept:?}");
    }

    #[test]
    fn filter_drops_fat_build_version_scripts_and_no_undefined_version() {
        // Mirrors the actual capture from a dylib fat build: rustc
        // emits an enormous version-script enumerating every Rust
        // symbol it expects exported, plus --no-undefined-version
        // to harden the check. The patch link only defines the one
        // changed function, so re-applying these would fail with
        // "symbol not defined" for every absent symbol.
        let kept = filter_captured_linker_args(&s(&[
            "-Wl,--version-script=/tmp/rustcXX/list",
            "-Wl,--version-script=/ws/target/.tuft/android-jni-exports.ver",
            "-Wl,--no-undefined-version",
            "-Wl,--as-needed",
            "-arch", "arm64",
        ]));
        assert_eq!(kept, s(&["-Wl,--as-needed", "-arch", "arm64"]));
    }

    #[test]
    fn filter_drops_separated_version_script_form() {
        // Some clang drivers split `-Wl,--version-script=/p` into
        // `--version-script /p` when forwarding to ld. Defensive.
        let kept = filter_captured_linker_args(&s(&[
            "--version-script", "/tmp/rustcXX/list",
            "-pie",
        ]));
        assert_eq!(kept, s(&["-pie"]));
    }

    #[test]
    fn filter_drops_existing_undefined_dynamic_lookup() {
        // Both the separated and the comma-bundled form.
        let kept = filter_captured_linker_args(&s(&[
            "-undefined", "dynamic_lookup",
            "-Wl,-undefined,dynamic_lookup",
            "-Wl,--unresolved-symbols=ignore-all",
            "-arch", "arm64",
        ]));
        assert_eq!(kept, s(&["-arch", "arm64"]));
    }

    #[test]
    fn filter_keeps_dash_l_with_path_that_starts_with_l() {
        // Regression: `-llog` is `-l log` (link library named "log"),
        // not an object file. starts_with('-') already covers this
        // but make sure.
        let kept = filter_captured_linker_args(&s(&["-llog", "-lstdc++"]));
        assert_eq!(kept, s(&["-llog", "-lstdc++"]));
    }

    #[test]
    fn filter_keeps_framework_pairs() {
        // -framework Foundation must keep the bare "Foundation" arg
        // (it doesn't end in an object extension).
        let kept = filter_captured_linker_args(&s(&[
            "-framework", "Foundation",
            "-framework", "CoreFoundation",
        ]));
        assert_eq!(
            kept,
            s(&[
                "-framework", "Foundation",
                "-framework", "CoreFoundation",
            ]),
        );
    }

    // ----- is_object_or_archive_input ----------------------------------

    #[test]
    fn object_detection_covers_common_extensions() {
        for path in [
            "foo.o", "foo.rlib", "foo.a", "foo.so", "foo.dylib",
            "foo.OBJ", "foo.LIB", // case-insensitive (Windows)
            "/abs/path/lib.a", "rel/dir/foo.o",
        ] {
            assert!(is_object_or_archive_input(path), "{path}");
        }
    }

    #[test]
    fn object_detection_rejects_flags_and_non_object_paths() {
        for path in [
            "-shared", "-o", "-Llib", "-llog",
            "/some/source.rs", "Foundation",
            "foo.txt", "bar",
        ] {
            assert!(!is_object_or_archive_input(path), "{path}");
        }
    }

    // ----- build_link_plan: macOS --------------------------------------

    #[test]
    fn macos_plan_appends_shared_dynamic_lookup_object_and_output() {
        let plan = build_link_plan(
            &s(&["-isysroot", "/sdk", "-arch", "arm64"]),
            Path::new("/o/demo.o"),
            Path::new("/o/libdemo.dylib"),
            LinkerOs::Macos,
            None,
        );
        assert_eq!(
            plan.args,
            s(&[
                "-isysroot", "/sdk",
                "-arch", "arm64",
                "-shared",
                "-Wl,-undefined,dynamic_lookup",
                "/o/demo.o",
                "-o", "/o/libdemo.dylib",
            ]),
        );
        assert_eq!(plan.output, Path::new("/o/libdemo.dylib"));
    }

    #[test]
    fn macos_plan_does_not_double_shared_when_captured_already_had_it() {
        let plan = build_link_plan(
            &s(&["-shared", "-isysroot", "/sdk"]),
            Path::new("/o/demo.o"),
            Path::new("/o/libdemo.dylib"),
            LinkerOs::Macos,
            None,
        );
        let shared_count = plan.args.iter().filter(|a| *a == "-shared").count();
        assert_eq!(shared_count, 1, "got args: {:?}", plan.args);
    }

    #[test]
    fn macos_plan_replaces_old_object_inputs_with_just_the_new_one() {
        let plan = build_link_plan(
            &s(&["/old/a.o", "/old/b.o", "/old/libstd.rlib"]),
            Path::new("/new/demo.o"),
            Path::new("/new/libdemo.dylib"),
            LinkerOs::Macos,
            None,
        );
        // The output path *itself* is .dylib-shaped, so we walk
        // by index and skip the arg immediately after `-o`.
        let mut object_args: Vec<&str> = Vec::new();
        let mut i = 0;
        while i < plan.args.len() {
            if plan.args[i] == "-o" {
                i += 2;
                continue;
            }
            if is_object_or_archive_input(&plan.args[i]) {
                object_args.push(&plan.args[i]);
            }
            i += 1;
        }
        assert_eq!(object_args, vec!["/new/demo.o"]);
    }

    #[test]
    fn macos_plan_replaces_old_output_with_new_one() {
        let plan = build_link_plan(
            &s(&["-o", "/old/libold.dylib"]),
            Path::new("/new/demo.o"),
            Path::new("/new/libnew.dylib"),
            LinkerOs::Macos,
            None,
        );
        // Find the position of -o and check the next arg is the new output.
        let dash_o = plan.args.iter().position(|a| a == "-o").unwrap();
        assert_eq!(plan.args.get(dash_o + 1).map(String::as_str), Some("/new/libnew.dylib"));
        assert_eq!(plan.args.iter().filter(|a| *a == "-o").count(), 1);
    }

    // ----- build_link_plan: Linux / Android ----------------------------

    #[test]
    fn linux_plan_uses_unresolved_symbols_ignore_all_directive() {
        let plan = build_link_plan(
            &s(&["-pie", "-L", "/ndk/lib"]),
            Path::new("/o/demo.o"),
            Path::new("/o/libdemo.so"),
            LinkerOs::Linux,
            None,
        );
        assert_eq!(
            plan.args,
            s(&[
                "-pie",
                "-L", "/ndk/lib",
                "-shared",
                "-Wl,--unresolved-symbols=ignore-all",
                "/o/demo.o",
                "-o", "/o/libdemo.so",
            ]),
        );
    }

    #[test]
    fn linux_plan_drops_pre_existing_unresolved_directive() {
        // Same flag captured twice (e.g. fat build already had it
        // from -C link-arg). Make sure we end up with one.
        let plan = build_link_plan(
            &s(&["-Wl,--unresolved-symbols=ignore-all", "-L", "/ndk/lib"]),
            Path::new("/o/demo.o"),
            Path::new("/o/libdemo.so"),
            LinkerOs::Linux,
            None,
        );
        let count = plan
            .args
            .iter()
            .filter(|a| *a == "-Wl,--unresolved-symbols=ignore-all")
            .count();
        assert_eq!(count, 1, "args: {:?}", plan.args);
    }

    #[test]
    fn linux_plan_appends_host_dylib_when_supplied_for_dt_needed() {
        // The host_dylib path lands AFTER --unresolved-symbols and
        // BEFORE the new object — wrapped in --no-as-needed so a
        // captured --as-needed earlier in the line can't strip it.
        let plan = build_link_plan(
            &s(&["-Wl,--as-needed", "-L", "/ndk/lib"]),
            Path::new("/o/demo.o"),
            Path::new("/o/libdemo.so"),
            LinkerOs::Linux,
            Some(Path::new("/jniLibs/libhello_world.so")),
        );
        assert_eq!(
            plan.args,
            s(&[
                "-Wl,--as-needed",
                "-L", "/ndk/lib",
                "-shared",
                "-Wl,--unresolved-symbols=ignore-all",
                "-Wl,--no-as-needed",
                "/jniLibs/libhello_world.so",
                "-Wl,--as-needed",
                "/o/demo.o",
                "-o", "/o/libdemo.so",
            ]),
        );
    }

    #[test]
    fn linux_plan_omits_host_dylib_when_none() {
        let plan = build_link_plan(
            &s(&["-L", "/ndk/lib"]),
            Path::new("/o/demo.o"),
            Path::new("/o/libdemo.so"),
            LinkerOs::Linux,
            None,
        );
        assert!(
            !plan.args.iter().any(|a| a.ends_with("libhello_world.so")),
            "args: {:?}",
            plan.args,
        );
    }

    #[test]
    fn macos_plan_ignores_host_dylib_because_dynamic_lookup_handles_it() {
        // macOS uses -Wl,-undefined,dynamic_lookup to route resolution
        // through all loaded images; DT_NEEDED isn't needed.
        let plan = build_link_plan(
            &s(&["-isysroot", "/sdk"]),
            Path::new("/o/demo.o"),
            Path::new("/o/libdemo.dylib"),
            LinkerOs::Macos,
            Some(Path::new("/path/host.dylib")),
        );
        assert!(
            !plan.args.iter().any(|a| a.ends_with("host.dylib")),
            "host dylib should not appear on macOS plan: {:?}",
            plan.args,
        );
    }

    // ----- build_link_plan: Other --------------------------------------

    #[test]
    fn other_os_plan_omits_unresolved_directive() {
        let plan = build_link_plan(
            &s(&["-machine:x64"]),
            Path::new("/o/demo.obj"),
            Path::new("/o/demo.dll"),
            LinkerOs::Other,
            None,
        );
        // No -Wl directive of any kind.
        assert!(
            !plan.args.iter().any(|a| a.starts_with("-Wl,")),
            "args: {:?}",
            plan.args,
        );
        // Still gets -shared, the new object, and -o.
        assert!(plan.args.contains(&"-shared".into()));
        assert!(plan.args.iter().any(|a| a.ends_with("demo.obj")));
    }

    // ----- linker_os_for_host ------------------------------------------

    #[test]
    fn linker_os_for_host_picks_an_os_consistent_with_cfg() {
        let os = linker_os_for_host();
        if cfg!(target_os = "macos") || cfg!(target_os = "ios") {
            assert_eq!(os, LinkerOs::Macos);
        } else if cfg!(target_os = "linux") || cfg!(target_os = "android") {
            assert_eq!(os, LinkerOs::Linux);
        } else {
            assert_eq!(os, LinkerOs::Other);
        }
    }
}
