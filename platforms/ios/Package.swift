// swift-tools-version:5.9
import PackageDescription

// WhiskerRuntime is the SPM package the iOS host app depends on. It
// composes:
//
//   Lynx*.xcframework       — Lynx engine + PrimJS, dynamic frameworks
//                             resolved by SPM via remote
//                             `binaryTarget(url:checksum:)` against
//                             whiskerrs/lynx's GitHub Releases. SPM
//                             caches them under the Xcode-managed
//                             SourcePackages dir; no local `target/`
//                             pre-population is required for the
//                             binaries themselves. The PrimJS public
//                             headers are still staged out of the
//                             tarball cache for `whisker-driver-sys`'s
//                             cargo build.
//   WhiskerCBridge          — header-only systemLibrary exposing the
//                             Whisker C ABI declarations. The actual
//                             implementation lives in
//                             `WhiskerDriver.framework`, which is built
//                             per-app by an Xcode Run Script Build
//                             Phase — see below.
//   WhiskerRuntime (Swift)  — thin Swift API: WhiskerView,
//                             WhiskerAppDelegate, CADisplayLink-driven
//                             render loop.
//
// `WhiskerDriver` is deliberately NOT declared here as a `binaryTarget`.
// The Rust crate it wraps contains user `#[whisker::main]` code, so it
// can't be pre-built and shipped — it has to be compiled per-app. The
// Run Script Build Phase that whisker-cng injects into the per-app
// pbxproj produces `WhiskerDriver.framework` inside
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
// The bridge is intentionally NOT an SPM target. The iOS dylib and the
// Android cdylib need the same bridge sources, so the build stays in
// `crates/whisker-driver-sys/build.rs` as the single source of truth.
// The sources live under `crates/whisker-driver-sys/bridge/`.

let package = Package(
    name: "WhiskerRuntime",
    platforms: [
        .iOS(.v13),
    ],
    products: [
        // The minimal surface a third-party Whisker module
        // depends on. Re-exports `WhiskerValue`, `WhiskerLynxAliases`
        // (WhiskerUI / WhiskerContext / WhiskerCustomEvent), and
        // `@_exported imports Lynx` so subclasses of `WhiskerUI<View>`
        // resolve. Module Package.swift files should depend on this
        // product, NOT on `WhiskerRuntime` (that's the *host* surface
        // including WhiskerView / WhiskerViewController / AppDelegate).
        .library(name: "WhiskerModule", targets: ["WhiskerModule"]),
        .library(name: "WhiskerRuntime", targets: ["WhiskerRuntime"]),
        // Each module package is its own SwiftPM library and needs to
        // `import Lynx` (etc.) directly to subclass `LynxUI<UIView>`,
        // so the binary frameworks are exposed as products and pulled
        // via `.product(name: "Lynx", package: "WhiskerRuntime")`.
        .library(name: "Lynx", targets: ["Lynx"]),
        .library(name: "LynxBase", targets: ["LynxBase"]),
        .library(name: "LynxServiceAPI", targets: ["LynxServiceAPI"]),
        .library(name: "PrimJS", targets: ["PrimJS"]),
    ],
    targets: [
        // Lynx engine + dependencies, as xcframeworks built from the
        // whiskerrs/lynx fork and published per release alongside the
        // legacy tarball. Each archive's SwiftPM-format checksum lives
        // in the matching release's `swiftpm-manifest-<ver>.txt`
        // (https://github.com/whiskerrs/lynx/releases). Bumping the
        // pinned tag means refreshing both the URL `<ver>` segment AND
        // the corresponding `checksum:` here — keep them in lockstep.
        //
        // SPM resolves these during xcodebuild's package-resolution
        // step (before any Build Phase runs), caches the unpacked
        // xcframeworks under the user's per-Xcode-project SourcePackages
        // dir, and shares them across every WhiskerRuntime consumer.
        // PrimJS *headers* are separate: `whisker-driver-sys`'s
        // build.rs still stages them out of `target/lynx-headers`.
        .binaryTarget(
            name: "Lynx",
            url: "https://github.com/whiskerrs/lynx/releases/download/v4.0.1-whisker.1/Lynx-4.0.1-whisker.1.xcframework.zip",
            checksum: "e5161bf110a1d22869412689b89362798fd35e53dad9467980e9390918261c1d"
        ),
        .binaryTarget(
            name: "LynxBase",
            url: "https://github.com/whiskerrs/lynx/releases/download/v4.0.1-whisker.1/LynxBase-4.0.1-whisker.1.xcframework.zip",
            checksum: "875c389e2a33ad846ae43a29bcb1dad131d8b1e75ab36a2d41f39355e68cd25e"
        ),
        .binaryTarget(
            name: "LynxServiceAPI",
            url: "https://github.com/whiskerrs/lynx/releases/download/v4.0.1-whisker.1/LynxServiceAPI-4.0.1-whisker.1.xcframework.zip",
            checksum: "9347769f21d9c41e0274cdb555698d7489f67ab9ed26fe2146688061f55a9b24"
        ),
        .binaryTarget(
            name: "PrimJS",
            url: "https://github.com/whiskerrs/lynx/releases/download/v4.0.1-whisker.1/PrimJS-4.0.1-whisker.1.xcframework.zip",
            checksum: "76502d535310b42d7c15d9dcb8e802622d17f8541bcee5a8dcf8863b59d8f45b"
        ),

        // Minimal module-author surface, kept apart from the larger
        // `WhiskerRuntime` target so a third-party Whisker
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
