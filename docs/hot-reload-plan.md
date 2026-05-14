# Hot Reload (Tier 1) — Implementation Plan

Status: in progress (I4g)
Owners: tuft-cli + tuft-dev-server + tuft-driver
Reference: [DioxusLabs/dioxus](https://github.com/DioxusLabs/dioxus/tree/main/packages/subsecond) / [subsecond](https://crates.io/crates/subsecond) / [`object`](https://crates.io/crates/object)

This document is the source-of-truth design for Tuft's Tier 1
hot-reload pipeline. Tier 2 ("cold rebuild + reinstall + relaunch")
is already shipped (commits `2116675..035eeaf`); Tier 1 ("function-
level patch swap, sub-second, state preserved") is what this plan
covers.

## Goal

When the user saves a `.rs` file inside a Tuft app:

```
user save  ──►  notify event  ──►  thin rebuild  ──►
WebSocket push  ──►  device subsecond::apply_patch  ──►
next frame calls the new function body
```

Target end-to-end latency: **< 1 second** on a warm cache, with
**state (signals, scroll positions, animation phase) preserved**
because we swap function pointers, not the whole binary.

## Architecture

```
┌────────── host (`tuft run --target android --hot-patch`) ───────────┐
│                                                                    │
│  notify  ──►  debounce 200ms  ──►  Patcher::build_patch(change)    │
│                                        │                           │
│                                        ▼                           │
│       ┌────────────────── Patcher ───────────────────────────────┐ │
│       │  HotpatchModuleCache  (original binary's symbol table,   │ │
│       │                        parsed once via `object`)         │ │
│       │                                                          │ │
│       │  thin_rebuild(changed_files, captured_args)              │ │
│       │     ├─ rustc          (only changed crate, incremental)  │ │
│       │     └─ link            (saved linker args + new objects) │ │
│       │     → patch_dylib_path                                    │ │
│       │                                                          │ │
│       │  parse_symbol_table(patch_dylib)                          │ │
│       │  build_jump_table(old_cache, new_table) → JumpTable       │ │
│       └──────────────────────────────────────────────────────────┘ │
│                                        │                           │
│                                        ▼                           │
│                           PatchSender::send(Envelope::Patch{...})  │
│                                        │                           │
└─ /tuft-dev WebSocket  ─────────────────┴───────────────────────────┘
                                         ▼
┌────────── device (Android emulator / iOS Simulator / host) ────────┐
│  tuft-dev-runtime::hot_reload                                      │
│    ws.recv() → PENDING.set(table)                                  │
│                                                                    │
│  tuft-driver::lynx::bootstrap::tick_callback (TASM thread)         │
│    apply_pending_hot_patch():                                      │
│      take_pending_patch()                                          │
│      unsafe { subsecond::apply_patch(table) }                      │
│    runtime.frame()  →  subsecond::call(|| user_app())              │
│                          ▲                                          │
│                          └─ now resolves to the patched function    │
└────────────────────────────────────────────────────────────────────┘
```

The receive side (right half) is **already shipped** — see commits
`0f09521` (tuft-dev-runtime WebSocket receiver) and `b1e060c`
(tuft-driver `subsecond::call` gate). The work in I4g is the **send
side**: building a valid `subsecond_types::JumpTable` from a thin
rebuild and shipping it.

## What `subsecond::JumpTable` actually is

From `subsecond-types::JumpTable`:

```rust
pub struct JumpTable {
    pub lib: PathBuf,           // patch dylib that lives on the device
    pub map: AddressMap,        // u64 (old address) → u64 (new address)
    pub aslr_reference: u64,    // base address of the ORIGINAL binary
    pub new_base_address: u64,  // base address the patch was linked at
    pub ifunc_count: u64,       // WASM-only; 0 elsewhere
}
```

`apply_patch` mmap-loads `lib`, walks `map`, and for each pair
rewrites `*(old + aslr_offset)` to point at the corresponding new
address. `aslr_reference` lets the runtime correct for ASLR slide
between the recorded base and the live process base.

## Subtask breakdown (I4g-1 .. I4g-8)

Each row is one self-contained commit with tests where it makes
sense.

| ID | Output | Test strategy |
|----|--------|---------------|
| **I4g-1** ✅ | `hotpatch::symbol_table::parse_symbol_table(bytes) -> SymbolTable` (host-binary readable) | Parse `target/debug/tuft` (which definitely exists during CI) and assert ≥ 1 known function symbol present. |
| **I4g-2** ✅ | `hotpatch::jump_table::build_jump_table(old, new, …) -> JumpTable` | Hand-built `SymbolTable` fixtures: identical → empty map; one function moved → 1-entry map; only-on-old / only-on-new → skipped. |
| **I4g-3** ✅ | `hotpatch::cache::HotpatchModuleCache` (parses original once, holds it) | Parse twice via `Cache::new(path)` vs `cache.symbols()` — verify the second is cheap (no file IO). |
| **I4g-4** ✅ | `tuft-rustc-shim` bin + `RUSTC_WORKSPACE_WRAPPER` plumbing — captures every rustc invocation's argv to `.tuft/cache/rustc-args/<crate>.json` | Spawn the shim with a fake rustc invocation; assert the JSON file appears with the expected fields. |
| ~~**I4g-5**~~ ❌ | ~~`thin_rebuild` via `--crate-type=cdylib`~~ — *abandoned, see "Pivot" below* | — |
| ~~**I4g-6**~~ ❌ | ~~`Patcher::build_patch` against cdylib~~ — *abandoned, see "Pivot" below* | — |
| **I4g-X1** ✅ | `tuft-linker-shim` bin + `-C linker=<shim>` plumbing — captures linker argv to `.tuft/cache/linker-args/<output>-<ts>.json` | Spawn shim with fake linker invocation; assert JSON appears. 11 unit tests + smoke. |
| **I4g-X2** ✅ | `thin_rebuild_obj` — rustc `--emit=obj --crate-type=rlib` + explicit linker invocation with `-undefined dynamic_lookup` (macOS) / `--unresolved-symbols=ignore-all` (Linux) | Fixture build, parse resulting `.dylib`, **assert mangled `__ZN18thin_build_fixture9calculate17h…E` IS exported and DEFINED**. Done in X2a (build_obj_plan), X2b (build_link_plan), X2c (runner + e2e). |
| **I4g-X3** ✅ | `Patcher::build_patch` rewired through the new pipeline | Done in X3a (wrapper linker capture), X3b (Patcher rewrite + cdylib code removal), X3c (e2e test now passing — JumpTable entry for mangled `calculate` confirmed). |
| **I4g-7** | `DevServer::run` branches on `HotPatchMode::Tier1Subsecond`, falls back to Tier 2 on Patcher error | Unit test: with mode=Tier1 and a stubbed Patcher returning Err, the run loop falls through to a cold rebuild. |
| **I4g-8** | Android emulator e2e: edit a string in hello-world, observe sub-second swap, confirm signal state survives | Manual e2e + screenshots. Logs `[tuft-dev] patch applied` on the device side. |

## Pivot: why I4g-5/6 were abandoned and the new shape

The first attempt had `thin_rebuild` produce a **cdylib** via
`cargo rustc --crate-type=cdylib`, then diff its symbol table
against the original binary. I4g-6's integration test surfaced
the load-bearing failure: **a `pub fn` that isn't `#[no_mangle]`
is dropped from a cdylib's symbol table** (rustc's default symbol
visibility for cdylib targets only exports `extern "C"` /
`#[no_mangle]` items, plus a couple of compiler-inserted entry
points). For Tuft user code — `#[tuft::main] fn app() -> Element`,
helper functions, closures inside `rsx!` — every interesting
target is mangled, so the diff would always be empty.

Re-reading `dx serve`'s `build/patch.rs::create_native_jump_table`
and `build/link.rs::compile_workspace_hotpatch` showed the right
shape:

  1. `cargo rustc --emit=obj --crate-type=lib` — produces an `.o`
     that **does** contain every `pub fn`'s mangled symbol (object
     files come pre-link, so dead-code elimination hasn't run yet
     and visibility flags don't restrict yet).
  2. **Explicit linker invocation** — combines workspace rlibs +
     the fresh `.o` into a `.so`/`.dylib`. By driving the linker
     directly (rather than letting cargo invoke it via cdylib),
     symbol stripping can be controlled and mangled symbols stay
     in the dynamic symbol table.
  3. The resulting `.so`/`.dylib` IS the patch passed to
     `subsecond::apply_patch`, and its symbol table — now full
     of mangled `pub fn`s — is what we diff.

That extra "drive the linker yourself" step is what
`tuft-linker-shim` (I4g-X1) and the rewritten `thin_rebuild`
(I4g-X2) implement. The Phase 1 / Phase 2-rustc-shim code from
I4g-1..4 stays as-is.

## Dependencies to add

Workspace `Cargo.toml`:

```toml
object = { version = "0.36", default-features = false, features = ["read", "std"] }
serde_json  # already present (for the rustc-args cache)
```

`tuft-dev-server`'s `Cargo.toml` picks them up. We deliberately do
not pull `goblin` even though some prior art uses it — `object` is
the same library `dx serve` uses and is the same library
`subsecond` uses internally, which keeps the symbol-resolution
semantics identical and saves one ABI-mismatch hazard.

## Known risks / non-goals

- **iOS device unsupported.** `mmap(PROT_WRITE | PROT_EXEC)` is
  blocked by Apple's W^X policy; `subsecond::apply_patch` cannot
  succeed on iPhone hardware. We target macOS host, Android, and
  iOS Simulator (where W^X is relaxed when launched with
  `DYLD_FORCE_FLAT_NAMESPACE=1`-style options — to be verified in
  I4g-8).
- **TLS handling.** thread-local storage in the patch must point
  at the same backing storage as the original. `dx serve` has a
  whole `cross-tls-*` test suite for this. We start without TLS-
  in-the-patch support; trying to hot-patch a function that
  introduces a new `thread_local!` is rejected with an error
  message rather than silently producing UB.
- **`Cargo.toml` changes are out of scope.** Tier 2 already handles
  `ChangeKind::CargoToml` by triggering a full restart; the Patcher
  bails out for that kind so the dev loop falls back to Tier 2.
- **`#[no_mangle]` symbol churn.** If a hot patch renames or
  removes a `#[no_mangle]` exported symbol, the JumpTable's old →
  new map will be missing that entry and the host shell may crash.
  We log a warning when a previously-exported symbol disappears.
- **rustc cdylib symbol stripping.** Discovered the hard way in
  the abandoned I4g-5/6: `cargo rustc --crate-type=cdylib` does
  NOT export mangled `pub fn` symbols, only `extern "C"` /
  `#[no_mangle]`. The new pipeline (`--emit=obj` + explicit
  linker invocation) bypasses this. Implementations that try to
  go through cargo's cdylib path will find the JumpTable
  perpetually empty for any non-`#[no_mangle]` function. Recorded
  in commit history (I4g-6 integration test failure).

## What "done" looks like (I4g exit criteria)

1. `tuft run --target android --hot-patch` from a fresh emulator
   reflects a string edit in `hello-world` in **under 2 seconds**
   on a warm cache.
2. The on-screen counter (the per-mix heart bitmask, etc.) keeps
   its value across a hot patch — i.e. signals are NOT reset.
3. Editing `Cargo.toml` triggers a clean Tier 2 fallback (no
   crash, no stuck dev loop).
4. All unit + integration tests pass: `cargo test --workspace`.
5. The doc above is kept current as we go (any deviation gets a
   commit that updates this file).

## Status tracker

- I4g-0..4: ✅
- I4g-5/6: abandoned (cdylib symbol stripping)
- I4g-X1/X2/X3: ✅ (mangled-symbol JumpTable empirically proven)
- I4g-7: in progress (DevServer wiring + Tier 2 fallback)
- I4g-8: pending (Android e2e + sub-second wall-clock measurement)
