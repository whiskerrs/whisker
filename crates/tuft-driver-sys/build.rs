//! Build script for `tuft-driver-sys`.
//!
//! Compiles the C++ bridge in `bridge/` into a static archive and
//! emits the link directives that thread it (and Lynx) into the user
//! crate's final cdylib (Android) or staticlib (iOS).
//!
//! No-op on host targets (`cargo check`, host tests, rust-analyzer …)
//! so the workspace stays buildable without any native toolchain.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

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
    // CARGO_MANIFEST_DIR is `<workspace>/crates/tuft-driver-sys`; the
    // workspace target dir is its great-grandparent's `target/`.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("target"))
        .expect("workspace layout")
}

fn lynx_staged_headers() -> PathBuf {
    workspace_target_dir().join("lynx-ios/sources")
}

// --- Lynx C++ header path tree (shared by Android and iOS) ----------

fn add_lynx_includes(build: &mut cc::Build) {
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
            "tuft-driver-sys currently supports only arm64-v8a on Android (got {other})"
        ),
    };

    let bridge_src = bridge_root().join("src");
    let lynx_jni = workspace_target_dir()
        .join("lynx-android-unpacked/jni")
        .join(abi_dir);
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
        .include(bridge_root().join("include"))
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
    // time and would otherwise be GC'd. Verified: this propagates to
    // the parent cdylib through cargo's link-lib transitive rules.
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    println!("cargo:rustc-link-search=native={out_dir}");
    println!("cargo:rustc-link-lib=static:+whole-archive=tuft_bridge_static");

    // Dynamic deps onto Lynx's shipped .so files.
    println!("cargo:rustc-link-search=native={}", lynx_jni.display());
    println!("cargo:rustc-link-lib=dylib=lynx");
    println!("cargo:rustc-link-lib=dylib=lynxbase");
    println!("cargo:rustc-link-lib=dylib=log");
    println!("cargo:rustc-link-lib=dylib=c++_shared");

    // libc / linker quirks — these are `rustc-link-arg-cdylib` (not
    // plain `rustc-link-arg`) because `rustc-link-arg` from a
    // build script applies only to the *current* crate's final link,
    // and `tuft-driver-sys` itself is an rlib. The `-cdylib` variant
    // targets the eventual cdylib the user crate produces and is the
    // mechanism cargo provides for *-sys crates to thread linker
    // flags through.
    println!("cargo:rustc-link-arg-cdylib=-Wl,-z,now");
    println!("cargo:rustc-link-arg-cdylib=-Wl,--no-as-needed");
    println!("cargo:rustc-link-lib=dylib=c");
    println!("cargo:rustc-link-arg-cdylib=-Wl,--as-needed");

    // Override the cdylib's auto-generated `--version-script` (which
    // hides every non-Rust symbol — including the JNI entry points
    // the Android runtime resolves by name at load time).
    let version_script = Path::new(&out_dir).join("tuft-android-exports.ver");
    std::fs::write(
        &version_script,
        b"{\n  global:\n    Java_*;\n    JNI_OnLoad;\n    tuft_app_main;\n    tuft_tick;\n  local: *;\n};\n",
    )
    .context("writing android version-script")?;
    println!(
        "cargo:rustc-link-arg-cdylib=-Wl,--version-script={}",
        version_script.display()
    );

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
                 Run `cargo xtask ios build-lynx-frameworks` first.",
                dir.display()
            );
        }
        // `-F` for the final rustc link: lets the host app's linker
        // find `<Lynx/LynxView.h>` style framework imports when this
        // staticlib is consumed. iOS builds produce a staticlib (not
        // cdylib) so use the `-staticlib` variant to thread args
        // through the *parent* user crate's final link.
        println!("cargo:rustc-link-arg-staticlib=-F{}", dir.display());
    }

    let bridge_src = bridge_root().join("src");
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
        .file(bridge_src.join("tuft_bridge_common.cc"))
        .file(bridge_src.join("tuft_bridge_ios.mm"))
        .include(bridge_root().join("include"))
        .include(&bridge_src);
    add_lynx_includes(&mut build);
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
    // `tuft_bridge_ios.mm`.
    build.compile("tuft_bridge_static");

    // We don't emit `-l` for Lynx here. The .a we produced has UND
    // references to `LynxShell::*` etc.; the host app's final link
    // resolves them against `Lynx.xcframework` (added via SPM's
    // `binaryTarget`). Same goes for `c++` / Foundation — those come
    // from `linkerSettings` on the TuftRuntime SPM target.
    Ok(())
}
