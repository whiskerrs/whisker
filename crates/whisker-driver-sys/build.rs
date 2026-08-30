//! Builds the Android-owned JNI entry shim when targeting Android.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=bridge/include/whisker_mobile.h");
    println!("cargo:rerun-if-changed=bridge/include/whisker_android_jni.h");
    println!("cargo:rerun-if-changed=bridge/src/whisker_mobile_android.c");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        compile_android();
    }
}

fn bridge_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bridge")
}

fn compile_android() {
    let abi = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    assert_eq!(
        abi, "aarch64",
        "whisker-driver-sys currently supports only arm64-v8a on Android"
    );

    let mut build = cc::Build::new();
    build.cargo_metadata(false);
    build
        .file(bridge_root().join("src/whisker_mobile_android.c"))
        .include(bridge_root().join("include"))
        .flag_if_supported("-std=c11")
        .compile("whisker_mobile_bridge");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    println!("cargo:rustc-link-search=native={out_dir}");
    println!("cargo:rustc-link-lib=static:+whole-archive=whisker_mobile_bridge");
}
