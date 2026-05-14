# Whisker

Cross-platform mobile UI framework for Rust, built on the [Lynx](https://github.com/lynx-family/lynx) C++ engine.

Website: [whisker.rs](https://whisker.rs) · Source: [github.com/whiskerrs/whisker](https://github.com/whiskerrs/whisker)

> **Status**: Pre-alpha. Active development on the initial scaffold. Not usable yet.

Whisker lets you build native iOS and Android apps in Rust with a Dioxus-style declarative API. Under the hood, the [Lynx](https://github.com/lynx-family/lynx) engine drives platform-native widgets — no self-rendering, no JavaScript runtime.

## Why Whisker

| | Whisker | Flutter | React Native |
|---|---|---|---|
| Language | Rust | Dart | TypeScript / JavaScript |
| Rendering | Native widgets (via Lynx) | Self-rendered (Skia/Impeller) | Native widgets |
| Runtime dependency | None | Dart VM | JS engine (Hermes / JSC) |
| Hot reload | Yes (rsx delta + dylib swap) | Yes (Dart VM) | Yes (Metro / Fast Refresh) |

## Project layout

```
whisker/
├── crates/                    Rust workspace
│   ├── whisker                  Umbrella crate (re-exports for users)
│   ├── whisker-app-config       AppConfig types used in whisker.rs
│   ├── whisker-cli              `whisker` / `cargo-whisker` CLI binary
│   ├── whisker-codegen          CNG (Continuous Native Generation) codegen
│   ├── whisker-dev-runtime      Dev-only runtime (WebSocket, hot reload)
│   ├── whisker-driver           Backend driver (host shim, BridgeRenderer)
│   ├── whisker-driver-sys       Raw FFI bindings + C++ bridge sources (bridge/)
│   ├── whisker-macros           Proc macros (#[whisker::main], rsx!)
│   ├── whisker-plugin           Plugin trait + PrebuildContext + typed mod APIs
│   └── whisker-runtime          Core runtime (reactive, element tree)
├── native/
│   ├── android/               Kotlin runtime (WhiskerApplication / WhiskerView etc.)
│   └── ios/                   Swift runtime (WhiskerAppDelegate / WhiskerView etc.)
├── examples/                  Sample apps
├── docs/                      Documentation
└── xtask/                     Build automation (cargo xtask pattern)
```

## Design decisions

Major decisions made so far:

- **Layered on Lynx Android/iOS SDK** for Phase 1' (Surface, vsync, lifecycle, touch, accessibility, native widgets all reused).
- **Element PAPI direct access** via custom JNI/Obj-C++ bridge — bypasses Lynx's template/JS layer.
- **No JavaScript dependency** — possible because we drive the C++ engine directly. Initial builds may include unused PrimJS bytes; full removal is a planned follow-up via a light Lynx fork.
- **Custom widgets in native languages** (Kotlin/Swift) bridged via uniffi.
- **Code-based CNG** — `whisker.rs` (Rust code) defines app config; plugins are Rust crates with a `pub fn whisker_plugin(ctx)` function.
- **Hybrid CLI** — `whisker` (primary) and `cargo whisker` (alias).
- **Hot reload** — Tier 1 (rsx delta, sub-second) + Tier 2 (dylib swap, 5–30s).

See `docs/` for design notes — currently
[`docs/hot-reload-plan.md`](docs/hot-reload-plan.md) for the
in-progress Tier 1 (subsecond) hot-reload pipeline.

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
| Workspace scaffold | ✅ |
| Lynx prebuilt integration | ⏳ |
| Element PAPI JNI bridge | ⏳ |
| Reactive runtime | ⏳ |
| `rsx!` macro | ⏳ |
| CNG (`whisker prebuild`) | ⏳ |
| `whisker dev` (hot reload) | ⏳ |
| iOS xcframework build | ⏳ |
| Android AAR build | ⏳ |

## License

Dual-licensed under MIT or Apache-2.0 at your option. See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).
