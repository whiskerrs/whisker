//! Build a "stub" object file that defines every symbol the patch
//! references but doesn't itself supply. Each defined-here-only-for-
//! the-patch symbol resolves to a tiny assembly trampoline that
//! branches to the corresponding *runtime* address in the live host
//! process.
//!
//! This is the load-bearing piece of the Option B / Dioxus-style
//! patch-resolution scheme:
//!
//! - The dev server already knows every symbol's *static* address in
//!   the host `.so` (parsed once into [`HotpatchModuleCache`]).
//! - The device tells us, on its `hello` handshake, its
//!   `subsecond::aslr_reference()` — the *runtime* address of `main`
//!   in the loaded host process.
//! - `aslr_offset = aslr_reference - host_static_main_addr` is the
//!   ASLR slide between the recorded `.so` and the live process.
//! - For each symbol the patch needs, we compute `runtime_addr =
//!   host_static_addr + aslr_offset` and write a stub that jumps
//!   straight there.
//!
//! After linking the patch with this stub object, the patch has *no*
//! `DT_NEEDED` back-edge to the host and no dlopen-time symbol
//! resolution to perform: every call from the patch into the host
//! lands at the correct address by construction. This sidesteps the
//! Android linker-namespace + `RTLD_LOCAL` problems that the prior
//! "back-edge to host dylib" scheme tripped over.
//!
//! Mirrors `dioxus-cli-0.7.9::build::patch::create_undefined_symbol_stub`.
//! Differences:
//!
//! - We don't support Windows (`__imp_` prefix handling and PE32+
//!   stubs are skipped — Whisker targets Android + iOS-sim + the
//!   macOS / Linux host).
//! - We only emit Text stubs; Data symbol stubs are deferred (none of
//!   our hot-patches reference data symbols in the host so far; the
//!   tests in B-4 confirm this).

use anyhow::{bail, Context, Result};
use object::write::{Object, StandardSection, Symbol, SymbolSection};
use object::{
    Architecture, BinaryFormat, Endianness, Object as _, ObjectSymbol, SymbolFlags, SymbolKind,
    SymbolScope,
};
use std::collections::HashSet;
use std::path::Path;

use crate::hotpatch::cache::HotpatchModuleCache;
use crate::hotpatch::LinkerOs;

/// Build a stub `.o` (bytes ready to write to disk) that satisfies
/// every undefined symbol in `patch_obj` whose name is also present
/// in `cache.symbols` as a defined symbol.
///
/// `aslr_reference` is the runtime address of `main` on the device
/// (`subsecond::aslr_reference()`'s return value). The cache's
/// `aslr_reference` field, populated in
/// [`HotpatchModuleCache::from_path`], stores `main`'s *static*
/// address in the host `.so`. The difference is the ASLR slide.
pub fn create_undefined_symbol_stub(
    cache: &HotpatchModuleCache,
    patch_obj: &Path,
    target_os: LinkerOs,
    aslr_reference: u64,
) -> Result<Vec<u8>> {
    let host_static_main = cache.aslr_reference;
    if host_static_main == 0 {
        bail!(
            "host cache has no `main` symbol address (aslr_reference=0); \
             ensure the `#[whisker::main]` macro emitted the synthetic \
             main stub and that the cache parsed it"
        );
    }
    if aslr_reference < host_static_main {
        bail!(
            "device-reported aslr_reference {:#x} is below host's static main address {:#x} — \
             would underflow when computing the ASLR slide. \
             Is the device running a stale build of the host .so?",
            aslr_reference,
            host_static_main,
        );
    }
    let aslr_offset = aslr_reference - host_static_main;

    let bytes = std::fs::read(patch_obj)
        .with_context(|| format!("read patch obj {}", patch_obj.display()))?;
    let file = object::File::parse(&*bytes).context("parse patch obj")?;

    // Collect names: the difference set (undefined ∖ defined) is the
    // set of symbols our patch will need someone else to satisfy.
    let mut undefined: HashSet<String> = HashSet::new();
    let mut defined: HashSet<String> = HashSet::new();
    for sym in file.symbols() {
        let Ok(name) = sym.name() else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        if sym.is_undefined() {
            undefined.insert(name.to_string());
        } else {
            defined.insert(name.to_string());
        }
    }
    let needed: Vec<String> = undefined.difference(&defined).cloned().collect();

    let (bin_fmt, endian) = match target_os {
        LinkerOs::Linux => (BinaryFormat::Elf, Endianness::Little),
        LinkerOs::Macos => (BinaryFormat::MachO, Endianness::Little),
        LinkerOs::Other => bail!("stub object generation: unsupported target_os {:?}", target_os),
    };
    let mut obj = Object::new(bin_fmt, Architecture::Aarch64, endian);

    let text = obj.section_id(StandardSection::Text);

    for name in &needed {
        // Trim `__imp_` (a Windows-only convention) so the lookup
        // works for ELF/Mach-O even if a Rust toolchain change starts
        // emitting it on those platforms too. Currently a no-op for
        // our supported targets.
        let lookup_name = name.trim_start_matches("__imp_");
        let Some(sym) = cache.symbols.by_name.get(lookup_name) else {
            continue;
        };
        if sym.is_undefined || sym.address == 0 {
            continue;
        }
        let abs_addr = sym.address + aslr_offset;

        // Only Text (= code) symbols get stubs right now. Data
        // symbols would need a different shape (pointer-sized Data
        // entry in `.data` rather than executable trampoline), and
        // we haven't seen the patch reference any host *data* in
        // practice.
        if !matches!(sym.kind, SymbolKind::Text) {
            continue;
        }

        let code = arm64_jump_stub(abs_addr);
        let off = obj.append_section_data(text, &code, 4);
        // **Weak**, not strong: the captured linker args bring in
        // archives like `libunwind.a` and `libwhisker_bridge_static.a`
        // that already define some of these symbols. If we emit
        // strong definitions the linker errors with "duplicate
        // symbol"; weak ones lose to the strong defs but still
        // satisfy the long tail (`core::fmt::*`, `alloc::*`, every
        // `pub fn` in the user crate) that nothing else provides.
        obj.add_symbol(Symbol {
            name: name.as_bytes().to_vec(),
            value: off,
            size: code.len() as u64,
            scope: SymbolScope::Linkage,
            kind: SymbolKind::Text,
            weak: true,
            section: SymbolSection::Section(text),
            flags: SymbolFlags::None,
        });
    }

    obj.write().context("serialize stub object")
}

/// ARM64 assembly that loads a 64-bit absolute address into `X16`
/// (the platform's intra-procedure-call scratch register) and
/// branches to it.
///
/// ```text
/// MOVZ X16, #imm0,  LSL #0    ; bits  0..15
/// MOVK X16, #imm1,  LSL #16   ; bits 16..31
/// MOVK X16, #imm2,  LSL #32   ; bits 32..47
/// MOVK X16, #imm3,  LSL #48   ; bits 48..63
/// BR   X16
/// ```
///
/// 5 × 4 = 20 bytes. Encoded little-endian; the constants below come
/// straight from the ARM64 instruction reference and match the
/// Dioxus implementation.
fn arm64_jump_stub(addr: u64) -> Vec<u8> {
    let mut code = Vec::with_capacity(20);
    let imm0 = (addr & 0xFFFF) as u32;
    code.extend_from_slice(&(0xD280_0010_u32 | (imm0 << 5)).to_le_bytes());
    let imm1 = ((addr >> 16) & 0xFFFF) as u32;
    code.extend_from_slice(&(0xF2A0_0010_u32 | (imm1 << 5)).to_le_bytes());
    let imm2 = ((addr >> 32) & 0xFFFF) as u32;
    code.extend_from_slice(&(0xF2C0_0010_u32 | (imm2 << 5)).to_le_bytes());
    let imm3 = ((addr >> 48) & 0xFFFF) as u32;
    code.extend_from_slice(&(0xF2E0_0010_u32 | (imm3 << 5)).to_le_bytes());
    // BR X16 = 0xD61F_0200, little-endian → `00 02 1F D6`
    code.extend_from_slice(&[0x00, 0x02, 0x1F, 0xD6]);
    code
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm64_stub_encodes_address_zero_as_clear_zero_movs() {
        // addr = 0 → all four immediate fields are 0. MOVZ#0 and the
        // three MOVKs all share Rd=X16 and base encoding. Verify the
        // bytes round-trip to the documented base instructions.
        let code = arm64_jump_stub(0);
        assert_eq!(code.len(), 20);
        assert_eq!(&code[0..4], &0xD280_0010_u32.to_le_bytes()); // MOVZ X16, #0
        assert_eq!(&code[4..8], &0xF2A0_0010_u32.to_le_bytes());
        assert_eq!(&code[8..12], &0xF2C0_0010_u32.to_le_bytes());
        assert_eq!(&code[12..16], &0xF2E0_0010_u32.to_le_bytes());
        assert_eq!(&code[16..20], &[0x00, 0x02, 0x1F, 0xD6]); // BR X16
    }

    #[test]
    fn arm64_stub_encodes_a_canonical_aarch64_userspace_address() {
        // 0x7B40_91FF_2C00 is a plausible Android arm64 user-space
        // address. Slice it into four 16-bit chunks and verify each
        // lands in the right MOV instruction.
        let addr = 0x7B40_91FF_2C00_u64;
        let code = arm64_jump_stub(addr);
        // imm0 = 0x2C00
        let imm0 = (addr & 0xFFFF) as u32;
        assert_eq!(
            &code[0..4],
            &(0xD280_0010_u32 | (imm0 << 5)).to_le_bytes(),
        );
        // imm3 = 0x7B40 — top word lands in the LSL#48 MOVK
        let imm3 = ((addr >> 48) & 0xFFFF) as u32;
        assert_eq!(
            &code[12..16],
            &(0xF2E0_0010_u32 | (imm3 << 5)).to_le_bytes(),
        );
    }
}
