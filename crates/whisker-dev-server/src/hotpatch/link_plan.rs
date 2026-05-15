//! Build the linker invocation for a hot-patch dylib by editing
//! the captured fat-build linker call (see I4g-X1
//! `whisker-linker-shim`) as little as possible.
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
//!       - any caller-supplied **extra objects** (typically the
//!         `stub.o` produced by
//!         [`crate::hotpatch::create_undefined_symbol_stub`]). The
//!         stub defines every host symbol the patch refers to as a
//!         tiny ARM64 trampoline branching to that symbol's
//!         *runtime* address, computed from the device's reported
//!         `subsecond::aslr_reference()`. Linking it in this slot
//!         means the patch has no `DT_NEEDED` back-edge to the
//!         host, and no dlopen-time symbol resolution to perform —
//!         which avoids the Android linker-namespace +
//!         `RTLD_LOCAL` corner cases the previous "back-edge to
//!         host dylib" scheme tripped over.
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
//! `whisker::println`) is left as an undefined-symbol marker, and
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
/// `extra_objects` are additional `.o` paths to link alongside
/// `new_object`. The typical caller is `thin_rebuild_obj`, which
/// passes the `stub.o` produced by
/// [`crate::hotpatch::create_undefined_symbol_stub`] here — that stub
/// defines every host symbol the patch references as a tiny ARM64
/// trampoline branching to the symbol's runtime address. After
/// linking with the stub, the patch dylib has no `DT_NEEDED`
/// back-edge to the host and no dlopen-time symbol resolution to
/// perform. See `docs/hot-reload-plan.md` "Option B" for the design.
pub fn build_link_plan(
    captured_linker_args: &[String],
    new_object: &Path,
    output: &Path,
    target_os: LinkerOs,
    extra_objects: &[std::path::PathBuf],
) -> LinkPlan {
    let mut args = filter_captured_linker_args(captured_linker_args);

    if !args.iter().any(|a| a == "-shared") {
        args.push("-shared".into());
    }
    match target_os {
        LinkerOs::Macos => {
            args.push("-Wl,-undefined,dynamic_lookup".into());

            // Re-add the macro-emitted user-crate exports that
            // `-exported_symbols_list <rustc-temp>` was carrying for
            // the fat build. We drop that file (it references
            // symbols our thin patch doesn't define and ld errors
            // out), but `subsecond::apply_patch` still needs at
            // least `whisker_aslr_anchor` in the patch dylib's
            // `.dynsym` — its `dlsym(patch, "whisker_aslr_anchor")`
            // unwraps on None and panics across the FFI boundary,
            // aborting the host app.
            //
            // `whisker_app_main` and `whisker_tick` aren't strictly
            // required by `apply_patch` (Swift calls them on the
            // host dylib, not the patch), but exporting them keeps
            // the patch's `.dynsym` symmetric with the host —
            // useful if a future patch path ever wants to dispatch
            // through the patch's own entry points.
            for sym in [
                "_whisker_aslr_anchor",
                "_whisker_app_main",
                "_whisker_tick",
            ] {
                args.push(format!("-Wl,-exported_symbol,{sym}"));
            }
            // If the captured args target the iOS Simulator (or
            // device), rustc's fat build resolved the SDK sysroot
            // implicitly through its own driver — that path doesn't
            // show up in the captured argv. Re-running clang
            // directly we need `-isysroot <iphonesimulator-sdk>`
            // or `-liconv` / `-lSystem` / iOS SDK frameworks fail
            // to resolve. Detect by looking at `-target ...-simulator`
            // / `-target ...-ios*` in the captured args, then ask
            // xcrun for the SDK path.
            if !args.iter().any(|a| a == "-isysroot") {
                if let Some(sdk_kind) = detect_apple_sdk(&args) {
                    if let Some(sdk_path) = xcrun_sdk_path(sdk_kind) {
                        args.push("-isysroot".into());
                        args.push(sdk_path);
                    }
                }
            }
        }
        LinkerOs::Linux => {
            // Safety net for any symbol that didn't end up in the
            // stub object (e.g., synthesised compiler intrinsics that
            // aren't in the host's symbol table). Stubbed symbols
            // satisfy the linker from `extra_objects` first; this
            // flag handles the long tail.
            args.push("-Wl,--unresolved-symbols=ignore-all".into());
        }
        LinkerOs::Other => {}
    }

    for obj in extra_objects {
        args.push(obj.to_string_lossy().into());
    }
    args.push(new_object.to_string_lossy().into());
    args.push("-o".into());
    args.push(output.to_string_lossy().into());

    LinkPlan {
        args,
        output: output.to_path_buf(),
    }
}

/// Look at the captured `-target …` triple and decide which Apple SDK
/// to ask `xcrun` for. Returns the `--sdk` argument value
/// (`"iphonesimulator"`, `"iphoneos"`, `"macosx"`) or `None` if the
/// captured args don't look like an Apple build.
fn detect_apple_sdk(args: &[String]) -> Option<&'static str> {
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if a != "-target" {
            continue;
        }
        let triple = iter.next()?;
        return Some(if triple.contains("-simulator") {
            "iphonesimulator"
        } else if triple.contains("apple-ios") {
            "iphoneos"
        } else if triple.contains("apple-darwin") {
            "macosx"
        } else {
            return None;
        });
    }
    None
}

/// Run `xcrun --sdk <kind> --show-sdk-path` and return the trimmed
/// stdout. `None` on any kind of failure — caller falls back to
/// "no -isysroot" which will work for host-macOS builds where
/// `/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk` is the
/// default lookup.
fn xcrun_sdk_path(kind: &str) -> Option<String> {
    let out = std::process::Command::new("xcrun")
        .args(["--sdk", kind, "--show-sdk-path"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(path)
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
        if arg.starts_with("-Wl,--version-script=") || arg.starts_with("--version-script=") {
            i += 1;
            continue;
        }
        if (arg == "-Wl,--version-script" || arg == "--version-script") && i + 1 < args.len() {
            i += 2;
            continue;
        }
        // Mach-O equivalent: `-Wl,-exported_symbols_list <path>` (or
        // the combined `,<path>` form). rustc emits this in the fat
        // build's linker invocation, naming a temp file that lists
        // every Rust `pub` symbol the full crate graph wanted to
        // export. The patch link only links a small subset of that
        // graph (one `.o` + the bridge `.a` + the stub `.o`), so the
        // file references symbols our inputs don't define and ld
        // errors out with `Undefined symbols … <initial-undefines>`.
        // Drop it from the patch link line.
        //
        // We deliberately DO keep per-symbol `-Wl,-exported_symbol,…`
        // directives (which rustc also emits, one per `#[no_mangle]
        // pub extern "C"` symbol). Those name symbols the user's
        // crate actually defines — `whisker_aslr_anchor`,
        // `whisker_app_main`, `whisker_tick`, the bridge entry
        // points — and `subsecond::apply_patch` needs at least
        // `whisker_aslr_anchor` to be in the patch dylib's `.dynsym`
        // so its dlsym lookup hits. Filtering them out would land
        // us with a patch dylib that loads fine but panics inside
        // subsecond's symbol lookup.
        if arg == "-Wl,-exported_symbols_list" && i + 1 < args.len() {
            i += 2;
            continue;
        }
        if arg.starts_with("-Wl,-exported_symbols_list,") {
            i += 1;
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
            "-l",
            "iconv",
        ]));
        assert_eq!(kept, s(&["-O3", "-l", "iconv"]));
    }

    #[test]
    fn filter_drops_dynamic_libraries_too() {
        // Captured fat-build linker may have re-linked an existing
        // .so/.dylib; we drop those for the same reason as static
        // archives — their symbols come back via dynamic_lookup.
        let kept =
            filter_captured_linker_args(&s(&["/tmp/libfoo.so", "/tmp/libbar.dylib", "-shared"]));
        // -shared also dropped (we re-add later).
        assert!(kept.is_empty(), "expected empty, got {kept:?}");
    }

    #[test]
    fn filter_keeps_search_path_and_link_flags() {
        let kept = filter_captured_linker_args(&s(&[
            "-L",
            "/sdk/lib",
            "-L/different/dir",
            "-lcurl",
            "-l",
            "z",
            "-Wl,-rpath,/some/path",
            "-isysroot",
            "/Applications/Xcode.app/.../MacOSX.sdk",
            "-arch",
            "arm64",
            "-target",
            "arm64-apple-macosx14.0.0",
            "-fuse-ld=lld",
            "-mmacosx-version-min=11.0",
        ]));
        assert_eq!(
            kept,
            s(&[
                "-L",
                "/sdk/lib",
                "-L/different/dir",
                "-lcurl",
                "-l",
                "z",
                "-Wl,-rpath,/some/path",
                "-isysroot",
                "/Applications/Xcode.app/.../MacOSX.sdk",
                "-arch",
                "arm64",
                "-target",
                "arm64-apple-macosx14.0.0",
                "-fuse-ld=lld",
                "-mmacosx-version-min=11.0",
            ]),
        );
    }

    #[test]
    fn filter_drops_existing_output_path() {
        let kept =
            filter_captured_linker_args(&s(&["-shared", "-o", "/old/libfoo.dylib", "/tmp/foo.o"]));
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
            "-Wl,--version-script=/ws/target/.whisker/android-jni-exports.ver",
            "-Wl,--no-undefined-version",
            "-Wl,--as-needed",
            "-arch",
            "arm64",
        ]));
        assert_eq!(kept, s(&["-Wl,--as-needed", "-arch", "arm64"]));
    }

    #[test]
    fn filter_drops_separated_version_script_form() {
        // Some clang drivers split `-Wl,--version-script=/p` into
        // `--version-script /p` when forwarding to ld. Defensive.
        let kept =
            filter_captured_linker_args(&s(&["--version-script", "/tmp/rustcXX/list", "-pie"]));
        assert_eq!(kept, s(&["-pie"]));
    }

    #[test]
    fn filter_drops_existing_undefined_dynamic_lookup() {
        // Both the separated and the comma-bundled form.
        let kept = filter_captured_linker_args(&s(&[
            "-undefined",
            "dynamic_lookup",
            "-Wl,-undefined,dynamic_lookup",
            "-Wl,--unresolved-symbols=ignore-all",
            "-arch",
            "arm64",
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
            "-framework",
            "Foundation",
            "-framework",
            "CoreFoundation",
        ]));
        assert_eq!(
            kept,
            s(&["-framework", "Foundation", "-framework", "CoreFoundation",]),
        );
    }

    // ----- is_object_or_archive_input ----------------------------------

    #[test]
    fn object_detection_covers_common_extensions() {
        for path in [
            "foo.o",
            "foo.rlib",
            "foo.a",
            "foo.so",
            "foo.dylib",
            "foo.OBJ",
            "foo.LIB", // case-insensitive (Windows)
            "/abs/path/lib.a",
            "rel/dir/foo.o",
        ] {
            assert!(is_object_or_archive_input(path), "{path}");
        }
    }

    #[test]
    fn object_detection_rejects_flags_and_non_object_paths() {
        for path in [
            "-shared",
            "-o",
            "-Llib",
            "-llog",
            "/some/source.rs",
            "Foundation",
            "foo.txt",
            "bar",
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
            &[],
        );
        assert_eq!(
            plan.args,
            s(&[
                "-isysroot",
                "/sdk",
                "-arch",
                "arm64",
                "-shared",
                "-Wl,-undefined,dynamic_lookup",
                "-Wl,-exported_symbol,_whisker_aslr_anchor",
                "-Wl,-exported_symbol,_whisker_app_main",
                "-Wl,-exported_symbol,_whisker_tick",
                "/o/demo.o",
                "-o",
                "/o/libdemo.dylib",
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
            &[],
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
            &[],
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
            &[],
        );
        // Find the position of -o and check the next arg is the new output.
        let dash_o = plan.args.iter().position(|a| a == "-o").unwrap();
        assert_eq!(
            plan.args.get(dash_o + 1).map(String::as_str),
            Some("/new/libnew.dylib")
        );
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
            &[],
        );
        assert_eq!(
            plan.args,
            s(&[
                "-pie",
                "-L",
                "/ndk/lib",
                "-shared",
                "-Wl,--unresolved-symbols=ignore-all",
                "/o/demo.o",
                "-o",
                "/o/libdemo.so",
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
            &[],
        );
        let count = plan
            .args
            .iter()
            .filter(|a| *a == "-Wl,--unresolved-symbols=ignore-all")
            .count();
        assert_eq!(count, 1, "args: {:?}", plan.args);
    }

    #[test]
    fn linux_plan_appends_extra_objects_before_new_object() {
        // Extra objects (typically `stub.o` from
        // `create_undefined_symbol_stub`) land AFTER
        // `--unresolved-symbols` and BEFORE the new object so the
        // linker resolves the patch's references against them first.
        let stub: std::path::PathBuf = "/o/stub.o".into();
        let plan = build_link_plan(
            &s(&["-L", "/ndk/lib"]),
            Path::new("/o/demo.o"),
            Path::new("/o/libdemo.so"),
            LinkerOs::Linux,
            std::slice::from_ref(&stub),
        );
        assert_eq!(
            plan.args,
            s(&[
                "-L",
                "/ndk/lib",
                "-shared",
                "-Wl,--unresolved-symbols=ignore-all",
                "/o/stub.o",
                "/o/demo.o",
                "-o",
                "/o/libdemo.so",
            ]),
        );
    }

    #[test]
    fn linux_plan_with_empty_extras_links_only_the_new_object() {
        let plan = build_link_plan(
            &s(&["-L", "/ndk/lib"]),
            Path::new("/o/demo.o"),
            Path::new("/o/libdemo.so"),
            LinkerOs::Linux,
            &[],
        );
        // Just `-Wl,--unresolved-symbols=ignore-all` + new object + -o
        // -shared + the captured arg `-L /ndk/lib`. No DT_NEEDED, no
        // back-edge to a host dylib.
        assert!(
            !plan.args.iter().any(|a| a.contains("--no-as-needed")
                || a.ends_with(".so") && a != "/o/libdemo.so"),
            "no host-dylib back-edge expected: {:?}",
            plan.args,
        );
    }

    #[test]
    fn macos_plan_also_threads_extra_objects() {
        // Same shape on macOS — stub objects are platform-portable
        // (we generate Mach-O bytes there).
        let stub: std::path::PathBuf = "/o/stub.o".into();
        let plan = build_link_plan(
            &s(&["-isysroot", "/sdk"]),
            Path::new("/o/demo.o"),
            Path::new("/o/libdemo.dylib"),
            LinkerOs::Macos,
            std::slice::from_ref(&stub),
        );
        assert!(
            plan.args.iter().any(|a| a == "/o/stub.o"),
            "stub object should appear on macOS plan: {:?}",
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
            &[],
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
