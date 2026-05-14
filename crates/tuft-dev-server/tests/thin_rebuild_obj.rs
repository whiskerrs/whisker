//! End-to-end check that the `--emit=obj` + own-linker pipeline
//! actually preserves mangled `pub fn` symbols in the resulting
//! patch dylib (I4g-X2c). This is the bit cdylib couldn't give us
//! and the load-bearing prerequisite for Tier 1 hot-patch.
//!
//! Flow:
//!   1. Build the fixture's lib.rs into an "obj" via the new
//!      build_obj_plan + run_obj_plan pipeline (rustc).
//!   2. Hand-craft a minimal captured linker invocation (just the
//!      flags rustc would have passed, minus the object input list)
//!      and run build_link_plan + run_link_plan against it.
//!   3. Parse the resulting `.so`/`.dylib` symbol table and assert
//!      the mangled `thin_build_fixture::calculate` is present.
//!
//! We deliberately don't go through tuft-linker-shim here (its
//! capture path is verified by tuft-cli's unit tests). This test
//! focuses on the question "does --emit=obj + dynamic_lookup link
//! actually keep the mangled symbol exported?", which the abandoned
//! cdylib path could not.

use std::path::{Path, PathBuf};

use tuft_dev_server::hotpatch::{
    build_link_plan, build_obj_plan, library_filename, linker_os_for_host,
    parse_symbol_table, run_link_plan, run_obj_plan, CapturedRustcInvocation,
};

const FIXTURE_CRATE_NAME: &str = "thin_build_fixture";

fn fixture_lib_rs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/thin-build-fixture/src/lib.rs")
}

fn unique_tempdir(label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let p = std::env::temp_dir()
        .join(format!("tuft-thin-rebuild-obj-{label}-{pid}-{n}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn rustc_path() -> PathBuf {
    PathBuf::from(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
}

/// Resolve the linker driver we want to spawn. We deliberately use
/// the same `cc` rustc itself uses by default — `clang` on macOS
/// (resolved via `xcrun -f clang` so SDK env vars are picked up),
/// `cc` on Linux (PATH-resolved). Override via `CC=...`.
fn linker_path() -> PathBuf {
    if let Some(cc) = std::env::var_os("CC") {
        return PathBuf::from(cc);
    }
    if cfg!(target_os = "macos") {
        // xcrun -f clang gives us the active toolchain's clang.
        let out = std::process::Command::new("xcrun")
            .args(["-f", "clang"])
            .output();
        if let Ok(out) = out {
            if out.status.success() {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !path.is_empty() {
                    return PathBuf::from(path);
                }
            }
        }
        return PathBuf::from("clang");
    }
    PathBuf::from("cc")
}

/// macOS SDK path via `xcrun --show-sdk-path`. clang needs `-isysroot
/// <path>` to resolve `-lSystem`; in production this comes through
/// the captured linker invocation, but the test hand-builds an empty
/// captured-args list, so we synthesise the minimum here.
fn host_sdk_args() -> Vec<String> {
    if !cfg!(target_os = "macos") {
        return vec![];
    }
    let Ok(out) = std::process::Command::new("xcrun")
        .args(["--show-sdk-path"])
        .output()
    else {
        return vec![];
    };
    if !out.status.success() {
        return vec![];
    }
    let sdk = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if sdk.is_empty() {
        return vec![];
    }
    vec!["-isysroot".into(), sdk]
}

/// Hand-built captured rustc invocation that compiles the fixture's
/// lib.rs into a single rlib (or, after build_obj_plan rewrites it,
/// into a single .o). Mirrors what cargo would have generated minus
/// the dependency `-L` paths that the standalone fixture doesn't need.
fn captured_rustc_for_fixture(lib_rs: &Path) -> CapturedRustcInvocation {
    CapturedRustcInvocation {
        crate_name: FIXTURE_CRATE_NAME.into(),
        args: vec![
            "--edition=2021".into(),
            "--crate-name".into(),
            FIXTURE_CRATE_NAME.into(),
            "--crate-type".into(),
            "rlib".into(),
            "--emit=link".into(),
            lib_rs.to_string_lossy().into(),
        ],
        timestamp_micros: 0,
    }
}

/// Find a mangled `calculate`-shaped symbol in a name iterator.
/// Mach-O prefixes user symbols with `_`, ELF doesn't; the mangled
/// hash suffix changes per rustc release but the crate name +
/// `9calculate` (Itanium ABI: `<len><name>`, calculate is 9 chars)
/// substring is stable for a given source.
fn find_calculate(table_keys: impl Iterator<Item = String>) -> Option<String> {
    for k in table_keys {
        if k.contains("thin_build_fixture") && k.contains("9calculate") {
            return Some(k);
        }
    }
    None
}

#[tokio::test]
async fn thin_rebuild_obj_plus_dynamic_lookup_link_preserves_mangled_symbols() {
    // ---- Setup ------------------------------------------------------
    let work = unique_tempdir("happy");
    let lib_rs = work.join("lib.rs");
    std::fs::copy(fixture_lib_rs(), &lib_rs).unwrap();
    let captured = captured_rustc_for_fixture(&lib_rs);

    // ---- 1. rustc --emit=obj ----------------------------------------
    let obj_dir = work.join("obj");
    std::fs::create_dir_all(&obj_dir).unwrap();
    let obj_plan = build_obj_plan(&captured, &obj_dir);
    let object = run_obj_plan(&obj_plan, &rustc_path(), &work)
        .await
        .expect("rustc --emit=obj should succeed");
    assert!(
        object.is_file(),
        "expected `{}` to exist after run_obj_plan",
        object.display(),
    );

    // ---- 2. own linker invocation -----------------------------------
    // Hand-built captured linker args: just the SDK path (macOS
    // needs -isysroot to resolve -lSystem; Linux's cc finds libc on
    // its own). build_link_plan adds -shared,
    // -Wl,-undefined,dynamic_lookup (Macos) or
    // -Wl,--unresolved-symbols=ignore-all (Linux), the new object,
    // and the output. That's the minimum needed for the linker to
    // produce a shared library that re-exports unresolved refs back
    // to the host process.
    let dylib = obj_dir.join(library_filename(FIXTURE_CRATE_NAME));
    let link_plan = build_link_plan(
        &host_sdk_args(),
        &object,
        &dylib,
        linker_os_for_host(),
    );
    run_link_plan(&link_plan, &linker_path(), &work)
        .await
        .expect("clang/cc -shared should succeed");
    assert!(
        dylib.is_file(),
        "expected `{}` after run_link_plan",
        dylib.display(),
    );

    // ---- 3. parse the dylib + look for mangled `calculate` ----------
    let table = parse_symbol_table(&dylib).expect("parse the produced dylib");
    let calc = find_calculate(table.by_name.keys().cloned()).unwrap_or_else(|| {
        // Diagnostic dump on failure: this is the same shape of
        // failure the abandoned cdylib path produced — printing the
        // first 30 keys helps tell "no calculate, only #[no_mangle]
        // ones present" vs "didn't even produce a symbol table".
        let mut keys: Vec<&String> = table.by_name.keys().collect();
        keys.sort();
        let preview: Vec<&String> = keys.into_iter().take(30).collect();
        panic!(
            "mangled `calculate` not found in {} symbols. \
             First 30 (sorted): {:?}",
            table.by_name.len(),
            preview,
        )
    });
    let info = table.by_name.get(&calc).unwrap();
    assert!(
        !info.is_undefined,
        "calculate should be DEFINED in the patch dylib, not undefined: {info:?}",
    );

    // Sanity: the #[no_mangle] `answer` should also be exported (the
    // easy case — if this ever regresses, something other than
    // mangling broke).
    let has_answer = table
        .by_name
        .keys()
        .any(|k| k == "answer" || k == "_answer");
    assert!(has_answer, "no `answer` symbol in dylib");

    let _ = std::fs::remove_dir_all(&work);
}
