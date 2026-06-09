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

use anyhow::{anyhow, Context, Result};
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
    /// common case (`whisker run android` / `whisker run ios`) reads
    /// naturally without a `--target=` prefix.
    #[arg(value_enum)]
    pub target: CliTarget,

    /// WebSocket bind address. The Whisker app on the device dials this
    /// (via `WHISKER_DEV_ADDR`) to receive patches.
    #[arg(long, default_value = "127.0.0.1:9876")]
    pub bind: SocketAddr,

    /// Opt out of Tier 1 subsecond hot-patching and fall back to Tier 2
    /// cold rebuilds. `whisker run` defaults to Tier 1; this flag is
    /// for situations where the hot-patch path is misbehaving and you
    /// just want the slower-but-bulletproof path.
    #[arg(long)]
    pub no_hot_patch: bool,

    /// Override the workspace root (= directory containing the
    /// `Cargo.toml` with `[workspace]`). Defaults to walking up from
    /// the resolved manifest's parent dir.
    #[arg(long)]
    pub workspace_root: Option<PathBuf>,

    /// Show every line of the device's stdout/stderr stream, including
    /// Lynx C++ engine chatter (`s_glBindAttribLocation: …` and
    /// friends) that the curated default suppresses. Useful when
    /// triaging engine-level issues; noisy for typical app
    /// development. Pair with `WHISKER_VERBOSE=1` for the full picture.
    #[arg(long)]
    pub show_native_logs: bool,

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
}

impl From<CliTarget> for Target {
    fn from(t: CliTarget) -> Self {
        match t {
            CliTarget::Android => Target::Android,
            CliTarget::Ios => Target::IosSimulator,
        }
    }
}

pub fn run(args: Args) -> Result<()> {
    // Set the cross-crate TUI signal before any `whisker_build::ui::*`
    // call fires — `whisker_build::ui::mode()` caches its lookup in a
    // `OnceLock` on the first call, so flipping this env later doesn't
    // unstick a `Curated` cache.
    let tui_enabled = !args.no_tui && std::io::IsTerminal::is_terminal(&std::io::stderr());
    if tui_enabled {
        std::env::set_var("WHISKER_TUI", "1");
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
    // Sync the native host project (gen/{android,ios}/) before doing
    // anything else. The cargo-side `build_discovered_plugins` step
    // happens inside `sync_for_target` and is the long pole here.
    // `set_phase(Setup)` already fired from `run()` before we got
    // here, so re-issuing it would duplicate the "▶ Setup" entry in
    // scrollback.
    //
    // No iOS Lynx pre-fetch: `platforms/ios/Package.swift` now uses
    // `binaryTarget(url:checksum:)`, so xcodebuild resolves the four
    // xcframeworks via SPM during package resolution. Android pulls
    // its aar from `whiskerrs.github.io/lynx/maven` transitively via
    // the SDK pom. Neither path needs the workspace `target/lynx-*`
    // tree the cli used to stage here.

    let sync = crate::platforms::sync_for_target(
        target,
        &m.config,
        &m.crate_dir,
        &workspace_root,
        &m.package,
    )
    .context("sync native project (gen/{android,ios}/)")?;
    if sync.regenerated {
        eprintln!(
            "[whisker run] native project regenerated at {}",
            sync.gen_dir.display(),
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

    let watch_paths = vec![m.crate_dir.join("src"), m.crate_dir.join("whisker.rs")];

    let config = Config {
        workspace_root,
        crate_dir: m.crate_dir,
        package: m.package,
        target,
        watch_paths: watch_paths.clone(),
        bind_addr: args.bind,
        hot_patch_mode: if args.no_hot_patch {
            HotPatchMode::Tier2ColdRebuild
        } else {
            HotPatchMode::Tier1Subsecond
        },
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
    let show_native_logs = args.show_native_logs;
    let tui_for_events = tui.cloned();

    let server = DevServer::new(config)?.on_event(move |e| {
        if let Some(h) = &tui_for_events {
            // TUI mode: the handle's `apply_event` already pushes
            // `Event::DeviceLog` into scrollback via `insert_before`
            // as a `[device]` / `[device:err]` row. Routing the
            // same event through `forward_event_to_ui` would
            // double-print every device log line (once raw, once
            // wrapped in `whisker_build::ui::info`'s `· ` prefix
            // and captured back through stderr). Skip the legacy
            // path entirely when the TUI is on.
            h.apply_event(&e);
        } else {
            forward_event_to_ui(e, show_native_logs);
        }
    });

    rt.block_on(server.run())
}

/// Friendly label for the TUI header. `whisker_dev_server::Target`'s
/// Debug impl renders `IosSimulator` which is a mouthful — pick a
/// short noun for screen real estate.
fn target_label(target: Target) -> &'static str {
    match target {
        Target::Android => "Android",
        Target::IosSimulator => "iOS Simulator",
    }
}

/// Translate dev-server [`Event`]s into the existing line-based UI
/// output. Phase 2 (ratatui TUI) will replace this with a routed
/// dispatch into per-pane state; until then, the relevant signal we
/// need to surface is the device's own stdout/stderr — everything else
/// is already covered by `whisker_build::ui` calls inside the dev
/// loop.
///
/// When `show_native_logs` is false (the default), device lines that
/// match [`is_native_engine_noise`] are dropped silently. The escape
/// hatch is `whisker run --show-native-logs`.
fn forward_event_to_ui(event: whisker_dev_server::Event, show_native_logs: bool) {
    use whisker_dev_server::Event;
    if let Event::DeviceLog {
        stream,
        line,
        ts_micros: _,
    } = event
    {
        if !show_native_logs && is_native_engine_noise(&line) {
            return;
        }
        // Short `[device]` / `[device:err]` prefix keeps the column
        // alignment compact next to `whisker-build::ui::info`'s own
        // output. The Phase-2 TUI can surface stream / timestamp /
        // colour separately.
        let tag = match stream.as_str() {
            "stderr" => "device:err",
            _ => "device",
        };
        whisker_build::ui::info(format!("[{tag}] {line}"));
    }
}

/// Identify lines that come from the Lynx C++ engine's debug stderr
/// rather than the user's own Rust code. Lynx's Skia/GL backend
/// prints per-program attribute-binding traces (`s_glBindAttribLocation:
/// bind attrib N name X`) on every frame draw and a handful of other
/// engine-internal log lines that are not actionable from app code.
///
/// The filter intentionally errs toward letting unknown lines through
/// — these patterns are bounded to specific known-noisy Lynx prefixes,
/// so genuine error output and user `eprintln!`s are never silenced.
fn is_native_engine_noise(line: &str) -> bool {
    let t = line.trim_start();
    // Lynx Skia / GL trace prefixes. The `s_gl<CamelCase>(` form is
    // distinctive — Skia internals only — and shows up dozens of
    // times per frame on first paint.
    const LYNX_NOISE_PREFIXES: &[&str] = &[
        "s_glBindAttribLocation:",
        "s_glGetUniformLocation:",
        "s_glGetAttribLocation:",
    ];
    for prefix in LYNX_NOISE_PREFIXES {
        if t.starts_with(prefix) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod device_log_filter_tests {
    use super::is_native_engine_noise;

    #[test]
    fn drops_lynx_skia_bind_attrib_traces() {
        assert!(is_native_engine_noise(
            "s_glBindAttribLocation: bind attrib 0 name position"
        ));
        assert!(is_native_engine_noise(
            "s_glBindAttribLocation: bind attrib 2 name inTextureCoords"
        ));
        assert!(is_native_engine_noise(
            "s_glGetUniformLocation: query uniform u_mvp"
        ));
    }

    #[test]
    fn drops_indented_lynx_traces() {
        // Belt-and-braces: native printf output sometimes lands with a
        // leading space or tab from libc buffering.
        assert!(is_native_engine_noise("  s_glBindAttribLocation: bind 1"));
        assert!(is_native_engine_noise(
            "\ts_glGetAttribLocation: query in_color"
        ));
    }

    #[test]
    fn preserves_user_println_output() {
        assert!(!is_native_engine_noise("podcast: app() starting"));
        assert!(!is_native_engine_noise("info: loaded 12 items from cache"));
        // Even patterns that touch `gl` but aren't Lynx's known
        // tracers should pass through — the filter list is precise
        // by design.
        assert!(!is_native_engine_noise("openglRenderer: skia init OK"));
        assert!(!is_native_engine_noise(
            "warning: glsl shader compilation took 42ms"
        ));
    }

    #[test]
    fn preserves_panics_and_errors() {
        assert!(!is_native_engine_noise(
            "thread 'main' panicked at 'index out of bounds'"
        ));
        assert!(!is_native_engine_noise("error: failed to parse JSON"));
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
fn find_workspace_root(start: &Path) -> Option<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn cli_target_maps_to_dev_server_target() {
        assert_eq!(Target::from(CliTarget::Android), Target::Android);
        assert_eq!(Target::from(CliTarget::Ios), Target::IosSimulator);
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
