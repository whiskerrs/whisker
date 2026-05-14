//! Build a `subsecond::JumpTable` from old vs new symbol tables.
//!
//! This is the diffing brain of Tier 1: given the original binary's
//! symbols and the freshly-linked patch dylib's symbols, walk the
//! ones that exist in both and produce the address-to-address map
//! that `subsecond::apply_patch` will use to rewrite call sites.
//!
//! What we *don't* try to do here:
//!   - Resolve undefined symbols. Those have address 0 in either
//!     side; including them would lie to the runtime.
//!   - Touch data symbols. Hot-patching globals would race the
//!     program, which is harder than function hot-patching and
//!     not on the I4g critical path.
//!   - Touch zero-sized symbols. These are typically PLT stubs and
//!     compiler-introduced markers; no actual code to swap.
//!   - Special-case weak symbols. They get a warning so the dev
//!     loop can surface ambiguity, but the entry is still emitted —
//!     subsecond will pick whichever the dynamic linker chose.

use std::path::PathBuf;

use object::SymbolKind;
use subsecond_types::{AddressMap, JumpTable};

use super::symbol_table::SymbolTable;

/// Names of symbols that exist in `old` and were dropped in `new`.
/// Reported alongside the JumpTable so the dev loop can warn the
/// user that calls into one of those would crash after a patch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffReport {
    /// Symbols only in `old`. A call to one of these post-patch
    /// would resolve to an address subsecond hasn't relocated.
    pub removed: Vec<String>,
    /// Symbols only in `new`. Brand-new functions in the patch.
    /// Safe to ignore: pre-patch code can't reference them.
    pub added: Vec<String>,
    /// Weak symbols included in the map. Subsecond will use
    /// whichever the linker resolved to; this is just a hint to
    /// the user that something might shift.
    pub weak: Vec<String>,
}

/// Result of [`build_jump_table`]: the `subsecond` payload + a
/// human-readable diff summary.
#[derive(Debug, Clone)]
pub struct PatchPlan {
    pub table: JumpTable,
    pub report: DiffReport,
}

/// Compose a [`JumpTable`] from `old` (the live binary's symbol
/// table, parsed once and cached) and `new` (the freshly-built
/// patch dylib's symbol table). `new_lib` is the on-device path
/// the runtime will `dlopen`; `aslr_reference` and `new_base` are
/// what subsecond uses to correct for ASLR slide.
pub fn build_jump_table(
    old: &SymbolTable,
    new: &SymbolTable,
    new_lib: PathBuf,
    aslr_reference: u64,
    new_base_address: u64,
) -> PatchPlan {
    let mut map = AddressMap::default();
    let mut report = DiffReport::default();

    for (name, new_sym) in &new.by_name {
        let old_sym = match old.by_name.get(name) {
            Some(s) => s,
            None => {
                report.added.push(name.clone());
                continue;
            }
        };

        // Skip the things hot-patch can't (or shouldn't) touch.
        if !is_patchable(old_sym.kind) || !is_patchable(new_sym.kind) {
            continue;
        }
        if old_sym.is_undefined || new_sym.is_undefined {
            continue;
        }
        // Skip zero-sized symbols only when *both* are sized — Mach-O
        // never populates `size` on its symbol table (it's an ELF
        // concept), so on macOS every Text symbol comes back with
        // size 0 and we'd discard everything otherwise. ELF defined
        // symbols always have non-zero size, so PLT stubs / markers
        // (size 0 on ELF) still get filtered out there.
        if old_sym.size == 0 && new_sym.size == 0 && cfg!(target_os = "linux") {
            continue;
        }

        if old_sym.is_weak || new_sym.is_weak {
            report.weak.push(name.clone());
        }

        map.insert(old_sym.address, new_sym.address);
    }

    // `removed`: symbols that existed in old but no longer in new.
    for name in old.by_name.keys() {
        if !new.by_name.contains_key(name) {
            report.removed.push(name.clone());
        }
    }
    report.removed.sort();
    report.added.sort();
    report.weak.sort();

    PatchPlan {
        table: JumpTable {
            lib: new_lib,
            map,
            aslr_reference,
            new_base_address,
            ifunc_count: 0, // WASM-only; not relevant for native targets
        },
        report,
    }
}

fn is_patchable(kind: SymbolKind) -> bool {
    matches!(kind, SymbolKind::Text)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotpatch::symbol_table::SymbolInfo;
    use std::collections::HashMap;

    fn text(addr: u64, size: u64) -> SymbolInfo {
        SymbolInfo {
            address: addr,
            kind: SymbolKind::Text,
            size,
            is_undefined: false,
            is_weak: false,
        }
    }
    fn data(addr: u64, size: u64) -> SymbolInfo {
        SymbolInfo {
            address: addr,
            kind: SymbolKind::Data,
            size,
            is_undefined: false,
            is_weak: false,
        }
    }
    fn weak(addr: u64, size: u64) -> SymbolInfo {
        SymbolInfo {
            address: addr,
            kind: SymbolKind::Text,
            size,
            is_undefined: false,
            is_weak: true,
        }
    }
    fn undef() -> SymbolInfo {
        SymbolInfo {
            address: 0,
            kind: SymbolKind::Text,
            size: 0,
            is_undefined: true,
            is_weak: false,
        }
    }

    fn t(entries: Vec<(&str, SymbolInfo)>) -> SymbolTable {
        let mut by_name = HashMap::new();
        for (n, s) in entries {
            by_name.insert(n.to_string(), s);
        }
        SymbolTable { by_name }
    }

    fn lib() -> PathBuf {
        PathBuf::from("/tmp/patch.dylib")
    }

    // ----- happy path --------------------------------------------------

    #[test]
    fn identical_tables_produce_an_identity_like_map() {
        let same = t(vec![("foo", text(0x1000, 32)), ("bar", text(0x2000, 16))]);
        let plan = build_jump_table(&same, &same, lib(), 0, 0);
        assert_eq!(plan.table.map.len(), 2);
        assert_eq!(plan.table.map.get(&0x1000), Some(&0x1000));
        assert_eq!(plan.table.map.get(&0x2000), Some(&0x2000));
        assert!(plan.report.removed.is_empty());
        assert!(plan.report.added.is_empty());
    }

    #[test]
    fn moved_function_records_old_to_new_address() {
        let old = t(vec![("app", text(0x1000, 100))]);
        let new = t(vec![("app", text(0x1500, 120))]);
        let plan = build_jump_table(&old, &new, lib(), 0, 0);
        assert_eq!(plan.table.map.len(), 1);
        assert_eq!(plan.table.map.get(&0x1000), Some(&0x1500));
    }

    #[test]
    fn aslr_and_base_address_are_propagated_into_the_table() {
        let same = t(vec![("foo", text(0x1000, 32))]);
        let plan = build_jump_table(&same, &same, lib(), 0xCAFE_BABE, 0xDEAD_BEEF);
        assert_eq!(plan.table.aslr_reference, 0xCAFE_BABE);
        assert_eq!(plan.table.new_base_address, 0xDEAD_BEEF);
        assert_eq!(plan.table.lib, PathBuf::from("/tmp/patch.dylib"));
        assert_eq!(plan.table.ifunc_count, 0);
    }

    // ----- skipped categories ------------------------------------------

    #[test]
    fn data_symbols_are_skipped() {
        let old = t(vec![("g", data(0x4000, 8))]);
        let new = t(vec![("g", data(0x4100, 8))]);
        assert!(build_jump_table(&old, &new, lib(), 0, 0).table.map.is_empty());
    }

    #[test]
    fn undefined_symbols_are_skipped_either_side() {
        let old = t(vec![("undef", undef()), ("def", text(0x1000, 16))]);
        let new = t(vec![("undef", text(0x9000, 16)), ("def", undef())]);
        let plan = build_jump_table(&old, &new, lib(), 0, 0);
        // "undef" old=undef → skip; "def" new=undef → skip; map empty.
        assert!(plan.table.map.is_empty());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn zero_sized_symbols_are_skipped_on_elf() {
        // On ELF, defined Text symbols always have non-zero size, so
        // a size-0 entry is a PLT stub or compiler marker — skip.
        let old = t(vec![("plt_stub", text(0x1000, 0))]);
        let new = t(vec![("plt_stub", text(0x1100, 0))]);
        assert!(build_jump_table(&old, &new, lib(), 0, 0).table.map.is_empty());
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn zero_sized_symbols_are_kept_on_mach_o() {
        // Mach-O's nlist entries don't carry a size field, so every
        // Text symbol comes back as size 0; the filter must NOT
        // throw them away or we'd never patch anything on macOS.
        let old = t(vec![("foo", text(0x1000, 0))]);
        let new = t(vec![("foo", text(0x1100, 0))]);
        let plan = build_jump_table(&old, &new, lib(), 0, 0);
        assert_eq!(plan.table.map.len(), 1);
        assert_eq!(plan.table.map.get(&0x1000), Some(&0x1100));
    }

    // ----- diff report -------------------------------------------------

    #[test]
    fn added_and_removed_show_up_in_the_report() {
        let old = t(vec![
            ("kept", text(0x1000, 16)),
            ("gone", text(0x2000, 16)),
        ]);
        let new = t(vec![
            ("kept", text(0x1100, 16)),
            ("brand_new", text(0x3000, 16)),
        ]);
        let plan = build_jump_table(&old, &new, lib(), 0, 0);
        assert_eq!(plan.report.removed, vec!["gone".to_string()]);
        assert_eq!(plan.report.added, vec!["brand_new".to_string()]);
        assert_eq!(plan.table.map.len(), 1);
        assert_eq!(plan.table.map.get(&0x1000), Some(&0x1100));
    }

    #[test]
    fn weak_symbol_is_emitted_but_listed_in_report() {
        let old = t(vec![("maybe", weak(0x1000, 16))]);
        let new = t(vec![("maybe", weak(0x1100, 16))]);
        let plan = build_jump_table(&old, &new, lib(), 0, 0);
        assert_eq!(plan.table.map.len(), 1);
        assert_eq!(plan.report.weak, vec!["maybe".to_string()]);
    }

    #[test]
    fn report_lists_are_sorted_for_stable_diagnostics() {
        let old = t(vec![("c", text(0x1, 1)), ("a", text(0x2, 1)), ("b", text(0x3, 1))]);
        let new = t(vec![("z", text(0x4, 1)), ("y", text(0x5, 1)), ("x", text(0x6, 1))]);
        let plan = build_jump_table(&old, &new, lib(), 0, 0);
        assert_eq!(plan.report.removed, vec!["a", "b", "c"]);
        assert_eq!(plan.report.added, vec!["x", "y", "z"]);
    }

    // ----- both empty --------------------------------------------------

    #[test]
    fn empty_inputs_produce_empty_outputs() {
        let plan = build_jump_table(&SymbolTable::default(), &SymbolTable::default(), lib(), 0, 0);
        assert!(plan.table.map.is_empty());
        assert!(plan.report.added.is_empty());
        assert!(plan.report.removed.is_empty());
        assert!(plan.report.weak.is_empty());
    }
}
