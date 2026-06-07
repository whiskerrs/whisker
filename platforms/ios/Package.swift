// swift-tools-version:5.9
import PackageDescription

// WhiskerRuntime is the SPM package the iOS host app depends on. It
// composes:
//
//   Lynx*.xcframework       — Lynx engine + PrimJS, dynamic frameworks
//                             fetched at SPM resolve time from the
//                             monorepo-local `target/lynx-ios/` cache
//                             (`whisker-build` ensures it exists).
//   WhiskerCBridge          — header-only systemLibrary exposing the
//                             Whisker C ABI declarations. The actual
//                             implementation lives in
//                             `WhiskerDriver.framework`, which is built
//                             per-app by an Xcode Run Script Build
//                             Phase (Step 7) — see below.
//   WhiskerRuntime (Swift)  — thin Swift API: WhiskerView,
//                             WhiskerAppDelegate, CADisplayLink-driven
//                             render loop.
//
// Step-7 change: `WhiskerDriver` is NOT declared here as a `binaryTarget`.
// The Rust crate it wraps contains user `#[whisker::main]` code, so it
// can't be pre-built and shipped — it has to be compiled per-app. Pre-
// Step-7 the monorepo flow staged it under `target/whisker-driver/` so
// SPM could resolve a path-based binaryTarget, but that forced every
// build to go through the `whisker-build` CLI before Xcode opened. The
// Run Script Build Phase that whisker-cng injects into the per-app
// pbxproj now produces `WhiskerDriver.framework` inside
// `$(BUILT_PRODUCTS_DIR)/Frameworks/` during the build itself; the
// project's `OTHER_LDFLAGS` adds `-framework WhiskerDriver` so Xcode's
// link step picks it up, and `LD_RUNPATH_SEARCH_PATHS` includes
// `@executable_path/Frameworks` so dyld resolves it at app launch.
//
// The C-ABI surface Swift code calls into (`whisker_bridge_*`,
// `WhiskerValueRaw`, …) is declared by `WhiskerCBridge`'s
// module.modulemap. WhiskerRuntime's Swift sources do
// `@_exported import WhiskerCBridge` — at link time the consumer's app
// resolves the undefined refs against `WhiskerDriver.framework`.
//
// The bridge is intentionally NOT an SPM target. We used to have a
// `WhiskerBridge` C++ target here that compiled bridge sources via SPM;
// building an iOS dylib + an Android cdylib both require the same
// bridge sources, so keeping the build in `crates/whisker-driver-sys/
// build.rs` (where it already lived for Android) means a single source
// of truth. The bridge now lives under `crates/whisker-driver-sys/bridge/`.

let package = Package(
    name: "WhiskerRuntime",
    platforms: [
        .iOS(.v13),
    ],
    products: [
        // Phase J — the minimal surface a third-party Whisker module
        // depends on. Re-exports `WhiskerValue`, `WhiskerLynxAliases`
        // (WhiskerUI / WhiskerContext / WhiskerCustomEvent), and
        // `@_exported imports Lynx` so subclasses of `WhiskerUI<View>`
        // resolve. Module Package.swift files should depend on this
        // product, NOT on `WhiskerRuntime` (that's the *host* surface
        // including WhiskerView / WhiskerViewController / AppDelegate).
        .library(name: "WhiskerModule", targets: ["WhiskerModule"]),
        .library(name: "WhiskerRuntime", targets: ["WhiskerRuntime"]),
        // Phase 7-Φ.G: each module package is now its own SwiftPM
        // library and needs to `import Lynx` (etc.) directly to
        // subclass `LynxUI<UIView>`. Expose the binary frameworks
        // as products so module Package.swifts can pull them via
        // `.product(name: "Lynx", package: "WhiskerRuntime")`.
        .library(name: "Lynx", targets: ["Lynx"]),
        .library(name: "LynxBase", targets: ["LynxBase"]),
        .library(name: "LynxServiceAPI", targets: ["LynxServiceAPI"]),
        .library(name: "PrimJS", targets: ["PrimJS"]),
    ],
    targets: [
        // Lynx engine + dependencies, as xcframeworks built from upstream
        // CocoaPods source via `cargo xtask ios build-lynx-frameworks`.
        .binaryTarget(
            name: "Lynx",
            path: "../../target/lynx-ios/Lynx.xcframework"
        ),
        .binaryTarget(
            name: "LynxBase",
            path: "../../target/lynx-ios/LynxBase.xcframework"
        ),
        .binaryTarget(
            name: "LynxServiceAPI",
            path: "../../target/lynx-ios/LynxServiceAPI.xcframework"
        ),
        .binaryTarget(
            name: "PrimJS",
            path: "../../target/lynx-ios/PrimJS.xcframework"
        ),

        // Phase J — minimal module-author surface. Carved out of the
        // larger `WhiskerRuntime` target so a third-party Whisker
        // module's `Package.swift` only pulls in the types it actually
        // uses (`WhiskerValue`, `WhiskerUI`, `WhiskerContext`,
        // `WhiskerCustomEvent`) without dragging in the host-side
        // `WhiskerView` / `WhiskerViewController` / `WhiskerAppDelegate`
        // or the WhiskerDriver C ABI surface.
        //
        // `WhiskerLynxAliases.swift` does `@_exported import Lynx`,
        // so a consumer's `import WhiskerModule` transitively pulls
        // the Lynx symbols needed to subclass `LynxUI<View>`.
        //
        // Header-only mirror of `WhiskerDriver`'s public C ABI. The
        // Swift sources `@_exported import WhiskerCBridge` so the
        // call-site signatures are visible at compile time; the
        // implementing symbols come from `WhiskerDriver.framework`
        // (built per-app by an Xcode Run Script Build Phase — see
        // file header) and resolve at the host app's link step.
        // `WhiskerCBridge`'s `module.modulemap` carries the same C
        // declarations the framework's `Headers/` directory would
        // expose, so the symbol namespace overlaps cleanly.
        .systemLibrary(
            name: "WhiskerCBridge",
            path: "Sources/WhiskerCBridge/include"
        ),

        .target(
            name: "WhiskerModule",
            dependencies: ["Lynx", "WhiskerCBridge"],
            path: "Sources/WhiskerModule"
        ),

        .target(
            name: "WhiskerRuntime",
            dependencies: [
                "WhiskerModule",
                "WhiskerCBridge",
                "Lynx",
                "LynxBase",
                "LynxServiceAPI",
                "PrimJS",
            ],
            path: "Sources/WhiskerRuntime",
            linkerSettings: [
                // System frameworks Lynx depends on transitively.
                // WhiskerDriver.framework's dylib already declares
                // LC_LOAD_DYLIB for these (see
                // `whisker-driver-sys/build.rs`), so dyld would load
                // them anyway, but keeping the declaration here lets
                // the host app's static-analysis tooling see the
                // dependency.
                .linkedFramework("JavaScriptCore"),
                .linkedFramework("NaturalLanguage"),
                .linkedLibrary("c++"),
            ]
        ),
    ]
)
