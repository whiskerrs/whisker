// swift-tools-version:5.9
import PackageDescription

// TuftRuntime is the SPM package the iOS host app depends on. It
// composes:
//
//   TuftDriver.xcframework  — Rust crate (the user's `#[tuft::main]`
//                             code) + the C++ Lynx bridge, all
//                             baked into one static library by
//                             cargo + build.rs (cc::Build).
//   Lynx*.xcframework       — Lynx engine + PrimJS, built from the
//                             upstream CocoaPods source pods.
//   TuftRuntime (Swift)     — thin Swift API: TuftView, TuftAppDelegate,
//                             CADisplayLink-driven render loop.
//
// The bridge is intentionally NOT an SPM target. We used to have a
// `TuftBridge` C++ target here that compiled bridge sources via SPM;
// building an iOS xcframework + an Android cdylib both requires the
// same bridge sources, so keeping the build in `examples/<x>/build.rs`
// (where it already lived for Android) means a single source of truth.
// The bridge now lives under `crates/tuft-driver-sys/bridge/`.
//
// Build pre-reqs (run before opening Xcode):
//   cargo xtask ios build-lynx-frameworks
//   cargo xtask ios build-xcframework

let package = Package(
    name: "TuftRuntime",
    platforms: [
        .iOS(.v13),
    ],
    products: [
        .library(name: "TuftRuntime", targets: ["TuftRuntime"]),
    ],
    targets: [
        // Rust runtime + C++ bridge, packaged as a static xcframework.
        // build.rs compiles `crates/tuft-driver-sys/bridge/src/
        // {tuft_bridge_common.cc, tuft_bridge_ios.mm}` into the same
        // .a, so its UND symbols for Lynx (`LynxShell::*` etc.) get
        // resolved by the host app's link step against the Lynx
        // xcframeworks below.
        .binaryTarget(
            name: "TuftDriver",
            path: "../../target/tuft-driver/TuftDriver.xcframework"
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
            name: "TuftRuntime",
            dependencies: [
                "TuftDriver",
                "Lynx",
                "LynxBase",
                "LynxServiceAPI",
                "PrimJS",
            ],
            path: "Sources/TuftRuntime",
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
