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
| **I4g-7** ✅ | `DevServer::run` branches on `HotPatchMode::Tier1Subsecond`, falls back to Tier 2 on Patcher error | Done in 7a (shim_paths), 7b (Builder.with_capture), 7c (init path), 7d (change loop branch + Tier 2 fallback). |
| **I4g-8** | Android emulator e2e: edit a string in hello-world, observe sub-second swap, confirm signal state survives | 8a (NDK linker resolver), 8b (11 fixes for emulator path — install/INTERNET/adb-reverse/wire format/devlog/save-temps/target-linker), 8c-1 (timing logs), 8c-2 (dylib bytes delivery + host wake) ✅. 8c-3 stuck on cdylib symbol export — see "Second Pivot" below. |

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

## Second Pivot: cdylib → dylib (or bin), discovered in I4g-8c

After 8c-2 the e2e pipeline reaches `subsecond::apply_patch` on
the device but fails with:

```
android_dlopen_ext failed: dlopen failed:
cannot locate symbol "_ZN4core3fmt3num3imp52_$LT$impl$u20$core..fmt..Display$u20$for$u20$i32$GT$3fmt..."
referenced by "/memfd:subsecond-patch (deleted)"
```

The patch dylib is built with `-undefined dynamic_lookup` /
`--unresolved-symbols=ignore-all`, so its missing symbols are
deferred to the host process at `dlopen` time. But the host
process's `libhello_world.so` only carries **175 dynamic
symbols** — `readelf -d` confirms `core::fmt::*`, `alloc::*`, and
basically everything Rust-mangled is **not** in `.dynsym`.

The root cause is rustc's hardcoded behaviour: **for
`crate-type = ["cdylib"]` rustc injects `-Wl,--exclude-libs,ALL`
unconditionally**. This is what `cdylib` was designed for — keep
the implementation hidden behind whatever `#[no_mangle] pub
extern "C"` symbols the C ABI consumer needs — but it makes
subsecond's "resolve patches against host at runtime" model
impossible.

Adding `-Clink-arg=-Wl,--export-dynamic` to the fat build did
nothing observable (`.dynsym` still 175 entries) because rustc's
own `--exclude-libs,ALL` runs after it.

### How Dioxus avoids this

Confirmed by reading DioxusLabs/dioxus@main:

- The user app crate type is **`bin`**, not `cdylib` (the entire
  Dioxus repo has only two `crate-type = ["cdylib"]` Cargo.toml
  files, both unrelated to mobile apps).
- The Android pipeline compiles the user app as a plain PIE
  executable, **renames the resulting binary to `libmain.so`**,
  and packs it into `jniLibs/<arch>/` for `NativeActivity` to load.
- Bin crates do **not** get `--exclude-libs,ALL` from rustc.
- `dx` unconditionally adds `-Clink-arg=-Wl,--export-dynamic` for
  Android (`packages/cli/src/build/request.rs:778-789`).

Dioxus issues searched (`cannot locate symbol`, `subsecond
android`, `dlopen subsecond`) returned 0 hits — users don't trip
on this because they never build cdylibs.

### Options for Tuft

| Option | Crate-type | Architectural impact | Hackiness |
|---|---|---|---|
| **A. Dioxus-style** | `bin` (renamed to `libmain.so`) | Lynx-Kotlin integration upended — needs NativeActivity, removes `MainActivity.kt` / `LynxView` inflation. Architecturally large. | Low for Dioxus' model, but **fights Tuft's Kotlin-driven design**. |
| **B. Stay cdylib + workarounds** | `cdylib` | None | High — rustc-internal flags are not user-facing API. |
| **C. dylib** | `dylib` (Rust dynamic library, still `.so`) | Switch one token in xtask's `--crate-type=cdylib` → `--crate-type=dylib`. Kotlin Activity / LynxView keep working unchanged (System.loadLibrary takes any `.so`). rustc does **not** add `--exclude-libs,ALL` to dylib. | **Low** — one-line change in xtask; everything else inherits. |

### Why cdylib was chosen originally

Looking at `examples/hello-world/Cargo.toml`:

```toml
# Plain rlib. Host workflows (`cargo build`, `cargo test`, `cargo check`,
# rust-analyzer) only see this and succeed without a bridge — no
# unresolved `tuft_bridge_*` symbols. The mobile outputs are produced
# by xtask via `cargo rustc --crate-type X`:
#   Android: cargo xtask android cargo  → cdylib  (libhello_world.so)
#   iOS:     cargo xtask ios build-xcframework  → staticlib (libhello_world.a)
[lib]
crate-type = ["rlib"]
```

The cdylib choice was driven by **Android's `System.loadLibrary`
needs a `.so`** — not by a deep architectural commitment. cdylib
was simply "the obvious thing that produces a .so". dylib also
produces a .so on Android and is fully compatible with
`System.loadLibrary`, and is the natural fix here.

### Recommended next steps for I4g-8c-3a

1. In `xtask/src/android/cargo_build.rs`, change
   `--crate-type cdylib` → `--crate-type dylib`. (Or, less
   disruptive: add a `--crate-type-override` flag honoured by
   Tier 1 builds only; release builds keep cdylib.)
2. Rebuild + run `cargo xtask android build-example -p hello-world`.
   Verify `libhello_world.so` loads via `System.loadLibrary` (it
   should — the file shape is identical, just symbol visibility
   differs).
3. Run `tuft run --target android --hot-patch`. Expect the
   `dynsym` count to jump from ~175 to several thousand. Then
   the apply_patch dlopen should succeed and `patch applied (N
   entries in X ms)` should show in logcat.
4. Confirm:
   - the on-screen string actually changes,
   - tab selection and like-heart bitmask survive the swap
     (state preservation = the headline feature).

If dylib doesn't link (some Rust crates the workspace pulls in
might be cdylib-only — though Tuft is Rust-internal so unlikely),
fall back to Option A: switch the user crate to `bin` +
NativeActivity. That's a much larger refactor but is the only
fully-Dioxus-validated path.

### Open questions for the dylib path

- Does NDK clang link a Rust `dylib` cleanly? Rust dylibs export
  unstable Rust ABI metadata that some linkers strip; bionic's
  loader should be fine but worth verifying.
- Does the resulting `.so` still satisfy NativeActivity-less /
  `System.loadLibrary`-based loading on Android? Should — same
  ELF format.
- Are there crate-graph-level surprises? (dylib of a workspace
  member typically requires its rlib dependencies to also be
  dylib-loadable; for a Tier 1 dev build this might mean
  recompiling the entire dep graph as dylib. Acceptable for dev,
  unacceptable for release — hence keeping cdylib for release.)

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
- I4g-5/6: abandoned (cdylib symbol stripping — first pivot)
- I4g-X1/X2/X3: ✅ (mangled-symbol JumpTable empirically proven)
- I4g-7 (a-d): ✅ (DevServer wiring + Tier 2 fallback)
- I4g-8a: ✅ (NDK linker resolver)
- I4g-8b: ✅ (11 fixes for emulator-path wiring — INTERNET / adb-reverse /
  wire format / devlog / save-temps / target-linker / etc.)
- I4g-8c-1: ✅ (timing logs — sub-second edit→send confirmed)
- I4g-8c-2: ✅ (patch dylib bytes delivery + host wake)
- **I4g-8c-3a: pending — cdylib → dylib pivot. Apply_patch fails
  on missing std symbols; dylib avoids rustc's
  `--exclude-libs,ALL`. See "Second Pivot" above.**
- I4g-8c-3: pending (e2e visual + state preservation, blocked on 8c-3a)

### Numbers observed at session end (debug, warm cache, arm64 emulator)

| Stage | Time |
|---|---|
| edit detect → rustc obj | ~150 ms |
| thin link via NDK clang | ~400 ms |
| **edit → patch sent** | **~700 ms** (well under target) |
| WebSocket queue | ~10 µs |
| device receive + decode + materialise | ~3 ms |
| host wake → tick | ~5 ms |
| **apply_patch** | **fails (cdylib symbol exclusion)** |

The ~700 ms is the meaningful headline: it's the wall-clock cost
of producing the patch and getting it to the device. Tier 2 for
the same edit is 30+ s.
