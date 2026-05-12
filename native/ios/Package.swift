// swift-tools-version:5.9
import PackageDescription

// Phase 3a: introduces LyraBridge, a C++ / Obj-C++ target compiled by SPM
// directly from native/bridge/. For now it's a smoke test (one NSLog
// function). Phase 3b will start dispatching tasks onto Lynx's TASM
// thread; Phase 3c will drive Element PAPI.
//
// Build pre-reqs (run before opening Xcode):
//   scripts/build-lynx-xcframeworks.sh
//   scripts/build-ios-xcframework.sh

let package = Package(
    name: "LyraRuntime",
    platforms: [
        .iOS(.v13),
    ],
    products: [
        .library(name: "LyraRuntime", targets: ["LyraRuntime"]),
    ],
    targets: [
        // Rust runtime (C ABI), as xcframework.
        .binaryTarget(
            name: "LyraMobile",
            path: "../../target/lyra-mobile/LyraMobile.xcframework"
        ),

        // Lynx engine + dependencies, as xcframeworks built from upstream
        // CocoaPods source via scripts/build-lynx-xcframeworks.sh.
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

        // C++ / Obj-C++ glue between Swift, Rust, and the Lynx C++ API.
        // Compiled from source by SPM. The `bridge` directory is a symlink
        // to `native/bridge/` (SPM forbids paths outside the package root).
        .target(
            name: "LyraBridge",
            dependencies: ["Lynx", "LynxBase", "LynxServiceAPI", "PrimJS"],
            path: "bridge",
            sources: ["src"],
            publicHeadersPath: "include",
            cxxSettings: [
                // Lynx xcframeworks are built with NDEBUG=1 (Release mode).
                // `RefCountedThreadSafeBase` adds two extra `bool` members
                // in debug builds (adoption_required_, destruction_started_),
                // which would shift every offset in subclasses like
                // `Element` — so the bridge must match. Without this, our
                // EventTarget* points into the wrong half of the object
                // and AddEventListener crashes on a bogus vtable.
                .define("NDEBUG"),
                // Lynx C++ headers reference each other via tree-relative
                // paths (e.g. `#include "core/shell/lynx_engine.h"`). The
                // headers are staged under target/lynx-ios/sources/ by
                // scripts/build-lynx-xcframeworks.sh, and `lynx-sources`
                // (under native/ios/) is a symlink to that directory so
                // SPM accepts it as inside the package root.
                .headerSearchPath("../lynx-sources/Lynx"),
                .headerSearchPath("../lynx-sources/LynxBase"),
                .headerSearchPath("../lynx-sources/LynxServiceAPI"),
                // PrimJS uses several non-overlapping search roots
                // (mirrors the Lynx pod's own xcconfig).
                .headerSearchPath("../lynx-sources/PrimJS/src"),
                .headerSearchPath("../lynx-sources/PrimJS/src/interpreter"),
                .headerSearchPath("../lynx-sources/PrimJS/src/interpreter/quickjs/include"),
                .headerSearchPath("../lynx-sources/PrimJS/src/gc"),
                .headerSearchPath("../lynx-sources/PrimJS/src/napi"),
                .headerSearchPath("../lynx-sources/PrimJS/src/napi/env"),
                .headerSearchPath("../lynx-sources/PrimJS/src/napi/quickjs"),
                .headerSearchPath("../lynx-sources/PrimJS/src/napi/jsc"),
            ]
        ),

        .target(
            name: "LyraRuntime",
            dependencies: [
                "LyraMobile",
                "LyraBridge",
                "Lynx",
                "LynxBase",
                "LynxServiceAPI",
                "PrimJS",
            ],
            path: "Sources/LyraRuntime",
            linkerSettings: [
                .linkedFramework("JavaScriptCore"),
                .linkedFramework("NaturalLanguage"),
                .linkedLibrary("c++"),
                // Lynx ships many Obj-C categories whose methods are
                // stripped from a static framework unless the linker is
                // told to load every Obj-C class.
                .unsafeFlags(["-ObjC"]),
            ]
        ),
    ],
    // Lynx itself is compiled as gnu++17. Match it so the staged headers
    // (e.g. `std::optional`, `std::is_invocable_r_v`) compile in our bridge.
    cxxLanguageStandard: .gnucxx17
)
