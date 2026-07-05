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

/// What kind of binary the dev server is rebuilding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Android cdylib + APK + adb install + launch.
    Android,
    /// iOS Simulator app + xcrun simctl install + launch.
    IosSimulator,
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
/// shortcuts in `whisker run`'s TUI: `r` / `R`). Reloads are
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
        // In TUI mode the live region's header already shows the
        // package + target + phase; the `──── whisker run ────` +
        // `· podcast · Android` rows above just duplicated that
        // information. The `mode={:?}` debug line is debug-only
        // anyway, so it never made it to scrollback in the
        // production curated path. Non-TUI runs (CI, `--no-tui`)
        // still get the intro section + info line.
        if !whisker_build::ui::is_tui() {
            whisker_build::ui::section("whisker run");
            whisker_build::ui::info(format!(
                "{} · {:?}",
                self.config.package, self.config.target,
            ));
        }
        whisker_build::ui::debug(format!("mode={:?}", self.config.hot_patch_mode));

        // Configure the initial build first. The Builder + Installer
        // pair doesn't need the WS server, so we wire them up before
        // touching the socket — this lets the user see a clean
        // "Initial build" section open immediately after the
        // top-level "whisker run" section, with no intervening
        // dev-server chatter. Once the cargo step (the long pole)
        // succeeds we bind the WS, then `install_and_launch` so the
        // device app has somewhere to connect to.
        //
        // For hot-reload mode this build doubles as the fat build that
        // fills the rustc / linker capture caches; the shims are
        // resolved (built if missing) and installed into the builder
        // *before* the spawn. The same Builder is reused for Full
        // fallback rebuilds inside the change loop.
        let mut builder = Builder::new(
            self.config.workspace_root.clone(),
            self.config.crate_dir.clone(),
            self.config.package.clone(),
            self.config.target,
        )
        .with_features(vec!["whisker/hot-reload".into()]);

        let hot_reload_init = if self.config.hot_patch_mode == HotPatchMode::HotReload {
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
            self.config.workspace_root.clone(),
            self.config.package.clone(),
            hot_reload_init.as_ref().map(|p| p.capture.clone()),
            builder.features().to_vec(),
            self.config.bind_addr.port(),
            self.config.dev_token.clone(),
        );

        // Initial build — cargo only. `install_and_launch` is
        // deferred until *after* the WS is bound, because the device
        // app spins up its `whisker-dev-runtime` socket as soon as it
        // launches and would race a not-yet-bound dev-server.
        //
        // A build failure here is fatal: there's nothing actionable
        // the dev-loop can do (no app to patch, no install to
        // recover from a source-edit save), so we surface the error
        // and exit. The previous behaviour of "enter the loop anyway
        // and recover on next save" was misleading — users routinely
        // missed the warn line and assumed the build had succeeded.
        //
        // The `──── Initial build ────` section header is only
        // emitted in non-TUI mode. In TUI mode the live region's
        // phase indicator (`building`) + spinner already make it
        // obvious that the build started; the section divider
        // becomes pure noise above the in-line cargo/gradle/
        // xcodebuild step rows.
        if !whisker_build::ui::is_tui() {
            whisker_build::ui::section("Initial build");
        }
        emit(&self.on_event, Event::BuildingFull);
        if let Err(e) = builder.build().await {
            let msg = format!("{e:#}");
            emit(&self.on_event, Event::BuildFailed(msg.clone()));
            // cli main prints the bail message via `ui::error` (it
            // formats `e.root_cause()`), so emitting our own
            // `ui::error` here would double-print every install /
            // build failure to the user. Keep the bail message
            // user-actionable; the verbose chain is still reachable
            // via `WHISKER_VERBOSE=1`.
            anyhow::bail!("initial build failed: {msg}");
        }
        emit(&self.on_event, Event::BuildSucceeded);

        // Now bind the WS so `install_and_launch` (next) has
        // somewhere for the device's `whisker-dev-runtime` to dial.
        // `whisker_build::ui::section("dev server")` used to live
        // here as a visual divider between the cargo build and the
        // device install/launch. The TUI's live region already
        // surfaces the ws addr + client count, so the section
        // header was a redundant row of dashes. `ensure_status` /
        // `set_status` are no-ops in TUI mode (see
        // `whisker_build::ui::set_status`); we keep them for the
        // `--no-tui` and CI paths where the legacy status surface
        // is still the only signal.
        whisker_build::ui::ensure_status("dev-server");
        let (sender, bound, _server_handle) = server::serve(
            self.config.bind_addr,
            self.on_event.clone(),
            self.config.dev_token.clone(),
        )
        .await?;
        whisker_build::ui::set_status(format!("ws://{bound} · 0 client(s)"));
        whisker_build::ui::debug(format!("ws://{bound}/whisker-dev"));

        // Walk the user crate's dep graph for every workspace path
        // dep. The watcher attaches one notify root per `src/`, and
        // the change loop uses the same list to map a changed file
        // back to its owning crate. Registry / git deps are excluded
        // — their sources live outside the workspace; Cargo.toml /
        // Cargo.lock edits prompt a Full Reload, which picks them
        // up.
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
        // Always include the user crate's src dir as a fallback even
        // if cargo metadata returned nothing — the dev loop should
        // still work in degraded mode.
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
            // Last-resort: watch the user_src path even if it doesn't
            // exist yet — notify will fail and we'll surface the
            // error to the user.
            watch_roots.push(user_src.clone());
        }
        // Caller-specified extras (the cli passes `<crate>/src` — already
        // covered above — plus `<crate>/whisker.rs`). Files are fine:
        // notify watches single files too.
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

        // Install + launch the freshly-built artifact. A failure
        // here is fatal for the same reason a build failure is —
        // there's nothing to dev-loop against if the app never made
        // it onto the device (no `INSTALL_FAILED_INSUFFICIENT_STORAGE`
        // recovery story over file watching). `run_build_cycle`
        // reuses the build + install codepath for rebuilds inside
        // the loop (where WS is already bound and a failure there
        // does fall through — the user can then save again to retry).
        if let Err(e) = installer.install_and_launch().await {
            // See the `initial build failed` arm above for why we
            // bail instead of `ui::error`-ing here.
            anyhow::bail!("initial install failed: {e:#}");
        }
        whisker_build::ui::info(format!(
            "initial done · {} client(s) connected",
            sender.client_count()
        ));

        // After the fat build has happened, Patcher::initialize can
        // read the now-populated caches. Failure here is non-fatal
        // — log, prompt Full Reloads meanwhile, and retry the init
        // on later changes (each Full Reload runs with the capture shims
        // wired, so the caches that were missing or stale here may
        // have been repopulated by the time the user saves again).
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

        // Dev loop. Two input sources: debounced file changes and
        // explicit DevCommands (`r` Hot Reload / `R` Full Reload).
        //
        // Saves only ever hot-reload. Anything a patch can't express
        // — Cargo.toml edits, multi-crate batches, missing patcher,
        // patch build failures — *prompts* for an explicit Full
        // Reload instead of rebuilding + restarting the app behind
        // the user's back (an unexpected restart mid-interaction
        // costs more than it saves). The one non-prompt failure is a
        // compile error in the user's code (`RustcRejectedCode`): a
        // Full Reload would fail identically, so the loop reports
        // and waits for the next save.
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
                    // `──── Change ────` only in non-TUI mode (live
                    // region's phase flip already announces a save
                    // has been picked up).
                    if !whisker_build::ui::is_tui() {
                        whisker_build::ui::section("Change");
                    }
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
                            // Map the changed file paths to the owning
                            // crate. None = batch spans multiple crates
                            // or a path outside every known src dir —
                            // a patch covers one crate per batch, so
                            // that needs a Full Reload.
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
                    if !whisker_build::ui::is_tui() {
                        whisker_build::ui::section("Hot Reload");
                    }
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
                    if !whisker_build::ui::is_tui() {
                        whisker_build::ui::section("Full Reload");
                    }
                    run_build_cycle(&builder, &installer, &self.on_event, &sender, "full reload")
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
fn ensure_patcher(
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
async fn hot_reload_cycle(
    patcher: &hotpatch::Patcher,
    sender: &PatchSender,
    on_event: &Option<Arc<dyn Fn(Event) + Send + Sync>>,
    crate_key: Option<&str>,
) {
    // Open the step as soon as we know we're patching; the spinner
    // runs across both the build + the wire-up so the user sees a
    // single elapsed duration for "edit → app updated".
    let step = whisker_build::ui::step("hot reload", "subsecond patch");
    // Tell the cli to flip into "reloading" right now — the patcher
    // work that follows (`build_patch` + dylib read + send) is the
    // wall-clock-heavy bit, and the matching `PatchSent` flips back
    // to Idle.
    emit(on_event, Event::PatchBuilding);
    let Some(aslr_reference) = sender.latest_aslr_reference() else {
        // No client has reported its `aslr_reference` yet (handshake
        // hasn't completed, or never connected). Without that value
        // a stub-asm-style patch can't be built.
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
            let send_started = std::time::Instant::now();
            let n = sender.send(Patch {
                table: plan.table,
                dylib_bytes,
            });
            whisker_build::ui::debug(format!(
                "built {built_in:?} · queued {:?}",
                send_started.elapsed()
            ));
            step.done(format!("{n} client(s)"));
            emit(on_event, Event::PatchSent);
        }
        Err(e) if e.downcast_ref::<hotpatch::RustcRejectedCode>().is_some() => {
            // The user's code doesn't compile. A Full Reload would
            // fail with the exact same diagnostics after a 10-30s
            // wait, so don't prompt for one — rustc already printed
            // them (stderr is inherited). Report and wait for the
            // next save.
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
fn log_patch_diff(report: &hotpatch::DiffReport) {
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
        // Removed-symbol counts are noisy on every patch (typically
        // thousands of `GCC_except_table*` entries that compiled
        // away). Surface only in verbose mode; the "host shell may
        // crash" warning was alarmist for the normal case.
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
struct HotReloadPrep {
    capture: CaptureShims,
    real_linker: PathBuf,
}

/// Resolve shim paths (building them if missing) and assemble the
/// CaptureShims wiring. Returns `Err` if the shim binaries can't be
/// produced, in which case hot reload is unavailable for the session.
///
/// `config` carries the workspace + target + android/ios params the
/// linker/triple pickers need:
///   - Android → NDK clang for `config.android.abi`.
///   - others → host clang via [`hotpatch::wrapper::resolve_host_linker`].
fn prepare_hot_reload_capture(config: &Config) -> Result<HotReloadPrep> {
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
fn target_triple_for(config: &Config) -> Option<String> {
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
            // Pick the simulator triple that matches the host arch
            // running `whisker run`. Both arm64 Macs (`aarch64-apple-
            // ios-sim`) and Intel Macs (`x86_64-apple-ios`) need a
            // simulator slice. The Build Phase that xcodebuild fires
            // (via `whisker build-ios`) cross-compiles whichever arch
            // Xcode requests via `$ARCHS`; the hot-patch path rebuilds
            // just the thin obj for whichever triple the user is on.
            let triple = match std::env::consts::ARCH {
                "aarch64" => "aarch64-apple-ios-sim",
                "x86_64" => "x86_64-apple-ios",
                _ => return None,
            };
            Some(triple.to_string())
        }
    }
}

/// Pick the linker driver to use for `config.target`. Returned path
/// is what the linker shim forwards to during the fat build *and*
/// what the thin-rebuild link step spawns directly — the same binary
/// on both sides keeps SDK / sysroot resolution consistent.
fn resolve_linker_for(config: &Config) -> Result<PathBuf> {
    match config.target {
        Target::Android => {
            let abi = config
                .android
                .as_ref()
                .map(|a| a.abi.as_str())
                .unwrap_or("arm64-v8a");
            // API level: env override > 21 (Lynx baseline).
            let api = std::env::var("WHISKER_ANDROID_API")
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(21);
            hotpatch::android_ndk::android_clang_for(abi, api)
                .with_context(|| format!("resolve NDK clang for ABI {abi} API {api}"))
        }
        Target::IosSimulator => Ok(hotpatch::wrapper::resolve_host_linker()),
    }
}

/// Construct the patcher from the captures the fat build just wrote.
/// Splits out so [`DevServer::run`] is easier to read.
fn init_patcher_for(config: &Config, prep: &HotReloadPrep) -> Result<hotpatch::Patcher> {
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
fn original_binary_path(config: &Config) -> Result<PathBuf> {
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
    }
}

/// Map an Android ABI name to the camel-cased form gradle's
/// `WhiskerProjectPlugin` uses when synthesising
/// `whiskerBuild<Variant><AbiCamel>` task names. Each `-` or `_`
/// segment is titlecased and the parts are concatenated:
/// `arm64-v8a` → `Arm64V8a`, `armeabi-v7a` → `ArmeabiV7a`,
/// `x86_64` → `X8664`, `x86` → `X86`. Mirrors `String.toCamelCase()`
/// in `WhiskerProjectPlugin.kt`.
fn android_abi_to_camel(abi: &str) -> String {
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

fn target_os_for(target: Target) -> hotpatch::LinkerOs {
    match target {
        Target::Android => hotpatch::LinkerOs::Linux,
        Target::IosSimulator => hotpatch::LinkerOs::Macos,
    }
}

/// Slurp the patch dylib off disk so the dev-loop can hand it to the
/// WebSocket sender. The size is typically tens of KB (only the
/// changed crate's `.o` linked with `-undefined dynamic_lookup`), and
/// since switching to the binary frame protocol we ship it verbatim
/// — no base64.
fn read_lib_bytes(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("read {}", path.display()))
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
    fn config_defaults_pick_loopback_and_full_reload_only() {
        let cfg = Config::defaults_for(
            PathBuf::from("/tmp/ws"),
            "hello-world".to_string(),
            Target::Android,
        );
        assert_eq!(cfg.workspace_root, Path::new("/tmp/ws"));
        assert_eq!(cfg.package, "hello-world");
        assert_eq!(cfg.target, Target::Android);
        assert_eq!(cfg.bind_addr.port(), 9876);
        assert!(cfg.bind_addr.ip().is_loopback());
        assert_eq!(cfg.hot_patch_mode, HotPatchMode::FullReloadOnly);
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
        assert_ne!(HotPatchMode::HotReload, HotPatchMode::FullReloadOnly,);
    }

    #[test]
    fn dev_server_new_does_not_fail_for_a_well_formed_config() {
        let cfg = Config::defaults_for(
            PathBuf::from("/tmp/ws"),
            "hello-world".to_string(),
            Target::Android,
        );
        assert!(DevServer::new(cfg).is_ok());
    }

    // ----- original_binary_path ----------------------------------------

    fn mk_config(workspace_root: PathBuf, target: Target) -> Config {
        let mut cfg = Config::defaults_for(workspace_root.clone(), "hello-world".into(), target);
        cfg.crate_dir = workspace_root.clone();
        match target {
            Target::Android => {
                cfg.android = Some(crate::AndroidParams {
                    project_dir: workspace_root.join("android"),
                    application_id: "rs.whisker.examples.helloworld".into(),
                    launcher_activity: ".MainActivity".into(),
                    abi: "arm64-v8a".into(),
                });
            }
            Target::IosSimulator => {
                cfg.ios = Some(crate::IosParams {
                    project_dir: workspace_root.join("ios"),
                    scheme: "HelloWorld".into(),
                    bundle_id: "rs.whisker.examples.helloWorld".into(),
                    device_override: None,
                });
            }
        }
        cfg
    }

    #[test]
    fn original_binary_path_finds_ios_simulator_dylib_under_target() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let ws = std::env::temp_dir().join(format!("whisker-dev-test-ios-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&ws);
        let triple = match std::env::consts::ARCH {
            "aarch64" => "aarch64-apple-ios-sim",
            "x86_64" => "x86_64-apple-ios",
            other => panic!("unsupported test host arch {other}"),
        };
        let release_dir = ws.join("target").join(triple).join("release");
        std::fs::create_dir_all(&release_dir).unwrap();
        let dylib = release_dir.join("libhello_world.dylib");
        std::fs::write(&dylib, b"fake-macho").unwrap();

        let cfg = mk_config(ws.clone(), Target::IosSimulator);
        let resolved = original_binary_path(&cfg).unwrap();
        assert_eq!(resolved, dylib);

        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn original_binary_path_errors_when_ios_simulator_dylib_missing() {
        let cfg = mk_config(PathBuf::from("/nonexistent/ws"), Target::IosSimulator);
        let res = original_binary_path(&cfg);
        assert!(res.is_err());
    }

    #[test]
    fn original_binary_path_finds_android_so_under_gradle_output() {
        // Reads from the gradle plugin's `@OutputDirectory`, not from
        // `target/<triple>/debug/` — the latter can be cleaned out by
        // `cargo clean` while gradle still reports its task as
        // UP-TO-DATE (the cargo target dir is `@Internal`, not an
        // input). See `original_binary_path` for the rationale.
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let ws = std::env::temp_dir().join(format!("whisker-dev-test-orig-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&ws);
        // `mk_config` sets `crate_dir = ws` for Android, so the path
        // the patcher checks is `<ws>/gen/android/app/build/generated/
        // jniLibs/whiskerBuildDebug<AbiCamel>/<abi>/lib<pkg>.so`.
        let gradle_out_dir = ws
            .join("gen/android/app/build/generated/jniLibs")
            .join("whiskerBuildDebugArm64V8a")
            .join("arm64-v8a");
        std::fs::create_dir_all(&gradle_out_dir).unwrap();
        let so = gradle_out_dir.join("libhello_world.so");
        std::fs::write(&so, b"fake").unwrap();

        let cfg = mk_config(ws.clone(), Target::Android);
        let resolved = original_binary_path(&cfg).unwrap();
        assert_eq!(resolved, so);

        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn android_abi_to_camel_matches_gradle_plugin_naming() {
        // Mirrors `WhiskerProjectPlugin.kt::String.toCamelCase`. The
        // patcher's task-name suffix has to match exactly or the
        // gradle output path won't resolve.
        assert_eq!(android_abi_to_camel("arm64-v8a"), "Arm64V8a");
        assert_eq!(android_abi_to_camel("armeabi-v7a"), "ArmeabiV7a");
        assert_eq!(android_abi_to_camel("x86_64"), "X8664");
        assert_eq!(android_abi_to_camel("x86"), "X86");
    }

    #[test]
    fn original_binary_path_errors_when_android_so_missing() {
        let cfg = mk_config(PathBuf::from("/nonexistent/ws"), Target::Android);
        let res = original_binary_path(&cfg);
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
    fn rust_code_with_patcher_chooses_hot_reload() {
        assert_eq!(
            decide_action(ChangeKind::RustCode, true),
            LoopAction::HotReload,
        );
    }

    #[test]
    fn rust_code_without_patcher_prompts_full_reload() {
        assert_eq!(
            decide_action(ChangeKind::RustCode, false),
            LoopAction::PromptFullReload,
        );
    }

    #[test]
    fn cargo_toml_always_prompts_full_reload_even_with_patcher() {
        // Patcher can't reload deps — Cargo.toml needs a full
        // rebuild regardless of which mode we're in.
        assert_eq!(
            decide_action(ChangeKind::CargoToml, true),
            LoopAction::PromptFullReload,
        );
        assert_eq!(
            decide_action(ChangeKind::CargoToml, false),
            LoopAction::PromptFullReload,
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
