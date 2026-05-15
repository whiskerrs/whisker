//! Cached snapshot of the *original* binary's symbol table.
//!
//! Why a cache: the binary an app boots with is "hundreds of MB" on
//! a real-world build (subsecond's own comment). We pay that I/O +
//! parse cost once at the start of a `whisker run` session and re-use
//! it for every subsequent hot patch. Each save then only has to
//! parse the small patch dylib and diff it against the cached
//! original — closer to the sub-second budget.
//!
//! Apart from the symbols, we also capture the static virtual
//! address of `whisker_aslr_anchor` (emitted by `#[whisker::main]`)
//! in the host binary. That becomes `JumpTable::aslr_reference`,
//! and our vendored subsecond's `apply_patch` uses it as
//!
//! ```ignore
//! old_offset = aslr_reference()       // dlsym(RTLD_DEFAULT,
//!                                     //   "whisker_aslr_anchor")
//!            - table.aslr_reference    // static anchor addr (us)
//!            = runtime image base.
//! ```
//!
//! so the JumpTable's static keys can be adjusted to live runtime
//! addresses. Two earlier bugs led us here:
//!
//! 1. Setting this to `file.relative_address_base()` (always 0 for
//!    ELF PIE) shifted the keys by `runtime_main_addr` rather than
//!    the image base; `call_as_ptr`'s map lookup always missed.
//! 2. Anchoring on `main` instead of `whisker_aslr_anchor` — on
//!    Android, `dlsym(RTLD_DEFAULT, "main")` resolves to
//!    `app_process64`'s `main`, not the user's `.so`, so the slide
//!    math computed garbage. The unique anchor name fixes that.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::symbol_table::{parse_symbol_table_from_bytes, SymbolTable};

/// Pre-parsed snapshot of the original (== "fat") binary. Built once
/// per dev-server run; subsequent JumpTable construction reads from
/// here without touching the disk again.
#[derive(Debug, Clone)]
pub struct HotpatchModuleCache {
    /// Original binary path on the host. Useful in error messages.
    pub lib: PathBuf,
    /// All symbols projected through `parse_symbol_table_from_bytes`.
    pub symbols: SymbolTable,
    /// Static virtual address of `whisker_aslr_anchor` in the host
    /// binary. Goes straight into
    /// [`subsecond_types::JumpTable::aslr_reference`]. See module
    /// docs for why this is the anchor symbol's address rather than
    /// the file's image base, and why we use a dedicated anchor
    /// rather than upstream subsecond's `main`.
    pub aslr_reference: u64,
}

impl HotpatchModuleCache {
    /// Read the binary at `path` and capture everything we'll need
    /// for hot-patching. Errors out (rather than partially populating
    /// the struct) on read / parse failure — there's no useful cache
    /// to keep around if the original binary is malformed.
    pub fn from_path(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let symbols = parse_symbol_table_from_bytes(&bytes)
            .with_context(|| format!("parse {} symbols", path.display()))?;
        // Mach-O symbol tables keep the legacy underscore prefix
        // (`_whisker_aslr_anchor`); ELF strips it. Try both so the
        // cache works uniformly across host and Android. When
        // neither is present (test fixtures with no
        // `#[whisker::main]`) fall back to 0 — subsecond's
        // `aslr_reference()` math will be off-by-`runtime_anchor`
        // in that case, so device hot patches won't dispatch, but
        // the cache still parses and unit tests that just want
        // symbol-table access still work.
        let aslr_reference = symbols
            .by_name
            .get("whisker_aslr_anchor")
            .or_else(|| symbols.by_name.get("_whisker_aslr_anchor"))
            .map(|s| s.address)
            .unwrap_or(0);
        Ok(Self {
            lib: path,
            symbols,
            aslr_reference,
        })
    }

    /// Convenience accessor (saves the caller a `cache.symbols.by_name`
    /// where they really just want a lookup).
    pub fn symbol_count(&self) -> usize {
        self.symbols.by_name.len()
    }

    /// Convenience: borrow path back as a `&Path`.
    pub fn lib_path(&self) -> &Path {
        &self.lib
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::Instant;

    /// Same workspace-aware bin discovery the symbol-table tests use.
    fn ensure_whisker_binary() -> PathBuf {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let bin = workspace_root.join("target/debug/whisker");
        if !bin.is_file() {
            let status = Command::new("cargo")
                .args(["build", "-p", "whisker-cli", "--bin", "whisker"])
                .current_dir(&workspace_root)
                .status()
                .expect("spawn cargo");
            assert!(status.success());
        }
        bin
    }

    #[test]
    fn from_path_loads_symbols_and_aslr_reference() {
        let bin = ensure_whisker_binary();
        let cache = HotpatchModuleCache::from_path(&bin).expect("cache");
        assert_eq!(cache.lib, bin);
        assert!(
            cache.symbol_count() > 100,
            "expected hundreds of symbols in a debug build, got {}",
            cache.symbol_count(),
        );
        // macOS Mach-O default base is 0x1_0000_0000; Linux ELF is
        // 0; the actual host doesn't matter, we only assert the field
        // round-tripped (non-panic).
        let _ = cache.aslr_reference;
    }

    #[test]
    fn cached_symbol_lookup_is_cheap_after_construction() {
        // The point of the cache: do the heavy parse once. We assert
        // the *second* lookup is much faster than the construction.
        // Threshold is conservative — even a debug-mode HashMap get
        // is microseconds, while parsing the binary is dozens of ms.
        let bin = ensure_whisker_binary();
        let t0 = Instant::now();
        let cache = HotpatchModuleCache::from_path(&bin).expect("cache");
        let parse_time = t0.elapsed();

        let t1 = Instant::now();
        // Touch the table N times to measure something non-trivial.
        let mut found = 0_usize;
        for _ in 0..1_000 {
            if cache.symbols.by_name.contains_key("nonexistent_xyz") {
                found += 1;
            }
        }
        let lookup_time = t1.elapsed();
        assert_eq!(found, 0);

        assert!(
            lookup_time * 50 < parse_time,
            "1000 lookups ({lookup_time:?}) should be much faster than \
             one parse ({parse_time:?}); cache benefit unclear",
        );
    }

    #[test]
    fn missing_path_errors_out() {
        let err = HotpatchModuleCache::from_path("/no/such/binary/exists").unwrap_err();
        assert!(
            err.to_string().contains("read") || err.to_string().contains("/no/such"),
            "{err}",
        );
    }

    #[test]
    fn lib_path_round_trips_the_original_argument() {
        let bin = ensure_whisker_binary();
        let cache = HotpatchModuleCache::from_path(&bin).expect("cache");
        assert_eq!(cache.lib_path(), bin.as_path());
    }
}
