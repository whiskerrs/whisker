//! Host-side dev server for `tuft run`.
//!
//! Owns the long-running dev loop: file watch, cargo rebuild, install
//! to the device, and (eventually) subsecond patch construction +
//! WebSocket push. `tuft-cli`'s `run` subcommand is a thin wrapper
//! that builds a [`Config`] and calls [`DevServer::run`] — every
//! piece of UX-shaped logic lives here so future hosts (an editor
//! plugin, a notebook, a remote-controlled CI build) can reuse it.
//!
//! ## Status
//! Skeleton only. Each piece lands in its own task:
//!
//! | task | piece                                     |
//! |------|-------------------------------------------|
//! | I4c  | WebSocket server (axum)                   |
//! | I4d  | file watcher (notify) + change classifier |
//! | I4e  | builder + installer (Tier 2 cold rebuild) |
//! | I4f  | xtask `--features` flag plumbing          |
//! | I4g  | Tier 1 subsecond JumpTable construction   |

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub mod builder;
pub mod hotpatch;
pub mod installer;
pub mod server;
pub mod watcher;

pub use builder::{BuildPlan, Builder, CaptureShims};
pub use installer::Installer;
pub use server::{Envelope, PatchSender};
pub use watcher::{Change, ChangeKind};

// ----- Config & enums --------------------------------------------------------

/// Where the dev loop should run, what to build, and how to behave.
/// Constructed by `tuft-cli` from CLI flags; or by an editor plugin /
/// test harness directly.
#[derive(Debug, Clone)]
pub struct Config {
    /// Workspace root containing the user crate.
    pub workspace_root: PathBuf,
    /// User-crate package name (e.g. "hello-world").
    pub package: String,
    /// Where the rebuilt artifact gets installed + launched.
    pub target: Target,
    /// Directories to watch for source changes. Empty defaults to
    /// `<workspace_root>/<…>/src`.
    pub watch_paths: Vec<PathBuf>,
    /// Address the WebSocket server binds.
    pub bind_addr: SocketAddr,
    /// Strategy for reflecting code edits onto the running app.
    pub hot_patch_mode: HotPatchMode,
}

impl Config {
    /// A starting point with sensible defaults; callers override fields.
    pub fn defaults_for(workspace_root: PathBuf, package: String, target: Target) -> Self {
        Self {
            workspace_root,
            package,
            target,
            watch_paths: Vec::new(),
            bind_addr: "127.0.0.1:9876".parse().expect("valid default addr"),
            hot_patch_mode: HotPatchMode::Tier2ColdRebuild,
        }
    }
}

/// What kind of binary the dev server is rebuilding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Plain host binary — no Lynx, no device, mostly for runtime
    /// experiments (the `subsecond-poc` sort of thing).
    Host,
    /// Android cdylib + APK + adb install + launch.
    Android,
    /// iOS Simulator app + xcrun simctl install + launch.
    IosSimulator,
}

/// How aggressive the dev loop is about reflecting edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotPatchMode {
    /// Don't even try — every change requires a manual `tuft run` rerun.
    /// Useful for CI smoke-tests of the dev server itself.
    Disabled,
    /// Full cargo rebuild + reinstall + relaunch (5–30s). The default.
    Tier2ColdRebuild,
    /// `subsecond` JumpTable patches (sub-second). Requires the I4g
    /// pipeline to be wired up; otherwise behaves as `Tier2ColdRebuild`.
    Tier1Subsecond,
}

// ----- Public events ---------------------------------------------------------

/// Observable events that bubble out of the dev loop. `tuft-cli` uses
/// these to render terminal UI; an editor plugin would use them to
/// drive its own UX.
#[derive(Debug, Clone)]
pub enum Event {
    Started,
    BuildingFull,
    BuildSucceeded,
    BuildFailed(String),
    ClientConnected,
    ClientDisconnected,
    PatchSent,
}

// ----- Server ---------------------------------------------------------------

/// The dev loop. Construct with [`DevServer::new`], then drive with
/// [`DevServer::run`] (which returns when the server shuts down).
pub struct DevServer {
    config: Config,
    on_event: Option<Arc<dyn Fn(Event) + Send + Sync>>,
}

impl DevServer {
    pub fn new(config: Config) -> Result<Self> {
        Ok(Self { config, on_event: None })
    }

    /// Attach an observer for `Event`s — connect / disconnect /
    /// build progress. The CLI uses this to drive its terminal UI;
    /// other host shells (editor plugins) do their own thing.
    pub fn on_event(mut self, cb: impl Fn(Event) + Send + Sync + 'static) -> Self {
        self.on_event = Some(Arc::new(cb));
        self
    }

    /// Bring the dev loop up. The Tier 2 cold rebuild loop:
    ///
    ///   notify → debounce → cargo build → adb install → relaunch
    ///   → broadcast "rebuilt" hint over WebSocket.
    ///
    /// When `hot_patch_mode == Tier1Subsecond`, the initial build
    /// also captures rustc + linker invocations through the
    /// `tuft-{rustc,linker}-shim` binaries, and a `Patcher` is
    /// initialised from those captures + the original binary's
    /// symbol table. The change loop then prefers Tier 1
    /// `subsecond::JumpTable` patches over cold rebuilds for
    /// `ChangeKind::RustCode` events. Patcher initialisation or
    /// `build_patch` failure falls back to Tier 2 silently.
    pub async fn run(self) -> Result<()> {
        eprintln!(
            "[tuft-dev-server] starting (target={:?}, package={}, addr={}, mode={:?})",
            self.config.target,
            self.config.package,
            self.config.bind_addr,
            self.config.hot_patch_mode,
        );

        let (sender, bound, _server_handle) =
            server::serve(self.config.bind_addr, self.on_event.clone()).await?;
        eprintln!(
            "[tuft-dev-server] ws://{bound}/tuft-dev (waiting for clients; Ctrl-C to quit)"
        );

        // Watch the package's `src/`. For the workspace layout we
        // ship today, that's `examples/<package>/src`.
        let watch_root = self
            .config
            .workspace_root
            .join("examples")
            .join(&self.config.package)
            .join("src");
        let (tx, mut rx) = tokio::sync::mpsc::channel::<watcher::Change>(8);
        let _watcher = watcher::spawn_watcher(
            watch_root.clone(),
            std::time::Duration::from_millis(200),
            tx,
        )?;
        eprintln!("[tuft-dev-server] watching {}", watch_root.display());
        emit(&self.on_event, Event::Started);

        // Configure the initial build. For Tier 1 it doubles as the
        // fat build that fills the rustc / linker capture caches —
        // the shims are resolved (built if missing) and installed
        // into the builder *before* the spawn. The same Builder
        // object is then reused for Tier 2 fallback rebuilds, which
        // just inherit the capture env (harmless if the patcher
        // never reads the new captures).
        let mut builder = Builder::new(
            self.config.workspace_root.clone(),
            self.config.package.clone(),
            self.config.target,
        )
        .with_features(vec!["tuft/hot-reload".into()]);

        let tier1_init = if self.config.hot_patch_mode == HotPatchMode::Tier1Subsecond {
            match prepare_tier1_capture(
                &self.config.workspace_root,
                &self.config.package,
                self.config.target,
            ) {
                Ok(prep) => {
                    builder = builder.with_capture(prep.capture.clone());
                    Some(prep)
                }
                Err(e) => {
                    eprintln!(
                        "[tuft-dev-server] Tier 1 capture setup failed: {e:#} — \
                         falling back to Tier 2 cold rebuilds",
                    );
                    None
                }
            }
        } else {
            None
        };

        let installer = Installer::new(
            self.config.workspace_root.clone(),
            self.config.package.clone(),
            self.config.target,
        );

        // Initial build + install + launch. Without this the dev
        // server would just sit there until the user touched a file —
        // unfriendly when you've started an emulator and want the app
        // up immediately. A failure here doesn't abort: the loop
        // still enters, so fixing the source and saving recovers.
        eprintln!("[tuft-dev-server] initial build");
        run_build_cycle(&builder, &installer, &self.on_event, &sender, "initial").await;

        // After the fat build has happened, Patcher::initialize can
        // read the now-populated caches. Failure here is non-fatal
        // — log and proceed with Tier 2 only.
        let patcher = match tier1_init {
            Some(prep) => match init_patcher_for(&self.config, &prep) {
                Ok(p) => {
                    eprintln!("[tuft-dev-server] Tier 1 patcher ready");
                    Some(p)
                }
                Err(e) => {
                    eprintln!(
                        "[tuft-dev-server] Tier 1 patcher init failed: {e:#} — \
                         falling back to Tier 2 cold rebuilds",
                    );
                    None
                }
            },
            None => None,
        };

        // Change loop. For each debounced change, decide the
        // action (Tier 1 patch / Tier 2 rebuild / ignore) using
        // the kind + whether we have a Patcher, then execute it.
        // Tier 1 failures silently fall through to Tier 2 — saves
        // the dev loop from being killed by a transient build
        // glitch.
        while let Some(change) = rx.recv().await {
            eprintln!(
                "[tuft-dev-server] change ({:?}) — {} path(s)",
                change.kind,
                change.paths.len(),
            );
            let action = decide_action(change.kind, patcher.is_some());
            match action {
                LoopAction::Ignore => {
                    eprintln!("[tuft-dev-server] ignored ({:?})", change.kind);
                }
                LoopAction::Tier1Patch => {
                    let p = patcher.as_ref().expect("decide_action guarantees Some");
                    let started = std::time::Instant::now();
                    match p.build_patch().await {
                        Ok(plan) => {
                            let built_in = started.elapsed();
                            log_patch_diff(&plan.report);
                            let send_started = std::time::Instant::now();
                            let n = sender.send(Envelope::Patch { table: plan.table });
                            eprintln!(
                                "[tuft-dev-server] tier1 patch sent to {n} client(s) \
                                 (built in {built_in:?}, queued for send in {:?}, \
                                 total edit→send: {:?})",
                                send_started.elapsed(),
                                started.elapsed(),
                            );
                            emit(&self.on_event, Event::PatchSent);
                        }
                        Err(e) => {
                            eprintln!(
                                "[tuft-dev-server] tier1 patch failed: {e:#} — \
                                 falling back to Tier 2 cold rebuild",
                            );
                            run_build_cycle(
                                &builder,
                                &installer,
                                &self.on_event,
                                &sender,
                                "rebuild (tier2 fallback)",
                            )
                            .await;
                        }
                    }
                }
                LoopAction::Tier2Rebuild => {
                    run_build_cycle(
                        &builder,
                        &installer,
                        &self.on_event,
                        &sender,
                        "rebuild",
                    )
                    .await;
                }
            }
        }

        Ok(())
    }
}

/// Decision the change loop makes for one debounced change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopAction {
    /// Drop on the floor — `ChangeKind::Other` doesn't warrant
    /// either a patch or a rebuild.
    Ignore,
    /// Try a Tier 1 subsecond patch. Caller falls back to Tier 2
    /// on Patcher error.
    Tier1Patch,
    /// Full cargo rebuild + reinstall + relaunch (Tier 2). Used
    /// when the change is dependency-shaped (`Cargo.toml`) or
    /// when no Patcher is configured.
    Tier2Rebuild,
}

/// Pure decision helper for the change loop. Tier 1 only handles
/// `ChangeKind::RustCode` and only when a Patcher is available;
/// `Cargo.toml` always needs a full rebuild because the dependency
/// graph may have shifted; everything else is ignored.
pub fn decide_action(kind: ChangeKind, has_patcher: bool) -> LoopAction {
    match kind {
        ChangeKind::Other => LoopAction::Ignore,
        ChangeKind::CargoToml => LoopAction::Tier2Rebuild,
        ChangeKind::RustCode if has_patcher => LoopAction::Tier1Patch,
        ChangeKind::RustCode => LoopAction::Tier2Rebuild,
    }
}

/// Log added / removed symbols from a Tier 1 diff. Quiet when both
/// lists are empty (the common case) so the dev terminal stays
/// readable; loud when something interesting happens (`pub fn`
/// added or removed) so the user notices.
fn log_patch_diff(report: &hotpatch::DiffReport) {
    if report.added.is_empty() && report.removed.is_empty() {
        return;
    }
    if !report.added.is_empty() {
        eprintln!(
            "[tuft-dev-server] patch added {} symbol(s): {:?}",
            report.added.len(),
            report.added.iter().take(5).collect::<Vec<_>>(),
        );
    }
    if !report.removed.is_empty() {
        eprintln!(
            "[tuft-dev-server] patch removed {} symbol(s) — \
             host shell may crash on stale callers: {:?}",
            report.removed.len(),
            report.removed.iter().take(5).collect::<Vec<_>>(),
        );
    }
}

/// State produced by [`prepare_tier1_capture`]: enough to make the
/// initial build a fat build, and to construct the patcher after the
/// build completes.
#[derive(Debug, Clone)]
struct Tier1Prep {
    capture: CaptureShims,
    real_linker: PathBuf,
}

/// Resolve shim paths (building them if missing) and assemble the
/// CaptureShims wiring. Returns `Err` if the shim binaries can't be
/// produced, in which case the caller falls back to Tier 2.
///
/// `package` + `target` are used to pick the right linker:
///   - Android → NDK clang for the ABI of the existing jniLibs/.so
///     (or arm64-v8a as a default if no built .so exists yet).
///   - others → host clang via [`hotpatch::wrapper::resolve_host_linker`].
fn prepare_tier1_capture(
    workspace_root: &Path,
    package: &str,
    target: Target,
) -> Result<Tier1Prep> {
    let shims = hotpatch::resolve_shim_paths(workspace_root)?;
    let rustc_cache_dir = hotpatch::wrapper::default_cache_dir(workspace_root);
    let linker_cache_dir = hotpatch::wrapper::default_linker_cache_dir(workspace_root);
    let real_linker = resolve_linker_for(target, workspace_root, package)?;
    let target_triple = target_triple_for(target, workspace_root, package);
    Ok(Tier1Prep {
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

/// What Rust target triple `target` compiles for. Android picks the
/// triple from the ABI of the existing jniLibs/.so (or arm64-v8a
/// for the first run). Host / iOS Simulator return `None`, falling
/// back to the global RUSTFLAGS form.
fn target_triple_for(
    target: Target,
    workspace_root: &Path,
    package: &str,
) -> Option<String> {
    match target {
        Target::Android => {
            let abi = existing_android_abi(workspace_root, package)
                .unwrap_or("arm64-v8a");
            // abi_to_triple is xtask logic, duplicated minimally:
            let triple = match abi {
                "arm64-v8a" => "aarch64-linux-android",
                "armeabi-v7a" => "armv7-linux-androideabi",
                "x86_64" => "x86_64-linux-android",
                "x86" => "i686-linux-android",
                _ => return None,
            };
            Some(triple.to_string())
        }
        Target::Host | Target::IosSimulator => None,
    }
}

/// Pick the linker driver to use for `target`. Returned path is what
/// the linker shim forwards to during the fat build *and* what the
/// thin-rebuild link step spawns directly — the same binary on both
/// sides keeps SDK / sysroot resolution consistent.
fn resolve_linker_for(
    target: Target,
    workspace_root: &Path,
    package: &str,
) -> Result<PathBuf> {
    match target {
        Target::Android => {
            // Pick the API level + ABI from whatever jniLibs/.so the
            // last xtask build produced. Default to arm64-v8a + API
            // 21 when nothing is on disk yet (first run).
            let abi = existing_android_abi(workspace_root, package)
                .unwrap_or("arm64-v8a");
            let api = std::env::var("TUFT_ANDROID_API")
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(21);
            hotpatch::android_ndk::android_clang_for(abi, api)
                .with_context(|| format!("resolve NDK clang for ABI {abi} API {api}"))
        }
        Target::Host | Target::IosSimulator => {
            Ok(hotpatch::wrapper::resolve_host_linker())
        }
    }
}

/// Walk the package's jniLibs tree and return the first ABI that
/// has a libxxx.so under it. None means no prior xtask build.
fn existing_android_abi(workspace_root: &Path, package: &str) -> Option<&'static str> {
    let crate_us = package.replace('-', "_");
    let so_name = format!("lib{crate_us}.so");
    let jni_libs = workspace_root
        .join("examples")
        .join(package)
        .join("android/app/src/main/jniLibs");
    for abi in ["arm64-v8a", "armeabi-v7a", "x86_64", "x86"] {
        if jni_libs.join(abi).join(&so_name).is_file() {
            return Some(abi);
        }
    }
    None
}

/// Construct the patcher from the captures the fat build just wrote.
/// Splits out so [`DevServer::run`] is easier to read.
fn init_patcher_for(
    config: &Config,
    prep: &Tier1Prep,
) -> Result<hotpatch::Patcher> {
    let original_binary = original_binary_path(
        &config.workspace_root,
        &config.package,
        config.target,
    )?;
    hotpatch::Patcher::initialize(
        &config.workspace_root,
        config.package.clone(),
        &prep.capture.rustc_cache_dir,
        &prep.capture.linker_cache_dir,
        &prep.real_linker,
        &original_binary,
        target_os_for(config.target),
    )
}

/// Locate the device-loadable original binary for `target`. Tier 1
/// only supports targets that produce a `.so`/`.dylib` we can mmap
/// and diff against; `Host` (which produces just an `.rlib` today)
/// returns `Err` so the caller falls back to Tier 2.
fn original_binary_path(
    workspace_root: &Path,
    package: &str,
    target: Target,
) -> Result<PathBuf> {
    let crate_underscored = package.replace('-', "_");
    match target {
        Target::Android => {
            // xtask's NDK build drops the device-loadable .so into
            // the Gradle jniLibs tree before APK packaging. Pick the
            // first ABI we find (debug builds typically have one).
            let jni_libs = workspace_root
                .join("examples")
                .join(package)
                .join("android/app/src/main/jniLibs");
            let so_name = format!("lib{crate_underscored}.so");
            for abi in [
                "arm64-v8a",
                "armeabi-v7a",
                "x86_64",
                "x86",
            ] {
                let candidate = jni_libs.join(abi).join(&so_name);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
            anyhow::bail!(
                "no Android cdylib found under {} — run the initial xtask build first",
                jni_libs.display(),
            );
        }
        Target::Host => {
            anyhow::bail!(
                "Tier 1 not supported for Host target yet \
                 (hello-world is rlib-only — no patchable shared library)"
            );
        }
        Target::IosSimulator => {
            anyhow::bail!(
                "Tier 1 not supported for iOS Simulator yet (staticlib-based xcframework)"
            );
        }
    }
}

fn target_os_for(target: Target) -> hotpatch::LinkerOs {
    match target {
        Target::Android => hotpatch::LinkerOs::Linux,
        Target::IosSimulator => hotpatch::LinkerOs::Macos,
        Target::Host => hotpatch::linker_os_for_host(),
    }
}

async fn run_build_cycle(
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
                eprintln!("[tuft-dev-server] {label} install failed: {e}");
            }
            eprintln!(
                "[tuft-dev-server] {label} done; {} client(s) connected",
                sender.client_count()
            );
        }
        Err(e) => {
            let msg = format!("{e:#}");
            eprintln!("[tuft-dev-server] {label} build failed: {msg}");
            emit(on_event, Event::BuildFailed(msg));
        }
    }
}

fn emit(on_event: &Option<Arc<dyn Fn(Event) + Send + Sync>>, ev: Event) {
    if let Some(cb) = on_event {
        cb(ev);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn config_defaults_pick_loopback_and_tier2() {
        let cfg = Config::defaults_for(
            PathBuf::from("/tmp/ws"),
            "hello-world".to_string(),
            Target::Host,
        );
        assert_eq!(cfg.workspace_root, Path::new("/tmp/ws"));
        assert_eq!(cfg.package, "hello-world");
        assert_eq!(cfg.target, Target::Host);
        assert_eq!(cfg.bind_addr.port(), 9876);
        assert!(cfg.bind_addr.ip().is_loopback());
        assert_eq!(cfg.hot_patch_mode, HotPatchMode::Tier2ColdRebuild);
        assert!(cfg.watch_paths.is_empty());
    }

    #[test]
    fn target_variants_compare_by_value() {
        assert_eq!(Target::Android, Target::Android);
        assert_ne!(Target::Android, Target::IosSimulator);
    }

    #[test]
    fn hot_patch_mode_variants_compare_by_value() {
        assert_eq!(HotPatchMode::Disabled, HotPatchMode::Disabled);
        assert_ne!(
            HotPatchMode::Tier1Subsecond,
            HotPatchMode::Tier2ColdRebuild,
        );
    }

    #[test]
    fn dev_server_new_does_not_fail_for_a_well_formed_config() {
        let cfg = Config::defaults_for(
            PathBuf::from("/tmp/ws"),
            "hello-world".to_string(),
            Target::Host,
        );
        assert!(DevServer::new(cfg).is_ok());
    }

    // ----- original_binary_path ----------------------------------------

    #[test]
    fn original_binary_path_errors_for_host_target() {
        let res = original_binary_path(
            Path::new("/tmp/ws"),
            "hello-world",
            Target::Host,
        );
        let err = res.unwrap_err();
        assert!(format!("{err:#}").contains("Host"), "got: {err:#}");
    }

    #[test]
    fn original_binary_path_errors_for_ios_simulator() {
        let res = original_binary_path(
            Path::new("/tmp/ws"),
            "hello-world",
            Target::IosSimulator,
        );
        assert!(res.is_err());
    }

    #[test]
    fn original_binary_path_finds_android_so_under_jni_libs() {
        // Create a fake workspace layout with libhello_world.so
        // under arm64-v8a, then verify the resolver picks it.
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let ws = std::env::temp_dir().join(format!("tuft-dev-test-orig-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&ws);
        let abi_dir = ws
            .join("examples/hello-world/android/app/src/main/jniLibs/arm64-v8a");
        std::fs::create_dir_all(&abi_dir).unwrap();
        let so = abi_dir.join("libhello_world.so");
        std::fs::write(&so, b"fake").unwrap();

        let resolved = original_binary_path(&ws, "hello-world", Target::Android).unwrap();
        assert_eq!(resolved, so);

        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn original_binary_path_errors_when_android_so_missing() {
        let res = original_binary_path(
            Path::new("/nonexistent/ws"),
            "hello-world",
            Target::Android,
        );
        assert!(res.is_err());
    }

    // ----- target_os_for -----------------------------------------------

    #[test]
    fn target_os_for_maps_android_to_linux() {
        assert_eq!(target_os_for(Target::Android), hotpatch::LinkerOs::Linux);
    }

    #[test]
    fn target_os_for_maps_ios_to_macos() {
        assert_eq!(
            target_os_for(Target::IosSimulator),
            hotpatch::LinkerOs::Macos,
        );
    }

    // ----- decide_action -----------------------------------------------

    #[test]
    fn rust_code_with_patcher_chooses_tier1_patch() {
        assert_eq!(
            decide_action(ChangeKind::RustCode, true),
            LoopAction::Tier1Patch,
        );
    }

    #[test]
    fn rust_code_without_patcher_falls_through_to_tier2_rebuild() {
        assert_eq!(
            decide_action(ChangeKind::RustCode, false),
            LoopAction::Tier2Rebuild,
        );
    }

    #[test]
    fn cargo_toml_always_chooses_tier2_rebuild_even_with_patcher() {
        // Patcher can't reload deps — Cargo.toml needs a full
        // rebuild regardless of which mode we're in.
        assert_eq!(
            decide_action(ChangeKind::CargoToml, true),
            LoopAction::Tier2Rebuild,
        );
        assert_eq!(
            decide_action(ChangeKind::CargoToml, false),
            LoopAction::Tier2Rebuild,
        );
    }

    #[test]
    fn other_changes_are_ignored() {
        assert_eq!(decide_action(ChangeKind::Other, true), LoopAction::Ignore);
        assert_eq!(decide_action(ChangeKind::Other, false), LoopAction::Ignore);
    }

    // ----- log_patch_diff (smoke: shouldn't panic) ---------------------

    #[test]
    fn log_patch_diff_handles_empty_report_silently() {
        let r = hotpatch::DiffReport {
            added: vec![],
            removed: vec![],
            weak: vec![],
        };
        log_patch_diff(&r); // no panic, no output
    }

    #[test]
    fn log_patch_diff_summarises_added_and_removed() {
        let r = hotpatch::DiffReport {
            added: vec!["new1".into(), "new2".into()],
            removed: vec!["old1".into()],
            weak: vec![],
        };
        log_patch_diff(&r); // smoke — output goes to stderr
    }
}
