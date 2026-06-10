# Whisker

Cross-platform mobile UI framework for Rust, built on the [Lynx](https://github.com/lynx-family/lynx) C++ engine.

Website: [whisker.rs](https://whisker.rs) · Source: [github.com/whiskerrs/whisker](https://github.com/whiskerrs/whisker)

> **Status**: Early release (`0.1.x`). The core runtime, `render!` macro,
> routing, hot reload, and the iOS/Android pipelines work end-to-end.
> APIs may still change before `1.0` — see [Status](#status) for the
> per-area breakdown and [known gaps](#known-gaps).

Whisker lets you build native iOS and Android apps in Rust with a Leptos-style **fine-grained reactive** API — components run once, signals + effects drive granular updates, no virtual DOM. Under the hood, the [Lynx](https://github.com/lynx-family/lynx) engine drives platform-native widgets — no self-rendering, no JavaScript runtime.

```rust
use whisker::prelude::*;

#[whisker::main]
fn app() -> Element {
    let count = RwSignal::new(0);
    let label = computed(move || format!("Count: {}", count.get()));
    render! {
        page(style: "padding: 20px; display: flex; flex-direction: column; gap: 12px;") {
            text(value: label)
            view(
                style: "padding: 8px 16px; background: #3b82f6; border-radius: 6px;",
                on_tap: move |_| count.update(|n| *n += 1),
            ) {
                text(style: "color: white;", value: "+1")
            }
        }
    }
}
```

## Getting started

Whisker apps are regular Rust crates driven by the `whisker` CLI.

```sh
# 1. Install the CLI. This is the only install needed — the `whisker`
#    binary drives both iOS and Android builds (Xcode / Gradle invoke it).
cargo install whisker-cli

# 2. Check your local toolchain (Rust targets, Android NDK/SDK/JDK, Xcode).
whisker doctor

# 3. Scaffold and run an app.
whisker new my-app
cd my-app
whisker run ios       # or: whisker run android
```

`whisker run` watches `src/lib.rs` and hot-patches the running app in
under a second — no restart, no state loss. App-level config (bundle id,
app name, deployment settings) lives in `whisker.rs`.

**Prerequisites**

- **Rust** 1.85+ with the relevant target(s):
  `rustup target add aarch64-apple-ios-sim aarch64-linux-android`
- **iOS**: Xcode + Command Line Tools (Simulator builds work out of the box).
- **Android**: Android SDK + NDK `21.1.6352462` + a JDK. `whisker doctor`
  reports exactly what's missing.

> **Note**: the iOS dev pipeline currently expects to run from a checkout
> of this repository (it resolves the bundled Swift runtime by path).
> Standalone iOS support from a `cargo install`-only setup is in progress
> — see [known gaps](#known-gaps). Android is distributed via Maven and
> works standalone.

The reactive contract at a glance:

| Pass                                         | Behaviour                                                                   |
|----------------------------------------------|-----------------------------------------------------------------------------|
| `text(value: "hi")`                          | static — set the attribute once                                             |
| `text(value: my_string)` (or `&str`)         | static — set once                                                           |
| `text(value: my_signal)` / `my_rw_signal`    | **dynamic** — the element re-updates when the signal changes                |
| `text(value: computed(move \|\| …))`         | **dynamic** — the `computed`'s memo re-runs, the element re-updates with it |
| `text(value: my_signal.get())`               | static — read happens at the call site, no subscription                     |

Same rule for built-in tags, `#[component]`s, and
`#[whisker::platform_component]`s. See
[`docs/reactivity-design.md`](docs/reactivity-design.md#signalt--the-prop-value-type-phase-7-φ)
for the full design.

## Why Whisker

| | Whisker | Flutter | React Native |
|---|---|---|---|
| Language | Rust | Dart | TypeScript / JavaScript |
| Rendering | Native widgets (via Lynx) | Self-rendered (Skia/Impeller) | Native widgets |
| Runtime dependency | None | Dart VM | JS engine (Hermes / JSC) |
| Reactivity model | Fine-grained signals + effects (Leptos-style) | StatefulWidget rebuilds | Component re-render + diff |
| Hot reload | Yes (subsecond function-body patch) | Yes (Dart VM) | Yes (Metro / Fast Refresh) |

## Project layout

```
whisker/
├── crates/                    Rust workspace
│   ├── whisker                  Umbrella crate (re-exports for users)
│   ├── whisker-config           Config types used in whisker.rs
│   ├── whisker-cli              `whisker` / `cargo-whisker` CLI binary
│   ├── whisker-build            Build-orchestration library (cargo cross-compile, used by whisker-cli)
│   ├── whisker-cng              CNG (Continuous Native Generation) codegen
│   ├── whisker-dev-server       Host-side dev server (file watch, hot patch)
│   ├── whisker-dev-runtime      Dev-only device runtime (WebSocket, hot reload)
│   ├── whisker-driver           Backend driver (host shim, BridgeRenderer)
│   ├── whisker-driver-sys       Raw FFI bindings + C++ bridge sources (bridge/)
│   ├── whisker-css              Typed CSS keyword / `css!` helpers
│   ├── whisker-macros           Proc macros (#[whisker::main], #[component], render!)
│   ├── whisker-plugin           Plugin trait + GenerateContext + typed mod APIs
│   ├── whisker-runtime          Core runtime (reactive arena, view layer)
│   └── whisker-subsecond        Whisker fork of Dioxus `subsecond` (hot patch)
├── packages/                  First-party modules (router, image, svg, icons,
│                                video, audio, safe-area, local-store)
├── platforms/
│   ├── android/               Kotlin runtime + Gradle plugins
│   └── ios/                   Swift runtime (WhiskerView / WhiskerModule etc.)
├── examples/                  Sample apps (see examples/podcast)
└── docs/                      Documentation
```

## Design decisions

Major decisions made so far:

- **Layered on Lynx Android/iOS SDK** for Phase 1' (Surface, vsync, lifecycle, touch, accessibility, native widgets all reused).
- **Element PAPI direct access** via custom JNI/Obj-C++ bridge — bypasses Lynx's template/JS layer.
- **No JavaScript dependency** — possible because we drive the C++ engine directly. Initial builds may include unused PrimJS bytes; full removal is a planned follow-up via a light Lynx fork.
- **Custom widgets in native languages** (Kotlin/Swift) bridged via uniffi.
- **Code-based CNG** — `whisker.rs` (Rust code) defines app config; plugins are Rust crates with a `pub fn whisker_plugin(ctx)` function.
- **Hybrid CLI** — `whisker` (primary) and `cargo whisker` (alias).
- **Hot reload** — Tier 1 (subsecond function-body patch, ~1s) + Tier 2 (dylib swap, 5–30s).
- **Leptos-style fine-grained reactivity** — components run once,
  `signal` + `effect` + `computed` form a dependency graph, the
  `render!` macro wires up per-property effects so a signal write
  updates only the affected element attribute. No virtual DOM, no
  diff pass.
- **Unified prop reactivity via `Signal<T>`** (Phase 7-Φ) —
  built-in tags, `#[component]`, and `#[whisker::platform_component]`
  all accept the same `Signal<T> = Static(T) | Dynamic(ReadSignal<T>)`
  prop shape, so call sites have one rule across every component
  surface (pass a signal handle → reactive; pass a value or
  `.get()` → static snapshot).

See `docs/` for design notes:

- [`docs/reactivity.md`](docs/reactivity.md) — user guide for
  signals, effects, components, control flow.
- [`docs/render-macro.md`](docs/render-macro.md) — `render!`
  syntax reference.
- [`docs/reactivity-design.md`](docs/reactivity-design.md) —
  internal architecture (arena, owner tree, batching, hot-reload).
- [`docs/hot-reload-plan.md`](docs/hot-reload-plan.md) — Tier 1
  subsecond pipeline.

## Development

### Coverage

Test coverage is measured with [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov)
on every push (see `.github/workflows/ci.yml`). Locally:

```sh
cargo install cargo-llvm-cov     # one-time
rustup component add llvm-tools-preview

cargo coverage         # HTML report opened in the browser
cargo coverage-text    # short terminal summary
cargo coverage-lcov    # writes lcov.info (for editors / CI)
# Doctest coverage needs nightly: `cargo +nightly llvm-cov --workspace --doctests`.
```

In CI the coverage table is appended to the workflow's **Summary**
tab and (on pull requests) posted as a sticky comment. The raw
`lcov.info` is uploaded as a workflow artifact, retained for 14 days.
No external coverage service is involved — everything stays on
GitHub.

## Status

| Component | Status |
|---|---|
| Lynx prebuilt integration | ✅ |
| Element PAPI Obj-C++/JNI bridge | ✅ |
| Reactive runtime (signals, effects, computed, resource) | ✅ |
| `render!` macro | ✅ |
| `Signal<T>` unified prop reactivity | ✅ |
| Routing (`whisker-router`: stack / tabs / modal, transitions) | ✅ |
| First-party modules (image, svg, icons, video, audio, local-store, safe-area) | ✅ |
| `#[whisker::platform_component]` (iOS) | ✅ |
| CNG (continuous native generation, plugin-driven) | ✅ |
| `whisker run` (Tier 1 hot reload) | ✅ iOS / ✅ Android |
| iOS xcframework build | ✅ |
| Android AAR build (Maven-distributed) | ✅ |

### Known gaps

Things to be aware of before adopting (tracked for upcoming releases):

- **No text input.** Built-in tags are `page` / `view` / `text` /
  `raw_text` / `scroll_view` / `list` / `fragment`. There is no
  `TextInput` / `TextField` equivalent yet, so forms and search boxes
  aren't buildable today.
- **iOS needs a repo checkout.** `whisker run ios` resolves the bundled
  Swift runtime by path; standalone `cargo install`-only iOS support is
  in progress. Android works standalone via Maven.
- **Deep linking is a stub** (`whisker-router` `linking` always returns
  no initial URL).
- **No release-build / code-signing wrapper.** Drive `xcodebuild` /
  `gradle` directly (the generated app's README shows how).

## License

Dual-licensed under MIT or Apache-2.0 at your option. See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).
