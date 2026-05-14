// swift-tools-version:5.9
import PackageDescription

// WhiskerRuntime is the SPM package the iOS host app depends on. It
// composes:
//
//   WhiskerDriver.xcframework  — Rust crate (the user's `#[whisker::main]`
//                             code) + the C++ Lynx bridge, all
//                             baked into one static library by
//                             cargo + build.rs (cc::Build).
//   Lynx*.xcframework       — Lynx engine + PrimJS, built from the
//                             upstream CocoaPods source pods.
//   WhiskerRuntime (Swift)     — thin Swift API: WhiskerView, WhiskerAppDelegate,
//                             CADisplayLink-driven render loop.
//
// The bridge is intentionally NOT an SPM target. We used to have a
// `WhiskerBridge` C++ target here that compiled bridge sources via SPM;
// building an iOS xcframework + an Android cdylib both requires the
// same bridge sources, so keeping the build in `examples/<x>/build.rs`
// (where it already lived for Android) means a single source of truth.
// The bridge now lives under `crates/whisker-driver-sys/bridge/`.
//
// Build pre-reqs (run before opening Xcode):
//   cargo xtask ios build-lynx-frameworks
//   cargo xtask ios build-xcframework

let package = Package(
    name: "WhiskerRuntime",
    platforms: [
        .iOS(.v13),
    ],
    products: [
        .library(name: "WhiskerRuntime", targets: ["WhiskerRuntime"]),
    ],
    targets: [
        // Rust runtime + C++ bridge, packaged as a static xcframework.
        // build.rs compiles `crates/whisker-driver-sys/bridge/src/
        // {whisker_bridge_common.cc, whisker_bridge_ios.mm}` into the same
        // .a, so its UND symbols for Lynx (`LynxShell::*` etc.) get
        // resolved by the host app's link step against the Lynx
        // xcframeworks below.
        .binaryTarget(
            name: "WhiskerDriver",
            path: "../../target/whisker-driver/WhiskerDriver.xcframework"
        ),

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

        .target(
            name: "WhiskerRuntime",
            dependencies: [
                "WhiskerDriver",
                "Lynx",
                "LynxBase",
                "LynxServiceAPI",
                "PrimJS",
            ],
            path: "Sources/WhiskerRuntime",
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
