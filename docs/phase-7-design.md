# Phase 7 — native module & native element support

Design summary for the Phase 7 work. Captures the architectural
decisions that came out of the design conversation; each subtask
issue (#2, #3, #4, #6, #7) links back here.

## Goal

Let Whisker apps call platform-native (Kotlin / Swift) code and
render platform-native UI elements from Rust, with a fully
Whisker-branded developer surface — no `@Lynx*` annotations or
`Lynx*` types leaking into user code.

## Layer architecture

```
Rust user code
    ↑
#[whisker::native_module(…)] / #[whisker::native_element(…)] generated client / view
    ↑
whisker-native-runtime           ← safe Rust wrapper crate
    ↑
whisker-driver-sys C ABI         ← extern "C" bindings
    ↑
LynxNativeDirectModuleManager    ← 4th sibling of LynxJSIModuleManager
                                    (Whisker addition; no JS engine)
    ↑
LynxNativeModule::InvokeMethod / ElementManager::CreateFiberNode
                                                    (Lynx upstream)
    ↑
runtime-platforms/android        ← Kotlin @WhiskerModule → @LynxBehavior via KSP
runtime-platforms/ios            ← Swift @WhiskerModule → LynxUI subclass via Swift Macros
    ↑
User's Kotlin / Swift code, distributed via cargo
```

## Key design decisions

| Decision | Rationale |
|----------|---------|
| **Whisker-branded annotations everywhere** | `@WhiskerModule`, `@WhiskerElement`, `@WhiskerMethod`, `@WhiskerProp`, `@WhiskerEvent` on the platform side. KSP (Android) / Swift Macros (iOS) lower to Lynx's registration mechanisms underneath. Users never type `@Lynx*` anything. |
| **Distinct macro names** | `#[whisker::native_module]` and `#[whisker::native_element]` — the two surfaces are different enough (Stream subscriptions vs prop setters) that combining was awkward. |
| **`pub::Value` direct over C ABI** | The internal Lynx variant type travels as an opaque pointer between Rust and C++. No JSON. Bytes go through `byte_array` — no base64 wire bloat for images / audio. The ABI is wide (per-type builder fns) but the user wrapper hides it. |
| **`async fn → Result<T, E>` for one-shot results** | Standard async Rust. Dropping the future is a no-op (no cancellation semantics in v1). |
| **`fn → impl Stream<Item = Result<T, E>>` for subscriptions** | The Rust async-stream pattern. Drop = unsubscribe — the `Stream`'s `Drop` impl calls `lynx_unsubscribe_native_module`. Events carry `Result<T, _>` so transient errors (sensor permission dropped, etc.) flow through the stream without breaking the subscription. |
| **iOS package manager: SPM only** | CocoaPods support dropped. 2026 ecosystem survey: every major SDK (Firebase, Alamofire, Realm, Sentry, …) has shipped SPM support. RN / Flutter are also migrating to SPM. |
| **Module distribution: cargo, with native sources bundled** | A module crate is one `cargo publish` — its `Cargo.toml` `include` field bundles `android/` and `ios/` directories alongside Rust. No Maven / SPM publishing in v1. Optional binary-cache (`[android.prebuilt]`) is future work. |
| **`whisker.module.toml` manifest filename** | Dot-separated namespace lets future `whisker.app.toml` / `whisker.workspace.toml` slot in. See [`whisker-module-toml.md`](./whisker-module-toml.md) for the full schema. |
| **Native deps live in real `build.gradle.kts` / `Package.swift`** | Earlier design pass duplicated them in `whisker.module.toml`; that broke Android Studio / Xcode autocomplete. Now the IDE opens the module's native project directly and sees normal Gradle / SPM files. The manifest only points at paths. |
| **`runtime-platforms/{android,ios}/` workspace dirs** | Distinct from `crates/` (Rust-only). Host the Kotlin gradle project + Swift SPM package. Wired into user apps' `gen/android/` and `gen/ios/` by the autolink layer (Phase 7-D). |
| **Local modules (Expo-style)** | `modules/*/whisker.module.toml` as path deps in the user app's `Cargo.toml`. Same autolink pipeline as crates.io-resolved modules. No special "local module" code path. |
| **First-party xelement library** | Whisker ships its own `whisker-element-input` / `whisker-element-refresh` / `whisker-element-overlay` / `whisker-element-svg` / `whisker-element-webview`. Lynx's `lynx_xelement_*` packages are reference material only — not used at runtime. |
| **Version compatibility via cargo semver** | Each module declares `whisker = "0.x"` like a normal cargo dep. Cargo's resolver enforces compatibility. Native runtime version is auto-pinned to the user's Whisker version by `whisker-cli`. |

## Workspace layout

```
whisker/
├── crates/
│   ├── whisker-driver/
│   ├── whisker-driver-sys/         ← extended: pub::Value C ABI, call_module
│   ├── whisker-runtime/            ← extended: ElementTag::Custom + create_element_by_name
│   ├── whisker-native-runtime/     ← NEW: safe wrapper + proc-macro helpers (Phase 7-A.2)
│   ├── whisker-macros/             ← extended: #[whisker::native_module/_element]
│   ├── whisker-build/              ← extended: module discovery
│   ├── whisker-cng/                ← extended: gen/* materialisation
│   └── …
└── runtime-platforms/              ← NEW (Phase 7-A.3 / A.4)
    ├── android/                    ← gradle project: WhiskerView, KSP processor
    └── ios/                        ← SPM package: WhiskerView, Swift Macros
```

## Subtask map

| # | Subtask | Status |
|---|---------|-------|
| #7 | 7-A Foundation: schema lock + runtime-platforms skeleton + this doc | started; this PR |
| #2 | 7-B Native Element: C API tag-by-name + render! support + `@WhiskerElement` runtime + sample | layers 1-4 + iOS smoke test in this PR |
| #4 | 7-C Native Module: pub::Value C ABI + `LynxNativeDirectModuleManager` + `#[whisker::native_module]` | pending |
| #6 | 7-D Autolink: discovery (whisker-build) + materialisation (whisker-cng via includeBuild + local SPM) | pending |
| #3 | 7-E First-party xelement library | pending |
| #1 | Epic | tracking |

## See also

- [`whisker-module-toml.md`](./whisker-module-toml.md) — manifest schema reference
- [`hot-reload-plan.md`](./hot-reload-plan.md) — Tier 1 hot-reload pipeline (Phase 6.5)
- [`architecture.md`](./architecture.md) — workspace overview
