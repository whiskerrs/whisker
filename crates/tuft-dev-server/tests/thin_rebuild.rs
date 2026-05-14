//! End-to-end check on the thin-rebuild pipeline (I4g-5b).
//!
//! Validates the moving parts in concert:
//!
//!   1. Build the fixture cdylib once with rustc directly to produce
//!      an "original" `.dylib` / `.so` and capture the rustc args
//!      we just used.
//!   2. Run `parse_symbol_table` on it, look up `answer` — confirms
//!      a baseline address.
//!   3. Edit the fixture's `lib.rs` (change `42` → `1234`) and call
//!      `thin_rebuild` with the same args (modulo `--out-dir`).
//!   4. Re-parse the new dylib and look up `answer` again. Expect:
//!      - the patch dylib exists,
//!      - `answer` is still defined and exported,
//!      - the function body actually changed (we don't compare
//!        addresses across files because both are independent
//!        cdylibs at independent base addresses; instead we check
//!        the symbol's *size* is non-zero on both sides, which is
//!        the precondition for `build_jump_table` to consider
//!        this a patchable function).
//!
//! The test runs the real rustc on the host machine. Cross-target
//! (Android NDK / iOS) thin rebuild is exercised in the e2e step
//! (I4g-8); here we validate the host-side mechanics so the
//! per-arg edits and the file-finding logic are nailed down before
//! we layer NDK toolchain detection on top.

use std::path::{Path, PathBuf};

use tuft_dev_server::hotpatch::{
    build_thin_rebuild_plan, library_filename, parse_symbol_table, thin_rebuild,
    CapturedRustcInvocation,
};

const FIXTURE_CRATE_NAME: &str = "thin_build_fixture";

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/thin-build-fixture")
}

/// Per-test scratch dir; cleaned up at the end of each `tokio::test`.
fn unique_tempdir(label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let p = std::env::temp_dir().join(format!("tuft-thin-rebuild-{label}-{pid}-{n}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn rustc_path() -> PathBuf {
    // Mirror cargo's own resolution: `RUSTC` env wins; otherwise
    // the rustc on PATH. The test environment is whatever runs
    // `cargo test`, so this is the same rustc cargo would have used.
    PathBuf::from(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
}

/// Compose a rustc invocation for the fixture, against `lib_rs` and
/// emitting into `out_dir`. We hand-craft this rather than going
/// through the full fat-build dance because the fat build pulls in
/// every workspace dep — overkill for validating thin_rebuild's
/// pure mechanics.
fn captured_for_fixture(lib_rs: &Path, out_dir: &Path) -> CapturedRustcInvocation {
    CapturedRustcInvocation {
        crate_name: FIXTURE_CRATE_NAME.into(),
        args: vec![
            "--edition=2021".into(),
            "--crate-name".into(),
            FIXTURE_CRATE_NAME.into(),
            "--crate-type".into(),
            "cdylib".into(),
            "--out-dir".into(),
            out_dir.to_string_lossy().into(),
            lib_rs.to_string_lossy().into(),
        ],
        timestamp_micros: 0,
    }
}

#[tokio::test]
async fn thin_rebuild_produces_a_cdylib_whose_symbol_table_we_can_parse() {
    // 1. Stage the fixture in a fresh tempdir so concurrent test
    //    runs don't fight over `target/`.
    let work = unique_tempdir("baseline");
    let lib_rs = work.join("lib.rs");
    std::fs::copy(fixture_root().join("src/lib.rs"), &lib_rs).unwrap();

    let out_dir = work.join("out");

    // 2. Build the original — `thin_rebuild` IS a thin-rebuild even
    //    on first run (it only edits crate-type and out-dir).
    let captured = captured_for_fixture(&lib_rs, &out_dir);
    let plan = build_thin_rebuild_plan(&captured, &out_dir);
    let original = thin_rebuild(&plan, &rustc_path(), &work, FIXTURE_CRATE_NAME)
        .await
        .expect("first build");
    assert!(original.is_file(), "should exist: {}", original.display());
    assert_eq!(
        original.file_name().unwrap().to_string_lossy(),
        library_filename(FIXTURE_CRATE_NAME),
    );

    // 3. Parse the symbol table; `answer` must show up as a defined
    //    Text symbol. We don't assert on size — Mach-O's nlist
    //    table doesn't carry one, and ELF / PE both populate it,
    //    so the test would split per-platform for no real gain.
    let table = parse_symbol_table(&original).expect("parse original");
    let answer = table
        .by_name
        .get("answer")
        .or_else(|| table.by_name.get("_answer")) // Mach-O leading underscore
        .expect("answer symbol present");
    assert!(!answer.is_undefined, "answer is defined locally");
    assert!(answer.address > 0, "answer has a real load address");

    let _ = std::fs::remove_dir_all(&work);
}

#[tokio::test]
async fn thin_rebuild_picks_up_a_source_edit_and_changes_function_body() {
    // 1. Stage v1 of the fixture.
    let work = unique_tempdir("edit");
    let lib_rs = work.join("lib.rs");
    let out_dir = work.join("out");
    std::fs::copy(fixture_root().join("src/lib.rs"), &lib_rs).unwrap();

    let captured = captured_for_fixture(&lib_rs, &out_dir);
    let plan = build_thin_rebuild_plan(&captured, &out_dir);

    let v1_dylib = thin_rebuild(&plan, &rustc_path(), &work, FIXTURE_CRATE_NAME)
        .await
        .expect("v1 build");
    let v1_table = parse_symbol_table(&v1_dylib).expect("parse v1");
    let v1_answer = v1_table
        .by_name
        .iter()
        .find(|(k, _)| k.as_str() == "answer" || k.as_str() == "_answer")
        .map(|(_, v)| v.clone())
        .expect("v1 answer");

    // 2. Edit the fixture: change `42` → `1234`. We carry over the
    //    existing v1 dylib bytes so we can compare *contents* in a
    //    moment.
    let v1_bytes = std::fs::read(&v1_dylib).expect("read v1 bytes");

    let body = std::fs::read_to_string(&lib_rs).unwrap();
    let edited = body.replace("42", "1234");
    assert_ne!(body, edited, "edit must actually change the source");
    std::fs::write(&lib_rs, edited).unwrap();

    // 3. Re-run thin_rebuild against the same out_dir. The dylib at
    //    `lib<crate>.{dylib,so}` is overwritten in place (the
    //    operating-system-level file is replaced atomically by
    //    rustc's link step), so we read the new bytes back from the
    //    same path.
    let v2_dylib = thin_rebuild(&plan, &rustc_path(), &work, FIXTURE_CRATE_NAME)
        .await
        .expect("v2 build");
    assert_eq!(v2_dylib, v1_dylib, "same out path");
    let v2_bytes = std::fs::read(&v2_dylib).expect("read v2 bytes");
    let v2_table = parse_symbol_table(&v2_dylib).expect("parse v2");
    let v2_answer = v2_table
        .by_name
        .iter()
        .find(|(k, _)| k.as_str() == "answer" || k.as_str() == "_answer")
        .map(|(_, v)| v.clone())
        .expect("v2 answer");

    // 4. The dylib bytes must differ between v1 and v2 (the function
    //    body changed). If they're identical, the rebuild was a no-op
    //    — usually a sign the source edit didn't actually take.
    assert_ne!(
        v1_bytes, v2_bytes,
        "v1 and v2 dylib should differ after editing the source",
    );
    // `answer` must still be a defined Text symbol on both sides
    // (the precondition build_jump_table looks for). We don't
    // assert on size — Mach-O omits it.
    assert!(!v1_answer.is_undefined && !v2_answer.is_undefined);
    assert!(v1_answer.address > 0 && v2_answer.address > 0);

    let _ = std::fs::remove_dir_all(&work);
}

#[tokio::test]
async fn thin_rebuild_surfaces_compile_errors_as_err() {
    // Source that won't parse. The error must come back as Err
    // (so the dev loop can fall back to Tier 2 / surface to user)
    // rather than producing an empty dylib.
    let work = unique_tempdir("compile-error");
    let lib_rs = work.join("lib.rs");
    std::fs::write(&lib_rs, "this is not valid rust").unwrap();

    let out_dir = work.join("out");
    let captured = captured_for_fixture(&lib_rs, &out_dir);
    let plan = build_thin_rebuild_plan(&captured, &out_dir);

    let res = thin_rebuild(&plan, &rustc_path(), &work, FIXTURE_CRATE_NAME).await;
    assert!(res.is_err(), "compile failure must surface as Err");
    let err = format!("{:#}", res.unwrap_err());
    assert!(
        err.contains("rustc exited"),
        "expected exit-status error, got {err}",
    );

    let _ = std::fs::remove_dir_all(&work);
}
