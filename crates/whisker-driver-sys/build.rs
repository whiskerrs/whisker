//! Build script for `whisker-driver-sys`.
//!
//! Compiles the C++ bridge in `bridge/` into a static archive and
//! emits the link directives that thread it (and Lynx) into the user
//! crate's final dylib (Android and iOS — both targets now use the
//! same `--crate-type=dylib` shape so subsecond hot-patches can
//! resolve mangled host symbols at `dlopen` time).
//!
//! No-op on host targets (`cargo check`, host tests, rust-analyzer …)
//! so the workspace stays buildable without any native toolchain.

use anyhow::Result;
use std::path::PathBuf;

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=bridge");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    match target_os.as_str() {
        "android" => compile_android(),
        "ios" => compile_ios(),
        _ => Ok(()),
    }
}

// --- Paths -----------------------------------------------------------

fn bridge_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bridge")
}

fn workspace_target_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR is `<workspace>/crates/whisker-driver-sys`; the
    // workspace target dir is its great-grandparent's `target/`.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("target"))
        .expect("workspace layout")
}

fn lynx_staged_headers() -> PathBuf {
    workspace_target_dir().join("lynx-headers")
}

// --- Lynx C++ header path tree (iOS only) ----------------------------
//
// Phase 6-α: the bridge proper (whisker_bridge_common.cc + the
// platform-specific .cc/.mm) only needs `lynx_native_renderer_capi.h`,
// which is vendored under `bridge/include/`. On Android, the C API
// symbols live inside liblynx.so (compiled by the Lynx fork CI from
// `core/native_renderer_capi/lynx_native_renderer.cc`), so the
// bridge needs zero Lynx C++ headers at all.
//
// On iOS, upstream Lynx 3.7.0's CocoaPods spec doesn't include the
// new `core/native_renderer_capi/` subtree — so we compile
// `lynx_native_renderer.cc` ourselves into WhiskerDriver.framework.
// THAT file pokes Lynx C++ internals (LynxShell, ElementManager,
// FiberElement family), so the deep header tree is still required
// for the iOS build.
fn add_lynx_includes_for_capi_impl(build: &mut cc::Build) {
    let staged = lynx_staged_headers();
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

// --- Android ---------------------------------------------------------

fn compile_android() -> Result<()> {
    let abi = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let abi_dir = match abi.as_str() {
        "aarch64" => "arm64-v8a",
        other => anyhow::bail!(
            "whisker-driver-sys currently supports only arm64-v8a on Android (got {other})"
        ),
    };

    let bridge_src = bridge_root().join("src");
    let lynx_jni = workspace_target_dir()
        .join("lynx-android-unpacked/jni")
        .join(abi_dir);
    if !lynx_jni.is_dir() {
        anyhow::bail!(
            "Lynx Android jniLibs missing at {}\n  \
             Run `whisker build --target android` (or `whisker run`) first \
             so `whisker-build` can fetch the pinned Lynx tarball + \
             symlink it into target/. Set WHISKER_LYNX_DIR=/abs/path to \
             override with a local build.",
            lynx_jni.display()
        );
    }

    // --- Bridge compile ---------------------------------------------
    //
    // Android only needs the vendored `lynx_native_renderer_capi.h`
    // (under bridge/include/); the symbols it declares come from
    // liblynx.so. No deep Lynx C++ header tree required — Phase 6-α
    // removed every direct C++-internal include from the bridge.
    let mut build = cc::Build::new();
    // Silence cc's auto `cargo:rustc-link-lib=static=...` so we can
    // emit our own with `+whole-archive` (cargo refuses duplicates).
    build.cargo_metadata(false);
    build
        .cpp(true)
        .std("c++17")
        .file(bridge_src.join("whisker_bridge_common.cc"))
        .file(bridge_src.join("whisker_bridge_android.cc"))
        .include(bridge_root().join("include"))
        .include(&bridge_src);
    // Force inline LSE atomics so the C++ side never reaches for
    // compiler-rt's outline-atomics dispatcher (`__aarch64_cas*`,
    // `init_have_lse_atomics`). The Rust side gets the same treatment
    // via `.cargo/config.toml` target-feature flags.
    build.flag("-march=armv8.1-a");
    build.flag("-mno-outline-atomics");
    build.compile("whisker_bridge_static");

    // --- Link-line emission -----------------------------------------
    // `+whole-archive` keeps every .o regardless of whether any Rust
    // code references its symbols — JNI exports (`JNI_OnLoad`,
    // `Java_*`) are only "referenced" by the Android runtime at load
    // time and would otherwise be GC'd. Verified: this propagates to
    // the parent dylib through cargo's link-lib transitive rules.
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    println!("cargo:rustc-link-search=native={out_dir}");
    println!("cargo:rustc-link-lib=static:+whole-archive=whisker_bridge_static");

    // Dynamic deps onto Lynx's shipped .so files.
    println!("cargo:rustc-link-search=native={}", lynx_jni.display());
    println!("cargo:rustc-link-lib=dylib=lynx");
    println!("cargo:rustc-link-lib=dylib=lynxbase");
    println!("cargo:rustc-link-lib=dylib=log");
    println!("cargo:rustc-link-lib=dylib=c++_shared");
    println!("cargo:rustc-link-lib=dylib=c");

    // No `rustc-link-arg-cdylib` directives — the Android user crate
    // is now built as `dylib`, and `rustc-link-arg-cdylib` is silently
    // dropped (with a cargo warning) for non-cdylib consumers. The
    // JNI export visibility that the previous `--version-script` here
    // handled is now applied by `whisker-build`'s Android Cargo
    // wrapper via a `--version-script` that's merged with rustc's
    // auto-generated dylib export list. See docs/hot-reload-plan.md
    // "Second Pivot" for the cdylib → dylib rationale.

    Ok(())
}

// --- iOS -------------------------------------------------------------

const LYNX_FRAMEWORKS: &[&str] = &["Lynx", "LynxBase", "LynxServiceAPI", "PrimJS"];

fn compile_ios() -> Result<()> {
    let triple = std::env::var("TARGET").expect("cargo sets TARGET");
    let slice = match triple.as_str() {
        "aarch64-apple-ios" => "ios-arm64",
        "aarch64-apple-ios-sim" | "x86_64-apple-ios" => "ios-arm64_x86_64-simulator",
        other => anyhow::bail!("unsupported iOS target triple: {other}"),
    };
    let lynx_root = workspace_target_dir().join("lynx-ios");
    for fw in LYNX_FRAMEWORKS {
        let dir = lynx_root.join(format!("{fw}.xcframework")).join(slice);
        if !dir.is_dir() {
            anyhow::bail!(
                "Lynx xcframework slice missing: {} \n  \
                 Run `whisker build --target ios-sim` (or ios-device, or \
                 `whisker run`) first so `whisker-build` can fetch the \
                 pinned Lynx tarball + symlink it into target/. Set \
                 WHISKER_LYNX_DIR=/abs/path to override with a local build.",
                dir.display()
            );
        }
    }

    // --- Bridge compile ---------------------------------------------
    //
    // The bridge proper (whisker_bridge_common.cc + whisker_bridge_ios.mm)
    // only includes the vendored C API header. But the iOS Lynx
    // distribution (CocoaPods upstream 3.7.0) doesn't carry the new
    // `core/native_renderer_capi/` subtree the Whisker fork adds — so
    // we compile its impl ourselves into WhiskerDriver.framework
    // (`lynx_native_renderer.cc`). That file still touches Lynx C++
    // internals (LynxShell, ElementManager, FiberElement family), so
    // the deep header tree from `add_lynx_includes_for_capi_impl` is
    // still required.
    let bridge_src = bridge_root().join("src");
    let mut build = cc::Build::new();
    // Silence cc::Build's auto `cargo:rustc-link-lib=static=…`; we
    // emit `+whole-archive` ourselves below so Swift-callable bridge
    // entry points (`whisker_bridge_engine_attach` etc.) survive
    // dead-strip. Same rationale as Android's JNI exports.
    build.cargo_metadata(false);
    build
        .cpp(true)
        // Lynx itself is gnu++17; staged headers (`std::optional`,
        // `std::is_invocable_r_v`) need this dialect to compile.
        .flag("-std=gnu++17")
        // Lynx public headers use `__weak` Obj-C references, which
        // need ARC. cc-rs doesn't enable it by default — SPM's
        // Obj-C/Obj-C++ defaults did, so we have to opt back in.
        .flag("-fobjc-arc")
        .file(bridge_src.join("whisker_bridge_common.cc"))
        .file(bridge_src.join("whisker_bridge_ios.mm"))
        .file(bridge_src.join("lynx_native_renderer.cc"))
        .include(bridge_root().join("include"))
        .include(&bridge_src);
    add_lynx_includes_for_capi_impl(&mut build);
    for fw in LYNX_FRAMEWORKS {
        build.flag("-F");
        build.flag(
            lynx_root
                .join(format!("{fw}.xcframework"))
                .join(slice)
                .to_str()
                .expect("xcframework path is valid UTF-8"),
        );
    }
    // Match Lynx xcframework's Release build (NDEBUG=1 → no
    // `adoption_required_` / `destruction_started_` debug fields in
    // `RefCountedThreadSafeBase`).
    build.define("NDEBUG", Some("1"));
    // cc::Build picks Obj-C++ semantics for `.mm` files automatically
    // (it appends `-x objective-c++`), so no extra flag is needed for
    // `whisker_bridge_ios.mm`.
    build.compile("whisker_bridge_static");

    // --- Link-line emission for the parent dylib --------------------
    // The user crate is now built as `dylib` on iOS (matching
    // Android — see `whisker-build/src/ios.rs::build_xcframework`),
    // so cargo *does* have a final link step and these directives
    // flow into it. The previous `rustc-link-arg-staticlib=…`
    // directives were silently dropped because staticlibs have no
    // link step.

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    println!("cargo:rustc-link-search=native={out_dir}");
    // Bridge entry points are called by Swift through the framework's
    // header, not from Rust — without `+whole-archive` they'd be
    // dead-stripped before reaching the dylib's `.dynsym`.
    println!("cargo:rustc-link-lib=static:+whole-archive=whisker_bridge_static");

    // Lynx framework search + dependent-dylib references. The bridge
    // `.a` has UND refs to `LynxShell::*` etc.; resolving them at the
    // dylib's link step bakes LC_LOAD_DYLIB entries pointing at
    // `@rpath/<Lynx>.framework/<Lynx>`, which dyld resolves at runtime
    // when the host app's `@executable_path/Frameworks` rpath hits
    // the SPM-embedded Lynx frameworks.
    for fw in LYNX_FRAMEWORKS {
        let dir = lynx_root.join(format!("{fw}.xcframework")).join(slice);
        println!("cargo:rustc-link-search=framework={}", dir.display());
        println!("cargo:rustc-link-lib=framework={fw}");
    }

    // Apple system frameworks the bridge `.mm` directly references
    // (NSLog, NSObject machinery, Obj-C runtime, etc.). When Lynx
    // was static-linked into our dylib, these symbols got pulled in
    // transitively from the Lynx archive's UND refs — but now Lynx
    // is a dynamic framework with its own LC_LOAD_DYLIB list, so
    // our dylib has to declare them itself.
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=UIKit");
    println!("cargo:rustc-link-lib=framework=CoreFoundation");
    println!("cargo:rustc-link-lib=framework=QuartzCore");
    // Lynx engine's transitive dependencies (still relevant — even
    // with dynamic Lynx, declaring them here lets the host app's
    // static-analysis tooling see the dependency).
    println!("cargo:rustc-link-lib=framework=JavaScriptCore");
    println!("cargo:rustc-link-lib=framework=NaturalLanguage");
    // libc++ for the bridge's C++ standard-library uses.
    println!("cargo:rustc-link-lib=dylib=c++");
    // Obj-C runtime stubs (`_objc_msgSend`, `_objc_release_x19`,
    // `_class_getInstanceVariable`, …). Apple linkers usually
    // auto-link `libobjc` for any binary that touches Obj-C, but
    // declaring it explicitly avoids the auto-link omission we saw
    // when going from static-Lynx (carrying libobjc transitively)
    // to dynamic-Lynx.
    println!("cargo:rustc-link-lib=dylib=objc");

    // NOTE: forcing bridge entry points (`_whisker_bridge_*`) into the
    // dylib's `.dynsym` happens in
    // `whisker-build/src/ios.rs::build_xcframework`, not here.
    // `cargo:rustc-link-arg=…` only flows into the link of the crate
    // that owns the build.rs (whisker-driver-sys is an rlib — no
    // link step) and does NOT propagate to the parent dylib build of
    // the user crate. `whisker-build` appends the
    // `-Wl,-exported_symbol` flags directly to the `cargo rustc`
    // invocation that produces the user-crate dylib, where they
    // actually take effect.

    Ok(())
}
