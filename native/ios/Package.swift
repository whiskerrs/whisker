// swift-tools-version:5.9
import PackageDescription

// WhiskerRuntime is the SPM package the iOS host app depends on. It
// composes:
//
//   WhiskerDriver.xcframework  — Rust crate (the user's `#[whisker::main]`
//                             code) + the C++ Lynx bridge, packaged as
//                             a dynamic `.framework` so subsecond can
//                             hot-patch it at runtime. Build by:
//                             `cargo xtask ios build-xcframework`.
//   Lynx*.xcframework       — Lynx engine + PrimJS, dynamic frameworks
//                             built from the upstream CocoaPods source
//                             pods. Build by:
//                             `cargo xtask ios build-lynx-frameworks`.
//   WhiskerRuntime (Swift)     — thin Swift API: WhiskerView, WhiskerAppDelegate,
//                             CADisplayLink-driven render loop.
//
// The bridge is intentionally NOT an SPM target. We used to have a
// `WhiskerBridge` C++ target here that compiled bridge sources via SPM;
// building an iOS dylib + an Android cdylib both require the same
// bridge sources, so keeping the build in `crates/whisker-driver-sys/
// build.rs` (where it already lived for Android) means a single source
// of truth. The bridge now lives under `crates/whisker-driver-sys/bridge/`.
//
// Build pre-reqs (run before opening Xcode):
//   cargo xtask ios build-lynx-frameworks
//   cargo xtask ios build-xcframework
//
// Runtime layout: the host app embeds WhiskerDriver.framework under
// `<App>.app/Frameworks/`. The dylib's LC_ID_DYLIB is
// `@rpath/WhiskerDriver.framework/WhiskerDriver` (set by xtask via
// `install_name_tool -id`), so the host app needs
// `@executable_path/Frameworks` in `LD_RUNPATH_SEARCH_PATHS` — set in
// `examples/<pkg>/ios/project.yml` (XcodeGen → Xcode project setting).

let package = Package(
    name: "WhiskerRuntime",
    platforms: [
        .iOS(.v13),
    ],
    products: [
        .library(name: "WhiskerRuntime", targets: ["WhiskerRuntime"]),
    ],
    targets: [
        // Rust runtime + C++ bridge, packaged as a dynamic xcframework
        // (one `.framework` per slice). The cargo dylib's build.rs
        // emits dependent-dylib refs (LC_LOAD_DYLIB) to the Lynx
        // frameworks below, so dyld resolves them at app launch when
        // SPM auto-embeds the Lynx xcframeworks into the host app.
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
                // System frameworks Lynx depends on transitively.
                // WhiskerDriver.dylib already declares LC_LOAD_DYLIB
                // for these (see `whisker-driver-sys/build.rs`), so
                // dyld would load them anyway, but keeping the
                // declaration here lets the host app's static-analysis
                // tooling see the dependency.
                .linkedFramework("JavaScriptCore"),
                .linkedFramework("NaturalLanguage"),
                .linkedLibrary("c++"),
                // `-ObjC` is no longer required here: when iOS was a
                // staticlib, the host app's link step had to be told
                // to pull every Obj-C class from the archived `.o`
                // files. With the dylib path, that responsibility
                // moves into the dylib's own link step in
                // `whisker-driver-sys/build.rs`
                // (`cargo:rustc-link-arg=-Wl,-ObjC`). The Obj-C classes
                // end up in the dylib's `__objc_classlist` and dyld
                // picks them up at load time, so the host app no
                // longer needs the flag.
            ]
        ),
    ]
)
