// Compile the lyra_bridge C++ sources into this crate so the static
// library / cdylib that cargo emits is self-contained from the
// linker's point of view.
//
// On Android the cdylib that ships in the APK is fully self-contained
// (apart from the dynamic deps on liblynx.so / liblynxbase.so) — there
// is no separate build step that wires up the bridge later. The Rust
// crate IS the JNI entry point.
//
// On iOS the staticlib is bundled into LyraMobile.xcframework and
// linked into the host app by Xcode. The bridge .o files end up in
// the same .a so the app's linker can resolve `lyra_bridge_*` symbols
// without SPM having to compile the bridge a second time. Keeping the
// build in one place (build.rs for both platforms) means the SPM
// LyraBridge target can stay deleted.
//
// On any other host (`cargo check`, `cargo test` running on macOS
// against the host triple, …) build.rs is a no-op and Rust falls back
// to UND references for the bridge symbols. That's fine for type-only
// checks; running tests that actually invoke the bridge isn't
// supported off-device.

use std::path::{Path, PathBuf};

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let repo_root = repo_root();
    let bridge = repo_root.join("native/bridge");

    match target_os.as_str() {
        "android" => build_android(&repo_root, &bridge),
        "ios" => build_ios(&repo_root, &bridge),
        _ => {
            // Host build (cargo check, cargo test --target native).
            // No bridge → unresolved symbols, but harmless for non-link
            // workflows.
        }
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Header search paths for Lynx C++ sources (both platforms read from
/// the same `target/lynx-ios/sources/…` staging tree the iOS xtask
/// produces, regardless of the build target — the headers are
/// platform-independent C++).
fn add_lynx_includes(build: &mut cc::Build, repo_root: &Path) {
    let staged = repo_root.join("target/lynx-ios/sources");
    let primjs = staged.join("PrimJS/src");
    build
        .include(staged.join("Lynx"))
        .include(staged.join("LynxBase"))
        .include(staged.join("LynxServiceAPI"))
        .include(&primjs)
        .include(primjs.join("interpreter"))
        .include(primjs.join("interpreter/quickjs/include"))
        .include(primjs.join("gc"))
        .include(primjs.join("napi"))
        .include(primjs.join("napi/env"))
        .include(primjs.join("napi/quickjs"))
        .include(primjs.join("napi/jsc"));
}

// =====================================================================
// Android
// =====================================================================

fn build_android(repo_root: &Path, bridge: &Path) {
    let abi = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let abi_dir = match abi.as_str() {
        "aarch64" => "arm64-v8a",
        _ => panic!("hello-world Android build only supports arm64-v8a (got {abi})"),
    };
    let lynx_jni = repo_root
        .join("target/lynx-android-unpacked/jni")
        .join(abi_dir);

    let mut build = cc::Build::new();
    // Suppress cc's auto `cargo:rustc-link-lib=static=...` so we can
    // emit our own with `+whole-archive` (cargo refuses duplicates).
    build.cargo_metadata(false);
    build
        .cpp(true)
        .std("c++17")
        .file(bridge.join("src/lyra_bridge_common.cc"))
        .file(bridge.join("src/lyra_bridge_android.cc"))
        .include(bridge.join("include"))
        .include(bridge.join("src"));
    add_lynx_includes(&mut build, repo_root);
    // NDEBUG=1 keeps `RefCountedThreadSafeBase` at the same layout
    // Lynx's release build uses (debug builds add two extra bool
    // members and shift every offset in subclasses like FiberElement).
    build.define("NDEBUG", Some("1"));
    // Force inline LSE atomics so the C++ side never reaches for
    // compiler-rt's outline-atomics dispatcher.
    build.flag("-march=armv8.1-a");
    build.flag("-mno-outline-atomics");
    build.compile("lyra_bridge_static");

    // Re-emit the link line ourselves with `+whole-archive` so the
    // linker keeps every .o regardless of whether any Rust code
    // references its symbols (JNI exports `JNI_OnLoad` / `Java_*` are
    // only "referenced" by the Android runtime at load time).
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    println!("cargo:rustc-link-search=native={}", out_dir);
    println!("cargo:rustc-link-lib=static:+whole-archive=lyra_bridge_static");

    // Dynamic deps onto Lynx's shipped .so files.
    println!("cargo:rustc-link-search=native={}", lynx_jni.display());
    println!("cargo:rustc-link-lib=dylib=lynx");
    println!("cargo:rustc-link-lib=dylib=lynxbase");
    println!("cargo:rustc-link-lib=dylib=log");
    println!("cargo:rustc-link-lib=dylib=c++_shared");

    // We deliberately do NOT add `-L $sysroot/usr/lib/<triple>` (the
    // non-API-versioned dir): clang's driver adds it at lower priority
    // automatically, and the non-versioned dir contains `libc.a` but
    // no `libc.so` — putting it ahead of clang's `/24/` dir makes lld
    // resolve `-lc` to the static `.a`, dragging static bionic guts
    // into the cdylib and breaking Rust `thread_local!` at runtime.

    // Force-list libc.so before `--as-needed` strips it. Without an
    // explicit dynamic reference, the NDK driver's tail `-lc` lands
    // after our static archives and `--as-needed` drops it when no
    // PLT call has been seen yet.
    println!("cargo:rustc-link-arg=-Wl,-z,now");
    println!("cargo:rustc-link-arg=-Wl,--no-as-needed");
    println!("cargo:rustc-link-lib=dylib=c");
    println!("cargo:rustc-link-arg=-Wl,--as-needed");

    // Override the cdylib's auto-generated `--version-script` (which
    // hides every non-Rust symbol — including the JNI entry points the
    // Android runtime resolves by name).
    let version_script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("android-exports.ver");
    std::fs::write(
        &version_script,
        b"{\n  global:\n    Java_*;\n    JNI_OnLoad;\n    lyra_mobile_*;\n  local: *;\n};\n",
    )
    .expect("write version script");
    println!(
        "cargo:rustc-link-arg=-Wl,--version-script={}",
        version_script.display()
    );
}

// =====================================================================
// iOS
// =====================================================================

fn build_ios(repo_root: &Path, bridge: &Path) {
    // Pick the right xcframework slice for the Rust target. The host
    // tool `xtask ios build-lynx-frameworks` produces these two:
    //   ios-arm64                       (real device)
    //   ios-arm64_x86_64-simulator      (lipo of arm64-sim + x86_64-sim)
    let triple = std::env::var("TARGET").expect("cargo sets TARGET");
    let slice = match triple.as_str() {
        "aarch64-apple-ios" => "ios-arm64",
        "aarch64-apple-ios-sim" | "x86_64-apple-ios" => "ios-arm64_x86_64-simulator",
        other => panic!("unsupported iOS target triple: {other}"),
    };
    let xcframeworks = repo_root.join("target/lynx-ios");
    for fw in ["Lynx", "LynxBase", "LynxServiceAPI", "PrimJS"] {
        let dir = xcframeworks.join(format!("{fw}.xcframework")).join(slice);
        if !dir.is_dir() {
            panic!(
                "Lynx xcframework slice missing: {} \n\
                 Run `cargo xtask ios build-lynx-frameworks` first.",
                dir.display()
            );
        }
        // -F lets clang resolve `<Lynx/LynxView.h>` style framework
        // imports to <slice>/Lynx.framework/Headers/LynxView.h.
        println!("cargo:rustc-link-arg=-F{}", dir.display());
    }

    let mut build = cc::Build::new();
    build
        .cpp(true)
        // Lynx itself is gnu++17; staged headers (`std::optional`,
        // `std::is_invocable_r_v`) need this dialect to compile.
        .flag("-std=gnu++17")
        // Lynx public headers use `__weak` Obj-C references, which
        // need ARC. cc-rs doesn't enable it by default — SPM's
        // Obj-C/Obj-C++ defaults did, so we have to opt back in.
        .flag("-fobjc-arc")
        .file(bridge.join("src/lyra_bridge_common.cc"))
        .file(bridge.join("src/lyra_bridge_ios.mm"))
        .include(bridge.join("include"))
        .include(bridge.join("src"));
    add_lynx_includes(&mut build, repo_root);
    // Add framework search paths to *cc::Build* too so the bridge .mm
    // can resolve `<Lynx/...>` imports during cc-rs's compile step.
    // The link-arg above is for the final rustc link; this one is for
    // the per-TU compile we drive ourselves.
    for fw in ["Lynx", "LynxBase", "LynxServiceAPI", "PrimJS"] {
        build.flag("-F");
        build.flag(
            xcframeworks
                .join(format!("{fw}.xcframework"))
                .join(slice)
                .to_str()
                .expect("xcframework path is valid UTF-8"),
        );
    }
    // Same NDEBUG=1 reason as Android: Lynx xcframeworks ship Release
    // build, our bridge has to agree on RefCounted layout.
    build.define("NDEBUG", Some("1"));
    // `cc::Build` picks up Obj-C++ semantics for `.mm` files
    // automatically (it appends `-x objective-c++` for that extension)
    // so no extra flag is needed for `lyra_bridge_ios.mm`.
    build.compile("lyra_bridge_static");

    // The Lynx C++ symbols (`LynxShell::RunOnTasmThread`, the Element
    // PAPI etc.) live in Lynx.xcframework, which is added to the host
    // app by SPM via Package.swift's binaryTarget. We don't tell
    // rustc to *link* them here — staticlib output is allowed to have
    // unresolved externs; the host app's linker does the final
    // resolution when both LyraMobile.xcframework and Lynx.xcframework
    // are linked together. For the same reason we don't link `c++` /
    // Foundation here either — those land via SPM's `linkerSettings`
    // on the LyraRuntime target.
}
