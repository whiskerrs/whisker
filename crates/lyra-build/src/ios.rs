//! Per-crate iOS compilation logic. Called from `lyra_build::compile()`
//! when `CARGO_CFG_TARGET_OS == "ios"`.

use anyhow::Result;

use crate::paths;

const LYNX_FRAMEWORKS: &[&str] = &["Lynx", "LynxBase", "LynxServiceAPI", "PrimJS"];

pub fn compile() -> Result<()> {
    // Map `TARGET` to the xcframework slice the Lynx build produces.
    let triple = std::env::var("TARGET").expect("cargo sets TARGET");
    let slice = match triple.as_str() {
        "aarch64-apple-ios" => "ios-arm64",
        "aarch64-apple-ios-sim" | "x86_64-apple-ios" => "ios-arm64_x86_64-simulator",
        other => anyhow::bail!("unsupported iOS target triple: {other}"),
    };
    let lynx_root = paths::lynx_ios_root();
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
        // staticlib is consumed.
        println!("cargo:rustc-link-arg=-F{}", dir.display());
    }

    let bridge_src = paths::bridge_src();
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
        .file(bridge_src.join("lyra_bridge_common.cc"))
        .file(bridge_src.join("lyra_bridge_ios.mm"))
        .include(paths::bridge_include())
        .include(&bridge_src);
    crate::android::add_lynx_includes(&mut build);
    // Add framework search paths to cc::Build too — the per-TU
    // compile step needs them to resolve `<Lynx/...>` imports.
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
    // `lyra_bridge_ios.mm`.
    build.compile("lyra_bridge_static");

    // We don't emit `-l` for Lynx here. The .a we produced has UND
    // references to `LynxShell::*` etc.; the host app's final link
    // resolves them against `Lynx.xcframework` (added via SPM's
    // `binaryTarget`). Same goes for `c++` / Foundation — those come
    // from `linkerSettings` on the LyraRuntime SPM target.
    Ok(())
}
