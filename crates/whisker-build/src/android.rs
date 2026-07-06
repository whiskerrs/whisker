//! Android cargo + gradle orchestration. Shared by `whisker-cli`'s
//! the `whisker-build` binary (gradle plugin path) and `whisker-dev-server`'s full reload
//! path.
//!
//! Three phases:
//!
//! 1. [`cargo_build_dylib`] — cross-compile the user crate as a Mach-O
//!    `.so` via `cargo rustc --crate-type dylib --target <triple>`.
//!    Why `dylib` (and not `cdylib`)? rustc unconditionally injects
//!    `-Wl,--exclude-libs,ALL` for `cdylib`, which strips every
//!    mangled Rust symbol from `.dynsym`. The `dylib` flavour keeps
//!    them — `System.loadLibrary` doesn't care which flavour, but
//!    the symmetric hot-reload patch path (dev mode) does. Production
//!    builds use the same shape for consistency.
//!
//! 2. [`stage_jni_libs`] — drop the `.so` plus the matching
//!    `libc++_shared.so` from the NDK sysroot into the gen tree's
//!    `app/src/main/jniLibs/<abi>/`. The bridge is dynamically linked
//!    against `libc++_shared`; without it `System.loadLibrary` fails
//!    with `dlopen failed: cannot locate symbol _ZNSt6__ndk1…`.
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

/// NDK versions Whisker is known to link against. Preferred first.
const PREFERRED_NDKS: &[&str] = &[
    "23.1.7779620",
    "25.1.8937393",
    "26.1.10909125",
    "26.3.11579264",
    "27.0.12077973",
    "27.1.12297006",
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

/// Run `cargo rustc --crate-type dylib --target <triple>` against the
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
        b"{\n  global:\n    Java_*;\n    JNI_OnLoad;\n};\n",
    )
    .with_context(|| format!("write {}", vs_path.display()))?;

    let triple = b.toolchain.triple;
    let triple_env = triple.replace('-', "_");
    let triple_upper = triple_env.to_uppercase();

    let mut cmd = Command::new("cargo");
    cmd.arg("rustc")
        .args(["--target", triple])
        .args(["-p", b.package])
        .args(["--crate-type", "dylib"]);
    if let Some(flag) = b.profile.cargo_flag() {
        cmd.arg(flag);
    }
    for feat in b.features {
        cmd.args(["--features", feat]);
    }
    cmd.arg("--").args([
        "-C".to_string(),
        format!("link-arg=-Wl,--version-script={}", vs_path.display()),
    ]);

    cmd.env(format!("CC_{triple_env}"), &b.toolchain.clang);
    cmd.env(format!("CXX_{triple_env}"), &b.toolchain.clang_cpp);
    cmd.env(format!("AR_{triple_env}"), &b.toolchain.ar);
    let linker_env = format!("CARGO_TARGET_{triple_upper}_LINKER");
    if std::env::var_os(&linker_env).is_none() {
        cmd.env(&linker_env, &b.toolchain.clang);
    }
    cmd.env("ANDROID_NDK_HOME", &b.toolchain.ndk);
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

    // Resolve the output `.so` up-front and snapshot its mtime, so the step
    // summary can report whether cargo actually relinked a fresh lib or
    // no-op'd (`up-to-date`). This is the "did my change reach native code?"
    // signal the dev loop was missing: a stale `.so` (#260) shows as
    // `up-to-date` exactly when you expected a rebuild. mtime-only, so it's free.
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

// ----- jniLibs staging ------------------------------------------------------

/// Copy `so` plus the NDK-shipped `libc++_shared.so` into `abi_dir`.
/// Lower-level than [`stage_jni_libs`] — the caller hands in the
/// already-resolved abi leaf directory rather than the gen-android
/// root. Used by the `whisker build-android` binary path, where the
/// Gradle plugin computes the destination as
/// `<buildDir>/intermediates/whisker_jni_libs/<variant>/<abi>/` and
/// passes it in via `--jni-libs-dir`.
pub fn stage_so_files(abi_dir: &Path, so: &Path, tc: &AndroidToolchain, abi: &str) -> Result<()> {
    std::fs::create_dir_all(abi_dir).with_context(|| format!("mkdir -p {}", abi_dir.display()))?;

    let so_name = so
        .file_name()
        .ok_or_else(|| anyhow!("so path has no filename: {}", so.display()))?;
    let dst_so = abi_dir.join(so_name);
    std::fs::copy(so, &dst_so)
        .with_context(|| format!("copy {} → {}", so.display(), dst_so.display()))?;

    let libcxx = find_libcxx_shared(&tc.ndk, abi)?;
    let dst_libcxx = abi_dir.join("libc++_shared.so");
    std::fs::copy(&libcxx, &dst_libcxx)
        .with_context(|| format!("copy {} → {}", libcxx.display(), dst_libcxx.display()))?;

    crate::ui::info(format!(
        "stage jniLibs ({} + libc++_shared.so)",
        so_name.to_string_lossy(),
    ));
    Ok(())
}

/// Copy `so` plus the NDK-shipped `libc++_shared.so` into
/// `gen/android/app/src/main/jniLibs/<abi>/`. Used by the cng-driven
/// legacy non-gradle CLI path; the Gradle-plugin path goes through
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
/// `gen/android/`. Phase 7-Φ.G: replaces the previous file-copy
/// flow — each Whisker module package is now its own Android
/// library subproject with a hand-written `build.gradle.kts`. We
/// emit three files that wire those subprojects into the user
/// app's composite Gradle build:
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

    // 1. Settings include script.
    let settings_include_path = gen_android.join("whisker_modules.settings.gradle.kts");
    std::fs::write(
        &settings_include_path,
        render_module_settings_include(&android_modules),
    )
    .with_context(|| format!("write {}", settings_include_path.display()))?;

    // 2. App-level dependencies script.
    let deps_script_path = gen_android.join("whisker_module_deps.gradle.kts");
    std::fs::write(
        &deps_script_path,
        render_module_deps_script(&android_modules),
    )
    .with_context(|| format!("write {}", deps_script_path.display()))?;

    // 3. Aggregator Kotlin file. Always (re)create the directory
    // so a removed module doesn't leave behind a stale aggregator.
    let aggregator_dir =
        gen_android.join("app/src/main/whisker_generated/rs/whisker/runtime/generated");
    // Also drop the legacy staging dir so removed-Phase-F builds
    // don't leave behind stale `.kt` files that gradle would try
    // to compile.
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
         // both `@WhiskerElement` Lynx registrations and\n\
         // `@WhiskerModule` dispatch registrations.\n\n",
    );
    out.push_str("package rs.whisker.runtime.generated\n\n");
    out.push_str("import java.util.concurrent.atomic.AtomicBoolean\n\n");
    out.push_str("public object WhiskerModuleBehaviors {\n");
    out.push_str("    private val registered = AtomicBoolean(false)\n\n");
    out.push_str("    @JvmStatic\n");
    out.push_str("    public fun registerAll() {\n");
    out.push_str("        if (!registered.compareAndSet(false, true)) return\n");
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

/// Locate `libc++_shared.so` inside the NDK sysroot for `abi`. NDKs
/// place it under the host-prebuilt sysroot's lib/<triple>/ dir.
fn find_libcxx_shared(ndk: &Path, abi: &str) -> Result<PathBuf> {
    let host = host_tag()?;
    let triple = match abi {
        "arm64-v8a" => "aarch64-linux-android",
        "armeabi-v7a" => "arm-linux-androideabi",
        "x86_64" => "x86_64-linux-android",
        "x86" => "i686-linux-android",
        other => return Err(anyhow!("unknown ABI for libc++_shared lookup: {other}")),
    };
    let cand = ndk
        .join("toolchains/llvm/prebuilt")
        .join(host)
        .join("sysroot/usr/lib")
        .join(triple)
        .join("libc++_shared.so");
    if !cand.is_file() {
        return Err(anyhow!(
            "libc++_shared.so missing at {} (check NDK install)",
            cand.display(),
        ));
    }
    Ok(cand)
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
    // Pipe stdout + stderr through the spinner. Gradle's per-task
    // chatter (`> Task :app:assembleDebug`, BUILD SUCCESSFUL, …) and
    // the JVM daemon advisory block fold into the spinner message
    // instead of leaking into scrollback, mirroring the cargo build
    // path's behaviour and matching the user's expectation that
    // `whisker run` shows one summary line per subprocess.
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
