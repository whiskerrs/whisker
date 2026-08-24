//! Android cargo + gradle orchestration. Shared by `whisker-cli`, the
//! `whisker-build` binary (gradle plugin path) and
//! `whisker-dev-server`'s full reload path.
//!
//! Three phases:
//!
//! 1. [`cargo_build_dylib`] — cross-compile the user crate as an ELF
//!    `.so`. Production uses a stripped, LTO'd `cdylib`; hot reload uses a
//!    `dylib` so its Rust symbols remain available to the patcher.
//!
//! 2. [`stage_jni_libs`] — drop the self-contained Rust `.so` into the gen
//!    tree's `app/src/main/jniLibs/<abi>/`.
//!
//! 3. [`run_gradle_assemble`] — invoke `gradle :app:assemble{Release,Debug}`
//!    against the generated project. Output is `app-{release,debug}.apk`
//!    under `app/build/outputs/apk/<profile>/`.
//!
//! hot reload fat-build capture (see [`crate::capture`]) is opt-in via
//! the `capture` field on [`CargoBuild`] — dev-server's full reload
//! cold rebuild passes `Some(&shims)`; gradle-plugin and direct gradle invocations pass `None`.

use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Profile;
use crate::capture::{CaptureShims, capture_env_vars};

// ----- NDK toolchain resolution --------------------------------------------

/// NDK versions Whisker is known to link against, newest first.
///
/// Newest wins so the bare-clang toolchain follows current Android ABI and
/// page-alignment behavior. Older entries remain supported for existing
/// development machines; Whisker supplies the 16 KB linker flag itself.
const PREFERRED_NDKS: &[&str] = &[
    "27.1.12297006",
    "27.0.12077973",
    "26.3.11579264",
    "26.1.10909125",
    "25.1.8937393",
    "23.1.7779620",
];

/// Toolchain paths for a given (ABI, API level) pair.
pub struct AndroidToolchain {
    pub ndk: PathBuf,
    pub clang: PathBuf,
    pub clang_cpp: PathBuf,
    pub ar: PathBuf,
    pub triple: &'static str,
}

pub fn resolve_toolchain(abi: &str, api: u32) -> Result<AndroidToolchain> {
    let ndk = ndk_home()?;
    let host = host_tag()?;
    let bin = ndk.join("toolchains/llvm/prebuilt").join(host).join("bin");
    let clang_prefix = clang_target_prefix(abi)?;
    let clang = bin.join(format!("{clang_prefix}{api}-clang"));
    let clang_cpp = bin.join(format!("{clang_prefix}{api}-clang++"));
    let ar = bin.join("llvm-ar");
    for p in [&clang, &clang_cpp, &ar] {
        if !p.exists() {
            return Err(anyhow!(
                "expected NDK toolchain binary not found: {} \
                 (check `sdkmanager --install \"ndk;{}\"`)",
                p.display(),
                PREFERRED_NDKS[0],
            ));
        }
    }
    Ok(AndroidToolchain {
        ndk,
        clang,
        clang_cpp,
        ar,
        triple: abi_to_triple(abi)?,
    })
}

fn android_home() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("ANDROID_HOME").map(PathBuf::from) {
        if p.is_dir() {
            return Ok(p);
        }
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let cand = home.join("Library/Android/sdk");
        if cand.is_dir() {
            return Ok(cand);
        }
    }
    Err(anyhow!(
        "ANDROID_HOME not set and no SDK at $HOME/Library/Android/sdk",
    ))
}

fn ndk_home() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("ANDROID_NDK_HOME").map(PathBuf::from) {
        if p.is_dir() {
            return Ok(p);
        }
    }
    let ndk_dir = android_home()?.join("ndk");
    for v in PREFERRED_NDKS {
        let cand = ndk_dir.join(v);
        if cand.is_dir() {
            return Ok(cand);
        }
    }
    Err(anyhow!(
        "no supported NDK at {} (need one of: {})",
        ndk_dir.display(),
        PREFERRED_NDKS.join(", "),
    ))
}

fn host_tag() -> Result<&'static str> {
    if cfg!(target_os = "macos") {
        Ok("darwin-x86_64") // universal, runs on Apple Silicon too
    } else if cfg!(target_os = "linux") {
        Ok("linux-x86_64")
    } else if cfg!(target_os = "windows") {
        Ok("windows-x86_64")
    } else {
        Err(anyhow!("unsupported host OS for Android cross-compile"))
    }
}

pub fn abi_to_triple(abi: &str) -> Result<&'static str> {
    match abi {
        "arm64-v8a" => Ok("aarch64-linux-android"),
        "armeabi-v7a" => Ok("armv7-linux-androideabi"),
        "x86_64" => Ok("x86_64-linux-android"),
        "x86" => Ok("i686-linux-android"),
        other => Err(anyhow!("unknown Android ABI: {other}")),
    }
}

fn clang_target_prefix(abi: &str) -> Result<&'static str> {
    match abi {
        "arm64-v8a" => Ok("aarch64-linux-android"),
        "armeabi-v7a" => Ok("armv7a-linux-androideabi"),
        "x86_64" => Ok("x86_64-linux-android"),
        "x86" => Ok("i686-linux-android"),
        other => Err(anyhow!("unknown Android ABI: {other}")),
    }
}

// ----- cargo build ----------------------------------------------------------

pub struct CargoBuild<'a> {
    pub workspace_root: &'a Path,
    pub package: &'a str,
    pub toolchain: &'a AndroidToolchain,
    pub profile: Profile,
    /// Cargo features to forward (`--features <each>`). Empty for prod.
    pub features: &'a [String],
    /// `Some` → fold rustc/linker shim env vars into the cargo
    /// invocation, populating the hot reload capture caches. `None` →
    /// plain build. Dev-server passes `Some(&shims)` for its initial
    /// fat build and full reloads; gradle-plugin invocations pass
    /// `None` (no hot reload in prod).
    pub capture: Option<&'a CaptureShims>,
}

/// Run `cargo rustc --crate-type {cdylib,dylib} --target <triple>` against the
/// user crate. Returns the absolute path to the produced `.so`.
pub fn cargo_build_dylib(b: &CargoBuild<'_>) -> Result<PathBuf> {
    // Version-script: rustc auto-generates one that lists Rust-mangled
    // symbols in `global:` and ends with `local: *;`, which would
    // demote `Java_*` and `JNI_OnLoad` to LOCAL — `System.loadLibrary`
    // would then fail to find them. We pass a second, additive
    // version-script listing the JNI symbols; lld unions multiple
    // anonymous scripts, so JNI exports survive without touching
    // rustc's Rust-symbol list.
    let vs_dir = b.workspace_root.join("target/.whisker");
    std::fs::create_dir_all(&vs_dir).with_context(|| format!("create {}", vs_dir.display()))?;
    let vs_path = vs_dir.join("android-jni-exports.ver");
    std::fs::write(
        &vs_path,
        b"{\n  global:\n    Java_*;\n    JNI_OnLoad;\n    whisker_view_create;\n    whisker_view_tick;\n    whisker_view_destroy;\n    whisker_view_dispatch_event;\n    whisker_view_dispatch_module_event;\n    whisker_view_dispatch_resource_event;\n};\n",
    )
    .with_context(|| format!("write {}", vs_path.display()))?;

    let triple = b.toolchain.triple;
    let triple_env = triple.replace('-', "_");
    let triple_upper = triple_env.to_uppercase();

    let crate_type = if b.capture.is_some() {
        "dylib"
    } else {
        "cdylib"
    };
    let mut cmd = Command::new("cargo");
    cmd.arg("rustc")
        .args(["--target", triple])
        .args(["-p", b.package])
        .args(["--crate-type", crate_type]);
    if let Some(flag) = b.profile.cargo_flag() {
        cmd.arg(flag);
    }
    for feat in b.features {
        cmd.args(["--features", feat]);
    }
    cmd.arg("--").args([
        "-C".to_string(),
        format!("link-arg=-Wl,--version-script={}", vs_path.display()),
        // 16 KB page alignment. Android 15 runs on devices with a 16 KB
        // page size, where a library laid out for 4 KB pages fails to
        // load, and Play rejects an upload that contains one. The NDK
        // links this way by default from r27 — but only through its own
        // build systems, not through the bare clang driver cargo calls,
        // so the flag has to be explicit. Passed here rather than in
        // `RUSTFLAGS` because that reaches host build scripts too,
        // where the macOS linker rejects it outright.
        "-C".to_string(),
        "link-arg=-Wl,-z,max-page-size=16384".to_string(),
    ]);

    cmd.env(format!("CC_{triple_env}"), &b.toolchain.clang);
    cmd.env(format!("CXX_{triple_env}"), &b.toolchain.clang_cpp);
    cmd.env(format!("AR_{triple_env}"), &b.toolchain.ar);
    let linker_env = format!("CARGO_TARGET_{triple_upper}_LINKER");
    if std::env::var_os(&linker_env).is_none() {
        cmd.env(&linker_env, &b.toolchain.clang);
    }
    cmd.env("ANDROID_NDK_HOME", &b.toolchain.ndk);
    if b.profile == Profile::Release {
        configure_minimum_release_profile(&mut cmd);
    }
    cmd.current_dir(b.workspace_root);

    // hot reload capture shims (rustc-shim + linker-shim + cache dirs).
    // `CARGO_TARGET_<triple>_LINKER` set above is overridden here so
    // the linker shim wins for this triple — the shim forwards to
    // `WHISKER_REAL_LINKER` after writing its capture JSON. Host-only
    // artifacts (build scripts, proc-macros) keep their default
    // linker since the env is keyed by target triple.
    if let Some(c) = b.capture {
        std::fs::create_dir_all(&c.rustc_cache_dir)
            .with_context(|| format!("create rustc cache dir {}", c.rustc_cache_dir.display()))?;
        std::fs::create_dir_all(&c.linker_cache_dir)
            .with_context(|| format!("create linker cache dir {}", c.linker_cache_dir.display()))?;
        for (k, v) in capture_env_vars(c) {
            cmd.env(k, v);
        }
    }

    // Snapshot the output `.so`'s mtime so the step summary can say
    // whether cargo relinked or no-op'd — the signal that answers "did
    // my change reach native code?" when a stale `.so` is suspected.
    let lib_name = format!("lib{}.so", b.package.replace('-', "_"));
    let so_path = b
        .workspace_root
        .join("target")
        .join(triple)
        .join(b.profile.dir_name())
        .join(&lib_name);
    let so_mtime = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let before = so_mtime(&so_path);

    let cargo_step = crate::ui::step("compile", format!("{} ({triple})", b.package));
    let status = cargo_step
        .pipe(&mut cmd)
        .with_context(|| format!("spawn cargo for {triple}"))?;
    if !status.success() {
        cargo_step.done("failed");
        return Err(anyhow!("cargo build failed ({status}) for {triple}"));
    }
    cargo_step.done(match (before, so_mtime(&so_path)) {
        (None, Some(_)) => "linked",
        (Some(b), Some(a)) if a > b => "relinked",
        (_, Some(_)) => "up-to-date",
        (_, None) => "",
    });

    if !so_path.is_file() {
        return Err(anyhow!(
            "cargo finished but {} is missing",
            so_path.display(),
        ));
    }
    Ok(so_path)
}

fn configure_minimum_release_profile(cmd: &mut Command) {
    cmd.env("CARGO_PROFILE_RELEASE_LTO", "fat")
        .env("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "1")
        .env("CARGO_PROFILE_RELEASE_OPT_LEVEL", "z")
        .env("CARGO_PROFILE_RELEASE_STRIP", "symbols")
        .env("CARGO_PROFILE_RELEASE_PANIC", "abort");
}

// ----- jniLibs staging ------------------------------------------------------

/// Copy the retained runtime `so` into `abi_dir`.
/// Lower-level than [`stage_jni_libs`] — the caller hands in the
/// already-resolved abi leaf directory rather than the gen-android
/// root. Used by the `whisker build-android` binary path, where the
/// Gradle plugin computes the destination as
/// `<buildDir>/intermediates/whisker_jni_libs/<variant>/<abi>/` and
/// passes it in via `--jni-libs-dir`.
pub fn stage_so_files(abi_dir: &Path, so: &Path, _tc: &AndroidToolchain, _abi: &str) -> Result<()> {
    std::fs::create_dir_all(abi_dir).with_context(|| format!("mkdir -p {}", abi_dir.display()))?;

    let so_name = so
        .file_name()
        .ok_or_else(|| anyhow!("so path has no filename: {}", so.display()))?;
    let dst_so = abi_dir.join(so_name);
    std::fs::copy(so, &dst_so)
        .with_context(|| format!("copy {} → {}", so.display(), dst_so.display()))?;

    let stale_libcxx = abi_dir.join("libc++_shared.so");
    if stale_libcxx.is_file() {
        std::fs::remove_file(&stale_libcxx)
            .with_context(|| format!("remove stale {}", stale_libcxx.display()))?;
    }
    warn_if_not_16k_aligned(&dst_so);

    crate::ui::info(format!("stage jniLibs ({})", so_name.to_string_lossy()));
    Ok(())
}

/// Page size a 16 KB-page device needs every loadable segment aligned
/// to. Android 15 ships such devices, `dlopen` fails on a library laid
/// out for 4 KB pages, and Play rejects an upload containing one.
const REQUIRED_PAGE_ALIGN: u64 = 16 * 1024;

/// Say so when a staged library would not load on a 16 KB-page device.
///
/// A warning rather than an error: a machine whose newest NDK predates
/// r27 can still build and run on 4 KB devices, and failing there would
/// block development outright. The message names the library, because
/// the two this stages come from different places — one is linked here,
/// the other copied out of the NDK.
fn warn_if_not_16k_aligned(so: &Path) {
    let Some(align) = max_load_align(so) else {
        return;
    };
    if align >= REQUIRED_PAGE_ALIGN {
        return;
    }
    crate::ui::warn(format!(
        "{} is aligned to {} bytes, not {REQUIRED_PAGE_ALIGN} — it will not load on a \
         16 KB-page device, and Play rejects uploads containing it. Install NDK r27 or \
         newer (`sdkmanager 'ndk;{}'`).",
        so.file_name().unwrap_or(so.as_os_str()).to_string_lossy(),
        align,
        PREFERRED_NDKS[0],
    ));
}

/// The largest `p_align` across an ELF64 shared object's loadable
/// segments, or `None` when the file isn't one this can read.
///
/// Hand-parsed rather than pulled from a crate: the two fields needed
/// sit at fixed offsets, and a build tool that only ever reads its own
/// freshly linked output does not need a general ELF reader.
fn max_load_align(so: &Path) -> Option<u64> {
    const PT_LOAD: u32 = 1;
    const E_PHOFF: usize = 0x20;
    const E_PHENTSIZE: usize = 0x36;
    const E_PHNUM: usize = 0x38;
    const P_ALIGN: usize = 0x30;

    let bytes = std::fs::read(so).ok()?;
    // ELF64, little-endian — every Android ABI Whisker targets.
    if bytes.get(..5)? != b"\x7fELF\x02" || bytes.get(5)? != &1u8 {
        return None;
    }
    let u16_at = |off: usize| -> Option<usize> {
        Some(u16::from_le_bytes(bytes.get(off..off + 2)?.try_into().ok()?) as usize)
    };
    let u32_at = |off: usize| -> Option<u32> {
        Some(u32::from_le_bytes(
            bytes.get(off..off + 4)?.try_into().ok()?,
        ))
    };
    let u64_at = |off: usize| -> Option<u64> {
        Some(u64::from_le_bytes(
            bytes.get(off..off + 8)?.try_into().ok()?,
        ))
    };

    let phoff = u64_at(E_PHOFF)? as usize;
    let phentsize = u16_at(E_PHENTSIZE)?;
    let phnum = u16_at(E_PHNUM)?;
    (0..phnum)
        .filter_map(|i| {
            let ph = phoff.checked_add(i.checked_mul(phentsize)?)?;
            (u32_at(ph)? == PT_LOAD).then(|| u64_at(ph + P_ALIGN))?
        })
        .max()
}

/// Copy `so` into `gen/android/app/src/main/jniLibs/<abi>/`. The
/// Gradle-plugin path goes through
/// [`stage_so_files`] directly.
pub fn stage_jni_libs(
    gen_android: &Path,
    abi: &str,
    so: &Path,
    tc: &AndroidToolchain,
) -> Result<()> {
    let dst_dir = gen_android.join("app/src/main/jniLibs").join(abi);
    stage_so_files(&dst_dir, so, tc, abi)
}

/// Generate the per-app Gradle module-aggregator artefacts under
/// `gen/android/`. Each Whisker module package is its own Android
/// library subproject with a hand-written `build.gradle.kts`; three
/// emitted files wire those subprojects into the user app's composite
/// Gradle build:
///
/// 1. `whisker_modules.settings.gradle.kts` — `include(":<crate>")` +
///    `project(...).projectDir = file("...")` calls. Applied by the
///    cng-generated `settings.gradle.kts` via `apply(from = ...)`.
///
/// 2. `whisker_module_deps.gradle.kts` —
///    `dependencies { implementation(project(":<crate>")) }`. Applied
///    by the cng-generated `app/build.gradle.kts` so the user app
///    picks up each module's library AAR.
///
/// 3. `app/src/main/whisker_generated/.../WhiskerModuleBehaviors.kt`
///    — the aggregator object whose `registerAll()` imports each
///    subproject's per-module `<ModuleName>Behaviors` object and calls
///    its `registerAll()`. The aggregator's FQN matches what the user
///    app's `Application.onCreate()` already invokes, so the
///    user-facing surface is unchanged.
///
/// Built-in element implementations are checked into the Android SDK. The
/// generated aggregator only calls their ordinary module registration path.
///
/// Each module's KSP plugin emits its own `<ModuleName>Behaviors`
/// object into its subproject's generated-source set; the
/// aggregator stitches them together. Discovery signal:
/// presence of a `build.gradle.kts` at the module's package root.
/// The build script points its Kotlin source set at the package's
/// `android/` directory (Expo-style layout — native code lives in
/// `android/` / `ios/`, manifests stay at the package root).
pub fn stage_module_kotlin_sources(
    gen_android: &Path,
    modules: &[crate::modules::ResolvedModule],
) -> Result<()> {
    let android_modules: Vec<&crate::modules::ResolvedModule> = modules
        .iter()
        .filter(|m| m.manifest_dir.join("build.gradle.kts").is_file())
        .collect();

    let settings_include_path = gen_android.join("whisker_modules.settings.gradle.kts");
    std::fs::write(
        &settings_include_path,
        render_module_settings_include(&android_modules),
    )
    .with_context(|| format!("write {}", settings_include_path.display()))?;

    let deps_script_path = gen_android.join("whisker_module_deps.gradle.kts");
    std::fs::write(
        &deps_script_path,
        render_module_deps_script(&android_modules),
    )
    .with_context(|| format!("write {}", deps_script_path.display()))?;

    // Both directories are wiped and recreated so a removed module
    // can't leave a stale aggregator or `.kt` file behind for gradle
    // to compile.
    let aggregator_dir =
        gen_android.join("app/src/main/whisker_generated/rs/whisker/runtime/generated");
    let legacy_staging = gen_android.join("app/src/main/whisker_modules");
    if legacy_staging.exists() {
        std::fs::remove_dir_all(&legacy_staging)
            .with_context(|| format!("rm -rf {}", legacy_staging.display()))?;
    }
    if aggregator_dir.exists() {
        std::fs::remove_dir_all(&aggregator_dir)
            .with_context(|| format!("rm -rf {}", aggregator_dir.display()))?;
    }
    std::fs::create_dir_all(&aggregator_dir)
        .with_context(|| format!("mkdir -p {}", aggregator_dir.display()))?;
    let aggregator_path = aggregator_dir.join("WhiskerModuleBehaviors.kt");
    std::fs::write(&aggregator_path, render_aggregator_kt(&android_modules))
        .with_context(|| format!("write {}", aggregator_path.display()))?;
    if !android_modules.is_empty() {
        crate::ui::info(format!(
            "wire {n} module gradle subproject(s) into the app build",
            n = android_modules.len()
        ));
    }
    Ok(())
}

fn render_module_settings_include(modules: &[&crate::modules::ResolvedModule]) -> String {
    let mut out = String::new();
    out.push_str(
        "// AUTO-GENERATED by whisker-build. Do NOT edit — re-run\n\
         // `whisker run` to refresh.\n\
         //\n\
         // `apply(from = ...)`'d by the cng-generated\n\
         // settings.gradle.kts. Each `include` + `projectDir` pair\n\
         // wires a Whisker module package into the user app's\n\
         // composite Gradle build as a normal subproject.\n\n",
    );
    if modules.is_empty() {
        out.push_str("// (no Whisker module deps)\n");
        return out;
    }
    for m in modules {
        // The Gradle library subproject is rooted at the package
        // directory (build.gradle.kts lives there); its Kotlin
        // source set points at the package's `android/` subdir.
        let path = m.manifest_dir.display().to_string();
        out.push_str(&format!("include(\":{name}\")\n", name = m.package));
        out.push_str(&format!(
            "project(\":{name}\").projectDir = file({path:?})\n",
            name = m.package
        ));
    }
    out
}

fn render_module_deps_script(modules: &[&crate::modules::ResolvedModule]) -> String {
    let mut out = String::new();
    out.push_str(
        "// AUTO-GENERATED by whisker-build. Do NOT edit — re-run\n\
         // `whisker run` to refresh.\n\
         //\n\
         // `apply(from = ...)`'d by the cng-generated\n\
         // app/build.gradle.kts. Adds an `implementation(project(...))`\n\
         // entry for every Whisker module subproject so the user\n\
         // app links against their AARs.\n\n",
    );
    if modules.is_empty() {
        out.push_str("// (no Whisker module deps)\n");
        return out;
    }
    out.push_str("dependencies {\n");
    for m in modules {
        out.push_str(&format!(
            "    \"implementation\"(project(\":{name}\"))\n",
            name = m.package
        ));
    }
    out.push_str("}\n");
    out
}

fn render_aggregator_kt(modules: &[&crate::modules::ResolvedModule]) -> String {
    let mut out = String::new();
    out.push_str(
        "// AUTO-GENERATED by whisker-build. Do NOT edit — re-run\n\
         // `whisker run` to refresh.\n\
         //\n\
         // Aggregates every Whisker module subproject's KSP-\n\
         // generated `<ModuleName>Behaviors` object into a single\n\
         // `rs.whisker.runtime.generated.WhiskerModuleBehaviors`\n\
         // entry point. The user app's `WhiskerApplication.onCreate()`\n\
         // (generated from the cng `Application.kt` template) calls\n\
         // `registerAll()` once at launch — that fans out to each\n\
         // subproject's per-module behaviors, which themselves wire\n\
         // both native element and function-module registrations.\n\n",
    );
    out.push_str("package rs.whisker.runtime.generated\n\n");
    out.push_str("import rs.whisker.runtime.BuiltInElementModule\n");
    out.push_str("import rs.whisker.runtime.registerWithWhisker\n");
    out.push_str("import java.util.concurrent.atomic.AtomicBoolean\n\n");
    out.push_str("public object WhiskerModuleBehaviors {\n");
    out.push_str("    private val registered = AtomicBoolean(false)\n\n");
    out.push_str("    @JvmStatic\n");
    out.push_str("    public fun registerAll() {\n");
    out.push_str("        if (!registered.compareAndSet(false, true)) return\n");
    out.push_str("        BuiltInElementModule().registerWithWhisker()\n");
    if modules.is_empty() {
        out.push_str("        // (no Whisker module deps)\n");
    }
    for m in modules {
        let obj = crate::modules::crate_to_behaviors_class(&m.package);
        out.push_str(&format!("        {obj}.registerAll()\n"));
    }
    out.push_str("    }\n");
    out.push_str("}\n");
    out
}

// ----- gradle ---------------------------------------------------------------

/// Invoke `./gradlew :app:assemble{Release,Debug}` on the gen tree.
/// Returns the path to the produced APK.
///
/// `features` is forwarded to the gradle plugin's `WhiskerBuildTask`
/// via the `WHISKER_FEATURES` env var (space-separated). The Kotlin
/// task splits it back into `--features <feat>` args on every
/// `whisker build-android` invocation so the resulting `.so` carries
/// the dev-runtime WebSocket client when `whisker run` asks for
/// `whisker/hot-reload`. Empty list → env stays unset and the gradle
/// plugin builds the release-shaped `.so` it always has.
///
/// `capture` is forwarded to the gradle subprocess as the same env
/// envelope `cargo_build_dylib` would apply directly. The env vars
/// inherit naturally to the gradle plugin's `whisker build-android`
/// subprocess and then to cargo, so the gradle-built `.so` picks up
/// the same `-Csave-temps` / `-Cdebug-assertions=on` / `--export-dynamic`
/// flags. Without this the gradle-built `.so` lacks `--export-dynamic`
/// and the patch dylib dlopen fails with `cannot locate symbol` for any
/// inter-crate reference (`whisker_audio::runtime::NEXT_ID` in practice).
pub fn run_gradle_assemble(
    gen_android: &Path,
    profile: Profile,
    features: &[String],
    capture: Option<&CaptureShims>,
) -> Result<PathBuf> {
    let task = match profile {
        Profile::Release => ":app:assembleRelease",
        Profile::Debug => ":app:assembleDebug",
    };
    let gradle_step = crate::ui::step("gradle", task.to_string());
    let mut cmd = gradle_command(gen_android, task)?;
    if !features.is_empty() {
        cmd.env("WHISKER_FEATURES", features.join(" "));
    }
    if let Some(c) = capture {
        for (k, v) in capture_env_vars(c) {
            cmd.env(k, v);
        }
    }
    // Piping folds gradle's per-task chatter and the JVM daemon
    // advisory block into the spinner instead of scrollback.
    let status = gradle_step
        .pipe(&mut cmd)
        .with_context(|| format!("spawn {}", gen_android.join("gradlew").display()))?;
    if !status.success() {
        gradle_step.fail(format!("{status}"));
        return Err(anyhow!("gradle {task} failed ({status})"));
    }
    gradle_step.done("");
    let kind = profile.dir_name();
    // Release APKs are unsigned by default; sniff both filenames so the
    // function works whether the user has wired up a signingConfig.
    let outputs = gen_android.join(format!("app/build/outputs/apk/{kind}"));
    for name in [
        format!("app-{kind}.apk"),
        format!("app-{kind}-unsigned.apk"),
    ] {
        let cand = outputs.join(&name);
        if cand.is_file() {
            return Ok(cand);
        }
    }
    Err(anyhow!(
        "gradle succeeded but no APK found under {}",
        outputs.display(),
    ))
}

/// Shared skeleton for one `./gradlew <task>` invocation against a
/// synced `gen/android/` tree: JDK resolution, gradlew existence,
/// plain-console piping, and TUI/verbose env forwarding.
///
/// `--console=plain` forces gradle to emit line-by-line output
/// instead of its default `auto` heuristic, which on a TTY upgrades
/// to ANSI-escape-driven in-place progress redraws. We pipe gradle
/// through `Step::pipe` so the ANSI codes never reach a real
/// terminal — but our line-based classifier doesn't know how to
/// strip them, and the curated TUI's inline viewport gets corrupted
/// by cursor-moving sequences leaking through. Plain console mode
/// side-steps both.
///
/// The TUI/verbose env vars are set on the outermost gradle
/// invocation (not relied on via inheritance) because the gradle
/// Plugin sits behind a published Maven artifact — older plugin
/// versions whose `exec {}` block doesn't explicitly forward env
/// names won't propagate them to grandchild processes on every
/// gradle version.
fn gradle_command(gen_android: &Path, task: &str) -> Result<Command> {
    let java_home = resolve_java_home()?;
    let gradlew = gen_android.join("gradlew");
    if !gradlew.is_file() {
        return Err(anyhow!(
            "gradlew missing at {} — has the gen tree been synced?",
            gradlew.display(),
        ));
    }
    let mut cmd = Command::new(&gradlew);
    cmd.arg(task)
        .arg("--no-daemon")
        .arg("--console=plain")
        .current_dir(gen_android)
        .env("JAVA_HOME", &java_home);
    if crate::ui::is_tui() {
        cmd.env("WHISKER_TUI", "1");
    }
    if crate::ui::is_verbose() {
        cmd.env("WHISKER_VERBOSE", "1");
    }
    Ok(cmd)
}

/// Release artifact kinds `whisker build` produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseArtifact {
    /// `.aab` for Play Store upload (`:app:bundleRelease`).
    AppBundle,
    /// `.apk` for direct / internal distribution
    /// (`:app:assembleRelease`).
    Apk,
}

/// Run the release gradle task for `artifact`, with signing material
/// injected through `signing_env` (the `WHISKER_ANDROID_*` variables
/// the generated `app/build.gradle.kts` reads — see the template).
/// The values point into a credential staging dir that outlives this
/// call and vanishes after the build; nothing is written to the gen
/// tree. Returns the produced artifact path.
pub fn run_gradle_release(
    gen_android: &Path,
    artifact: ReleaseArtifact,
    signing_env: &[(String, String)],
) -> Result<PathBuf> {
    let (task, out_dir, candidates): (&str, &str, &[&str]) = match artifact {
        ReleaseArtifact::AppBundle => (
            ":app:bundleRelease",
            "app/build/outputs/bundle/release",
            &["app-release.aab"],
        ),
        ReleaseArtifact::Apk => (
            ":app:assembleRelease",
            "app/build/outputs/apk/release",
            // `-unsigned` shows up when signing env was absent; keep
            // it discoverable so the error path can name what it found.
            &["app-release.apk", "app-release-unsigned.apk"],
        ),
    };
    let gradle_step = crate::ui::step("gradle", task.to_string());
    let mut cmd = gradle_command(gen_android, task)?;
    for (k, v) in signing_env {
        cmd.env(k, v);
    }
    let status = gradle_step
        .pipe(&mut cmd)
        .with_context(|| format!("spawn {}", gen_android.join("gradlew").display()))?;
    if !status.success() {
        gradle_step.fail(format!("{status}"));
        return Err(anyhow!("gradle {task} failed ({status})"));
    }
    gradle_step.done("");
    let outputs = gen_android.join(out_dir);
    let found = candidates
        .iter()
        .map(|name| outputs.join(name))
        .find(|cand| cand.is_file())
        .ok_or_else(|| {
            anyhow!(
                "gradle succeeded but no release artifact found under {}",
                outputs.display(),
            )
        })?;
    ensure_release_artifact_signed(artifact, &found)?;
    Ok(found)
}

/// Refuse to hand back an UNSIGNED release artifact — Android
/// rejects it at install time with an opaque "app can't be
/// installed" dialog, so failing here with the real cause is
/// strictly better. Unsigned output means the generated gradle
/// script never saw the `WHISKER_ANDROID_*` signingConfig (a gen
/// tree from before signing support, or a bypassed sync).
///
/// Detection is per-format: an unsigned APK announces itself via the
/// `-unsigned` filename AGP gives it; an unsigned AAB keeps the same
/// filename, so it's checked with `jarsigner -verify` (bundles are
/// v1/jar-signed — unlike APKs, whose v2+ signatures jarsigner can't
/// see).
fn ensure_release_artifact_signed(artifact: ReleaseArtifact, path: &Path) -> Result<()> {
    let unsigned = match artifact {
        ReleaseArtifact::Apk => path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().contains("-unsigned")),
        ReleaseArtifact::AppBundle => {
            let jarsigner = resolve_java_home()?.join("bin/jarsigner");
            let out = Command::new(&jarsigner)
                .arg("-verify")
                .arg(path)
                .output()
                .with_context(|| format!("spawn {}", jarsigner.display()))?;
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr),
            );
            text.contains("jar is unsigned")
        }
    };
    if unsigned {
        return Err(anyhow!(
            "{} is UNSIGNED — the generated gradle project predates release-signing \
             support. Re-run the build (the gen tree regenerates automatically); if \
             this persists, delete gen/android/ and try again.",
            path.display(),
        ));
    }
    Ok(())
}

/// Java 17 home for AGP 8.x. Looks at JAVA_HOME first; otherwise tries
/// `/usr/libexec/java_home -v 17` on macOS.
///
/// Public because the CLI's `whisker credential android` reuses it to
/// locate `keytool` (`<java_home>/bin/keytool`) for upload-keystore
/// generation — same JDK the gradle build will run under.
pub fn resolve_java_home() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("JAVA_HOME").map(PathBuf::from) {
        if p.is_dir() {
            return Ok(p);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let out = Command::new("/usr/libexec/java_home")
            .args(["-v", "17"])
            .output()
            .context("spawn /usr/libexec/java_home -v 17")?;
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let p = PathBuf::from(&path);
            if p.is_dir() {
                return Ok(p);
            }
        }
    }
    Err(anyhow!(
        "JAVA_HOME unset and could not auto-detect a Java 17 JDK",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_entrypoint_registers_the_built_in_element_module() {
        let source = render_aggregator_kt(&[]);
        assert!(source.contains("import rs.whisker.runtime.BuiltInElementModule"));
        assert!(source.contains("BuiltInElementModule().registerWithWhisker()"));
        assert!(!source.contains("BuiltInElementBindings"));
    }

    #[test]
    fn a_file_that_is_not_an_elf_reads_as_unknown() {
        let path = std::env::temp_dir().join("whisker-build-not-an-elf");
        std::fs::write(&path, b"MZ not an elf at all").unwrap();
        assert_eq!(max_load_align(&path), None);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn unsigned_apk_is_rejected() {
        let tmp = std::env::temp_dir().join("whisker-build-unsigned-apk-test");
        let _ = std::fs::create_dir_all(&tmp);
        let unsigned = tmp.join("app-release-unsigned.apk");
        std::fs::write(&unsigned, b"zip").unwrap();
        let err = ensure_release_artifact_signed(ReleaseArtifact::Apk, &unsigned).unwrap_err();
        assert!(err.to_string().contains("UNSIGNED"), "got: {err:#}");

        // A properly named release APK passes the filename check.
        let signed = tmp.join("app-release.apk");
        std::fs::write(&signed, b"zip").unwrap();
        assert!(ensure_release_artifact_signed(ReleaseArtifact::Apk, &signed).is_ok());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn abi_to_triple_maps_known_abis() {
        assert_eq!(abi_to_triple("arm64-v8a").unwrap(), "aarch64-linux-android");
        assert_eq!(abi_to_triple("x86_64").unwrap(), "x86_64-linux-android");
        assert!(abi_to_triple("bogus").is_err());
    }

    #[test]
    fn clang_target_prefix_uses_armv7a_for_armeabi() {
        assert_eq!(
            clang_target_prefix("armeabi-v7a").unwrap(),
            "armv7a-linux-androideabi",
        );
        // arm64 prefix matches the rust triple.
        assert_eq!(
            clang_target_prefix("arm64-v8a").unwrap(),
            "aarch64-linux-android",
        );
    }
}
