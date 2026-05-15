# Whisker — Architecture Overview

How the workspace is sliced into crates, what each crate is for, and how
the **`whisker run` hot-reload dev loop** wires them together.

## Crate graph

```
                                      whisker-macros
                                      (proc-macros)
                                            │
                                            │ rsx! / #[whisker::main]
                                            ▼
   whisker-app-config ─────────► whisker (umbrella)
   (AppConfig types)                │   prelude
                                    │   __main_runtime
                                    │
                                    ├──► whisker-runtime
                                    │    (element tree, diff, signals)
                                    │
                                    └──► whisker-driver  ──► whisker-driver-sys
                                         (Lynx backend)     (unsafe FFI + bridge)
                                              │
                                              ▼  (only with --features hot-reload)
                                         whisker-dev-runtime
                                         (WebSocket receiver,
                                          subsecond::apply_patch)

   User crate (e.g. examples/hello-world)
   ├── src/lib.rs              — user code: `#[whisker::main] fn app() { rsx!{…} }`
   ├── whisker.rs              — `fn configure(&mut AppConfig)` for `whisker run`
   ├── android/                — Gradle project
   ├── ios/                    — Xcode project (xcodegen-generated)
   └── Cargo.toml              — depends on `whisker` (umbrella)

   Host shells
   whisker-cli                 — `whisker run`, manifest+probe, doctor
   ├── manifest.rs             — Cargo.toml discovery
   ├── probe.rs                — compile+run user's whisker.rs → AppConfig JSON
   └── run.rs                  — AppConfig → dev_server::Config (flat)
        │
        ▼
   whisker-dev-server          — file-watch + cargo/xtask builds +
                                 install/launch + Tier 1 subsecond patches
                                 + WebSocket push. **Does not depend on
                                 whisker-app-config — accepts only flat
                                 fields via `Config`.**

   xtask                       — build orchestration (NDK/Xcode invocations)

   crates/whisker-subsecond    — forked subsecond (whisker_aslr_anchor),
                                 lib name = `subsecond` so consumers keep
                                 `use subsecond::*`.
```

## Crate responsibilities

| Crate | One-line | Depended on by |
|---|---|---|
| `whisker-app-config` | App-metadata types users build in `whisker.rs` | `whisker` (umbrella), `whisker-cli` |
| `whisker-runtime` | Element tree, diff, reactive signals. Renderer-agnostic. | `whisker-driver` |
| `whisker-driver-sys` | Unsafe `extern "C"` declarations matching the C++ bridge | `whisker-driver` |
| `whisker-driver` | Safe Rust wrappers + Lynx backend; bootstraps `subsecond` when `hot-reload` is on | `whisker` |
| `whisker-dev-runtime` | App-side WebSocket receiver for hot patches. **Compiled only with `hot-reload`** | `whisker-driver` (feature-gated) |
| `whisker-macros` | `#[whisker::main]` and `rsx!` proc-macros | `whisker` |
| `whisker` | Umbrella crate users `use whisker::prelude::*` from | user crates |
| `whisker-dev-server` | Host dev loop, manifest-agnostic. Drives Tier 1 patch construction. | `whisker-cli` |
| `whisker-cli` | `whisker run`, manifest probe, doctor. Resolves AppConfig and hands flat Config to dev-server. | (binary) |
| `xtask` | Cargo build automation: NDK, Lynx framework, xcframework wrapping | (binary) |
| `whisker-subsecond` | Forked subsecond engine — anchors ASLR on `whisker_aslr_anchor` instead of `main`. Exposed as `subsecond` to consumers via `[lib] name = "subsecond"`. | `whisker`, `whisker-driver`, `whisker-dev-runtime` |

## `hot-reload` feature flow

The `hot-reload` feature is **off by default**. Release builds get a
compact binary with no subsecond / no WebSocket / no tokio. `whisker
run` flips it on when invoking the fat build:

```
$ whisker run --target android --hot-patch
            │
            ▼  (whisker-cli adds `--features whisker/hot-reload`)
whisker = { features = ["hot-reload"] }   ← in user crate's Cargo.toml
  │
  ├── whisker-driver = { features = ["hot-reload"] }
  │     │
  │     ├── subsecond                       ← runtime hot-patch engine
  │     └── whisker-dev-runtime = { features = ["hot-reload"] }
  │           │
  │           └── tokio, tokio-tungstenite  ← WebSocket receiver
  │
  └── subsecond                             ← so `subsecond::call(…)` exists
                                               in user code's compilation unit
```

The user crate itself doesn't need a `hot-reload` feature — `whisker`'s
feature gates do everything.

## End-to-end hot-reload flow (Tier 1)

What happens between "user saves a `.rs` file" and "screen updates":

```
        user edits src/lib.rs
                  │
                  ▼
        notify watcher (whisker-dev-server)  ──► ChangeKind::RustCode
                  │
                  ▼
        Patcher::build_patch
        ├── thin rustc --emit=obj             (captured rustc args)
        ├── create_undefined_symbol_stub.o    (host runtime addresses
        │                                       baked in as ARM64
        │                                       jump trampolines)
        ├── clang -shared + thin .o + stub.o  (captured linker args)
        └── parse symbols → build JumpTable
                  │
                  ▼  serialize → base64
        WebSocket envelope to all clients
                  │
                  ▼
        whisker-dev-runtime (in user app)
        ├── deserialize JumpTable
        ├── decode base64 → write dylib to <cache>/patch-NNN.so / .dylib
        └── push onto pending-patch slot
                  │
                  ▼  (next tick on Lynx TASM thread)
        whisker-driver::tick_callback
        ├── apply_pending_hot_patch
        │   └── subsecond::apply_patch(table)
        │       ├── dlopen the patch dylib
        │       ├── dlsym "whisker_aslr_anchor" → runtime base
        │       └── adjust JumpTable keys/values for ASLR slide
        └── runtime.force_frame()
            │
            ▼
        subsecond::call(move || app())
        │     ├── transmute_copy closure → fn pointer
        │     ├── jump_table.map.get(&runtime_app_addr) → patch fn
        │     └── call patch's app() instead of host's
            │
            ▼
        new Element tree → diff vs old → renderer patches → screen update
```

Total wall-clock: ~500 ms – 1 s on hello-world, dominated by the
thin rustc rebuild.

## Why this layering

A few decisions worth remembering:

- **dev-server is manifest-agnostic.** It accepts flat fields
  (`AndroidParams`, `IosParams`), not `AppConfig`. The cli does the
  `whisker.rs` → probe → `AppConfig` → flat translation. Lets a
  future editor plugin construct the same flat `Config` and reuse
  the dev loop without dragging in `whisker-app-config`.

- **`whisker-app-config` is intentionally tiny.** It's the only
  thing the `whisker run` config-probe binary depends on (plus
  `serde_json`). Pulling in the umbrella `whisker` crate would
  inflate probe builds from seconds to minutes (Lynx headers,
  whisker-runtime, etc.).

- **`whisker-driver-sys` is unsafe-only.** Every `extern "C"` decl
  matches `bridge/include/whisker_bridge.h`. Safe wrappers live in
  `whisker-driver`. This is the standard `*-sys` crate pattern.

- **`whisker-dev-runtime` is feature-gated end-to-end.** Without
  `hot-reload`, the crate compiles to nothing — no tokio, no
  tungstenite, no subsecond.

- **subsecond is in-tree, not a published-crate dep.** The
  `whisker-subsecond` crate is Whisker's fork; `[lib] name = "subsecond"`
  preserves the upstream-style `use subsecond::*` API. The fork swaps
  the ASLR anchor from `main` to `whisker_aslr_anchor`. On Android,
  multiple `main` symbols share the linker namespace
  (`app_process64`'s, prior memfd patches'); a `dlsym` for the
  upstream sentinel returns garbage and the dispatch math fails.
  See `crates/whisker-subsecond/src/lib.rs` for the patch.
