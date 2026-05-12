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
            publicHeadersPath: "include"
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
    ]
)
