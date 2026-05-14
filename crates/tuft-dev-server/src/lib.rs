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

use anyhow::Result;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

pub mod builder;
pub mod installer;
pub mod server;
pub mod watcher;

pub use builder::{BuildPlan, Builder};
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
    /// Tier 1 subsecond patches replace the cargo build / install
    /// steps in the I4g follow-up.
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

        let builder = Builder::new(
            self.config.workspace_root.clone(),
            self.config.package.clone(),
            self.config.target,
        )
        .with_features(vec!["tuft/hot-reload".into()]);
        let installer = Installer::new(
            self.config.workspace_root.clone(),
            self.config.package.clone(),
            self.config.target,
        );

        // Tier 2 cold rebuild loop: drain Change events, run a build,
        // install, and tell connected clients (Tier 2 is for now a
        // hint-only "we rebuilt" — actual subsecond JumpTables come
        // with I4g).
        while let Some(change) = rx.recv().await {
            eprintln!(
                "[tuft-dev-server] change ({:?}) — {} path(s)",
                change.kind,
                change.paths.len()
            );
            emit(&self.on_event, Event::BuildingFull);

            match builder.build().await {
                Ok(()) => {
                    emit(&self.on_event, Event::BuildSucceeded);
                    if let Err(e) = installer.install_and_launch().await {
                        eprintln!("[tuft-dev-server] install failed: {e}");
                    }
                    eprintln!(
                        "[tuft-dev-server] rebuild done; {} client(s) connected",
                        sender.client_count()
                    );
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    eprintln!("[tuft-dev-server] build failed: {msg}");
                    emit(&self.on_event, Event::BuildFailed(msg));
                }
            }
        }

        Ok(())
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
}
