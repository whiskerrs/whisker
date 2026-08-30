//! `whisker run` — start the dev server.
//!
//! Thin wrapper: resolves the user crate's `whisker.rs` config (via
//! [`super::manifest::resolve`] + [`super::probe::run`]), translates
//! the resulting [`whisker_config::Config`] into a flat
//! [`whisker_dev_server::Config`], and hands off to
//! `DevServer::run`. All the heavy lifting (file watch / cargo build
//! / WebSocket push / subsecond patches) lives in
//! `whisker-dev-server` so other host shells (an editor plugin, a
//! notebook front-end, …) can reuse the same loop without a
//! whisker-config dependency.

use anyhow::{Context, Result, anyhow};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use whisker_dev_server::{AndroidParams, Config, DevServer, HotPatchMode, IosParams, Target};

use crate::manifest;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// Path to the user crate's `Cargo.toml`. Defaults to walking up
    /// from `cwd` until a `Cargo.toml` with a `[package]` section is
    /// found (cargo-style).
    #[arg(long)]
    pub manifest_path: Option<PathBuf>,

    /// Where to deploy the rebuilt artifact. Positional so the
    /// common case (`whisker run android` / `whisker run ios` /
    /// `whisker run desktop`) reads
    /// naturally without a `--target=` prefix.
    #[arg(value_enum)]
    pub target: CliTarget,

    /// Development bind address. Mobile uses it for the hot-reload WebSocket;
    /// Web uses it for the local HTTP server.
    #[arg(long, default_value = "127.0.0.1:9876")]
    pub bind: SocketAddr,

    /// Disable Hot Reload (subsecond patching). Every save then
    /// prompts for an explicit Full Reload (`R`) instead. For
    /// situations where the hot-patch path is misbehaving and you
    /// just want the slower-but-bulletproof path.
    #[arg(long)]
    pub no_hot_patch: bool,

    /// Override the workspace root (= directory containing the
    /// `Cargo.toml` with `[workspace]`). Defaults to walking up from
    /// the resolved manifest's parent dir.
    #[arg(long)]
    pub workspace_root: Option<PathBuf>,

    /// Disable the inline ratatui status bar at the bottom of the
    /// terminal. On by default when stderr is a TTY; auto-off when
    /// piping to a file or running under CI. Use this when running
    /// against a tmux pane that doesn't like inline viewports, or
    /// when you specifically want grep'able scrollback-only output.
    #[arg(long)]
    pub no_tui: bool,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliTarget {
    Android,
    Ios,
    Desktop,
    Web,
}

impl From<CliTarget> for Target {
    fn from(t: CliTarget) -> Self {
        match t {
            CliTarget::Android => Target::Android,
            CliTarget::Ios => Target::IosSimulator,
            CliTarget::Desktop => Target::Macos,
            CliTarget::Web => Target::Web,
        }
    }
}

pub fn run(args: Args) -> Result<()> {
    // Set the cross-crate TUI signal before any `whisker_build::ui::*`
    // call fires — `whisker_build::ui::mode()` caches its lookup in a
    // `OnceLock` on the first call, so flipping this env later doesn't
    // unstick a `Curated` cache.
    let tui_enabled = args.target != CliTarget::Web
        && !args.no_tui
        && std::io::IsTerminal::is_terminal(&std::io::stderr());
    if tui_enabled {
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("WHISKER_TUI", "1") };
    }

    // Resolve the user-facing manifest before doing anything UI-y so
    // that the TUI header can display the bundle id from the moment
    // it first paints.
    let m = manifest::resolve(args.manifest_path.as_deref())
        .context("resolve user-crate manifest (Cargo.toml + whisker.rs)")?;
    let workspace_root = match &args.workspace_root {
        Some(p) => p.clone(),
        None => find_workspace_root(&m.crate_dir).ok_or_else(|| {
            anyhow!(
                "no [workspace] Cargo.toml at or above {}",
                m.crate_dir.display()
            )
        })?,
    };
    let target: Target = args.target.into();
    if target == Target::Macos && !cfg!(target_os = "macos") {
        return Err(anyhow!(
            "`whisker run desktop` currently supports the macOS Host only"
        ));
    }
    let target_label = target_label(target);
    let bundle = m
        .config
        .bundle_id
        .clone()
        .unwrap_or_else(|| m.package.clone());

    // Start the TUI as the very first user-visible action so the
    // long setup steps (sync, plugin build, initial build, install)
    // render with a proper progress indicator instead of leaking
    // ahead of an inline status bar.
    let tui_pieces = if tui_enabled {
        match crate::tui::Tui::start(target_label.to_string(), bundle.clone()) {
            Ok((tui, handle)) => {
                handle.set_phase(crate::tui::AppPhase::Setup);
                let render_handle = std::thread::Builder::new()
                    .name("whisker-tui-render".into())
                    .spawn(move || run_tui_render_loop(tui))
                    .ok();
                Some((handle, render_handle))
            }
            Err(e) => {
                eprintln!("couldn't start TUI ({e:#}); falling back to plain output");
                None
            }
        }
    } else {
        None
    };
    let tui_handle = tui_pieces.as_ref().map(|(h, _)| h.clone());

    // Run the rest of the cli pipeline. Each phase pushes its progress
    // through `tui_handle`. If the TUI isn't running, every step is
    // a no-op + the existing `whisker_build::ui::*` lines fall back
    // to scrollback.
    let result = run_inner(args, m, workspace_root, target, tui_handle.as_ref());

    // Stop the render thread + restore the terminal. Use should_quit
    // as the signal so the render thread exits cleanly.
    if let Some((handle, render_thread)) = tui_pieces {
        handle.request_quit();
        if let Some(t) = render_thread {
            let _ = t.join();
        }
    }
    result
}

fn run_tui_render_loop(mut tui: crate::tui::Tui) {
    let _ = tui.render_until_quit();
    let user_quit = tui.was_user_quit();
    let _ = tui.shutdown();
    if user_quit {
        // The dev-server runs to completion (i.e. forever) inside
        // `rt.block_on(server.run())` on the cli thread, so simply
        // tearing the TUI down here would leave a headless `whisker
        // run` process alive after `q`. Hard-exit with a normal
        // status; tokio sockets / file watchers get reaped by the
        // kernel. cli-initiated shutdowns (build failed, etc.) take
        // the other branch and let `run()`'s normal return path
        // surface the error.
        //
        // `exit` skips destructors, so an in-flight cargo / gradle /
        // xcodebuild would be orphaned — SIGTERM the tracked build
        // children first (the gradle daemon, in its own session, is
        // spared).
        whisker_build::child_guard::kill_all();
        std::process::exit(0);
    }
}

fn run_inner(
    args: Args,
    m: manifest::ResolvedManifest,
    workspace_root: PathBuf,
    target: Target,
    tui: Option<&crate::tui::TuiHandle>,
) -> Result<()> {
    // `set_phase(Setup)` already fired in `run()`; re-issuing it here
    // would duplicate the "▶ Setup" scrollback entry.
    //
    // Android and iOS generation is self-contained. Their bootstrap projects
    // are plain platform applications backed by the Whisker Host SDK.

    // cng templates are `include_str!`-baked into this binary, so a CLI
    // older than the sources under `crates/whisker-cng/src` renders
    // stale gen/ files. Only fires inside a whisker checkout.
    warn_if_cli_older_than_cng(&workspace_root);

    let sync = crate::platforms::sync_for_target(
        target,
        &m.config,
        &m.crate_dir,
        &workspace_root,
        &m.package,
    )
    .context("sync generated platform project (gen/<platform>/)")?;
    // Always say which path was taken: a template edit that didn't bump
    // cng's `template_version` leaves the old gen/ tree on disk, and a
    // silent "reused" makes that undiagnosable.
    let tv = sync
        .template_version
        .map(|v| format!(" (template_version {v})"))
        .unwrap_or_default();
    if sync.regenerated {
        whisker_build::ui::info(format!(
            "regenerated gen/ at {}{tv}",
            sync.gen_dir.display(),
        ));
    } else {
        whisker_build::ui::info(format!(
            "reused cached gen/ at {}{tv}",
            sync.gen_dir.display(),
        ));
    }

    if target == Target::Macos {
        return run_macos(
            &m,
            &workspace_root,
            &sync.gen_dir,
            &watch_paths_for(&m),
            tui,
        );
    }
    if target == Target::Web {
        return run_web(&sync.gen_dir, args.bind);
    }

    let android = match target {
        Target::Android => Some(android_params_from(&m, &sync.gen_dir)?),
        _ => None,
    };
    let ios = match target {
        Target::IosSimulator => Some(ios_params_from(&m, &sync.gen_dir)?),
        _ => None,
    };

    let watch_paths = watch_paths_for(&m);

    // Android/iOS currently stop at the native-shell bootstrap boundary.
    // There is deliberately no Rust dylib or patch receiver until the mobile
    // FramePacket ABI lands, so advertising subsecond hot reload here would
    // only produce a capture/connection failure after an otherwise healthy
    // launch. Full Reload remains available while this slice is in place.
    if !args.no_hot_patch {
        whisker_build::ui::info(
            "mobile native-shell mode · Rust hot reload activates with the renderer ABI slice",
        );
    }
    let config = Config {
        workspace_root,
        crate_dir: m.crate_dir,
        package: m.package,
        target,
        watch_paths: watch_paths.clone(),
        bind_addr: args.bind,
        // Random per-session token authenticating the device to the
        // hot-reload WebSocket. The patch channel `dlopen`s whatever it
        // receives, so without this an unauthenticated peer on a
        // LAN-exposed bind could push arbitrary native code; the gate
        // also defends an accidental `--bind 0.0.0.0`.
        dev_token: Some(generate_dev_token()),
        hot_patch_mode: HotPatchMode::FullReloadOnly,
        android,
        ios,
    };

    let watching_paths: Vec<String> = watch_paths
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    if let Some(t) = tui {
        t.set_dev_server(config.bind_addr.to_string(), watching_paths);
        t.set_phase(crate::tui::AppPhase::Initializing);
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;
    let tui_for_events = tui.cloned();

    // Reload shortcuts (`r` / `R` in the TUI) flow through this
    // channel into the dev loop. Reloads are user-triggered only —
    // the dev-server never full-reloads on its own.
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    if let Some(t) = tui {
        t.set_command_sender(cmd_tx);
    }

    let server = DevServer::new(config)?
        .with_command_receiver(cmd_rx)
        .on_event(move |e| {
            if let Some(h) = &tui_for_events {
                // `apply_event` already puts `Event::DeviceLog` in
                // scrollback; also calling `forward_event_to_ui` would
                // print each device line twice (raw, then again via
                // `ui::info` captured back through stderr).
                h.apply_event(&e);
            } else {
                forward_event_to_ui(e);
            }
        });

    rt.block_on(server.run())
}

fn watch_paths_for(manifest: &manifest::ResolvedManifest) -> Vec<PathBuf> {
    vec![
        manifest.crate_dir.join("src"),
        manifest.crate_dir.join("Cargo.toml"),
        manifest.crate_dir.join("whisker.rs"),
    ]
}

/// Run the generated browser project. Trunk owns incremental WASM rebuilds,
/// serves the output, opens the browser, and reloads the page after each
/// successful build; a page reload remounts the Whisker runtime from scratch.
fn run_web(gen_dir: &Path, bind: SocketAddr) -> Result<()> {
    whisker_build::ui::info(format!(
        "starting Web Host at http://{bind} · source changes remount the app"
    ));
    let status = std::process::Command::new("trunk")
        .arg("serve")
        .arg("--config")
        .arg(gen_dir.join("Trunk.toml"))
        .arg("--address")
        .arg(bind.ip().to_string())
        .arg("--port")
        .arg(bind.port().to_string())
        .arg("--open")
        // Whisker's subprocess UI convention uses `NO_COLOR=1`, while
        // Trunk's clap parser accepts the boolean spellings only.
        .env("NO_COLOR", "true")
        .current_dir(gen_dir)
        .status()
        .context(
            "start Trunk for Web Host (install it with `cargo install trunk --locked` if missing)",
        )?;
    if !status.success() {
        return Err(anyhow!("Trunk Web development server exited with {status}"));
    }
    Ok(())
}

/// Drive the first Desktop development loop. The generated Cargo project is
/// exactly the one `whisker build macos` consumes; development adds only file
/// watching and process supervision around its Debug build.
fn run_macos(
    manifest: &manifest::ResolvedManifest,
    workspace_root: &Path,
    gen_dir: &Path,
    explicit_watch_paths: &[PathBuf],
    tui: Option<&crate::tui::TuiHandle>,
) -> Result<()> {
    let app_name = manifest
        .config
        .name
        .as_deref()
        .ok_or_else(|| anyhow!("whisker.rs: app.name(\"…\") is required for macOS"))?;
    let binary_name = format!("{}-whisker-macos", manifest.package);
    let target_dir = workspace_root.join("target/.whisker/macos");

    let mut watch_roots: Vec<PathBuf> = whisker_dev_server::discover_path_deps(
        &manifest.crate_dir.join("Cargo.toml"),
        &manifest.package,
    )
    .unwrap_or_default()
    .into_iter()
    .map(|dependency| dependency.src_dir)
    .filter(|path| path.is_dir())
    .collect();
    for path in explicit_watch_paths {
        if path.exists() && !watch_roots.contains(path) {
            watch_roots.push(path.clone());
        }
    }
    if watch_roots.is_empty() {
        watch_roots.push(manifest.crate_dir.join("src"));
    }
    if let Some(tui) = tui {
        tui.set_dev_server(
            "local process",
            watch_roots
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
        );
        tui.apply_event(&whisker_dev_server::Event::BuildingFull);
    }

    let build = || {
        whisker_build::macos::build_app(&whisker_build::macos::MacosBuild {
            project_dir: gen_dir,
            target_dir: &target_dir,
            app_name,
            binary_name: &binary_name,
            profile: whisker_build::Profile::Debug,
        })
    };
    let bundle = match build() {
        Ok(bundle) => bundle,
        Err(error) => {
            if let Some(tui) = tui {
                tui.apply_event(&whisker_dev_server::Event::BuildFailed(format!(
                    "{error:#}"
                )));
            }
            return Err(error).context("initial macOS build");
        }
    };
    if let Some(tui) = tui {
        tui.apply_event(&whisker_dev_server::Event::BuildSucceeded);
        tui.apply_event(&whisker_dev_server::Event::Started);
    }
    let mut running = launch_macos_bundle(&bundle, &binary_name)?;
    let launch_binary_name = binary_name.clone();
    whisker_build::ui::info(format!(
        "launched {} · watching for rebuild/relaunch",
        bundle.display()
    ));

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build macOS development runtime")?;
    runtime.block_on(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let _watcher = whisker_dev_server::watcher::spawn_watcher(
            watch_roots,
            std::time::Duration::from_millis(200),
            tx,
        )?;
        while let Some(change) = rx.recv().await {
            let config_file = manifest.crate_dir.join("whisker.rs");
            if change.paths.iter().any(|path| path == &config_file) {
                whisker_build::ui::warn(
                    "whisker.rs changed — restart `whisker run desktop` to regenerate gen/macos",
                );
                continue;
            }
            if change.kind == whisker_dev_server::ChangeKind::Other {
                continue;
            }
            if let Some(tui) = tui {
                tui.apply_event(&whisker_dev_server::Event::BuildingFull);
            }
            match build() {
                Ok(new_bundle) => {
                    if let Some(tui) = tui {
                        tui.apply_event(&whisker_dev_server::Event::BuildSucceeded);
                    }
                    stop_macos_app(&mut running);
                    running = launch_macos_bundle(&new_bundle, &launch_binary_name)?;
                    whisker_build::ui::info("macOS app rebuilt and relaunched");
                }
                Err(error) => {
                    if let Some(tui) = tui {
                        tui.apply_event(&whisker_dev_server::Event::BuildFailed(format!(
                            "{error:#}"
                        )));
                    }
                    whisker_build::ui::warn(format!(
                        "macOS rebuild failed; the previous app remains running: {error:#}"
                    ));
                }
            }
        }
        Ok::<(), anyhow::Error>(())
    })
}

struct RunningMacosApp {
    child: std::process::Child,
    _track: whisker_build::child_guard::TrackGuard,
}

fn launch_macos_bundle(bundle: &Path, binary_name: &str) -> Result<RunningMacosApp> {
    let executable = bundle.join("Contents/MacOS").join(binary_name);
    let child = std::process::Command::new(&executable)
        .current_dir(bundle)
        .spawn()
        .with_context(|| format!("launch {}", executable.display()))?;
    let track = whisker_build::child_guard::track(child.id());
    Ok(RunningMacosApp {
        child,
        _track: track,
    })
}

fn stop_macos_app(app: &mut RunningMacosApp) {
    let _ = app.child.kill();
    let _ = app.child.wait();
}

/// Generate a random hex token for the hot-reload session.
///
/// Reads 16 bytes from `/dev/urandom` (every host we run on is POSIX)
/// and hex-encodes them into a 32-char token. If `/dev/urandom` is
/// somehow unreadable we fall back to a time+pid-seeded value — weaker,
/// but the token only needs to be unguessable within a dev session on
/// the local machine, and the dev loop shouldn't hard-fail over it.
fn generate_dev_token() -> String {
    let mut buf = [0u8; 16];
    let strong = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut buf))
        .is_ok();
    if !strong {
        // Fallback seed: nanos since epoch XOR pid, splatted across the
        // buffer. Not cryptographic, but non-constant per session.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seed = nanos ^ (std::process::id() as u128);
        for (i, b) in buf.iter_mut().enumerate() {
            *b = (seed >> ((i % 16) * 8)) as u8;
        }
    }
    let mut s = String::with_capacity(32);
    for b in buf {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Friendly label for the TUI header. `whisker_dev_server::Target`'s
/// Debug impl renders `IosSimulator`, which is a mouthful for the
/// screen real estate available.
fn target_label(target: Target) -> &'static str {
    match target {
        Target::Android => "Android",
        Target::IosSimulator => "iOS Simulator",
        Target::Macos => "Desktop (macOS)",
        Target::Web => "Web",
    }
}

/// Translate dev-server [`Event`]s into line-based UI output — the
/// non-TUI path. Only the device's own stdout/stderr needs surfacing
/// here; everything else is already covered by `whisker_build::ui`
/// calls inside the dev loop.
///
fn forward_event_to_ui(event: whisker_dev_server::Event) {
    use whisker_dev_server::Event;
    if let Event::DeviceLog {
        stream,
        line,
        ts_micros: _,
    } = event
    {
        // Short prefix so the column alignment stays compact next to
        // `whisker_build::ui::info`'s own output.
        let tag = match stream.as_str() {
            "stderr" => "device:err",
            _ => "device",
        };
        whisker_build::ui::info(format!("[{tag}] {line}"));
    }
}

/// Build [`AndroidParams`] from the resolved manifest. Returns an
/// error if the user's `whisker.rs` left required fields (like the
/// `applicationId`) unset.
///
/// `project_dir` is the *generated* Gradle project under
/// `gen/android/` — `whisker-cng` writes the tree, this function just
/// stitches in the `applicationId` + launcher activity for installer
/// use.
fn android_params_from(
    m: &manifest::ResolvedManifest,
    project_dir: &Path,
) -> Result<AndroidParams> {
    let a = &m.config.android;
    let application_id = a
        .application_id
        .clone()
        .or_else(|| m.config.bundle_id.clone())
        .ok_or_else(|| {
            anyhow!(
                "whisker.rs: app.android(|a| a.application_id(\"…\")) is required for the android target"
            )
        })?;
    let launcher_activity = a
        .launcher_activity
        .clone()
        .unwrap_or_else(|| ".MainActivity".into());
    Ok(AndroidParams {
        project_dir: project_dir.to_path_buf(),
        application_id,
        launcher_activity,
        // Single-ABI dev loops only — multi-ABI is a release concern.
        abi: "arm64-v8a".into(),
    })
}

/// Build [`IosParams`] from the resolved manifest. `project_dir` is
/// the generated `gen/ios/` tree (after `whisker-cng` + xcodegen
/// have run).
fn ios_params_from(m: &manifest::ResolvedManifest, project_dir: &Path) -> Result<IosParams> {
    let i = &m.config.ios;
    let bundle_id = i
        .bundle_id
        .clone()
        .or_else(|| m.config.bundle_id.clone())
        .ok_or_else(|| {
            anyhow!(
                "whisker.rs: app.ios(|i| i.bundle_id(\"…\")) or app.bundle_id(\"…\") is required for the ios target"
            )
        })?;
    let scheme = i
        .scheme
        .clone()
        .or_else(|| m.config.name.clone())
        .ok_or_else(|| {
            anyhow!(
                "whisker.rs: app.ios(|i| i.scheme(\"…\")) or app.name(\"…\") is required for the ios target"
            )
        })?;
    Ok(IosParams {
        project_dir: project_dir.to_path_buf(),
        scheme,
        bundle_id,
        device_override: std::env::var("WHISKER_IOS_SIMULATOR").ok(),
    })
}

/// Walk up from `start` looking for a `Cargo.toml` containing a
/// `[workspace]` section. Returns the directory holding the matching
/// Cargo.toml, or `None` if we walk off the top of the filesystem.
pub(crate) fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    // Canonicalize so the upward walk doesn't bottom out at an empty
    // PathBuf when `start` is relative and the workspace root happens
    // to be the process's cwd. An empty `workspace_root` later feeds
    // `Command::current_dir("")`, which posix-spawns ENOENT and
    // surfaces as "spawn cargo: No such file or directory".
    let mut cur = std::fs::canonicalize(start).unwrap_or_else(|_| start.to_path_buf());
    loop {
        let cargo = cur.join("Cargo.toml");
        if cargo.is_file() {
            if let Ok(txt) = std::fs::read_to_string(&cargo) {
                if txt.contains("[workspace]") {
                    return Some(cur);
                }
            }
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// Warn if the running `whisker` binary predates the cng
/// template/renderer sources, i.e. its `include_str!`-baked templates
/// are stale relative to the repo. No-op unless
/// `crates/whisker-cng/src` exists under `workspace_root` — installed
/// users never see it. Read-only: compares mtimes, warns, nothing else.
fn warn_if_cli_older_than_cng(workspace_root: &Path) {
    let cng_src = workspace_root.join("crates/whisker-cng/src");
    if !cng_src.is_dir() {
        return; // not a whisker repo checkout
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Ok(cli_mtime) = std::fs::metadata(&exe).and_then(|m| m.modified()) else {
        return;
    };
    let Some(cng_mtime) = newest_mtime_under(&cng_src) else {
        return;
    };
    if cng_mtime > cli_mtime {
        whisker_build::ui::warn(format!(
            "running `whisker` ({}) is older than crates/whisker-cng/src — its \
             embedded templates may be stale and gen/ could be generated from old \
             templates. Rebuild and use the workspace CLI \
             (e.g. `cargo run -p whisker-cli -- run …`).",
            exe.display(),
        ));
    }
}

/// Newest file mtime anywhere under `dir` (recursive). `None` if the tree is
/// unreadable or empty. Best-effort: unreadable entries are skipped.
fn newest_mtime_under(dir: &Path) -> Option<std::time::SystemTime> {
    let mut newest: Option<std::time::SystemTime> = None;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                stack.push(entry.path());
                continue;
            }
            if let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) {
                newest = Some(newest.map_or(mtime, |n| n.max(mtime)));
            }
        }
    }
    newest
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn cli_target_maps_to_dev_server_target() {
        assert_eq!(Target::from(CliTarget::Android), Target::Android);
        assert_eq!(Target::from(CliTarget::Ios), Target::IosSimulator);
        assert_eq!(Target::from(CliTarget::Desktop), Target::Macos);
        assert_eq!(Target::from(CliTarget::Web), Target::Web);
    }

    fn unique_tempdir() -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let p = std::env::temp_dir().join(format!("whisker-cli-run-test-{pid}-{n}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn find_workspace_root_returns_dir_when_cargo_toml_at_start() {
        let tmp = unique_tempdir();
        std::fs::write(tmp.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        // Compare against the canonical form — `find_workspace_root`
        // canonicalises its input to avoid the empty-PathBuf ENOENT
        // (see fn docs), and on macOS `std::env::temp_dir()` returns a
        // path under `/var/folders/...` which is a symlink to
        // `/private/var/folders/...`.
        let canonical_tmp = std::fs::canonicalize(&tmp).unwrap();
        assert_eq!(
            find_workspace_root(&tmp).as_deref(),
            Some(canonical_tmp.as_path()),
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn find_workspace_root_walks_up_from_a_member_dir() {
        let tmp = unique_tempdir();
        std::fs::write(tmp.join("Cargo.toml"), "[workspace]\nmembers = [\"app\"]\n").unwrap();
        let nested = tmp.join("app");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            nested.join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.0.0\"\n",
        )
        .unwrap();
        let canonical_tmp = std::fs::canonicalize(&tmp).unwrap();
        assert_eq!(
            find_workspace_root(&nested).as_deref(),
            Some(canonical_tmp.as_path()),
        );
        std::fs::remove_dir_all(&tmp).ok();
    }
}
