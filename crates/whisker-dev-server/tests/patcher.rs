//! End-to-end check on `Patcher::build_patch` through the new
//! pipeline (rustc --emit=obj + own linker invoke + dynamic_lookup).
//! The load-bearing question this test answers is the same one the
//! abandoned cdylib path could not:
//!
//!     "Is rustc's name mangling stable enough across two builds
//!      from different sources that build_jump_table can match
//!      `calculate` (a mangled function) on both sides and emit a
//!      JumpTable entry for it?"
//!
//! If the answer is no, Tier 1 hot-patch is impossible for any Rust
//! function that isn't `#[no_mangle]` — which is approximately every
//! Rust function in a real Whisker app. We need to prove yes here, in
//! isolation, before wiring Patcher into the dev loop (I4g-7).
//!
//! Flow:
//!   1. Build v1 of the fixture into a "patch-shaped" dylib via the
//!      same thin_rebuild_obj pipeline production uses — this is
//!      the **original** in this test. (Doing it through the new
//!      pipeline is what gives the original its mangled symbols;
//!      a cdylib build would strip them and there'd be nothing to
//!      diff against.)
//!   2. Wrap that dylib in a HotpatchModuleCache (= what
//!      `Patcher::initialize` would produce in production).
//!   3. Hand-craft captured rustc + linker maps for the fixture.
//!   4. Patcher::new(...) with the above.
//!   5. Edit lib.rs to change `calculate`'s body (`x * 2` → `x * 3`).
//!   6. Patcher::build_patch().await — internally re-runs the
//!      pipeline against the edited source, parses the new dylib,
//!      diffs.
//!   7. Assert the JumpTable map contains an entry whose key/value
//!      lines up with `calculate`'s mangled symbol on both sides.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use whisker_dev_server::hotpatch::{
    build_link_plan, build_obj_plan, library_filename, linker_os_for_host, parse_symbol_table,
    run_link_plan, run_obj_plan, CapturedLinkerInvocation, CapturedRustcInvocation,
    HotpatchModuleCache, LinkerOs, Patcher,
};

const FIXTURE_CRATE_NAME: &str = "thin_build_fixture";

fn fixture_lib_rs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/thin-build-fixture/src/lib.rs")
}

fn unique_tempdir(label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let p = std::env::temp_dir().join(format!("whisker-patcher-{label}-{pid}-{n}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn rustc_path() -> PathBuf {
    PathBuf::from(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
}

fn linker_path() -> PathBuf {
    if let Some(cc) = std::env::var_os("CC") {
        return PathBuf::from(cc);
    }
    if cfg!(target_os = "macos") {
        if let Ok(out) = std::process::Command::new("xcrun")
            .args(["-f", "clang"])
            .output()
        {
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

/// Hand-built captured rustc invocation for the fixture's lib.rs.
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

/// Hand-built captured linker invocation, with just enough OS/SDK
/// flags for clang/cc to find libc/libSystem. Output points at the
/// fat-build dylib name the patcher's lookup expects.
fn captured_linker_for_fixture(output_dylib: &Path) -> CapturedLinkerInvocation {
    let mut args = vec![];
    if cfg!(target_os = "macos") {
        if let Ok(out) = std::process::Command::new("xcrun")
            .args(["--show-sdk-path"])
            .output()
        {
            if out.status.success() {
                let sdk = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !sdk.is_empty() {
                    args.push("-isysroot".into());
                    args.push(sdk);
                }
            }
        }
    }
    CapturedLinkerInvocation {
        output: Some(output_dylib.to_string_lossy().to_string()),
        args,
        timestamp_micros: 0,
    }
}

/// Build the v1 dylib through the same pipeline production uses,
/// so its symbol table contains the mangled `calculate` we want to
/// diff against.
async fn build_original_via_pipeline(lib_rs: &Path, out_dir: &Path, cwd: &Path) -> PathBuf {
    let captured = captured_rustc_for_fixture(lib_rs);
    let obj_plan = build_obj_plan(&captured, out_dir);
    let object = run_obj_plan(&obj_plan, &rustc_path(), cwd)
        .await
        .expect("v1 obj");
    let dylib = out_dir.join(library_filename(FIXTURE_CRATE_NAME));
    let captured_linker = captured_linker_for_fixture(&dylib);
    let link_plan = build_link_plan(
        &captured_linker.args,
        &object,
        &dylib,
        linker_os_for_host(),
        &[],
        &[],
    );
    run_link_plan(&link_plan, &linker_path(), cwd)
        .await
        .expect("v1 link");
    dylib
}

/// Find the mangled `calculate` symbol — `calculate` is 9 chars
/// (Itanium ABI: `<len><name>`), so the substring `9calculate`
/// uniquely identifies it within the crate's mangled names.
fn find_calculate(table_keys: impl Iterator<Item = String>) -> Option<String> {
    table_keys
        .into_iter()
        .find(|k| k.contains("thin_build_fixture") && k.contains("9calculate"))
}

#[tokio::test]
async fn build_patch_emits_a_jump_table_entry_for_a_mangled_function() {
    // ---- Setup: v1 source + original dylib + cache ----------------
    let work = unique_tempdir("happy");
    let lib_rs = work.join("lib.rs");
    std::fs::copy(fixture_lib_rs(), &lib_rs).unwrap();

    let original_out = work.join("original");
    std::fs::create_dir_all(&original_out).unwrap();
    let original_dylib = build_original_via_pipeline(&lib_rs, &original_out, &work).await;

    let original_table = parse_symbol_table(&original_dylib).expect("parse v1");
    let original_calc =
        find_calculate(original_table.by_name.keys().cloned()).unwrap_or_else(|| {
            panic!(
                "no `calculate`-shaped symbol in v1; keys: {:?}",
                original_table.by_name.keys().take(20).collect::<Vec<_>>(),
            )
        });

    let original_cache = HotpatchModuleCache::from_path(&original_dylib).expect("cache");

    // ---- Construct Patcher with hand-built captured maps ----------
    let patch_out = work.join("patches");
    let captured_rustc = captured_rustc_for_fixture(&lib_rs);
    let mut captured_rustc_args = HashMap::new();
    captured_rustc_args.insert(FIXTURE_CRATE_NAME.into(), captured_rustc);

    // The patcher looks up the linker capture by the basename of the
    // original library file, so key the map under the dylib name the
    // patcher will produce (= the same library_filename).
    let lib_filename = library_filename(FIXTURE_CRATE_NAME);
    let captured_linker = captured_linker_for_fixture(&original_dylib);
    let mut captured_linker_args = HashMap::new();
    captured_linker_args.insert(lib_filename, captured_linker);

    let patcher = Patcher::new(
        FIXTURE_CRATE_NAME.into(),
        rustc_path(),
        linker_path(),
        work.clone(),
        patch_out.clone(),
        match linker_os_for_host() {
            LinkerOs::Macos => LinkerOs::Macos,
            LinkerOs::Linux => LinkerOs::Linux,
            LinkerOs::Other => LinkerOs::Other,
        },
        original_cache,
        captured_rustc_args,
        captured_linker_args,
    );

    // ---- Edit the source: `x * 2` → `x * 3` -----------------------
    let body = std::fs::read_to_string(&lib_rs).unwrap();
    let edited = body.replace("x * 2", "x * 3");
    assert_ne!(body, edited, "edit must change something");
    std::fs::write(&lib_rs, edited).unwrap();

    // ---- The actual test: build_patch produces an entry for `calculate`
    let plan = patcher.build_patch(0).await.expect("build_patch");

    // The diff report must NOT list `calculate` as added or removed
    // (mangled name should be identical across both sides).
    assert!(
        !plan.report.added.iter().any(|n| n == &original_calc),
        "calculate shouldn't be in `added`; report: {:?}",
        plan.report,
    );
    assert!(
        !plan.report.removed.iter().any(|n| n == &original_calc),
        "calculate shouldn't be in `removed` — this would mean rustc \
         produced a different mangled name on rebuild. Report: {:?}",
        plan.report,
    );

    // The JumpTable map must contain an entry for calculate's
    // *original* address. The new address may or may not differ
    // from the original (function bytes might land at the same
    // offset by luck), but the entry's existence proves that
    // rustc gave us the same mangled symbol on both sides.
    let original_calc_info = original_table.by_name.get(&original_calc).unwrap();
    let mapped = plan
        .table
        .map
        .get(&original_calc_info.address)
        .copied()
        .unwrap_or_else(|| {
            panic!(
                "calculate not in JumpTable map. \
                 original_addr={:#x}, map keys: {:?}",
                original_calc_info.address,
                plan.table.map.keys().collect::<Vec<_>>(),
            )
        });
    let _ = mapped;

    let _ = std::fs::remove_dir_all(&work);
}

#[tokio::test]
async fn build_patch_errors_when_no_captured_rustc_for_the_package() {
    // No captured rustc → bail with a clear "did you run the fat
    // build?" message. We don't even need a real dylib for this.
    let work = unique_tempdir("missing-rustc");
    let lib_rs = work.join("lib.rs");
    std::fs::copy(fixture_lib_rs(), &lib_rs).unwrap();
    let original_out = work.join("original");
    std::fs::create_dir_all(&original_out).unwrap();
    let original_dylib = build_original_via_pipeline(&lib_rs, &original_out, &work).await;
    let original_cache = HotpatchModuleCache::from_path(&original_dylib).unwrap();

    let patcher = Patcher::new(
        "package-not-in-cache".into(),
        rustc_path(),
        linker_path(),
        work.clone(),
        work.join("patches"),
        linker_os_for_host(),
        original_cache,
        HashMap::new(),
        HashMap::new(),
    );

    let err = patcher.build_patch(0).await.unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("no captured rustc invocation"), "{msg}");

    let _ = std::fs::remove_dir_all(&work);
}

#[tokio::test]
async fn build_patch_errors_when_captured_linker_is_missing() {
    // Captured rustc present, but no captured linker for the
    // package. Should surface a clear "no captured linker
    // invocation" error rather than silently falling through.
    let work = unique_tempdir("missing-linker");
    let lib_rs = work.join("lib.rs");
    std::fs::copy(fixture_lib_rs(), &lib_rs).unwrap();
    let original_out = work.join("original");
    std::fs::create_dir_all(&original_out).unwrap();
    let original_dylib = build_original_via_pipeline(&lib_rs, &original_out, &work).await;
    let original_cache = HotpatchModuleCache::from_path(&original_dylib).unwrap();

    let mut captured_rustc_args = HashMap::new();
    captured_rustc_args.insert(
        FIXTURE_CRATE_NAME.into(),
        captured_rustc_for_fixture(&lib_rs),
    );

    let patcher = Patcher::new(
        FIXTURE_CRATE_NAME.into(),
        rustc_path(),
        linker_path(),
        work.clone(),
        work.join("patches"),
        linker_os_for_host(),
        original_cache,
        captured_rustc_args,
        HashMap::new(), // empty linker map
    );

    let err = patcher.build_patch(0).await.unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("no captured linker invocation"), "{msg}");

    let _ = std::fs::remove_dir_all(&work);
}
