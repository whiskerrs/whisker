# Whisker — Architecture Overview

How the workspace is sliced into crates, what each crate is for, and how
the **`whisker run` dev loop** wires them together.

Whisker is a cross-platform UI framework for Rust migrating from its legacy
Lynx C++ backend to a Rust-owned retained scene, layout, and scheduling model.
App code remains plain Rust — a `#[whisker::main]` entry point and
`render! { … }` views over fine-grained reactive signals. CNG now generates
Lynx-free Android and iOS launch shells. Connecting those shells to the retained
frame protocol is the next mobile slice; the legacy bridge crates remain only
while that Host implementation is completed.

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
                                   │    (css! authoring facade + temporary
                                   │     Lynx CSS serializer)
                                   │             │
                                   │             ▼
                                   │         whisker-style
                                   │         (typed inline-style model,
                                   │          stable property registry)
                                   │
                                   └──► whisker-driver ──► whisker-driver-sys
                                        (safe Lynx backend)  (unsafe FFI +
                                              │               C++ bridge)
                                              ▼  (only with `hot-reload`)
                                        whisker-dev-runtime
                                        (WebSocket receiver,
                                         subsecond::apply_patch)

   whisker-protocol
   (Host-independent frame, measurement, and input model with strict batch
    validation and a transactional reference validator)

   whisker-engine ──────────► whisker-layout + whisker-style
          │                  (surface orchestration + dirty layout)
          └────────────────► whisker-protocol
   (Host-independent retained scene + incremental frame journal + batched
    measurement state machine + Rust-facing Host traits; wired through
    SurfaceRuntime and awaiting platform Hosts)

   whisker-layout ──────────► whisker-style + whisker-protocol
   (Host-independent retained Taffy tree + intrinsic-measurement boundary;
    paired with the retained scene by whisker-engine::SurfaceEngine)

   subsecond  (= whisker-subsecond, [lib] name = "subsecond")
     pulled into whisker / whisker-driver / whisker-dev-runtime
     when `hot-reload` is on.

   User crate (e.g. examples/podcast)
   ├── src/lib.rs   — `#[whisker::main] fn app() -> Element { render!{…} }`
   ├── whisker.rs   — `fn configure(&mut Config)` (app metadata)
   └── Cargo.toml   — depends on `whisker` (umbrella)
       Platform projects are GENERATED under gen/<platform>/ by CNG —
       not committed.

   Host tooling (never in the shipped app)
   whisker-cli      — the `whisker` / `cargo-whisker` binary:
                      run / doctor / new / new-module
   ├── probe.rs     — compile+run user's whisker.rs → Config
   ├── platforms.rs — drives whisker-cng (CNG sync) before a build
   └── run.rs       — Config → dev_server::Config (flat)
        │
        ▼
   whisker-dev-server  — the mobile dev loop: file-watch → platform build →
                         install/launch. Manifest-agnostic (flat Config).
        │
        ▼
   whisker-build       — per-platform builds and packaging. The mobile
                         bootstrap currently delegates to Gradle/Xcode; native
                         Rust artifacts return when the retained ABI is wired.

   whisker-cng         — Continuous Native Generation: renders
                         complete gen/<platform>/ projects from Config.

   platforms/macos     — native Rust macOS Host: window/event loop and the
                         MeasurementHost + FrameSink boundary. GPU paint,
                         native text, input, and accessibility fill this Host
                         out incrementally; Windows/Linux are peer OS Hosts.
   platforms/web       — Rust/WASM browser Host: DOM text measurement,
                         requestAnimationFrame scheduling, and semantic frame
                         application to explicitly positioned DOM nodes.
   whisker-plugin      — CNG plugin trait + JSON envelope + subprocess
                         runner for 3rd-party plugins.
```

## Crate responsibilities

| Crate | One-line | Depended on by |
|---|---|---|
| `whisker` | Umbrella. Users `use whisker::prelude::*`; almost everything is a re-export surfaced through one import root. `SurfaceRuntime` accepts `render!` mutations and drives retained rendering. `RuntimeInstance` owns the final Host-driven application lifecycle and enters an isolated runtime context for events and frames; it has no thread or event loop of its own. | user crates |
| `whisker-config` | `Config` metadata types users build in `whisker.rs`. Intentionally tiny. | `whisker`, `whisker-cli`, `whisker-cng` |
| `whisker-runtime` | Signals/effects/computed/owners, renderer-agnostic view operations, events, local async tasks, any-thread wake handles, and background-to-UI dispatch. `RuntimeContext` isolates these values per mounted instance while letting the Host drive short transactions on one UI thread. | `whisker`, `whisker-driver` |
| `whisker-style` | Renderer-independent typed inline-style model and stable common-property registry. It owns declaration composition, fixed inheritance for seven text properties, and computed text plus box/flex layout inputs without exposing Taffy types. | `whisker-css`, future UI modules, `whisker-layout`, and `whisker-engine` |
| `whisker-css` | Compatibility authoring facade for the existing `css!` API plus the temporary Lynx CSS serializer. It constructs and re-exports `whisker-style` identities rather than owning renderer semantics. | `whisker` |
| `whisker-driver-sys` | Raw `extern "C"` decls matching the C++ bridge (`bridge/…`), plus the bridge sources themselves. Unsafe-only. | `whisker-driver` |
| `whisker-driver` | Safe Rust wrappers over the bridge + the Lynx backend; exposes the host shims (`run`/`tick`) the iOS/Android shells call into. Bootstraps `subsecond` under `hot-reload`. | `whisker` |
| `whisker-dev-runtime` | App-side WebSocket receiver + log capture for hot patches. **Compiled only with `hot-reload`** — release builds drop it entirely. | `whisker-driver` (feature-gated) |
| `whisker-macros` | `#[whisker::main]`, `#[component]`, `#[module_component]`, and the `render!` DSL. | `whisker` |
| `whisker-cli` | The `whisker` / `cargo-whisker` binary: `run`, `doctor`, `new`, `new-module`. Resolves Config via the `whisker.rs` probe; hands a flat Config to dev-server. | (binary) |
| `whisker-dev-server` | Host dev loop, manifest-agnostic. Android/iOS currently use explicit full rebuild → install → relaunch; the retained mobile ABI will re-enable Rust hot reload. | `whisker-cli` |
| `whisker-build` | Per-platform builds and packaging, including generated mobile shell builds and native macOS `.app` assembly. Legacy artifact helpers remain until migration cleanup. | `whisker-cli`, `whisker-dev-server` |
| `whisker-cng` | Continuous Native Generation: pure, fingerprint-gated renderer of complete `gen/<platform>/` projects from Config. No CLI surface. | `whisker-cli` |
| `whisker-macos` | Native Rust macOS Host and direct runtime composition boundary. It currently owns lifecycle/frame scheduling with deterministic measurement and a recording sink while native text/GPU/input are implemented. | generated `gen/macos` app |
| `whisker-web` | Browser DOM Host. It drives `RuntimeInstance` from `requestAnimationFrame`, measures intrinsic text in the DOM, and applies layout/paint/text operations without making browser layout authoritative. | generated `gen/web` WASM app |
| `whisker-plugin` | CNG plugin surface: `Plugin` trait, IR types, JSON envelope, subprocess runner shared by the engine and 3rd-party plugin binaries. | `whisker-cng`, 3rd-party plugins |
| `whisker-protocol` | Host-independent semantic frame, intrinsic-measurement, and normalized input types; stable IDs; strict batch validation; and transactional retained-tree validation. Plain text and common box paint are retained semantic presentation, while pointer/provider events enter Rust through typed input values. The legacy production Lynx path does not consume this protocol. | scene engine and Host providers |
| `whisker-engine` | Host-independent retained scene, coalescing mutation journal, snapshot/delta production, frame acceptance/recovery, and retained measurement coordination. `SurfaceEngine` is the core surface state machine, not a Lynx migration adapter: it pairs Scene and Taffy, batches Host measurements, lowers computed text/box paint and overflow clips, presents directly through `FrameSink`, and applies acknowledgements. Mobile cross-language bindings and providers are not implemented. | scene runtime and renderer providers |
| `whisker-layout` | Host-independent retained box layout. It privately owns Taffy, accepts `ComputedLayoutStyle` and stable `NodeId`s, calls an abstract intrinsic measurer using protocol-owned constraints, and returns deterministic logical-pixel snapshots. `whisker-engine::SurfaceEngine` owns its coordination with scene/frame production. | `whisker-engine`, future scene runtime |
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
2. **View / renderer** (`whisker-runtime/src/view`) — `Element` is a small,
   `Copy`, runtime-local handle. The installed renderer maps it either to a
   retained `NodeId` or, on the migration path, a Lynx element. `render!`
   creates the tree and dynamic props use effects to emit typed mutations.
3. **Retained surface** (`whisker::SurfaceRuntime` → `whisker-engine`) — maps
   authoring operations into scene/layout state, routes input in Rust, batches
   Host measurement, and presents transactional frame packets.

The current product path also retains a separate **legacy Driver / bridge**
(`whisker-driver` + `whisker-driver-sys`) for Lynx. It is not an architectural
layer in the new retained path and will be removed after the platform Hosts
replace it.

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

The `hot-reload` feature is **off by default**. Release builds get a compact
binary with no subsecond, no WebSocket, and no tokio. The subsecond pipeline
still exists for the legacy Rust/mobile composition, but the generated
Android/iOS bootstrap shells deliberately do not enable it: there is no Rust
mobile library to patch until the retained renderer ABI is connected.

```
$ whisker run <legacy-mobile-composition>
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

`whisker run <platform>` is the developer's primary command. The CLI is
a thin wrapper: it probes `whisker.rs` into a `Config`, runs CNG to
materialise `gen/<platform>/`, then starts the target's development loop. The
mobile paths hand a flat `Config` to `whisker-dev-server`; the macOS path
builds the same generated Cargo project used by `whisker build macos`, launches
its `.app`, and automatically rebuilds/relaunches on source changes:

The Web path emits a Cargo/Trunk project at `gen/web`. `whisker run web`
starts Trunk, opens the browser, and uses Trunk's page reload as the initial
remount-style hot reload implementation. Android and iOS generate plain AGP
and Xcode/UIKit projects respectively, build them, install them on an emulator
or Simulator, and launch them without downloading or embedding Lynx.

```
  edit src/lib.rs
        │
        ▼
  watcher (notify)  →  ChangeKind::{RustCode | CargoToml | Other}
        │
        ▼
  platform loop
   ├── Web: Trunk rebuild → browser remount
   ├── macOS: Cargo rebuild → `.app` relaunch
   └── Android/iOS bootstrap: save prompts for explicit Full Reload (`R`)
       → Gradle/Xcode build → install/launch via adb/simctl
```

After the mobile retained ABI is implemented, its Rust composition library can
restore subsecond patch delivery without coupling scheduling to a Lynx thread.
Until then, mobile source changes require the explicit full-reload path and the
bootstrap screen does not execute user `render!` output.

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
  treats `whisker.rs`'s `Config` as the source of truth and renders complete
  `gen/<platform>/` projects on demand, fingerprint-gated so the fast path is
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
