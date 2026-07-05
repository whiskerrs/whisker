# Whisker — Architecture Overview

How the workspace is sliced into crates, what each crate is for, and how
the **`whisker run` dev loop** wires them together.

Whisker is a cross-platform mobile UI framework for Rust built on the
Lynx C++ engine. App code is plain Rust — a `#[whisker::main]` entry
point and `render! { … }` views over fine-grained reactive signals — and
runs on iOS and Android by driving Lynx's element tree directly.

## Crate graph

```
                                  whisker-macros
                                  (#[main], #[component],
                                   #[module_component], render!)
                                        │  emits ::whisker::… paths
                                        ▼
   whisker-config ──────────► whisker (umbrella)
   (Config types)                  │   prelude
                                   │   re-export root
                                   │
                                   ├──► whisker-runtime
                                   │    (reactive runtime, element tree,
                                   │     events, tasks). Renderer-agnostic.
                                   │
                                   ├──► whisker-css
                                   │    (type-safe CSS builder + css!)
                                   │
                                   └──► whisker-driver ──► whisker-driver-sys
                                        (safe Lynx backend)  (unsafe FFI +
                                              │               C++ bridge)
                                              ▼  (only with `hot-reload`)
                                        whisker-dev-runtime
                                        (WebSocket receiver,
                                         subsecond::apply_patch)

   subsecond  (= whisker-subsecond, [lib] name = "subsecond")
     pulled into whisker / whisker-driver / whisker-dev-runtime
     when `hot-reload` is on.

   User crate (e.g. examples/podcast)
   ├── src/lib.rs   — `#[whisker::main] fn app() -> Element { render!{…} }`
   ├── whisker.rs   — `fn configure(&mut Config)` (app metadata)
   └── Cargo.toml   — depends on `whisker` (umbrella)
       Native projects are GENERATED under gen/{android,ios}/ by CNG —
       not committed.

   Host tooling (never in the shipped app)
   whisker-cli      — the `whisker` / `cargo-whisker` binary:
                      run / doctor / new / new-module
   ├── probe.rs     — compile+run user's whisker.rs → Config
   ├── platforms.rs — drives whisker-cng (CNG sync) before a build
   └── run.rs       — Config → dev_server::Config (flat)
        │
        ▼
   whisker-dev-server  — the dev loop: file-watch → whisker-build →
                         install/launch + subsecond hot-reload patches +
                         WebSocket push. Manifest-agnostic (flat Config).
        │
        ▼
   whisker-build       — Lynx artifact fetch + per-platform cargo +
                         NDK/Xcode packaging (Android jniLibs/AAR, iOS
                         xcframework). Driven by whisker-dev-server.

   whisker-cng         — Continuous Native Generation: renders
                         gen/{android,ios}/ from whisker.rs's Config.
   whisker-plugin      — CNG plugin trait + JSON envelope + subprocess
                         runner for 3rd-party plugins.
```

## Crate responsibilities

| Crate | One-line | Depended on by |
|---|---|---|
| `whisker` | Umbrella. Users `use whisker::prelude::*`; almost everything is a re-export surfaced through one import root. | user crates |
| `whisker-config` | `Config` metadata types users build in `whisker.rs`. Intentionally tiny. | `whisker`, `whisker-cli`, `whisker-cng` |
| `whisker-runtime` | The reactive runtime (signals/effects/computed/owners/scheduler), the element tree, events, async tasks, and the renderer that wires effects to Lynx handles. Renderer-agnostic. | `whisker`, `whisker-driver` |
| `whisker-css` | Type-safe CSS builder mirroring Lynx's CSS surface + the `css!` macro. | `whisker` |
| `whisker-driver-sys` | Raw `extern "C"` decls matching the C++ bridge (`bridge/…`), plus the bridge sources themselves. Unsafe-only. | `whisker-driver` |
| `whisker-driver` | Safe Rust wrappers over the bridge + the Lynx backend; exposes the host shims (`run`/`tick`) the iOS/Android shells call into. Bootstraps `subsecond` under `hot-reload`. | `whisker` |
| `whisker-dev-runtime` | App-side WebSocket receiver + log capture for hot patches. **Compiled only with `hot-reload`** — release builds drop it entirely. | `whisker-driver` (feature-gated) |
| `whisker-macros` | `#[whisker::main]`, `#[component]`, `#[module_component]`, and the `render!` DSL. | `whisker` |
| `whisker-cli` | The `whisker` / `cargo-whisker` binary: `run`, `doctor`, `new`, `new-module`. Resolves Config via the `whisker.rs` probe; hands a flat Config to dev-server. | (binary) |
| `whisker-dev-server` | Host dev loop, manifest-agnostic. Owns watch → build → install → hot-reload patch → WebSocket push. | `whisker-cli` |
| `whisker-build` | Lynx artifact fetch, cargo cross-compile, AAR/xcframework packaging. | `whisker-dev-server` |
| `whisker-cng` | Continuous Native Generation: pure renderer of `gen/{android,ios}/` from Config, fingerprint-gated. No CLI surface, no side effects. | `whisker-cli` |
| `whisker-plugin` | CNG plugin surface: `Plugin` trait, IR types, JSON envelope, subprocess runner shared by the engine and 3rd-party plugin binaries. | `whisker-cng`, 3rd-party plugins |
| `whisker-subsecond` | Whisker's fork of DioxusLabs `subsecond` — anchors the ASLR-slide lookup on `whisker_aslr_anchor` (emitted by `#[whisker::main]`) instead of `main`. `[lib] name = "subsecond"` keeps `use subsecond::*`. | `whisker`, `whisker-driver`, `whisker-dev-runtime` |

### Modules and the router (`packages/*`)

First-party, app-facing add-on crates that depend on `whisker` like any
user crate would. They are *not* part of the framework core:

- **`whisker-router`** (+ `whisker-router-macros`) — type-safe,
  signal-backed routing: single Lynx engine, custom transitions, nested
  layouts (tabs/modal). `StackLayout` uses `Owner::pause`/`resume` to
  freeze off-screen back-stack entries.
- **Platform modules** (`whisker-local-store`, `whisker-safe-area`,
  `whisker-audio`, `whisker-video`, `whisker-image`) — native bridges
  exposed through `#[module_component]` / the `module!` macro and
  reactive signals fed by native events.
- **Widgets** (`whisker-svg`, `whisker-icons`) — pure-Rust components
  built on the public API.

`whisker-local-store` doubles as the documented template for writing a
first-party module; see [`module-api-design.md`](module-api-design.md).

## The runtime layers

Three layers, each renderer-agnostic until the bottom:

1. **Reactive runtime** (`whisker-runtime/src/reactive`) — fine-grained
   signals, effects, computed, owners/scopes, batching scheduler. No
   virtual DOM and **no diff pass**. See
   [`reactivity-design.md`](reactivity-design.md).
2. **View / renderer** (`whisker-runtime/src/view`) — `Element` is a
   `Copy` handle wrapping a Lynx `FiberElement`. The `render!` macro and
   builder chains create elements, set attributes once for static props,
   and wrap dynamic props in `effect`s that call `SetAttribute` /
   `SetRawInlineStyles` directly. Control flow (`Show`, `ForEach`) and
   the native `<list>` provider live here too.
3. **Driver / bridge** (`whisker-driver` + `whisker-driver-sys`) — the
   Lynx C++ engine boundary.

### The Lynx bridge

`whisker-driver-sys` carries the C++ bridge sources and the raw
`extern "C"` declarations that match them; `whisker-driver` provides the
safe Rust wrappers and the host shims (`run`, `tick`) that the iOS and
Android shells invoke. The runtime's view layer calls these wrappers to
allocate Lynx elements, set attributes, register event listeners, and
invoke element methods (`bounding_client_rect`, `animate`, …).

Whisker ships a pinned **fork of Lynx**. How that fork is built and
distributed (iOS SwiftPM binary targets, Android Maven AARs) and how
versions stay in lockstep is covered in
[`lynx-integration.md`](lynx-integration.md) and
[`ios-spm-distribution.md`](ios-spm-distribution.md).

## `hot-reload` feature flow

The `hot-reload` feature is **off by default**. Release builds get a
compact binary with no subsecond, no WebSocket, no tokio. `whisker run`
flips it on for the dev build:

```
$ whisker run android --hot-patch
            │
            ▼  (cli adds `--features whisker/hot-reload`)
whisker = { features = ["hot-reload"] }
  ├── whisker-driver = { features = ["hot-reload"] }
  │     ├── subsecond                        ← runtime hot-patch engine
  │     └── whisker-dev-runtime = { features = ["hot-reload"] }
  │           └── tokio + tokio-tungstenite  ← WebSocket receiver
  └── subsecond                              ← so `subsecond::call(…)`
                                                exists in user code's
                                                compilation unit
```

The user crate needs no `hot-reload` feature of its own — `whisker`'s
feature gates do everything.

## The `whisker run` dev loop

`whisker run <platform>` is the developer's primary command. The cli is
a thin wrapper: it probes `whisker.rs` into a `Config`, runs CNG to
materialise `gen/{android,ios}/`, then hands a flat `Config` to
`whisker-dev-server`, which owns the long-running loop:

```
  edit src/lib.rs
        │
        ▼
  watcher (notify)  →  ChangeKind::{RustCode | CargoToml | Other}
        │
        ▼
  decide_action
   ├── Hot Reload (RustCode, on save): build a thin patch dylib from
   │   the changed user crate (captured rustc + linker args + a host-
   │   symbol jump stub), parse it into a subsecond JumpTable, push it
   │   over the WebSocket to connected devices. ~½–1 s on a small app.
   │
   └── Full Reload (explicit `R` only; Cargo.toml changes prompt for it):
       full whisker-build (cargo cross-compile + per-platform package)
       → install/launch via adb / simctl.
```

On the device, `whisker-dev-runtime` receives a patch, writes the dylib
to a cache path, and queues it. On the next Lynx TASM-thread tick,
`whisker-driver` applies it via `subsecond::apply_patch` and re-drives
the frame; `subsecond::call(move || app())` then dispatches into the
patched function. Per-component remount preserves higher-owner state.

The end-to-end mechanics of both tiers — captured-args replay, the ASLR
anchor, the jump-table math, and the per-component remount strategy —
are documented in
[`hot-reload-internals.md`](hot-reload-internals.md).

## Why this layering

- **dev-server is manifest-agnostic.** It accepts flat fields, not
  `Config`. The cli does the `whisker.rs` → probe → `Config` → flat
  translation, so a future editor plugin can construct the same flat
  Config and reuse the dev loop without dragging in `whisker-config`.

- **`whisker-config` is intentionally tiny.** It's the only crate the
  `whisker run` config-probe binary depends on (plus `serde_json`).
  Pulling in the umbrella `whisker` crate would inflate probe builds
  from seconds to minutes (Lynx headers, whisker-runtime, …).

- **Native projects are generated, not committed.** CNG (Expo-style)
  treats `whisker.rs`'s `Config` as the source of truth and renders
  `gen/{android,ios}/` on demand, fingerprint-gated so the fast path is
  a single file read. Regeneration is implicit — the command that needs
  the native tree syncs it first.

- **`whisker-driver-sys` is unsafe-only.** Every `extern "C"` decl
  matches the C++ bridge header; safe wrappers live in `whisker-driver`.
  The standard `*-sys` crate pattern.

- **`whisker-dev-runtime` is feature-gated end-to-end.** Without
  `hot-reload`, the crate compiles to nothing — no tokio, no
  tungstenite, no subsecond.

- **subsecond is in-tree, not a published-crate dep.** The fork swaps
  the ASLR anchor from `main` to `whisker_aslr_anchor`. On Android,
  multiple `main` symbols can share the linker namespace
  (`app_process64`'s, prior memfd patches'); a `dlsym` for the upstream
  sentinel returns garbage and the dispatch math fails. See
  `crates/whisker-subsecond/src/lib.rs`.
</content>
