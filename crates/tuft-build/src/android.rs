//! Per-crate Android compilation logic. Called from `tuft_build::compile()`
//! when `CARGO_CFG_TARGET_OS == "android"`.

use anyhow::Result;

use crate::paths;

pub fn compile() -> Result<()> {
    let abi = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let abi_dir = match abi.as_str() {
        "aarch64" => "arm64-v8a",
        other => anyhow::bail!(
            "tuft-build currently supports only arm64-v8a on Android (got {other})"
        ),
    };

    let bridge_src = paths::bridge_src();
    let lynx_jni = paths::lynx_android_jni(abi_dir);
    if !lynx_jni.is_dir() {
        anyhow::bail!(
            "Lynx Android jniLibs missing at {}\n  \
             Run `cargo xtask android build-lynx-aar` then \
             `cargo xtask android unpack-lynx` first.",
            lynx_jni.display()
        );
    }

    // --- Bridge compile ---------------------------------------------
    let mut build = cc::Build::new();
    // Silence cc's auto `cargo:rustc-link-lib=static=...` so we can
    // emit our own with `+whole-archive` (cargo refuses duplicates).
    build.cargo_metadata(false);
    build
        .cpp(true)
        .std("c++17")
        .file(bridge_src.join("tuft_bridge_common.cc"))
        .file(bridge_src.join("tuft_bridge_android.cc"))
        .include(paths::bridge_include())
        .include(&bridge_src);
    add_lynx_includes(&mut build);
    // Match Lynx's release build: NDEBUG=1 keeps
    // `RefCountedThreadSafeBase` at the same layout (debug builds add
    // two extra bool members and shift every offset in subclasses
    // like FiberElement).
    build.define("NDEBUG", Some("1"));
    // Force inline LSE atomics so the C++ side never reaches for
    // compiler-rt's outline-atomics dispatcher (`__aarch64_cas*`,
    // `init_have_lse_atomics`). The Rust side gets the same treatment
    // via `.cargo/config.toml` target-feature flags.
    build.flag("-march=armv8.1-a");
    build.flag("-mno-outline-atomics");
    build.compile("tuft_bridge_static");

    // --- Link-line emission -----------------------------------------
    // `+whole-archive` keeps every .o regardless of whether any Rust
    // code references its symbols — JNI exports (`JNI_OnLoad`,
    // `Java_*`) are only "referenced" by the Android runtime at load
    // time and would otherwise be GC'd.
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    println!("cargo:rustc-link-search=native={out_dir}");
    println!("cargo:rustc-link-lib=static:+whole-archive=tuft_bridge_static");

    // Dynamic deps onto Lynx's shipped .so files.
    println!("cargo:rustc-link-search=native={}", lynx_jni.display());
    println!("cargo:rustc-link-lib=dylib=lynx");
    println!("cargo:rustc-link-lib=dylib=lynxbase");
    println!("cargo:rustc-link-lib=dylib=log");
    println!("cargo:rustc-link-lib=dylib=c++_shared");

    // We deliberately do NOT add `-L $sysroot/usr/lib/<triple>` (the
    // non-API-versioned dir): clang's driver adds it at lower
    // priority automatically. Adding it ourselves at higher priority
    // makes lld resolve `-lc` to static `libc.a` (which sits there
    // alongside no `libc.so`), dragging static bionic guts into the
    // cdylib and breaking `thread_local!` at runtime.

    // Force-list libc.so before `--as-needed` strips it.
    println!("cargo:rustc-link-arg=-Wl,-z,now");
    println!("cargo:rustc-link-arg=-Wl,--no-as-needed");
    println!("cargo:rustc-link-lib=dylib=c");
    println!("cargo:rustc-link-arg=-Wl,--as-needed");

    // Override the cdylib's auto-generated `--version-script` (which
    // hides every non-Rust symbol — including the JNI entry points
    // the Android runtime resolves by name at load time).
    let version_script = std::path::PathBuf::from(out_dir).join("android-exports.ver");
    std::fs::write(
        &version_script,
        b"{\n  global:\n    Java_*;\n    JNI_OnLoad;\n    tuft_mobile_*;\n  local: *;\n};\n",
    )?;
    println!(
        "cargo:rustc-link-arg=-Wl,--version-script={}",
        version_script.display()
    );

    Ok(())
}

/// Lynx C++ header search paths. Same on iOS and Android — the
/// headers themselves are platform-agnostic C++ and we stage them
/// under `target/lynx-ios/sources/` regardless of the build target.
pub(crate) fn add_lynx_includes(build: &mut cc::Build) {
    let staged = paths::lynx_staged_headers();
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
