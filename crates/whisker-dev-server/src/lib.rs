//! Host-side dev server for `whisker run`.
//!
//! Owns the long-running dev loop: file watch, cargo rebuild, install
//! to the device, subsecond patch construction, and WebSocket push.
//! `whisker-cli`'s `run` subcommand is a thin wrapper that builds a
//! [`Config`] and calls [`DevServer::run`] — every piece of
//! UX-shaped logic lives here so future host shells (an editor
//! plugin, a notebook, a remote-controlled CI build) can reuse it.
//!
//! ## Architecture
//!
//! Constructed once via [`Config`], the dev server spins up six
//! cooperating pieces:
//!
//! - `builder` — translates [`Config`] into a `whisker-build`
//!   invocation (cargo + per-platform packaging) and runs it.
//!   Honours `RUSTC_WORKSPACE_WRAPPER` + linker shim env so the fat
//!   build doubles as a capture pass for hot reload.
//! - `installer` — for the cold-rebuild path: shells out to
//!   `adb install` / `simctl install + launch`. Identity (bundle id,
//!   applicationId, scheme, …) comes in flat via
//!   [`AndroidParams`] / [`IosParams`]; the cli resolves these from
//!   the user's `whisker.rs::configure(&mut Config)`. This crate
//!   never depends on `whisker-config`.
//! - `watcher` — `notify`-based, debounced, classifies events into
//!   `ChangeKind::{RustCode, CargoToml, Other}`.
//! - `server` — `axum` WebSocket endpoint at
//!   `ws://<bind>/whisker-dev`. Devices dial in, send a `hello`
//!   carrying their `subsecond::aslr_reference()`, then receive
//!   patch envelopes.
//! - `hotpatch` — the Hot Reload implementation. Builds a thin `.o` from the
//!   changed user crate via captured rustc args, links it into a
//!   patch dylib with a stub-object of host-symbol jumps, ships the
//!   resulting `subsecond_types::JumpTable` to connected clients.
//! - `lib.rs::run` — the orchestrator: file event → `decide_action`
//!   (hot-reload patch vs Full Reload prompt) → builder/hotpatch/sender.
//!
//! ## Layering
//!
//! Stays manifest-agnostic on purpose. The cli does the
//! `whisker.rs` → `Config` translation; this crate accepts only
//! flat `String` / `PathBuf` fields. That keeps the dev-server
//! reusable from any host shell that can produce the same flat
//! `Config` (the cli is one; an editor plugin could be another).

mod hot_reload;

use hot_reload::*;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub mod builder;
pub mod hotpatch;
pub mod installer;
pub mod server;
pub mod watcher;
pub mod workspace;

pub use builder::Builder;
pub use installer::Installer;
pub use server::{Patch, PatchSender};
pub use watcher::{Change, ChangeKind};
pub use whisker_build::CaptureShims;
pub use workspace::{PathDepCrate, discover_path_deps, identify_crate_for_paths};

// ----- Config & enums --------------------------------------------------------

/// Where the dev loop should run, what to build, and how to behave.
/// Constructed by `whisker-cli` from CLI flags + the user's
/// `whisker.rs` (via the cli's manifest/probe pipeline); or by an
/// editor plugin / test harness directly.
///
/// **Flat params, not Config.** Anything platform-specific lives
/// inside [`AndroidParams`] / [`IosParams`] as simple strings and
/// paths — the dev-server intentionally doesn't depend on
/// `whisker-config`. Translating the user's `configure(&mut
/// Config)` into these fields is the cli's job.
#[derive(Debug, Clone)]
pub struct Config {
    /// Workspace root (`Cargo.toml` with `[workspace]`). Used by
    /// `whisker-build` invocations + RUSTC capture directories.
    pub workspace_root: PathBuf,
    /// User-crate directory (`Cargo.toml` with `[package]`). This
    /// is what `whisker run --manifest-path` resolves to; for
    /// in-workspace examples it's `examples/<pkg>/`, for an
    /// external user it's wherever they keep their app.
    pub crate_dir: PathBuf,
    /// User-crate package name (e.g. "podcast").
    pub package: String,
    /// Where the rebuilt artifact gets installed + launched.
    pub target: Target,
    /// Extra paths (dirs or single files) to watch for changes, merged
    /// with the auto-discovered roots (`<crate_dir>/src` + every
    /// workspace path-dep's `src/`). The cli passes
    /// `<crate_dir>/whisker.rs` here so config-script saves get a
    /// "restart `whisker run`" hint instead of silence.
    pub watch_paths: Vec<PathBuf>,
    /// Address the WebSocket server binds.
    pub bind_addr: SocketAddr,
    /// Shared dev-session token. When `Some`, the WebSocket server only
    /// arms the patch channel for a client whose `hello` carries the
    /// matching token, and the cli delivers it to the device (iOS env /
    /// Android system property). `None` runs unauthenticated (the prior
    /// behaviour). `whisker run` generates a random one per session.
    pub dev_token: Option<String>,
    /// Strategy for reflecting code edits onto the running app.
    pub hot_patch_mode: HotPatchMode,
    /// Android install / launch params. Required iff
    /// `target == Target::Android`; absent for other targets.
    pub android: Option<AndroidParams>,
    /// iOS install / launch params. Required iff
    /// `target == Target::IosSimulator`; absent for other targets.
    pub ios: Option<IosParams>,
    /// Native macOS build / launch params. Required iff
    /// `target == Target::Macos`; absent for other targets.
    pub macos: Option<MacosParams>,
    /// Generated browser composition project. Required iff
    /// `target == Target::Web`.
    pub web: Option<WebParams>,
}

impl Config {
    /// A starting point with sensible defaults; callers override fields.
    pub fn defaults_for(workspace_root: PathBuf, package: String, target: Target) -> Self {
        Self {
            workspace_root: workspace_root.clone(),
            crate_dir: workspace_root,
            package,
            target,
            watch_paths: Vec::new(),
            bind_addr: "127.0.0.1:9876".parse().expect("valid default addr"),
            dev_token: None,
            hot_patch_mode: HotPatchMode::FullReloadOnly,
            android: None,
            ios: None,
            macos: None,
            web: None,
        }
    }
}

/// Flat Android install/launch parameters. Populated by `whisker-cli`
/// from the user's `whisker.rs::configure(&mut Config)` plus a few
/// hard defaults (jniLibs lives at `<project_dir>/app/src/main/jniLibs`,
/// APK at `<project_dir>/app/build/outputs/apk/debug/app-debug.apk`,
/// etc.). The dev-server never invents these values — if any are
/// missing the cli is expected to error out before constructing
/// `Config`.
#[derive(Debug, Clone)]
pub struct AndroidParams {
    /// Absolute path to the Gradle project (= the dir with
    /// `app/build.gradle.kts`). For the in-workspace podcast
    /// example this is `examples/podcast/android/`.
    pub project_dir: PathBuf,
    /// `applicationId` — used by `adb am start -n
    /// <application_id>/<launcher_activity>`.
    pub application_id: String,
    /// Launcher activity. Always starts with a dot
    /// (e.g. `.MainActivity`); `am start` expands it against
    /// `application_id`.
    pub launcher_activity: String,
    /// ABI directory under `jniLibs/` (e.g. `"arm64-v8a"`). Hard-
    /// coded by the cli for now; multi-ABI builds aren't on the
    /// dev loop's path.
    pub abi: String,
}

/// Flat iOS Simulator install/launch parameters. Same pattern as
/// [`AndroidParams`] — populated by the cli, consumed by the
/// dev-server's installer.
#[derive(Debug, Clone)]
pub struct IosParams {
    /// Absolute path to the Xcode project's parent dir (= where
    /// `<Scheme>.xcodeproj` lives). For podcast this is
    /// `examples/podcast/ios/`.
    pub project_dir: PathBuf,
    /// Xcode scheme. Doubles as the `.app` filename xcodebuild
    /// produces (`<Scheme>.app`). With XcodeGen this always
    /// matches the project name.
    pub scheme: String,
    /// CFBundleIdentifier. Used by `simctl install / terminate /
    /// launch` as the right-hand identifier.
    pub bundle_id: String,
    /// Optional simulator-device override; usually `None` to let
    /// the cli pick the first available iPhone. Honored if set.
    pub device_override: Option<String>,
}

/// Flat parameters for the generated Cargo-based macOS Host.
#[derive(Debug, Clone)]
pub struct MacosParams {
    /// Generated `gen/macos` Cargo project.
    pub project_dir: PathBuf,
    /// Dedicated Cargo target directory shared by builds and patches.
    pub target_dir: PathBuf,
    /// Human-readable `.app` bundle name.
    pub app_name: String,
    /// Generated Host executable and Cargo package name.
    pub binary_name: String,
}

/// Flat parameters for Whisker's generated browser Host.
#[derive(Debug, Clone)]
pub struct WebParams {
    /// Generated `gen/web` Cargo project.
    pub project_dir: PathBuf,
    /// Dedicated Cargo output directory for the wasm target.
    pub target_dir: PathBuf,
    /// Static directory served by the dev server.
    pub dist_dir: PathBuf,
    /// Generated composition crate/package name.
    pub generated_package: String,
}

/// What kind of binary the dev server is rebuilding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Android APK + adb install + launch.
    Android,
    /// iOS Simulator app + xcrun simctl install + launch.
    IosSimulator,
    /// Native macOS app. The dev server builds and launches the generated
    /// Cargo Host, and applies subsecond patches without restarting it.
    Macos,
    /// Browser WASM application built and served by Whisker.
    Web,
}

/// How the dev loop reflects edits. Note that no mode rebuilds or
/// restarts the app automatically — a Full Reload only ever runs on
/// an explicit [`DevCommand::FullReload`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotPatchMode {
    /// Don't even try — every change requires a manual `whisker run` rerun.
    /// Useful for CI smoke-tests of the dev server itself.
    Disabled,
    /// No hot reload: every save prompts for an explicit Full Reload
    /// (cargo rebuild + reinstall + relaunch, 5–30s). The
    /// `--no-hot-patch` escape hatch.
    FullReloadOnly,
    /// Hot Reload: `subsecond` JumpTable patches (sub-second, app
    /// keeps running). Requires the capture/patcher pipeline; when
    /// that's unavailable the loop prompts for a Full Reload instead.
    HotReload,
}

// ----- Public events ---------------------------------------------------------

/// Observable events that bubble out of the dev loop. `whisker-cli` uses
/// these to render terminal UI; an editor plugin would use them to
/// drive its own UX.
#[derive(Debug, Clone)]
pub enum Event {
    Started,
    BuildingFull,
    BuildSucceeded,
    BuildFailed(String),
    HostLaunching,
    HostLaunched,
    HostLaunchFailed(String),
    ClientConnected,
    ClientDisconnected,
    /// A hot-reload patch build kicked off. Fires *before* the
    /// `Patcher::build_patch` call so consumers (the cli TUI) can
    /// flip into "patching" state while the patch is still being
    /// compiled — without this paired event, `PatchSent` is the
    /// only signal and arrives so close to its own completion that
    /// any UI keying off it never shows a patch-in-flight indicator.
    PatchBuilding,
    PatchSent,
    /// The loop hit a change it cannot hot-reload (dependency-graph
    /// edit, hot-reload infrastructure unavailable, patch build
    /// failure). Nothing was rebuilt — the user decides when to pay
    /// for the restart by pressing `R` (Full Reload). The UI should
    /// surface `reason` persistently until a full reload starts
    /// (`BuildingFull` clears it).
    FullReloadRequired {
        reason: String,
    },
    /// A line captured from the device-side app's stdout / stderr (via
    /// the `whisker-dev-runtime::log_capture` `dup2` hook), forwarded
    /// over the WS connection. `whisker-cli` surfaces these in the
    /// dev-loop UI so users don't need a separate `adb logcat` /
    /// `simctl log stream` terminal to read their own `println!`s.
    DeviceLog {
        /// `"stdout"` or `"stderr"` — kept as a string mirror of the
        /// on-wire field so the variant stays trivially serialisable
        /// without dragging a `LogStream` enum across crate
        /// boundaries.
        stream: String,
        line: String,
        /// Device-stamped microseconds since UNIX_EPOCH. `0` if the
        /// device's clock was unavailable when the line was captured.
        ts_micros: u128,
    },
}

// ----- Server ---------------------------------------------------------------

/// Explicit user command delivered into the dev loop (keyboard
/// shortcuts in `whisker run`'s TUI: `r`, `R`, `o`, and `q`). Reloads are
/// user-triggered by design — the loop never full-reloads on its own,
/// because an unexpected app restart mid-interaction loses more time
/// than it saves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevCommand {
    /// Build and push a hot-reload patch now, independent of any file
    /// change (e.g. after fixing a compile error, or to force a
    /// re-sync).
    HotReload,
    /// Full reload: cargo rebuild + reinstall + relaunch. The only
    /// way dependency-graph changes (Cargo.toml) reach the device.
    FullReload,
    /// Relaunch the current artifact without compiling it again.
    Relaunch,
    /// Gracefully stop the watcher and server loop.
    Shutdown,
}

/// The dev loop. Construct with [`DevServer::new`], then drive with
/// [`DevServer::run`] (which returns when the server shuts down).
pub struct DevServer {
    config: Config,
    on_event: Option<Arc<dyn Fn(Event) + Send + Sync>>,
    commands: Option<tokio::sync::mpsc::UnboundedReceiver<DevCommand>>,
}

impl DevServer {
    pub fn new(config: Config) -> Result<Self> {
        Ok(Self {
            config,
            on_event: None,
            commands: None,
        })
    }

    /// Attach the channel that delivers explicit [`DevCommand`]s
    /// (the CLI's `r` / `R` keys). Without one, the loop is driven
    /// by file changes only and full reloads are unreachable — fine
    /// for tests, limiting for interactive use.
    pub fn with_command_receiver(
        mut self,
        rx: tokio::sync::mpsc::UnboundedReceiver<DevCommand>,
    ) -> Self {
        self.commands = Some(rx);
        self
    }

    /// Attach an observer for `Event`s — connect / disconnect /
    /// build progress. The CLI uses this to drive its terminal UI;
    /// other host shells (editor plugins) do their own thing.
    pub fn on_event(mut self, cb: impl Fn(Event) + Send + Sync + 'static) -> Self {
        self.on_event = Some(Arc::new(cb));
        self
    }

    /// Bring the dev loop up. The core loop:
    ///
    ///   notify → debounce → cargo build → adb install → relaunch
    ///   → broadcast "rebuilt" hint over WebSocket.
    ///
    /// When `hot_patch_mode == HotReload`, the initial build
    /// also captures rustc + linker invocations through the
    /// `whisker-{rustc,linker}-shim` binaries, and a `Patcher` is
    /// initialised from those captures + the original binary's
    /// symbol table. The change loop then serves
    /// `ChangeKind::RustCode` events with Hot Reload
    /// (`subsecond::JumpTable` patches). Nothing rebuilds or
    /// restarts the app automatically: changes a patch can't express
    /// prompt for an explicit Full Reload (`DevCommand::FullReload`).
    pub async fn run(mut self) -> Result<()> {
        whisker_build::ui::section("whisker run");
        whisker_build::ui::info(format!(
            "{} · {:?}",
            self.config.package, self.config.target,
        ));
        whisker_build::ui::debug(format!("mode={:?}", self.config.hot_patch_mode));

        // Builder + Installer are wired before the socket so no
        // dev-server chatter lands between the "whisker run" and
        // "Initial build" sections; the WS binds once cargo (the long
        // pole) succeeds, before `install_and_launch` gives the device
        // something to dial.
        //
        // In hot-reload mode this build doubles as the fat build
        // filling the rustc / linker capture caches, so the shims must
        // be installed into the builder *before* the spawn. The same
        // Builder serves Full Reload rebuilds inside the change loop.
        let mut builder = Builder::new(
            self.config.workspace_root.clone(),
            self.config.crate_dir.clone(),
            self.config.package.clone(),
            self.config.target,
        )
        .with_macos(self.config.macos.clone())
        .with_web(self.config.web.clone());

        let hot_reload_init = if self.config.hot_patch_mode == HotPatchMode::HotReload {
            let feature = match self.config.target {
                Target::Macos | Target::Web => "hot-reload",
                Target::Android | Target::IosSimulator => "whisker/hot-reload",
            };
            builder = builder.with_features(vec![feature.into()]);
            match prepare_hot_reload_capture(&self.config) {
                Ok(prep) => {
                    builder = builder.with_capture(prep.capture.clone());
                    Some(prep)
                }
                Err(e) => {
                    whisker_build::ui::warn(format!(
                        "hot-reload capture setup failed ({e:#}); hot reload unavailable — \
                         use R (Full Reload) to reflect changes",
                    ));
                    None
                }
            }
        } else {
            None
        };

        let installer = Installer::new(
            self.config.target,
            self.config.android.clone(),
            self.config.ios.clone(),
            self.config.macos.clone(),
            self.config.workspace_root.clone(),
            self.config.package.clone(),
            hot_reload_init.as_ref().map(|p| p.capture.clone()),
            builder.features().to_vec(),
            self.config.bind_addr.port(),
            self.config.dev_token.clone(),
        );

        // Initial build — cargo only. `install_and_launch` waits until
        // the WS is bound, because the device app opens its
        // `whisker-dev-runtime` socket the moment it launches and
        // would race a not-yet-bound dev-server.
        //
        // A build failure here is fatal: with no app on the device
        // there is nothing to patch and nothing a later save could
        // recover.
        //
        whisker_build::ui::section("Initial build");
        emit(&self.on_event, Event::BuildingFull);
        if let Err(e) = builder.build().await {
            let msg = format!("{e:#}");
            emit(&self.on_event, Event::BuildFailed(msg.clone()));
            // No `ui::error` here: cli main already prints the bail
            // message that way, so it would double-print. Keep the
            // message user-actionable — `WHISKER_VERBOSE=1` still
            // reaches the full chain.
            anyhow::bail!("initial build failed: {msg}");
        }
        emit(&self.on_event, Event::BuildSucceeded);

        // Bind the WS so `install_and_launch` (next) has somewhere for
        // the device's `whisker-dev-runtime` to dial. Status calls feed
        // deterministic plain output; the TUI adapter uses target-aware
        // lifecycle events for its live region and ignores these strings.
        whisker_build::ui::ensure_status("dev-server");
        let (sender, bound, _server_handle) = server::serve(
            self.config.bind_addr,
            self.on_event.clone(),
            self.config.dev_token.clone(),
            self.config.web.as_ref().map(|web| web.dist_dir.clone()),
        )
        .await?;
        whisker_build::ui::set_status(format!("ws://{bound} · 0 client(s)"));
        whisker_build::ui::debug(format!("ws://{bound}/whisker-dev"));

        // One notify root per workspace path dep's `src/`; the change
        // loop maps a changed file back to its owning crate through
        // the same list. Registry / git deps are out of scope — their
        // sources sit outside the workspace, and a Cargo.toml edit
        // prompts a Full Reload that picks them up anyway.
        let path_deps = workspace::discover_path_deps(
            &self.config.crate_dir.join("Cargo.toml"),
            &self.config.package,
        )
        .unwrap_or_else(|e| {
            whisker_build::ui::warn(format!(
                "cargo metadata failed ({e:#}); falling back to user crate only",
            ));
            Vec::new()
        });
        // The user crate's src dir goes in even when cargo metadata
        // returned nothing, so the loop still works degraded.
        let user_src = self.config.crate_dir.join("src");
        let mut watch_roots: Vec<PathBuf> = path_deps
            .iter()
            .map(|c| c.src_dir.clone())
            .filter(|p| p.is_dir())
            .collect();
        if !watch_roots.iter().any(|p| p == &user_src) && user_src.is_dir() {
            watch_roots.push(user_src.clone());
        }
        if watch_roots.is_empty() {
            // Push it even if absent: notify's failure is the error the
            // user needs to see.
            watch_roots.push(user_src.clone());
        }
        // Caller-specified extras. Single files are fine — notify
        // watches those too.
        for extra in &self.config.watch_paths {
            if extra.exists() && !watch_roots.contains(extra) {
                watch_roots.push(extra.clone());
            }
        }
        // `whisker.rs` is the config script: it's evaluated once at
        // `whisker run` startup (probe → Config), so neither a hot
        // reload nor a Full Reload re-applies an edit to it. Detect
        // saves and tell the user the only fix — restarting the dev
        // loop — instead of reacting with a doomed patch attempt.
        let whisker_config_file = self.config.crate_dir.join("whisker.rs");
        let (tx, mut rx) = tokio::sync::mpsc::channel::<watcher::Change>(8);
        let _watcher = watcher::spawn_watcher(
            watch_roots.clone(),
            std::time::Duration::from_millis(200),
            tx,
        )?;
        for root in &watch_roots {
            whisker_build::ui::debug(format!("watching {}", root.display()));
        }
        emit(&self.on_event, Event::Started);

        // Fatal for the same reason a build failure is: nothing to
        // dev-loop against if the app never reached the device. The
        // in-loop rebuild path (`run_build_cycle`) shares this code but
        // does fall through, so the user can retry with another save.
        emit(&self.on_event, Event::HostLaunching);
        if let Err(e) = installer.install_and_launch().await {
            emit(&self.on_event, Event::HostLaunchFailed(format!("{e:#}")));
            // Bails rather than `ui::error`s — see the initial-build
            // arm above.
            anyhow::bail!("initial install failed: {e:#}");
        }
        emit(&self.on_event, Event::HostLaunched);
        whisker_build::ui::info(format!(
            "initial done · {} client(s) connected",
            sender.client_count()
        ));

        // Only now, after the fat build, are the capture caches
        // populated for `Patcher::initialize`. A failure is non-fatal:
        // Full Reloads carry the session and rerun the capture shims,
        // so a later save may find the caches repaired.
        let mut patcher = match hot_reload_init.as_ref() {
            Some(prep) => match init_patcher_for(&self.config, prep) {
                Ok(p) => {
                    whisker_build::ui::debug("hot-reload patcher ready");
                    Some(p)
                }
                Err(e) => {
                    whisker_build::ui::warn(format!(
                        "hot-reload patcher init failed ({e:#}); \
                         will retry on the next save — use R (Full Reload) meanwhile",
                    ));
                    None
                }
            },
            None => None,
        };

        // Command channel: `r` / `R` from the CLI. When the caller
        // didn't attach one, park a receiver whose sender is leaked
        // so `recv()` pends forever (a *closed* channel would return
        // `None` immediately and spin the select).
        let mut commands = self.commands.take().unwrap_or_else(parked_command_receiver);

        // Saves only ever hot-reload. Anything a patch can't express
        // — Cargo.toml edits, multi-crate batches, missing patcher,
        // patch build failures — *prompts* for an explicit Full
        // Reload rather than restarting the app behind the user's
        // back. The exception is a compile error in the user's code
        // (`RustcRejectedCode`): a Full Reload would fail identically,
        // so the loop just waits for the next save.
        loop {
            enum Input {
                Change(watcher::Change),
                Command(DevCommand),
            }
            let input = tokio::select! {
                c = rx.recv() => match c {
                    Some(change) => Input::Change(change),
                    None => break,
                },
                c = commands.recv() => match c {
                    Some(cmd) => Input::Command(cmd),
                    None => {
                        // Command side hung up — park a fresh
                        // never-yielding receiver and keep serving
                        // file changes.
                        commands = parked_command_receiver();
                        continue;
                    }
                },
            };
            match input {
                Input::Change(mut change) => {
                    whisker_build::ui::section("Change");
                    whisker_build::ui::debug(format!(
                        "{:?} — {} path(s)",
                        change.kind,
                        change.paths.len(),
                    ));
                    // whisker.rs first: it classifies as RustCode by
                    // extension, but no reload of any kind re-applies
                    // it (see `whisker_config_file` above).
                    if change.paths.iter().any(|p| p == &whisker_config_file) {
                        whisker_build::ui::warn(
                            "whisker.rs changed — configuration is applied at startup; \
                             restart `whisker run` to pick it up",
                        );
                        change.paths.retain(|p| p != &whisker_config_file);
                        if change.paths.is_empty() {
                            continue;
                        }
                    }
                    if change.kind == ChangeKind::RustCode {
                        ensure_patcher(&self.config, &hot_reload_init, &mut patcher);
                    }
                    match decide_action(change.kind, patcher.is_some()) {
                        LoopAction::Ignore => {
                            whisker_build::ui::debug(format!("ignored ({:?})", change.kind));
                        }
                        LoopAction::HotReload => {
                            let p = patcher.as_ref().expect("decide_action guarantees Some");
                            // `None` = the batch spans several crates
                            // or an unknown path. A patch covers one
                            // crate, so that needs a Full Reload.
                            let crate_key =
                                workspace::identify_crate_for_paths(&change.paths, &path_deps);
                            if !path_deps.is_empty() && crate_key.is_none() {
                                prompt_full_reload(
                                    &self.on_event,
                                    "change spans multiple crates (hot reload patches \
                                     one crate per save)",
                                );
                                continue;
                            }
                            hot_reload_cycle(p, &sender, &self.on_event, crate_key.as_deref())
                                .await;
                        }
                        LoopAction::PromptFullReload => {
                            let reason = match change.kind {
                                ChangeKind::CargoToml => {
                                    "Cargo.toml / Cargo.lock changed — the dependency \
                                     graph may have moved"
                                }
                                _ => "hot reload unavailable (patcher not initialized)",
                            };
                            prompt_full_reload(&self.on_event, reason);
                        }
                    }
                }
                Input::Command(DevCommand::HotReload) => {
                    whisker_build::ui::section("Hot Reload");
                    ensure_patcher(&self.config, &hot_reload_init, &mut patcher);
                    match patcher.as_ref() {
                        Some(p) => hot_reload_cycle(p, &sender, &self.on_event, None).await,
                        None => prompt_full_reload(
                            &self.on_event,
                            "hot reload unavailable (patcher not initialized)",
                        ),
                    }
                }
                Input::Command(DevCommand::FullReload) => {
                    whisker_build::ui::section("Full Reload");
                    run_build_cycle(&builder, &installer, &self.on_event, &sender, "full reload")
                        .await;
                }
                Input::Command(DevCommand::Relaunch) => {
                    emit(&self.on_event, Event::HostLaunching);
                    match installer.relaunch().await {
                        Ok(()) => emit(&self.on_event, Event::HostLaunched),
                        Err(error) => {
                            let message = format!("{error:#}");
                            whisker_build::ui::error(format!("relaunch failed: {message}"));
                            emit(&self.on_event, Event::HostLaunchFailed(message));
                        }
                    }
                }
                Input::Command(DevCommand::Shutdown) => break,
            }
        }

        Ok(())
    }
}

/// Decision the change loop makes for one debounced change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopAction {
    /// Drop on the floor — `ChangeKind::Other` doesn't warrant
    /// either a patch or a prompt.
    Ignore,
    /// Build and push a hot-reload patch.
    HotReload,
    /// The change can't be hot-reloaded. Tell the user why and wait
    /// for an explicit `R` (Full Reload) — never rebuild + restart
    /// the app automatically.
    PromptFullReload,
}

/// Pure decision helper for the change loop. Hot reload only handles
/// `ChangeKind::RustCode` and only when a Patcher is available;
/// `Cargo.toml` always needs a Full Reload because the dependency
/// graph may have shifted; everything else is ignored.
pub fn decide_action(kind: ChangeKind, has_patcher: bool) -> LoopAction {
    match kind {
        ChangeKind::Other => LoopAction::Ignore,
        ChangeKind::CargoToml => LoopAction::PromptFullReload,
        ChangeKind::RustCode if has_patcher => LoopAction::HotReload,
        ChangeKind::RustCode => LoopAction::PromptFullReload,
    }
}

/// A command receiver that never yields: fresh channel whose sender
/// is leaked (a few bytes, once per `run`). Used when the caller
/// attached no command channel, and after a real one hangs up —
/// `tokio::select!` on a *closed* receiver would return `None`
/// immediately in a hot loop.
fn parked_command_receiver() -> tokio::sync::mpsc::UnboundedReceiver<DevCommand> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    std::mem::forget(tx);
    rx
}

/// Report "this change needs a Full Reload" without running one.
/// Prints the reason + the `R` hint to the terminal and emits
/// [`Event::FullReloadRequired`] so the TUI can keep a persistent
/// banner up until the user acts.
fn prompt_full_reload(on_event: &Option<Arc<dyn Fn(Event) + Send + Sync>>, reason: &str) {
    whisker_build::ui::warn(format!("{reason} — press R to Full Reload"));
    emit(
        on_event,
        Event::FullReloadRequired {
            reason: reason.to_string(),
        },
    );
}

/// Late patcher init. A failed init at startup shouldn't disable hot
/// reload for the whole session: a Full Reload runs with the capture
/// shims wired, so the caches `init_patcher_for` reads may have been
/// repopulated since the failure. No-op when the patcher is already
/// up or hot reload wasn't configured.
#[cfg(test)]
#[path = "lib/tests.rs"]
mod tests;
