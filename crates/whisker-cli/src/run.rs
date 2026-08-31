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
use whisker_dev_server::{
    AndroidParams, Config, DevServer, HotPatchMode, IosParams, Target, WebParams,
};

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

pub fn run(args: Args, no_tui: bool) -> Result<()> {
    let tui_enabled = crate::tui::should_start(no_tui);

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
    let tui_session = if tui_enabled {
        match crate::tui::TuiSession::start(
            crate::tui::WorkflowKind::Run,
            target_label.to_string(),
            bundle.clone(),
        ) {
            Ok(session) => {
                let handle = session.handle();
                handle.set_phase(crate::tui::AppPhase::Setup);
                Some(session)
            }
            Err(e) => {
                eprintln!("couldn't start TUI ({e:#}); falling back to plain output");
                None
            }
        }
    } else {
        None
    };
    let tui_handle = tui_session.as_ref().map(crate::tui::TuiSession::handle);

    // Run the rest of the cli pipeline. Each phase pushes its progress
    // through `tui_handle`. If the TUI isn't running, every step is
    // a no-op + the existing `whisker_build::ui::*` lines fall back
    // to scrollback.
    let result = run_inner(args, m, workspace_root, target, tui_handle.as_ref());

    // Stop the render thread + restore the terminal. Use should_quit
    // as the signal so the render thread exits cleanly.
    if let Some(session) = tui_session {
        session.finish();
    }
    result
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
            args.bind,
            args.no_hot_patch,
            tui,
        );
    }
    let android = match target {
        Target::Android => Some(android_params_from(&m, &sync.gen_dir)?),
        _ => None,
    };
    let ios = match target {
        Target::IosSimulator => Some(ios_params_from(&m, &sync.gen_dir)?),
        _ => None,
    };
    let web = match target {
        Target::Web => Some(WebParams {
            project_dir: sync.gen_dir.clone(),
            target_dir: workspace_root.join("target/.whisker/web"),
            dist_dir: sync.gen_dir.join("dist"),
            generated_package: format!("{}-whisker-web", m.package),
        }),
        _ => None,
    };

    let watch_paths = watch_paths_for(&m);

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
        // Web clients share this server's HTTP origin with the patch socket.
        // Native-code patch transport keeps its random per-session token.
        dev_token: (target != Target::Web).then(generate_dev_token),
        hot_patch_mode: if args.no_hot_patch {
            HotPatchMode::FullReloadOnly
        } else {
            HotPatchMode::HotReload
        },
        android,
        ios,
        macos: None,
        web,
    };

    let watching_paths: Vec<String> = watch_paths
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    if let Some(t) = tui {
        t.set_dev_server(target_destination(&config), watching_paths);
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

/// Drive the first Desktop development loop. The generated Cargo project is
/// exactly the one `whisker build macos` consumes; development adds only file
/// watching and process supervision around its Debug build.
fn run_macos(
    manifest: &manifest::ResolvedManifest,
    workspace_root: &Path,
    gen_dir: &Path,
    explicit_watch_paths: &[PathBuf],
    bind_addr: SocketAddr,
    no_hot_patch: bool,
    tui: Option<&crate::tui::TuiHandle>,
) -> Result<()> {
    let app_name = manifest
        .config
        .name
        .as_deref()
        .ok_or_else(|| anyhow!("whisker.rs: app.name(\"…\") is required for macOS"))?;
    let binary_name = format!("{}-whisker-macos", manifest.package);
    let target_dir = workspace_root.join("target/.whisker/macos");

    let watch_roots = explicit_watch_paths.to_vec();
    if let Some(tui) = tui {
        tui.set_dev_server(
            format!("{app_name} · local application"),
            watch_roots
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
        );
        tui.set_phase(crate::tui::AppPhase::Initializing);
    }
    let config = Config {
        workspace_root: workspace_root.to_path_buf(),
        crate_dir: manifest.crate_dir.clone(),
        package: manifest.package.clone(),
        target: Target::Macos,
        watch_paths: watch_roots,
        bind_addr,
        dev_token: Some(generate_dev_token()),
        hot_patch_mode: if no_hot_patch {
            HotPatchMode::FullReloadOnly
        } else {
            HotPatchMode::HotReload
        },
        android: None,
        ios: None,
        macos: Some(whisker_dev_server::MacosParams {
            project_dir: gen_dir.to_path_buf(),
            target_dir,
            app_name: app_name.to_string(),
            binary_name,
        }),
        web: None,
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build macOS development runtime")?;
    let tui_for_events = tui.cloned();
    let (command_sender, command_receiver) = tokio::sync::mpsc::unbounded_channel();
    if let Some(tui) = tui {
        tui.set_command_sender(command_sender);
    }
    let server = DevServer::new(config)?
        .with_command_receiver(command_receiver)
        .on_event(move |event| {
            if let Some(tui) = &tui_for_events {
                tui.apply_event(&event);
            } else {
                forward_event_to_ui(event);
            }
        });
    runtime.block_on(server.run())
}

fn target_destination(config: &Config) -> String {
    match config.target {
        Target::Android => config
            .android
            .as_ref()
            .map(|params| params.application_id.clone())
            .unwrap_or_else(|| "Android application".into()),
        Target::IosSimulator => config
            .ios
            .as_ref()
            .map(|params| {
                format!(
                    "{} · {}",
                    params.device_override.as_deref().unwrap_or("iOS Simulator"),
                    params.bundle_id
                )
            })
            .unwrap_or_else(|| "iOS Simulator".into()),
        Target::Macos => config
            .macos
            .as_ref()
            .map(|params| format!("{} · local application", params.app_name))
            .unwrap_or_else(|| "Desktop application".into()),
        Target::Web => format!("http://127.0.0.1:{}/", config.bind_addr.port()),
    }
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
