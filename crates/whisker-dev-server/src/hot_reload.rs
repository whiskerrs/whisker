use super::*;

pub(super) fn ensure_patcher(
    config: &Config,
    hot_reload_init: &Option<HotReloadPrep>,
    patcher: &mut Option<hotpatch::Patcher>,
) {
    if patcher.is_some() {
        return;
    }
    let Some(prep) = hot_reload_init.as_ref() else {
        return;
    };
    match init_patcher_for(config, prep) {
        Ok(p) => {
            whisker_build::ui::info("hot-reload patcher ready (recovered)");
            *patcher = Some(p);
        }
        Err(e) => {
            whisker_build::ui::debug(format!("hot-reload patcher init retry failed: {e:#}"));
        }
    }
}

/// One hot-reload attempt: build a patch for `crate_key` (`None` =
/// the user crate) and broadcast it to connected clients. Never
/// falls back to a rebuild — a compile error in the user's code
/// waits for the next save, and every infrastructure failure prompts
/// for an explicit Full Reload.
pub(super) async fn hot_reload_cycle(
    patcher: &hotpatch::Patcher,
    sender: &PatchSender,
    on_event: &Option<Arc<dyn Fn(Event) + Send + Sync>>,
    crate_key: Option<&str>,
) {
    // Opened before the build so the spinner spans the whole
    // "edit → app updated" duration.
    let step = whisker_build::ui::step("hot reload", "subsecond patch");
    // Emitted before the wall-clock-heavy patcher work; `PatchSent`
    // flips the cli back to Idle.
    emit(on_event, Event::PatchBuilding);
    let Some(aslr_reference) = sender.latest_aslr_reference() else {
        // Without an `aslr_reference` from a client handshake there
        // is no slide to build the stub trampolines against.
        step.fail("no connected device yet");
        prompt_full_reload(
            on_event,
            "no device connected — a Full Reload builds, installs and launches the app",
        );
        return;
    };
    let started = std::time::Instant::now();
    match patcher.build_patch(aslr_reference, crate_key).await {
        Ok(plan) => {
            let built_in = started.elapsed();
            log_patch_diff(&plan.report);
            let dylib_bytes = match read_lib_bytes(&plan.table.lib) {
                Ok(b) => Arc::new(b),
                Err(e) => {
                    step.fail(format!(
                        "could not read patch dylib ({}): {e:#}",
                        plan.table.lib.display(),
                    ));
                    prompt_full_reload(on_event, "hot reload failed (see log above)");
                    return;
                }
            };
            let patch_mib = dylib_bytes.len() as f64 / (1024.0 * 1024.0);
            let send_started = std::time::Instant::now();
            let n = sender.send(Patch {
                table: plan.table,
                dylib_bytes,
            });
            whisker_build::ui::debug(format!(
                "built {built_in:?} · queued {:?}",
                send_started.elapsed()
            ));
            step.done(format!("{n} client(s) · {patch_mib:.1} MiB patch"));
            emit(on_event, Event::PatchSent);
        }
        Err(e) if e.downcast_ref::<hotpatch::RustcRejectedCode>().is_some() => {
            // A Full Reload would fail with the same diagnostics
            // after a 10-30s wait, so don't prompt for one — rustc
            // already printed them on the inherited stderr.
            let msg = "compile error — fix the code and save again";
            step.fail(msg);
            emit(on_event, Event::BuildFailed(msg.to_string()));
        }
        Err(e) => {
            step.fail(format!("{e:#}"));
            prompt_full_reload(on_event, "hot reload failed (see log above)");
        }
    }
}

/// Log added / removed symbols from a hot-reload patch diff. Quiet when both
/// lists are empty (the common case) so the dev terminal stays
/// readable; loud when something interesting happens (`pub fn`
/// added or removed) so the user notices.
pub(super) fn log_patch_diff(report: &hotpatch::DiffReport) {
    if report.added.is_empty() && report.removed.is_empty() {
        return;
    }
    if !report.added.is_empty() {
        whisker_build::ui::debug(format!(
            "patch added {} symbol(s): {:?}",
            report.added.len(),
            report.added.iter().take(5).collect::<Vec<_>>(),
        ));
    }
    if !report.removed.is_empty() {
        // Verbose-only: a normal patch drops thousands of
        // `GCC_except_table*` entries, which means nothing is wrong.
        whisker_build::ui::debug(format!(
            "patch removed {} symbol(s): {:?}",
            report.removed.len(),
            report.removed.iter().take(5).collect::<Vec<_>>(),
        ));
    }
}

/// State produced by [`prepare_hot_reload_capture`]: enough to make the
/// initial build a fat build, and to construct the patcher after the
/// build completes.
#[derive(Debug, Clone)]
pub(super) struct HotReloadPrep {
    pub(super) capture: CaptureShims,
    real_linker: PathBuf,
}

/// Resolve shim paths (building them if missing) and assemble the
/// CaptureShims wiring. Returns `Err` if the shim binaries can't be
/// produced, in which case hot reload is unavailable for the session.
///
/// `config` carries the workspace + target + android/ios params the
/// linker/triple pickers need:
///   - Android → NDK clang for `config.android.abi`.
///   - others → host clang via [`hotpatch::wrapper::resolve_linker`].
pub(super) fn prepare_hot_reload_capture(config: &Config) -> Result<HotReloadPrep> {
    let shims = hotpatch::resolve_shim_paths(&config.workspace_root)?;
    let rustc_cache_dir = hotpatch::wrapper::default_cache_dir(&config.workspace_root);
    let linker_cache_dir = hotpatch::wrapper::default_linker_cache_dir(&config.workspace_root);
    let real_linker = resolve_linker_for(config)?;
    let target_triple = target_triple_for(config);
    Ok(HotReloadPrep {
        capture: CaptureShims {
            rustc_shim: shims.rustc_shim,
            linker_shim: shims.linker_shim,
            rustc_cache_dir,
            linker_cache_dir,
            real_linker: real_linker.clone(),
            target_triple,
        },
        real_linker,
    })
}

/// What Rust target triple `config.target` compiles for. Android
/// derives the triple from `Config::android.abi`. Host returns
/// `None`, falling back to the global RUSTFLAGS form.
pub(super) fn target_triple_for(config: &Config) -> Option<String> {
    match config.target {
        Target::Android => {
            let abi = config.android.as_ref().map(|a| a.abi.as_str())?;
            let triple = match abi {
                "arm64-v8a" => "aarch64-linux-android",
                "armeabi-v7a" => "armv7-linux-androideabi",
                "x86_64" => "x86_64-linux-android",
                "x86" => "i686-linux-android",
                _ => return None,
            };
            Some(triple.to_string())
        }
        Target::IosSimulator => {
            // The simulator triple has to match the host arch:
            // `aarch64-apple-ios-sim` on Apple silicon,
            // `x86_64-apple-ios` on Intel.
            let triple = match std::env::consts::ARCH {
                "aarch64" => "aarch64-apple-ios-sim",
                "x86_64" => "x86_64-apple-ios",
                _ => return None,
            };
            Some(triple.to_string())
        }
        Target::Macos => None,
        Target::Web => Some("wasm32-unknown-unknown".to_string()),
    }
}

/// Pick the linker driver to use for `config.target`. Returned path
/// is what the linker shim forwards to during the fat build *and*
/// what the thin-rebuild link step spawns directly — the same binary
/// on both sides keeps SDK / sysroot resolution consistent.
pub(super) fn resolve_linker_for(config: &Config) -> Result<PathBuf> {
    match config.target {
        Target::Android => {
            let abi = config
                .android
                .as_ref()
                .map(|a| a.abi.as_str())
                .unwrap_or("arm64-v8a");
            // API level: environment override, then the development fallback.
            let api = std::env::var("WHISKER_ANDROID_API")
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(21);
            hotpatch::android_ndk::android_clang_for(abi, api)
                .with_context(|| format!("resolve NDK clang for ABI {abi} API {api}"))
        }
        Target::IosSimulator => Ok(hotpatch::wrapper::resolve_linker()),
        Target::Macos => Ok(hotpatch::wrapper::resolve_linker()),
        Target::Web => anyhow::bail!("Web builds are owned by the generated Trunk project"),
    }
}

/// Construct the patcher from the captures the fat build just wrote.
/// Splits out so [`DevServer::run`] is easier to read.
pub(super) fn init_patcher_for(config: &Config, prep: &HotReloadPrep) -> Result<hotpatch::Patcher> {
    let original_binary = original_binary_path(config)?;
    hotpatch::Patcher::initialize(
        &config.workspace_root,
        config.package.clone(),
        &prep.capture.rustc_cache_dir,
        &prep.capture.linker_cache_dir,
        &prep.real_linker,
        &original_binary,
        target_os_for(config.target),
        prep.capture.target_triple.as_deref(),
    )
}

/// Locate the device-loadable original binary for the configured
/// target. Both [`Target::Android`] and [`Target::IosSimulator`]
/// produce a `.so` / `.dylib` we can mmap and diff against; reads
/// the paths from `Config::android` / `Config::ios` rather than
/// guessing — the cli populates these from the user's
/// `whisker.rs::configure` output.
pub(super) fn original_binary_path(config: &Config) -> Result<PathBuf> {
    let crate_underscored = config.package.replace('-', "_");
    match config.target {
        Target::Android => {
            // Read from the *gradle plugin's* output directory rather
            // than from `<workspace>/target/<triple>/debug/`. Why:
            // gradle's `WhiskerBuildTask` declares its `jniLibsDir`
            // as an `@OutputDirectory` but the cargo target dir as
            // `@Internal` (see
            // `platforms/android/gradle-plugin/whisker-gradle-plugin/
            // src/main/kotlin/rs/whisker/gradle/WhiskerBuildTask.kt`),
            // which means gradle treats the jniLibs path as the
            // ground-truth output it must guarantee, but happily
            // skips the task when only the cargo target dir is
            // missing. If the user runs `cargo clean` (or anything
            // that nukes `target/<triple>/debug/`) between sessions
            // gradle still reports UP-TO-DATE and the dev-server
            // sees nothing under the workspace's target dir.
            //
            // Stage location: `whisker_build::android::stage_so_files`
            // copies the freshly-built `.so` into the abi subdir of
            // gradle's `@OutputDirectory`. The directory layout is
            // `gen/android/app/build/generated/jniLibs/
            //  whiskerBuild<Variant><AbiCamel>/<abi>/lib<pkg>.so`,
            // where `<AbiCamel>` is the abi name with each `-`/`_`
            // segment titlecased (`arm64-v8a` → `Arm64V8a`,
            // `x86_64` → `X8664`) and `<Variant>` is the AGP build
            // type ("Debug" for the dev loop).
            let android = config.android.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "target=Android but Config.android is None — cli should have populated it from whisker.rs"
                )
            })?;
            let so_name = format!("lib{crate_underscored}.so");
            let abi_camel = android_abi_to_camel(&android.abi);
            let candidate = config
                .crate_dir
                .join("gen/android/app/build/generated/jniLibs")
                .join(format!("whiskerBuildDebug{abi_camel}"))
                .join(&android.abi)
                .join(&so_name);
            if !candidate.is_file() {
                anyhow::bail!(
                    "no Android cdylib at {} — gradle's whiskerBuildDebug{abi_camel} task didn't produce its output (run `whisker run android` first)",
                    candidate.display(),
                );
            }
            Ok(candidate)
        }
        Target::IosSimulator => {
            // Use the single-arch dylib that cargo dropped directly,
            // not the lipo'd fat binary inside the xcframework. The
            // `object` crate doesn't auto-resolve Mach-O FAT_MAGIC
            // (it requires the caller to pick a slice first via
            // `MachOFatFile`), and the static symbol layout of each
            // slice is byte-identical to the single-arch input —
            // lipo just prepends a fat header.
            //
            // Match the host arch so the slice we read corresponds
            // to what the Simulator actually loads at runtime (the
            // arm64 Mac runs the arm64-sim slice natively; Intel
            // Macs run the x86_64-sim slice).
            let _ios = config.ios.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "target=IosSimulator but Config.ios is None — cli should have populated it from whisker.rs"
                )
            })?;
            let dylib_name = format!("lib{crate_underscored}.dylib");
            let triple = match std::env::consts::ARCH {
                "aarch64" => "aarch64-apple-ios-sim",
                "x86_64" => "x86_64-apple-ios",
                arch => anyhow::bail!("unsupported host arch {arch} for iOS Simulator target"),
            };
            // xcodebuild's Build Phase Run Script (`whisker-build
            // ios`) invokes cargo with `--release` (see
            // `crates/whisker-build/src/ios.rs::cargo_build_ios_dylib`:
            // the comment there spells out that iOS dev wants the
            // same optimised codegen prod ships, so debug profile is
            // deliberately not used). Android uses Debug; the two
            // platforms can't share this path.
            let dylib = config
                .workspace_root
                .join("target")
                .join(triple)
                .join("release")
                .join(&dylib_name);
            if !dylib.is_file() {
                anyhow::bail!(
                    "no iOS Simulator dylib at {} — \
                     initial xcodebuild didn't drop the artifact where the dev loop expects it",
                    dylib.display(),
                );
            }
            Ok(dylib)
        }
        Target::Macos => anyhow::bail!(
            "macOS desktop hot-patch capture is not wired yet; use the automatic rebuild/relaunch loop"
        ),
        Target::Web => anyhow::bail!(
            "Web hot-patch capture is not used; Trunk reloads and remounts the WASM application"
        ),
    }
}

/// Map an Android ABI name to the camel-cased form gradle's
/// `WhiskerProjectPlugin` uses when synthesising
/// `whiskerBuild<Variant><AbiCamel>` task names. Each `-` or `_`
/// segment is titlecased and the parts are concatenated:
/// `arm64-v8a` → `Arm64V8a`, `armeabi-v7a` → `ArmeabiV7a`,
/// `x86_64` → `X8664`, `x86` → `X86`. Mirrors `String.toCamelCase()`
/// in `WhiskerProjectPlugin.kt`.
pub(super) fn android_abi_to_camel(abi: &str) -> String {
    abi.split(['-', '_'])
        .map(|seg| {
            let mut chars = seg.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect()
}

pub(super) fn target_os_for(target: Target) -> hotpatch::LinkerOs {
    match target {
        Target::Android => hotpatch::LinkerOs::Linux,
        Target::IosSimulator => hotpatch::LinkerOs::Macos,
        Target::Macos => hotpatch::LinkerOs::Macos,
        Target::Web => hotpatch::LinkerOs::Other,
    }
}

/// Slurp the patch dylib off disk so the dev-loop can hand it to the
/// WebSocket sender. The size is typically tens of KB (only the
/// changed crate's `.o` linked with `-undefined dynamic_lookup`), and
/// since switching to the binary frame protocol we ship it verbatim
/// — no base64.
pub(super) fn read_lib_bytes(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("read {}", path.display()))
}

pub(super) async fn run_build_cycle(
    builder: &Builder,
    installer: &Installer,
    on_event: &Option<Arc<dyn Fn(Event) + Send + Sync>>,
    sender: &PatchSender,
    label: &str,
) {
    emit(on_event, Event::BuildingFull);
    match builder.build().await {
        Ok(()) => {
            emit(on_event, Event::BuildSucceeded);
            if let Err(e) = installer.install_and_launch().await {
                whisker_build::ui::error(format!("{label} install failed: {e}"));
            }
            whisker_build::ui::info(format!(
                "{label} done · {} client(s) connected",
                sender.client_count()
            ));
        }
        Err(e) => {
            let msg = format!("{e:#}");
            whisker_build::ui::error(format!("{label} build failed: {msg}"));
            emit(on_event, Event::BuildFailed(msg));
        }
    }
}

pub(super) fn emit(on_event: &Option<Arc<dyn Fn(Event) + Send + Sync>>, ev: Event) {
    if let Some(cb) = on_event {
        cb(ev);
    }
}

// ============================================================================
// Tests
// ============================================================================
