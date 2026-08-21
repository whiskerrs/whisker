//! Build script for `whisker-driver-sys`.
//!
//! Compiles the C++ bridge in `bridge/` into a static archive and
//! emits the link directives that thread it into the user crate's
//! final dylib.
//!
//! The bridge reaches Lynx through a function-pointer table that
//! `whisker_bridge_lynx_loader.cc` fills with `dlopen` + `dlsym` at
//! engine-attach time, so its `.o` files carry no `lynx_*` UND refs:
//! no Lynx headers, no `target/lynx-*` staging, and no Lynx link line
//! are needed to `cargo build` for either device target.

use anyhow::Result;
use std::path::PathBuf;

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=bridge");

    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    // The browser Host uses the retained Rust scene directly and has no Lynx
    // C++ bridge. Keeping this build script a no-op for wasm also avoids asking
    // the `wasm32-unknown-unknown` target for a C++ standard library it does
    // not provide.
    if target_arch == "wasm32" {
        return Ok(());
    }

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    match target_os.as_str() {
        "android" => compile_android(),
        "ios" => compile_ios(),
        _ => compile_host_stub(),
    }
}

/// Compile `whisker_bridge_host_stub.cc` on non-iOS / non-Android
/// targets, so host tests link without pulling in
/// `whisker_bridge_common.cc`'s dispatch-table call sites.
fn compile_host_stub() -> Result<()> {
    let bridge_src = bridge_root().join("src");
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .flag_if_supported("-std=gnu++17")
        .file(bridge_src.join("whisker_bridge_host_stub.cc"))
        .include(bridge_root().join("include"))
        .include(&bridge_src);
    build
        .try_compile("whisker_bridge_host_stub")
        .map_err(|e| anyhow::anyhow!("compile whisker_bridge_host_stub.cc: {e}"))?;
    Ok(())
}

// --- Paths -----------------------------------------------------------

fn bridge_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bridge")
}

/// Quiet the bridge build's `-Wunused-parameter` chatter — the stub
/// Obj-C `@interface` getters take arguments they don't read, and
/// cc-rs has no per-file warning override.
fn silence_unused_parameter_warnings(build: &mut cc::Build) {
    build.flag_if_supported("-Wno-unused-parameter");
}

// --- Android ---------------------------------------------------------

fn compile_android() -> Result<()> {
    let abi = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    match abi.as_str() {
        "aarch64" => {}
        other => anyhow::bail!(
            "whisker-driver-sys currently supports only arm64-v8a on Android (got {other})"
        ),
    }

    let bridge_src = bridge_root().join("src");

    let mut build = cc::Build::new();
    // Silence cc's auto `cargo:rustc-link-lib=static=...` so we can
    // emit our own with `+whole-archive` (cargo refuses duplicates).
    build.cargo_metadata(false);
    build
        .cpp(true)
        .std("c++17")
        .file(bridge_src.join("whisker_bridge_common.cc"))
        .file(bridge_src.join("whisker_bridge_android.cc"))
        .file(bridge_src.join("whisker_bridge_lynx_loader.cc"))
        .include(bridge_root().join("include"))
        .include(&bridge_src);
    // Keep the C++ side away from compiler-rt's outline-atomics
    // dispatcher (`__aarch64_cas*`, `init_have_lse_atomics`) — its ELF
    // initializer crashes inside a local `getauxval` stub on some
    // bionic builds, and the helpers go unresolved at load time once
    // compiler-rt is stripped out. Clang then emits Armv8.0
    // `ldaxr`/`stlxr` loops inline, which is what `arm64-v8a`'s
    // baseline permits; must NOT be paired with `-march=armv8.1-a`,
    // whose LSE instructions SIGILL on a real Armv8.0 device.
    build.flag("-mno-outline-atomics");
    silence_unused_parameter_warnings(&mut build);
    build.compile("whisker_bridge_static");

    // `+whole-archive` keeps every .o regardless of whether any Rust
    // code references its symbols — JNI exports (`JNI_OnLoad`,
    // `Java_*`) are only "referenced" by the Android runtime at load
    // time and would otherwise be GC'd.
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    println!("cargo:rustc-link-search=native={out_dir}");
    println!("cargo:rustc-link-lib=static:+whole-archive=whisker_bridge_static");

    println!("cargo:rustc-link-lib=dylib=log");
    println!("cargo:rustc-link-lib=dylib=c++_shared");
    println!("cargo:rustc-link-lib=dylib=c");

    // No `rustc-link-arg-cdylib` directives: the Android user crate is
    // a `dylib`, for which cargo silently drops them. JNI export
    // visibility is applied by `whisker-build`'s Android cargo wrapper
    // instead, merging its `--version-script` with rustc's generated
    // dylib export list.

    Ok(())
}

// --- iOS -------------------------------------------------------------

fn compile_ios() -> Result<()> {
    let triple = std::env::var("TARGET").expect("cargo sets TARGET");
    match triple.as_str() {
        "aarch64-apple-ios" | "aarch64-apple-ios-sim" | "x86_64-apple-ios" => {}
        other => anyhow::bail!("unsupported iOS target triple: {other}"),
    }

    let bridge_src = bridge_root().join("src");
    let mut build = cc::Build::new();
    // Silence cc::Build's auto `cargo:rustc-link-lib=static=…`; we
    // emit `+whole-archive` ourselves below so Swift-callable bridge
    // entry points (`whisker_bridge_engine_attach` etc.) survive
    // dead-strip.
    build.cargo_metadata(false);
    build
        .cpp(true)
        .flag("-std=gnu++17")
        .flag("-fobjc-arc")
        .define("OS_IOS", "1")
        .file(bridge_src.join("whisker_bridge_common.cc"))
        .file(bridge_src.join("whisker_bridge_ios.mm"))
        .file(bridge_src.join("whisker_bridge_lynx_loader.cc"))
        .include(bridge_root().join("include"))
        .include(&bridge_src);
    // Match the iOS xcframework's Release build — a debug build
    // disagrees on the layout of shared types we reference indirectly.
    build.define("NDEBUG", Some("1"));
    silence_unused_parameter_warnings(&mut build);
    build.compile("whisker_bridge_static");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    println!("cargo:rustc-link-search=native={out_dir}");
    // Bridge entry points are called by Swift through the framework's
    // header, not from Rust — without `+whole-archive` they'd be
    // dead-stripped before reaching the dylib's `.dynsym`.
    println!("cargo:rustc-link-lib=static:+whole-archive=whisker_bridge_static");

    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=UIKit");
    println!("cargo:rustc-link-lib=framework=CoreFoundation");
    println!("cargo:rustc-link-lib=framework=QuartzCore");
    println!("cargo:rustc-link-lib=dylib=c++");
    // Apple linkers usually auto-link `libobjc`, but the auto-link is
    // unreliable for a dynamically-loaded Lynx — declare it.
    println!("cargo:rustc-link-lib=dylib=objc");

    // Forcing bridge entry points (`_whisker_bridge_*`) into the
    // dylib's `.dynsym` happens in
    // `whisker-build/src/ios.rs::build_framework_for_xcode_run_script`,
    // not here: `cargo:rustc-link-arg=…` only reaches the link of the
    // crate owning the build.rs, and this crate is an rlib with no
    // link step of its own.

    Ok(())
}
