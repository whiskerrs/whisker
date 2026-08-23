//! Native glue required by Whisker's retained mobile Host.

use anyhow::Result;
use std::path::PathBuf;

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=bridge");

    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if target_arch == "wasm32" {
        return Ok(());
    }

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    match target_os.as_str() {
        "android" => compile_android(),
        // iOS calls the exported Rust C ABI directly from Swift.
        "ios" => Ok(()),
        // Non-mobile compatibility tests still exercise the old module API
        // against an inert Host stub. It contains no Lynx runtime.
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

// --- Android ---------------------------------------------------------

fn compile_android() -> Result<()> {
    let abi = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    match abi.as_str() {
        "aarch64" => {}
        other => anyhow::bail!(
            "whisker-driver-sys currently supports only arm64-v8a on Android (got {other})"
        ),
    }

    let mut build = cc::Build::new();
    build.cargo_metadata(false);
    build
        .file(bridge_root().join("src/whisker_mobile_android.c"))
        .include(bridge_root().join("include"))
        .flag_if_supported("-std=c11")
        .compile("whisker_mobile_bridge");

    // `+whole-archive` keeps every .o regardless of whether any Rust
    // code references its symbols — JNI exports (`JNI_OnLoad`,
    // `Java_*`) are only "referenced" by the Android runtime at load
    // time and would otherwise be GC'd.
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    println!("cargo:rustc-link-search=native={out_dir}");
    println!("cargo:rustc-link-lib=static:+whole-archive=whisker_mobile_bridge");

    Ok(())
}
