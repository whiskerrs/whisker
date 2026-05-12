// On Android, compile the lyra_bridge C++ sources straight into this
// cdylib so libhello_world.so is fully self-contained (apart from the
// dynamic deps on liblynx.so / liblynxbase.so). This avoids Android's
// RTLD_LOCAL load semantics for app-bundled .so files — without it, the
// bridge's `extern "C"` symbols aren't visible across .so boundaries.
//
// On iOS the bridge ships as part of the LyraRuntime SPM package, not
// via Cargo, so this build.rs is a no-op.

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "android" {
        return;
    }

    let repo_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let bridge = repo_root.join("native/bridge");
    let lynx = repo_root.join("target/lynx-ios/sources/Lynx");
    let lynx_base = repo_root.join("target/lynx-ios/sources/LynxBase");
    let lynx_svc = repo_root.join("target/lynx-ios/sources/LynxServiceAPI");
    let primjs = repo_root.join("target/lynx-ios/sources/PrimJS/src");

    let abi = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    // cargo-ndk sets the target arch; only arm64 is wired up for now.
    let abi_dir = match abi.as_str() {
        "aarch64" => "arm64-v8a",
        _ => panic!("hello-world Android build only supports arm64-v8a (got {abi})"),
    };
    let lynx_jni = repo_root
        .join("target/lynx-android-unpacked/jni")
        .join(abi_dir);

    // --- Compile the bridge sources -------------------------------
    let mut build = cc::Build::new();
    // Silence cc's default `cargo:rustc-link-lib=static=...` so we can
    // emit our own with `+whole-archive` (cargo refuses duplicates).
    build.cargo_metadata(false);
    build
        .cpp(true)
        .std("c++17")
        .file(bridge.join("src/lyra_bridge_common.cc"))
        .file(bridge.join("src/lyra_bridge_android.cc"))
        .include(bridge.join("include"))
        .include(bridge.join("src"))
        .include(&lynx)
        .include(&lynx_base)
        .include(&lynx_svc)
        .include(&primjs)
        .include(primjs.join("interpreter"))
        .include(primjs.join("interpreter/quickjs/include"))
        .include(primjs.join("gc"))
        .include(primjs.join("napi"))
        .include(primjs.join("napi/env"))
        .include(primjs.join("napi/quickjs"))
        .include(primjs.join("napi/jsc"))
        // Match Lynx's release build: NDEBUG=1 changes the layout of
        // base::RefCountedThreadSafeBase (two extra debug-only members
        // when NDEBUG is undefined), and we have to agree with the
        // engine's layout or FiberElement offsets shift.
        .define("NDEBUG", Some("1"));

    // Force inline LSE atomics so the C++ side never reaches for
    // compiler-rt's outline-atomics dispatcher (`__aarch64_cas*`,
    // `init_have_lse_atomics`). The Rust side gets the same treatment
    // via `.cargo/config.toml` target-feature flags.
    build.flag("-march=armv8.1-a");
    build.flag("-mno-outline-atomics");

    build.compile("lyra_bridge_static");
    // cc emits nothing (`cargo_metadata(false)`); we emit our own with
    // `+whole-archive` so the linker pulls in every .o regardless of
    // whether any Rust code references its symbols. Without this the
    // JNI exports (`JNI_OnLoad`, `Java_*`) get dead-stripped — they're
    // only "referenced" by the Android runtime at load time.
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    println!("cargo:rustc-link-search=native={}", out_dir);
    println!("cargo:rustc-link-lib=static:+whole-archive=lyra_bridge_static");

    // --- Dynamic deps onto Lynx's shipped .so files ---------------
    println!("cargo:rustc-link-search=native={}", lynx_jni.display());
    // libc++_shared.so / libc++abi.a live in the non-versioned sysroot
    // dir. Clang's driver already adds it automatically (after the
    // API-versioned dir), so we deliberately do NOT add it ourselves —
    // a cargo-provided search path is inserted ahead of clang's, and
    // the non-versioned dir contains `libc.a` but no `libc.so`. In
    // -Bdynamic mode lld accepts that `.a` instead of falling through
    // to the /24 dir's `libc.so`, dragging static bionic guts into the
    // cdylib and breaking thread_local at runtime.
    println!("cargo:rustc-link-lib=dylib=lynx");
    println!("cargo:rustc-link-lib=dylib=lynxbase");
    // Logging used by the bridge.
    println!("cargo:rustc-link-lib=dylib=log");
    // C++ runtime — std::mutex / std::function destructors etc. land
    // here. cc::Build doesn't add it automatically for static-lib
    // outputs, so do it manually. libc++_shared.so is bundled into the
    // APK alongside libhello_world.so by build-android-example.sh.
    println!("cargo:rustc-link-lib=dylib=c++_shared");

    // Deliberately DO NOT link `clang_rt.builtins-aarch64-android`. The
    // only thing we'd want from it is `__aarch64_have_lse_atomics`
    // (referenced by the outline-atomic .S.o objects compiler-builtins
    // bundles), and we provide a `= 1` definition for that ourselves in
    // `lyra_bridge_android.cc`. Linking the real archive drags
    // `cpu_model.c.o` in, which references `getauxval`, which the
    // NDK then satisfies from static libc.a — and the cascade pulls a
    // duplicate bionic libc (pthread keys, jemalloc, __libc_shared_globals)
    // into our .so, breaking Rust's `thread_local!` at runtime.

    // Disable outline-atomics on the Rust side too (see comment on the
    // matching cc flag above).
    println!("cargo:rustc-link-arg=-Wl,-z,now");
    // Force-list libc.so BEFORE `--as-needed` strips it. Without an
    // explicit dynamic reference, the NDK driver's tail `-lc` lands
    // after our static archives and (with `--as-needed`) gets dropped
    // when no PLT call has been seen yet — so `getauxval`,
    // `pthread_key_create`, etc. wind up resolved to broken static
    // libc.a copies that read `__libc_shared_globals` from a stale
    // offset. Explicitly pulling libc.so in puts it in DT_NEEDED and
    // routes everything through real bionic.
    println!("cargo:rustc-link-arg=-Wl,--no-as-needed");
    println!("cargo:rustc-link-lib=dylib=c");
    println!("cargo:rustc-link-arg=-Wl,--as-needed");
    // Tell rustc -> linker to match.
    println!("cargo:rustc-cfg=mno_outline_atomics");
    // The above is a marker; the actual codegen flag comes via RUSTFLAGS
    // or the target spec. Recent rustc target aarch64-linux-android
    // already defaults to no outline atomics in stable Rust. If you
    // see the same crash from Rust .o files, set:
    //   RUSTFLAGS="-C target-feature=-outline-atomics"

    // Rust cdylib emits its own `--version-script` that hides every
    // non-Rust symbol — including the JNI entry points (`Java_*`,
    // `JNI_OnLoad`) the Android runtime resolves by name. Override the
    // auto-script with one of our own that re-exports the bridge
    // glue. (cdylib's Rust exports still get pub'd because rustc
    // doesn't apply its filter when our script wins.)
    let version_script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("android-exports.ver");
    std::fs::write(
        &version_script,
        b"{\n  global:\n    Java_*;\n    JNI_OnLoad;\n    lyra_mobile_*;\n  local: *;\n};\n",
    ).expect("write version script");
    println!(
        "cargo:rustc-link-arg=-Wl,--version-script={}",
        version_script.display()
    );
}
