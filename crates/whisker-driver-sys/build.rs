//! Build script for `whisker-driver-sys`.
//!
//! Compiles the C++ bridge in `bridge/` into a static archive and
//! emits the link directives that thread it into the user crate's
//! final dylib.
//!
//! Step-6 build decoupling (this file's main responsibility):
//! pre-Step-6 the bridge `.o` files carried link-time UND refs to
//! every `lynx_*` symbol they called, so this script had to emit
//! `-framework Lynx*` (iOS) / `-llynx` (Android) — and that required
//! the user to have run `whisker build` once to stage the Lynx
//! artifact tree under `target/lynx-{android-unpacked,ios}/`. Cold
//! `cargo build` couldn't succeed against a fresh checkout.
//!
//! Now the bridge calls Lynx through a function pointer table that
//! `whisker_bridge_lynx_loader.cc` populates with `dlopen` +
//! `dlsym` at engine-attach time. The bridge `.o` files carry zero
//! `lynx_*` UND refs, so this script no longer needs Lynx headers
//! OR a Lynx link line — `cargo build --target=aarch64-{linux-
//! android,apple-ios}` succeeds without any prior tooling.
//!
//! What still happens here:
//!   * Compile the bridge sources (whisker_bridge_common.cc +
//!     platform glue + the loader) into a static archive.
//!   * Emit `+whole-archive` so the bridge entry points
//!     (`whisker_bridge_*`) survive the parent dylib's dead-strip.
//!   * Declare the system frameworks / libs the bridge `.mm` /
//!     `.cc` actually use (Foundation, UIKit, libdl, libc++, …).
//!
//! No Lynx headers, no `target/lynx-*` staging, no
//! `WHISKER_IOS_MODULE_NATIVE_SOURCES` — module .mm sources used to
//! be plumbed through that env var with a pre-staged Lynx header
//! tree, but nothing ever declared `[package.metadata.whisker.ios].
//! native_sources`, so the path was unreferenced code that broke
//! silently the moment the workspace's `target/lynx-ios` symlink
//! went away. Removed wholesale. If module authors need iOS Obj-C++
//! sources later, the SPM xcframeworks already expose the necessary
//! headers via xcodebuild's framework-search-paths — re-introducing
//! the feature won't need any local cache plumbing.

use anyhow::Result;
use std::path::PathBuf;

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=bridge");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    match target_os.as_str() {
        "android" => compile_android(),
        "ios" => compile_ios(),
        _ => compile_host_stub(),
    }
}

/// Compile `whisker_bridge_host_stub.cc` on non-iOS / non-Android
/// targets. The stub satisfies the bridge's pure-C surface
/// (native-module dispatch registry, `whisker_bridge_invoke_module`,
/// `whisker_bridge_value_release`, `whisker_bridge_log_hello`) so
/// host tests link without pulling in `whisker_bridge_common.cc`'s
/// dispatch-table call sites.
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

/// Quiet the bridge `.mm` / `.cc` build's `-Wunused-parameter` chatter.
/// The bridge declares stub Obj-C `@interface` types whose getters take
/// arguments they don't read; cc-rs has no per-file warning override
/// so a `flag_if_supported` is the cheapest way to keep cargo logs
/// clean without rewriting every stub signature.
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

    // --- Bridge compile ---------------------------------------------
    //
    // Step-6: zero Lynx headers required. The bridge sees the
    // vendored `lynx_capi.h` (function pointer typedefs + dispatch
    // table) for type definitions and routes every call through
    // `whisker_lynx_capi()->fn`; the loader does `dlopen("liblynx.so")`
    // + `dlsym` at engine_attach time.
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
    // Force inline LSE atomics so the C++ side never reaches for
    // compiler-rt's outline-atomics dispatcher (`__aarch64_cas*`,
    // `init_have_lse_atomics`). The Rust side gets the same treatment
    // via `.cargo/config.toml` target-feature flags.
    build.flag("-march=armv8.1-a");
    build.flag("-mno-outline-atomics");
    silence_unused_parameter_warnings(&mut build);
    build.compile("whisker_bridge_static");

    // --- Link-line emission -----------------------------------------
    // `+whole-archive` keeps every .o regardless of whether any Rust
    // code references its symbols — JNI exports (`JNI_OnLoad`,
    // `Java_*`) are only "referenced" by the Android runtime at load
    // time and would otherwise be GC'd.
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    println!("cargo:rustc-link-search=native={out_dir}");
    println!("cargo:rustc-link-lib=static:+whole-archive=whisker_bridge_static");

    // Step-6: no more `-llynx` / `-llynxbase`. Lynx is resolved at
    // runtime by `whisker_bridge_load_lynx()` via dlopen; the loader
    // pulls libdl from bionic which is always available, no extra
    // link directive needed (libdl is part of libc on Android).
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

fn compile_ios() -> Result<()> {
    let triple = std::env::var("TARGET").expect("cargo sets TARGET");
    match triple.as_str() {
        "aarch64-apple-ios" | "aarch64-apple-ios-sim" | "x86_64-apple-ios" => {}
        other => anyhow::bail!("unsupported iOS target triple: {other}"),
    }

    // --- Bridge compile ---------------------------------------------
    //
    // Step-6: the bridge proper (whisker_bridge_common.cc +
    // whisker_bridge_ios.mm + whisker_bridge_lynx_loader.cc) compiles
    // against the vendored `lynx_capi.h` (function pointer typedefs)
    // and the vendored `lynx_objc_stubs.h` (minimal Obj-C @interface
    // declarations for LynxView / LynxEvent / LynxTouchEvent / …).
    // No `-F` paths into a staged Lynx xcframework needed; all Lynx
    // symbols resolve at runtime via dlopen + dlsym +
    // `objc_getClass`.
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
        // Lynx public headers used `__weak` Obj-C references; the
        // vendored stubs don't, but our stub @interface declarations
        // are imported into an ARC-managed .mm and keeping ARC on
        // matches the upstream convention.
        .flag("-fobjc-arc")
        .define("OS_IOS", "1")
        .file(bridge_src.join("whisker_bridge_common.cc"))
        .file(bridge_src.join("whisker_bridge_ios.mm"))
        .file(bridge_src.join("whisker_bridge_lynx_loader.cc"))
        .include(bridge_root().join("include"))
        .include(&bridge_src);
    // Match the iOS xcframework's Release build (suppresses debug-only
    // fields in shared types we still reference indirectly).
    build.define("NDEBUG", Some("1"));
    silence_unused_parameter_warnings(&mut build);
    build.compile("whisker_bridge_static");

    // --- Link-line emission for the parent dylib --------------------
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    println!("cargo:rustc-link-search=native={out_dir}");
    // Bridge entry points are called by Swift through the framework's
    // header, not from Rust — without `+whole-archive` they'd be
    // dead-stripped before reaching the dylib's `.dynsym`.
    println!("cargo:rustc-link-lib=static:+whole-archive=whisker_bridge_static");

    // Apple system frameworks the bridge `.mm` directly references
    // (NSLog, NSObject machinery, Obj-C runtime, etc.). Step-6 dropped
    // the `-framework Lynx*` line; those frameworks load themselves at
    // runtime via SwiftPM's auto-embed, and the loader dlopen's the
    // main one explicitly.
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=UIKit");
    println!("cargo:rustc-link-lib=framework=CoreFoundation");
    println!("cargo:rustc-link-lib=framework=QuartzCore");
    // libc++ for the bridge's C++ standard-library uses.
    println!("cargo:rustc-link-lib=dylib=c++");
    // Obj-C runtime stubs (`_objc_msgSend`, `_objc_getClass`, …). Apple
    // linkers usually auto-link `libobjc` for any binary that touches
    // Obj-C, but declaring it explicitly avoids the auto-link omission
    // we saw when going from static-Lynx to dynamic-Lynx.
    println!("cargo:rustc-link-lib=dylib=objc");

    // NOTE: forcing bridge entry points (`_whisker_bridge_*`) into the
    // dylib's `.dynsym` happens in
    // `whisker-build/src/ios.rs::build_framework_for_xcode_run_script`,
    // not here.
    // `cargo:rustc-link-arg=…` only flows into the link of the crate
    // that owns the build.rs (whisker-driver-sys is an rlib — no
    // link step) and does NOT propagate to the parent dylib build of
    // the user crate. `whisker-build` appends the
    // `-Wl,-exported_symbol` flags directly to the `cargo rustc`
    // invocation that produces the user-crate dylib, where they
    // actually take effect.

    Ok(())
}
