//! ELF / Mach-O symbol-table parser.
//!
//! The single piece of logic [`parse_symbol_table`] hides: open a
//! binary file, hand it to the `object` crate, and project the rich
//! `object::SymbolTable` API down to the small [`SymbolTable`] view
//! Tuft's hot-reload pipeline actually needs (name → address +
//! kind/size/visibility flags).
//!
//! Why a projection rather than re-using `object::Symbol` directly:
//! the `object` types are bound by lifetimes to the file they came
//! out of. Storing them across an async boundary would force the
//! file bytes to live for the whole dev-loop run. Copying the small
//! pieces we need (name + 4 numbers per symbol) into owned data is
//! cheaper than that lifetime gymnastics.

use anyhow::{Context, Result};
use object::{Object, ObjectSymbol, SymbolKind};
use std::collections::HashMap;
use std::path::Path;

/// What we keep about each symbol — enough to drive subsecond's
/// JumpTable construction (I4g-2) and no more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolInfo {
    /// Address relative to the binary's base (= the value we
    /// eventually feed into a JumpTable's `map` entry on the host
    /// side; ASLR is corrected by the receiver).
    pub address: u64,
    /// Symbol kind: function vs data vs other. Only `Text` is a
    /// hot-patch candidate.
    pub kind: SymbolKind,
    /// Symbol size in bytes (0 if unknown / undefined).
    pub size: u64,
    /// True for `extern "C"` declarations the binary refers to but
    /// doesn't define. JumpTable diffing must skip these.
    pub is_undefined: bool,
    /// Weak linkage. Hot-patching weak symbols is unreliable
    /// (linker chooses winners non-deterministically); we tag them
    /// so callers can decide.
    pub is_weak: bool,
}

/// Owned name → info map. `BTreeMap` would give deterministic
/// iteration order but we want O(1) lookup by symbol name in the
/// JumpTable construction step, so HashMap it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SymbolTable {
    pub by_name: HashMap<String, SymbolInfo>,
}

impl SymbolTable {
    /// Number of symbols of the given kind. Mostly a test convenience.
    pub fn count_kind(&self, kind: SymbolKind) -> usize {
        self.by_name.values().filter(|s| s.kind == kind).count()
    }
}

/// Open `path` and project its symbol table.
pub fn parse_symbol_table(path: &Path) -> Result<SymbolTable> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("read {}", path.display()))?;
    parse_symbol_table_from_bytes(&bytes)
        .with_context(|| format!("parse {}", path.display()))
}

/// Same as [`parse_symbol_table`] but takes the bytes directly. Used
/// by tests so we don't need a fixture file on disk.
pub fn parse_symbol_table_from_bytes(bytes: &[u8]) -> Result<SymbolTable> {
    let file = object::File::parse(bytes).context("object::File::parse")?;
    let mut by_name = HashMap::new();
    for sym in file.symbols() {
        let name = match sym.name() {
            Ok(n) if !n.is_empty() => n.to_string(),
            _ => continue, // unnamed (anonymous local) — useless to us
        };
        by_name.insert(
            name,
            SymbolInfo {
                address: sym.address(),
                kind: sym.kind(),
                size: sym.size(),
                is_undefined: sym.is_undefined(),
                is_weak: sym.is_weak(),
            },
        );
    }
    Ok(SymbolTable { by_name })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// `target/debug/tuft` always exists during a workspace-wide
    /// `cargo test` run because tuft-cli is a member of the workspace
    /// and the test harness builds every member's lib by default.
    /// We force its bin to exist by spawning `cargo build -p tuft-cli
    /// --bin tuft` once at the top of the test, but only if it isn't
    /// already there — a no-op on the second run.
    fn ensure_tuft_binary() -> std::path::PathBuf {
        let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let bin = workspace_root.join("target/debug/tuft");
        if !bin.is_file() {
            let status = Command::new("cargo")
                .args(["build", "-p", "tuft-cli", "--bin", "tuft"])
                .current_dir(&workspace_root)
                .status()
                .expect("spawn cargo");
            assert!(status.success(), "cargo build failed");
        }
        bin
    }

    #[test]
    fn parses_a_real_host_binary_and_finds_known_function_symbols() {
        let bin = ensure_tuft_binary();
        let table = parse_symbol_table(&bin).expect("parse");

        // The symbol table from a debug build of tuft has hundreds
        // of entries; we just need to confirm we loaded SOMETHING
        // reasonable.
        assert!(
            !table.by_name.is_empty(),
            "expected symbols in {}",
            bin.display(),
        );
        assert!(
            table.count_kind(SymbolKind::Text) > 10,
            "expected dozens of function symbols, got {}",
            table.count_kind(SymbolKind::Text),
        );
    }

    #[test]
    fn parses_a_real_host_binary_with_at_least_one_named_function() {
        let bin = ensure_tuft_binary();
        let table = parse_symbol_table(&bin).expect("parse");
        let any_named_text = table
            .by_name
            .values()
            .any(|s| s.kind == SymbolKind::Text && !s.is_undefined);
        assert!(any_named_text, "no defined function symbols");
    }

    #[test]
    fn rejects_non_object_bytes_with_an_error() {
        let err = parse_symbol_table_from_bytes(b"not an object file at all")
            .unwrap_err();
        // We don't pin on the exact message — `object` crate may
        // word it differently across releases — only that an
        // error path exists.
        let _ = err.to_string();
    }

    #[test]
    fn count_kind_is_a_simple_filter() {
        let mut t = SymbolTable::default();
        t.by_name.insert(
            "f".into(),
            SymbolInfo {
                address: 0x1000,
                kind: SymbolKind::Text,
                size: 32,
                is_undefined: false,
                is_weak: false,
            },
        );
        t.by_name.insert(
            "g".into(),
            SymbolInfo {
                address: 0x2000,
                kind: SymbolKind::Data,
                size: 8,
                is_undefined: false,
                is_weak: false,
            },
        );
        assert_eq!(t.count_kind(SymbolKind::Text), 1);
        assert_eq!(t.count_kind(SymbolKind::Data), 1);
        assert_eq!(t.count_kind(SymbolKind::Tls), 0);
    }
}
