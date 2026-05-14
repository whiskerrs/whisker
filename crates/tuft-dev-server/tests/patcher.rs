//! End-to-end check on `Patcher::build_patch` against the same
//! cdylib fixture `thin_rebuild` uses (I4g-6).
//!
//! The load-bearing question this test answers:
//!
//!     "Is rustc's name mangling stable enough across two builds
//!      from different sources that build_jump_table can match
//!      `calculate` (a mangled function) on both sides and emit
//!      a JumpTable entry for it?"
//!
//! If the answer is no, Tier 1 hot-patch is impossible for any
//! Rust function that isn't `#[no_mangle]` — which is approximately
//! every Rust function in a real Tuft app. We need to prove yes
//! here, in isolation, before wiring Patcher into the dev loop.
//!
//! The flow:
//!   1. Build v1 of the fixture → "original" dylib + symbol table.
//!   2. Wrap that dylib in a HotpatchModuleCache (= what
//!      `Patcher::initialize` would produce in production).
//!   3. Hand-craft a captured rustc invocation for the fixture.
//!   4. Patcher::new(...) with the above.
//!   5. Edit lib.rs to change `calculate` 's body (`x * 2` → `x * 3`).
//!   6. Patcher::build_patch().await — internally runs thin_rebuild
//!      against the edited source, parses the new dylib, diffs.
//!   7. Assert the JumpTable map contains an entry whose key/value
//!      lines up with `calculate`'s mangled symbol on both sides.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tuft_dev_server::hotpatch::{
    library_filename, parse_symbol_table, CapturedRustcInvocation,
    HotpatchModuleCache, Patcher,
};
use tuft_dev_server::Target;

const FIXTURE_CRATE_NAME: &str = "thin_build_fixture";
const _: Target = Target::Host; // ensure the crate import is alive

fn fixture_lib_rs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/thin-build-fixture/src/lib.rs")
}

fn unique_tempdir(label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let p = std::env::temp_dir().join(format!("tuft-patcher-{label}-{pid}-{n}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn rustc_path() -> PathBuf {
    PathBuf::from(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
}

/// Compose an invocation that builds the fixture's lib.rs into a
/// cdylib at `out_dir`. Mirrors what cargo would have generated
/// (modulo dependency `-L` paths the fixture doesn't need).
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

/// Spawn rustc directly (no Patcher) to produce the v1 dylib that
/// stands in for the original binary in this test.
fn build_original(lib_rs: &Path, out_dir: &Path) -> PathBuf {
    let captured = captured_for_fixture(lib_rs, out_dir);
    let status = std::process::Command::new(rustc_path())
        .args(&captured.args)
        .status()
        .expect("spawn rustc");
    assert!(status.success(), "v1 build failed");
    out_dir.join(library_filename(FIXTURE_CRATE_NAME))
}

/// Find a mangled `calculate` symbol in a symbol table, regardless
/// of platform decoration. Mach-O prefixes user symbols with `_`,
/// ELF doesn't; the mangled hash suffix changes with rustc release
/// but is identical for the same source under the same toolchain.
fn find_calculate(table_keys: impl Iterator<Item = String>) -> Option<String> {
    for k in table_keys {
        if k.contains("thin_build_fixture") && k.contains("8calculate") {
            return Some(k);
        }
    }
    None
}

#[tokio::test]
async fn build_patch_emits_a_jump_table_entry_for_a_mangled_function() {
    // ---- Setup: v1 source + original dylib + cache ----------------
    let work = unique_tempdir("happy");
    let lib_rs = work.join("lib.rs");
    std::fs::copy(fixture_lib_rs(), &lib_rs).unwrap();

    let original_out = work.join("original");
    std::fs::create_dir_all(&original_out).unwrap();
    let original_dylib = build_original(&lib_rs, &original_out);

    // Sanity: we can find `calculate` (mangled) in the original.
    let original_table = parse_symbol_table(&original_dylib).expect("parse v1");
    let original_calc =
        find_calculate(original_table.by_name.keys().cloned()).unwrap_or_else(|| {
            panic!(
                "no `calculate`-shaped symbol in v1; keys: {:?}",
                original_table.by_name.keys().take(20).collect::<Vec<_>>(),
            )
        });

    // Wrap original in a HotpatchModuleCache.
    let original_cache = HotpatchModuleCache::from_path(&original_dylib).expect("cache");

    // ---- Construct Patcher with hand-built captured args -----------
    let patch_out = work.join("patches");
    let captured =
        captured_for_fixture(&lib_rs, &patch_out);
    let mut captured_args = HashMap::new();
    captured_args.insert(FIXTURE_CRATE_NAME.into(), captured);

    let patcher = Patcher::new(
        FIXTURE_CRATE_NAME.into(),
        rustc_path(),
        work.clone(),
        patch_out.clone(),
        original_cache,
        captured_args,
    );

    // ---- Edit the source: `x * 2` → `x * 3` -----------------------
    let body = std::fs::read_to_string(&lib_rs).unwrap();
    let edited = body.replace("x * 2", "x * 3");
    assert_ne!(body, edited, "edit must change something");
    std::fs::write(&lib_rs, edited).unwrap();

    // ---- The actual test: build_patch produces an entry for `calculate`
    let plan = patcher.build_patch().await.expect("build_patch");

    // The diff report must list `calculate` as same-on-both-sides
    // (i.e. NOT in `added` or `removed`).
    assert!(
        !plan.report.added.iter().any(|n| n == &original_calc),
        "calculate shouldn't be in `added`",
    );
    assert!(
        !plan.report.removed.iter().any(|n| n == &original_calc),
        "calculate shouldn't be in `removed` (mangled name unstable!)",
    );

    // The JumpTable map must contain at least one entry — and at
    // least one of those entries must be the calculate function.
    // Look up calculate's *original* address; the map should have
    // it as a key with a different value (the v2 address).
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

    // The patched address may equal the original (function bytes
    // could happen to land at the same offset — pure luck), but
    // typically it differs. Either way, the test's main assertion
    // is "the entry exists at all" — that proves rustc gave us the
    // same mangled symbol on both sides.
    let _ = mapped;

    let _ = std::fs::remove_dir_all(&work);
}

#[tokio::test]
async fn build_patch_errors_when_no_captured_invocation_for_the_package() {
    // Path: Patcher.package is not in captured_args. Should bail
    // with a clear "did you run the fat build?" message.
    let work = unique_tempdir("missing");
    let lib_rs = work.join("lib.rs");
    std::fs::copy(fixture_lib_rs(), &lib_rs).unwrap();
    let original_out = work.join("original");
    std::fs::create_dir_all(&original_out).unwrap();
    let original_dylib = build_original(&lib_rs, &original_out);
    let original_cache = HotpatchModuleCache::from_path(&original_dylib).unwrap();

    let patcher = Patcher::new(
        "package-not-in-cache".into(),
        rustc_path(),
        work.clone(),
        work.join("patches"),
        original_cache,
        HashMap::new(),
    );

    let err = patcher.build_patch().await.unwrap_err();
    let msg = format!("{:#}", err);
    assert!(
        msg.contains("no captured rustc invocation"),
        "{msg}",
    );

    let _ = std::fs::remove_dir_all(&work);
}
