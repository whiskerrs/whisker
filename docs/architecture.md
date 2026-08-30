# Whisker — Architecture Overview

How the workspace is sliced into crates, what each crate is for, and how
the **`whisker run` dev loop** wires them together.

Whisker is a cross-platform UI framework with a Rust-owned retained scene,
layout, and scheduling model.
App code remains plain Rust — a `#[whisker::main]` entry point and
`render! { … }` views over fine-grained reactive signals. CNG-generated
Android and iOS launch shells consume the retained frame protocol
through a narrow FFI Driver; Desktop and Web compose the same runtime directly.

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
                                   │    (typed css! authoring facade)
                                   │             │
                                   │             ▼
                                   │         whisker-style
                                   │         (typed inline-style model,
                                   │          stable property registry)
                                   │
                                   └─ Android/iOS only ─► whisker-driver
                                                        └─► whisker-driver-sys
                                         (safe FFI adapter)    (raw borrowed
                                                                 mobile ABI)

   whisker-protocol
   (Host-independent frame, measurement, and input model with strict batch
    validation and a transactional reference validator)

   whisker-engine ──────────► whisker-layout + whisker-style
          │                  (surface orchestration + dirty layout)
          └────────────────► whisker-protocol
   (Host-independent retained scene + incremental frame journal + batched
    measurement state machine + Rust-facing Host traits; wired through
    SurfaceRuntime into every platform Host)

   whisker-layout ──────────► whisker-style + whisker-protocol
   (Host-independent retained Taffy tree + intrinsic-measurement boundary;
    paired with the retained scene by whisker-engine::SurfaceEngine)

   subsecond  (= whisker-subsecond, [lib] name = "subsecond")
     pulled into whisker when `hot-reload` is on.
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
                         bootstrap delegates to Gradle/Xcode and links the
                         Rust runtime through the mobile ABI.

   whisker-cng         — Continuous Native Generation: renders
                         complete gen/<platform>/ projects from Config. Mobile
                         application targets contain only their composition
                         root; Host implementation comes from the platform SDK.

   platforms/android   — Gradle SDK libraries: the module API plus the Android
                         WhiskerView, measurement, retained scene, and paint.
   platforms/ios       — SwiftPM SDK libraries with the same split between the
                         module API and UIKit WhiskerRuntime.

   platforms/desktop   — shared native Desktop Host services: cosmic-text
                         measurement/prepared glyphs, retained FrameSink
                         projection, common winit frame/event shell, and wgpu
                         paint.
                         Scene, batching, shaders, and GPU resources are common
                         to macOS, Windows, and Linux.
   platforms/macos     — macOS-named generated app interface and seam for
                         genuine native macOS integration.
   platforms/windows   — symmetric Windows target interface.
   platforms/linux     — symmetric Linux target interface.
   platforms/web       — Rust/WASM browser Host: DOM text measurement,
                         requestAnimationFrame scheduling, and semantic frame
                         application to explicitly positioned DOM nodes.
   whisker-plugin      — CNG plugin trait + JSON envelope + subprocess
                         runner for 3rd-party plugins.
```

## Crate responsibilities

| Crate | One-line | Depended on by |
|---|---|---|
| `whisker` | Authoring umbrella. Users `use whisker::prelude::*`; macros, styles, element refs, back/focus helpers, and core types are surfaced through one import root. Platform-independent runtime ownership lives below it in `whisker-runtime`. | user crates |
| `whisker-config` | `Config` metadata types users build in `whisker.rs`. Intentionally tiny. | `whisker`, `whisker-cli`, `whisker-cng` |
| `whisker-runtime` | Complete Host-independent runtime: signals/effects, renderer-agnostic view operations, element registry, `SurfaceRuntime`, `RuntimeInstance`, module dispatch, events, tasks, wake handles, and background-to-UI dispatch. `RuntimeContext` isolates each mounted instance while the Host drives short transactions on one UI thread. | `whisker`, all Hosts, `whisker-driver` |
| `whisker-style` | Renderer-independent typed inline-style model and stable common-property registry. It owns declaration composition, fixed inheritance for seven text properties, and computed text plus box/flex layout inputs without exposing Taffy types. | `whisker-css`, future UI modules, `whisker-layout`, and `whisker-engine` |
| `whisker-css` | Typed authoring facade for the existing `css!` API. It constructs and re-exports `whisker-style` identities rather than owning renderer semantics or parsing raw style strings. | `whisker` |
| `whisker-driver-sys` | Single Rust source of truth for the raw, borrowed Android/iOS ABI: version/tag constants, C-layout frame/measurement/resource/module values, callbacks, exported entry points, and Android's JNI entry shim. Checked-in C, Swift-imported, and Kotlin representations are generated and drift-checked from it. It contains no renderer or runtime ownership. Unsafe-only. | `whisker-driver` |
| `whisker-driver` | Safe Android/iOS FFI adapter. It owns the opaque runtime handle, borrowed-value conversion, native callback adapters, and delegates lifecycle/frame/input work to `whisker-runtime`. It does not redefine the wire ABI. | `whisker` on Android/iOS only |
| `whisker-dev-runtime` | Development WebSocket/log support used by tooling paths. It is not a runtime or Host abstraction. | development tooling |
| `whisker-macros` | `#[whisker::main]`, `#[component]`, `#[module_component]`, and the `render!` DSL. | `whisker` |
| `whisker-cli` | The `whisker` / `cargo-whisker` binary: `run`, `doctor`, `new`, `new-module`. Resolves Config via the `whisker.rs` probe; hands a flat Config to dev-server. | (binary) |
| `whisker-dev-server` | Host dev loop, manifest-agnostic. Android/iOS currently use explicit full rebuild → install → relaunch; the retained mobile ABI will re-enable Rust hot reload. | `whisker-cli` |
| `whisker-build` | Per-platform builds and packaging, including generated mobile shell builds and native macOS `.app` assembly. | `whisker-cli`, `whisker-dev-server` |
| `whisker-cng` | Continuous Native Generation: pure, fingerprint-gated renderer of complete `gen/<platform>/` projects from Config. No CLI surface. | `whisker-cli` |
| `whisker-runtime-android` | Android Host SDK library. It owns `WhiskerView`, frame scheduling, intrinsic measurement, retained View projection, module dispatch, and paint. Generated apps only compose it from `MainActivity`. | generated `gen/android` app |
| `WhiskerRuntime` | iOS Host SwiftPM library with the symmetric UIKit ownership. Generated apps receive it transitively through `WhiskerModules` and only compose it from `AppDelegate`. | generated `gen/ios` app |
| `whisker-desktop` | Common native Rust Desktop Host services and direct runtime composition seam. It owns cosmic-text intrinsic measurement with reusable prepared content, the transactionally retained Host projection, common frame driving, the shared winit lifecycle/event translation, and wgpu scene lowering, batching, shaders, and painting. Host conformance scenarios can drive measurement and frame presentation without `RuntimeInstance`, record normalized input at a mock Rust sink, and run offscreen GPU checkpoints. | macOS/Windows/Linux target crates |
| `whisker-macos` | Thin macOS target crate preserving the OS-named interface consumed by generated projects and providing the seam for future native-only integration. Common winit behavior remains in `whisker-desktop`. | generated `gen/macos` app |
| `whisker-windows` | Symmetric Windows target crate over `whisker-desktop`; CNG/build/run integration follows separately. | future generated `gen/windows` app |
| `whisker-linux` | Symmetric Linux target crate over `whisker-desktop`; CNG/build/run integration follows separately. | future generated `gen/linux` app |
| `whisker-web` | Browser DOM Host. It drives `RuntimeInstance` from `requestAnimationFrame`, supplies current browser viewport/scale metrics, measures intrinsic text in the DOM, and applies layout/paint/text operations without making browser layout authoritative. | generated `gen/web` WASM app |
| `whisker-plugin` | CNG plugin surface: `Plugin` trait, IR types, JSON envelope, subprocess runner shared by the engine and 3rd-party plugin binaries. | `whisker-cng`, 3rd-party plugins |
| `whisker-protocol` | Host-independent semantic frame, intrinsic-measurement, and normalized input types; stable IDs; strict batch validation; and transactional retained-tree validation. Plain text and common box paint are retained semantic presentation, while pointer/provider events enter Rust through typed input values. | scene engine and Host providers |
| `whisker-engine` | Host-independent retained scene, coalescing mutation journal, snapshot/delta production, frame acceptance/recovery, and retained measurement coordination. `SurfaceEngine` is the core surface state machine: it pairs Scene and Taffy, batches Host measurements, lowers computed text/box paint and overflow clips, presents directly through `FrameSink`, and applies acknowledgements. The mobile packed ABI and Android/iOS providers cover bootstrap, measurement, frames, module values, and the typed resource lifecycle; later protocol groups extend that ABI additively. | scene runtime and renderer providers |
| `whisker-layout` | Host-independent retained box layout. It privately owns Taffy and a protocol-invisible, viewport-sized `SurfaceRoot`, accepts `ComputedLayoutStyle` and stable `NodeId`s, calls an abstract intrinsic measurer using protocol-owned constraints, and returns deterministic logical-pixel border/content geometry. The application root is the surface root's flex child, so viewport stretch, growth, percentages, and absolute positioning use normal layout semantics without Host style overrides. `whisker-engine::SurfaceEngine` owns its coordination with scene/frame production. | `whisker-engine`, future scene runtime |
| `whisker-subsecond` | Whisker's fork of DioxusLabs `subsecond` — anchors the ASLR-slide lookup on `whisker_aslr_anchor` (emitted by `#[whisker::main]`) instead of `main`. `[lib] name = "subsecond"` keeps `use subsecond::*`. | `whisker`, development tooling |

### Modules and the router (`packages/*`)

First-party, app-facing add-on crates that depend on `whisker` like any
user crate would. They are *not* part of the framework core:

- **`whisker-router`** (+ `whisker-router-macros`) — type-safe,
  signal-backed routing over the shared Rust runtime, custom transitions, nested
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

## Host conformance boundary

Every Host is testable without starting the Rust runtime. Shared scenarios
under `tests/host-conformance` stand in for Rust by supplying intrinsic
measurement requests, frame packets, viewport changes, clock advances, and
input fixtures. A recording event sink captures the Host-to-Rust direction.
Only those boundary peers are mocked: measurement, retained projection, and
painting use the same code as the shipped Host. The initial input scenario
establishes the recording-sink contract; native event conversion joins that
path as each OS adapter implements input.

Each backend owns a runner beside its implementation. Desktop uses direct Rust
calls and a real or offscreen `wgpu` surface, Web runs against a real browser,
Android uses instrumentation tests, and iOS uses XCTest. Selected WPT cases
are converted into attributed, revision-pinned shared scenarios. The same case
identifier is checked by Rust semantic lowering, every required Host runner,
and a smaller full-stack suite, so neither side can define conformance by
recording the other side's current output.

## The runtime layers

Three layers, each renderer-agnostic until the bottom:

1. **Reactive runtime** (`whisker-runtime/src/reactive`) — fine-grained
   signals, effects, computed, owners/scopes, batching scheduler. No
   virtual DOM and **no diff pass**. See
   [`reactivity-design.md`](reactivity-design.md).
2. **View / renderer** (`whisker-runtime/src/view`) — `Element` is a small,
   `Copy`, runtime-local handle. The installed renderer maps it to a retained
   `NodeId`. `render!`
   creates the tree and dynamic props use effects to emit typed mutations.
3. **Retained surface** (`whisker_runtime::SurfaceRuntime` → `whisker-engine`) — maps
   authoring operations into scene/layout state, routes input in Rust, batches
   Host measurement, and presents transactional frame packets.

The Host boundary branches only at composition. Android/iOS instantiate the
runtime through `whisker-driver` because Swift/Kotlin require an FFI handle.
Desktop/Web instantiate `RuntimeInstance` directly and supply ordinary Rust
`MeasurementProvider` and `FrameSink` implementations. Core contains no
platform `cfg` selecting one model over the other.

## `hot-reload` feature flow

The `hot-reload` feature is **off by default**. Release builds get a compact
binary with no subsecond. Hot dispatch belongs to the authoring umbrella and
the user crate; it is not part of the FFI Driver.

```
$ whisker run <platform>
            │
            ▼  (cli adds `--features whisker/hot-reload`)
whisker = { features = ["hot-reload"] }
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
or Simulator, and launch them against the Whisker Host SDK.

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

Mobile source changes currently use the explicit full-reload path. The retained
ABI already executes user `render!` output; finer-grained patch delivery can be
added without changing the Host/runtime boundary.

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
  from seconds to minutes (`whisker-runtime`, renderer dependencies, …).

- **Native projects are generated, not committed.** CNG (Expo-style)
  treats `whisker.rs`'s `Config` as the source of truth and renders complete
  `gen/<platform>/` projects on demand, fingerprint-gated so the fast path is
  a single file read. Regeneration is implicit — the command that needs
  the native tree syncs it first.

- **`whisker-driver-sys` is unsafe-only.** The complete raw mobile ABI and the
  Android link anchor live there; `cargo xtask mobile-abi generate` materializes
  checked-in Host declarations, while CI's `mobile-abi check` rejects drift.
  Application builds consume those checked-in declarations and do not depend
  on xtask or CNG. Ownership and protocol conversion remain confined to safe
  wrappers in `whisker-driver`. The standard `*-sys` crate pattern.

- **`whisker-dev-runtime` is feature-gated end-to-end.** Without
  `hot-reload`, the crate compiles to nothing — no tokio, no
  tungstenite, no subsecond.

- **subsecond is in-tree, not a published-crate dep.** The fork swaps
  the ASLR anchor from `main` to `whisker_aslr_anchor`. On Android,
  multiple `main` symbols can share the linker namespace
  (`app_process64`'s, prior memfd patches'); a `dlsym` for the upstream
  sentinel returns garbage and the dispatch math fails. See
  `crates/whisker-subsecond/src/lib.rs`.
