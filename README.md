# Lyra

Cross-platform mobile UI framework for Rust, built on the [Lynx](https://github.com/lynx-family/lynx) C++ engine.

> **Status**: Pre-alpha. Active development on the initial scaffold. Not usable yet.

Lyra lets you build native iOS and Android apps in Rust with a Dioxus-style declarative API. Under the hood, the [Lynx](https://github.com/lynx-family/lynx) engine drives platform-native widgets — no self-rendering, no JavaScript runtime.

## Why Lyra

| | Lyra | Flutter | React Native |
|---|---|---|---|
| Language | Rust | Dart | TypeScript / JavaScript |
| Rendering | Native widgets (via Lynx) | Self-rendered (Skia/Impeller) | Native widgets |
| Runtime dependency | None | Dart VM | JS engine (Hermes / JSC) |
| Hot reload | Yes (rsx delta + dylib swap) | Yes (Dart VM) | Yes (Metro / Fast Refresh) |

## Project layout

```
lyra/
├── crates/                    Rust workspace
│   ├── lyra                  Umbrella crate (re-exports for users)
│   ├── lyra-app-config       AppConfig types used in lyra.rs
│   ├── lyra-cli              `lyra` / `cargo-lyra` CLI binary
│   ├── lyra-codegen          CNG (Continuous Native Generation) codegen
│   ├── lyra-dev-runtime      Dev-only runtime (WebSocket, hot reload)
│   ├── lyra-ffi-lynx         FFI bindings to native/bridge
│   ├── lyra-macros           Proc macros (#[lyra::main], rsx!)
│   ├── lyra-plugin           Plugin trait + PrebuildContext + typed mod APIs
│   └── lyra-runtime          Core runtime (reactive, element tree)
├── native/
│   ├── bridge/                C++ bridge to Lynx Element PAPI (C ABI)
│   ├── android/               Kotlin runtime (LyraApplication / LyraView etc.)
│   └── ios/                   Swift runtime (LyraAppDelegate / LyraView etc.)
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
- **Code-based CNG** — `lyra.rs` (Rust code) defines app config; plugins are Rust crates with a `pub fn lyra_plugin(ctx)` function.
- **Hybrid CLI** — `lyra` (primary) and `cargo lyra` (alias).
- **Hot reload** — Tier 1 (rsx delta, sub-second) + Tier 2 (dylib swap, 5–30s).

See `docs/` (forthcoming) for the full design rationale.

## Status

| Component | Status |
|---|---|
| Workspace scaffold | ✅ |
| Lynx prebuilt integration | ⏳ |
| Element PAPI JNI bridge | ⏳ |
| Reactive runtime | ⏳ |
| `rsx!` macro | ⏳ |
| CNG (`lyra prebuild`) | ⏳ |
| `lyra dev` (hot reload) | ⏳ |
| iOS xcframework build | ⏳ |
| Android AAR build | ⏳ |

## License

Dual-licensed under MIT or Apache-2.0 at your option. See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).
