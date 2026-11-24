//! full reload install + relaunch.
//!
//! After a successful cold-rebuild, the freshly-built artifact has to
//! land on the target and start (re-bootstrapping the dev-runtime so
//! it dials the dev-server back). For Android we shell out to `adb`,
//! for iOS Simulator to `xcrun simctl`, and for macOS we launch the
//! generated app executable while retaining its child handle.
//!
//! Application identity (bundle id, applicationId, launcher activity,
//! scheme, …) is **not** baked in here. The cli passes those as
//! `Config::android` / `Config::ios` / `Config::macos` after reading the user's
//! `whisker.rs::configure(&mut Config)`, so this module has zero
//! knowledge of which example or external user crate is in play.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::{AndroidParams, IosParams, LinuxParams, MacosParams, Target, WindowsParams};
use whisker_build::CaptureShims;

pub struct Installer {
    target: Target,
    android: Option<AndroidParams>,
    ios: Option<IosParams>,
    macos: Option<MacosParams>,
    workspace_root: PathBuf,
    package: String,
    /// Capture shims for hot reload. When `Some`, the xcodebuild
    /// Command in [`ios_install_and_launch`] gets
    /// `RUSTC_WORKSPACE_WRAPPER` + `CARGO_TARGET_*_LINKER` +
    /// `CARGO_TARGET_*_RUSTFLAGS` set, so the cargo invocation its
    /// Build Phase makes runs as a fat capture build.
    capture: Option<CaptureShims>,
    /// Cargo features forwarded to the iOS Build Phase via the
    /// `WHISKER_FEATURES` env var (the pbxproj's shell script expands
    /// it into `--features <feat>` args). `whisker run` populates this
    /// with `["whisker/hot-reload"]` so the dev-runtime WebSocket
    /// client gets compiled into the user dylib — without it the app
    /// never sends its `aslr_reference` and every change falls back to
    /// a full reload.
    features: Vec<String>,
    /// Host port the dev-server's WebSocket is bound to (= the port of
    /// `Config::bind_addr`). The device must reach this exact port:
    /// Android bridges it with `adb reverse tcp:9876 tcp:<dev_port>`
    /// (the device keeps dialing its default 9876), and the iOS
    /// Simulator dials `127.0.0.1:<dev_port>` directly via
    /// `SIMCTL_CHILD_WHISKER_DEV_ADDR`. Without this the `--bind` flag
    /// silently breaks hot reload (server on a custom port, device
    /// still on 9876).
    dev_port: u16,
    /// Per-session hot-reload secret ([`crate::Config::dev_token`]),
    /// delivered out of band. iOS gets it via
    /// `SIMCTL_CHILD_WHISKER_DEV_TOKEN`; Android via
    /// `adb shell setprop debug.whisker_dev_token`. `None` launches the
    /// app without one, which leaves native hot reload disabled.
    dev_token: Option<String>,
    linux: Option<LinuxParams>,
    windows: Option<WindowsParams>,
    /// The locally launched Host process (macOS, Linux and Windows).
    host_child: tokio::sync::Mutex<Option<HostChild>>,
}

struct HostChild {
    process: tokio::process::Child,
    // `whisker run` quits through `process::exit`, which skips
    // `kill_on_drop`; the guard puts the app on the quit path's kill list.
    _exit_guard: Option<whisker_build::child_guard::TrackGuard>,
}

impl Installer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        target: Target,
        android: Option<AndroidParams>,
        ios: Option<IosParams>,
        macos: Option<MacosParams>,
        linux: Option<LinuxParams>,
        windows: Option<WindowsParams>,
        workspace_root: PathBuf,
        package: String,
        capture: Option<CaptureShims>,
        features: Vec<String>,
        dev_port: u16,
        dev_token: Option<String>,
    ) -> Self {
        Self {
            target,
            android,
            ios,
            macos,
            linux,
            windows,
            workspace_root,
            package,
            capture,
            features,
            dev_port,
            dev_token,
            host_child: tokio::sync::Mutex::new(None),
        }
    }

    pub async fn install_and_launch(&self) -> Result<()> {
        // iOS opens this section itself, after the xcodebuild that runs here.
        if !matches!(self.target, Target::IosSimulator | Target::Web) {
            whisker_build::ui::section("Launch");
        }
        match self.target {
            Target::Android => {
                let p = self.android.as_ref().context(
                    "target=Android but no AndroidParams — cli must populate Config.android",
                )?;
                android_install_and_launch(p, self.dev_port, self.dev_token.as_deref()).await
            }
            Target::IosSimulator => {
                let p = self.ios.as_ref().context(
                    "target=IosSimulator but no IosParams — cli must populate Config.ios",
                )?;
                ios_install_and_launch(
                    p,
                    &self.workspace_root,
                    &self.package,
                    self.capture.as_ref(),
                    &self.features,
                    self.dev_port,
                    self.dev_token.as_deref(),
                )
                .await
            }
            Target::Macos => {
                let params = self.macos.as_ref().context(
                    "target=Macos but no MacosParams — cli must provide the generated Host",
                )?;
                let executable = whisker_build::macos::app_executable(
                    &params
                        .derived_data
                        .join("Build/Products/Debug")
                        .join(format!("{}.app", params.project_name)),
                    &params.project_name,
                );
                self.launch_host(&executable).await
            }
            Target::Linux => {
                let params = self.linux.as_ref().context(
                    "target=Linux but no LinuxParams — cli must provide the generated Host",
                )?;
                let executable = whisker_build::desktop::linux_executable(
                    &params.target_dir,
                    whisker_build::Profile::Debug,
                    &params.generated_package,
                );
                self.launch_host(&executable).await
            }
            Target::Windows => {
                let params = self.windows.as_ref().context(
                    "target=Windows but no WindowsParams — cli must provide the generated Host",
                )?;
                let executable = whisker_build::desktop::windows_executable(
                    &params.target_dir,
                    whisker_build::Profile::Debug,
                    &params.generated_package,
                );
                self.launch_host(&executable).await
            }
            Target::Web => Ok(()),
        }
    }

    /// End the running local Host, if any. Windows can't replace the
    /// files of a running program, so a rebuild there starts with this.
    pub async fn stop_host(&self) {
        if let Some(mut previous) = self.host_child.lock().await.take() {
            let _ = previous.process.kill().await;
            let _ = previous.process.wait().await;
        }
    }

    /// Replace the running local Host process with a fresh one.
    async fn launch_host(&self, executable: &Path) -> Result<()> {
        if !executable.is_file() {
            anyhow::bail!("app executable is missing at {}", executable.display());
        }
        let mut slot = self.host_child.lock().await;
        if let Some(mut previous) = slot.take() {
            let _ = previous.process.kill().await;
            let _ = previous.process.wait().await;
        }
        let launch_step = whisker_build::ui::step(
            whisker_build::ui::OperationKind::Launch,
            executable
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        let mut command = Command::new(executable);
        command
            .current_dir(executable.parent().expect("executable has a parent"))
            .stdin(std::process::Stdio::null())
            .env("WHISKER_DEV_ADDR", format!("127.0.0.1:{}", self.dev_port))
            .kill_on_drop(true);
        if let Some(token) = &self.dev_token {
            command.env("WHISKER_DEV_TOKEN", token);
        }
        let process = match command.spawn() {
            Ok(process) => process,
            Err(error) => {
                launch_step.fail(error.to_string());
                return Err(error).with_context(|| format!("launch app {}", executable.display()));
            }
        };
        launch_step.done("");
        *slot = Some(HostChild {
            _exit_guard: process.id().map(whisker_build::child_guard::track),
            process,
        });
        Ok(())
    }

    /// Start the artifact already present on the target. Unlike a Full Reload,
    /// this does not compile or reinstall anything.
    pub async fn relaunch(&self) -> Result<()> {
        match self.target {
            Target::Android => {
                let params = self.android.as_ref().context(
                    "target=Android but no AndroidParams — cli must populate Config.android",
                )?;
                // The emulator may have rebooted or dropped the reverse
                // since the install, and its property may hold a previous
                // session's secret.
                android_prepare_dev_channel(self.dev_port, self.dev_token.as_deref()).await?;
                android_launch(params).await
            }
            Target::IosSimulator => {
                let params = self.ios.as_ref().context(
                    "target=IosSimulator but no IosParams — cli must populate Config.ios",
                )?;
                ios_launch(params, self.dev_port, self.dev_token.as_deref()).await
            }
            Target::Macos | Target::Linux | Target::Windows => self.install_and_launch().await,
            Target::Web => Ok(()),
        }
    }
}

/// Run a `tokio::process::Command` to completion, capture its stderr,
/// and filter known-benign lines (`already booted`, `found nothing to
/// terminate`, xcodebuild's IDE noise). The actual exit status is
/// returned verbatim; the caller decides what counts as failure. Used
/// for `xcrun simctl ...` invocations where the stderr signal is
/// ~70 % noise.
async fn run_filtered(mut cmd: Command, kind: SimctlNoise) -> Result<std::process::ExitStatus> {
    use tokio::io::AsyncReadExt;
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env(whisker_build::ui::NESTED_ENV, "1");
    let mut child = cmd.spawn().context("spawn child")?;
    // Track the PID so workflow cancellation can SIGTERM an in-flight
    // xcodebuild / simctl instead of orphaning it. The guard unregisters
    // when this fn returns.
    let _child_guard = child.id().map(whisker_build::child_guard::track);
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();

    let (out_buf, err_buf) = tokio::join!(
        async {
            let mut s = Vec::new();
            if let Some(mut h) = stdout.take() {
                let _ = h.read_to_end(&mut s).await;
            }
            s
        },
        async {
            let mut s = Vec::new();
            if let Some(mut h) = stderr.take() {
                let _ = h.read_to_end(&mut s).await;
            }
            s
        }
    );
    let status = child.wait().await.context("wait for child")?;

    let stderr_str = String::from_utf8_lossy(&err_buf);
    for line in stderr_str.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if kind.is_benign(trimmed) {
            continue;
        }
        // Anything past the filter is real output.
        whisker_build::ui::warn(trimmed);
    }
    // Stdout from these tools is low-noise, so it goes through
    // info() at debug grade.
    let stdout_str = String::from_utf8_lossy(&out_buf);
    for line in stdout_str.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() && !kind.is_benign_stdout(trimmed) {
            whisker_build::ui::info(trimmed);
        }
    }
    Ok(status)
}

/// Classifies which sub-command produced the stderr — we tune the
/// "benign noise" set per tool because the false-positive shape
/// differs (simctl emits NSPOSIX error preambles, xcodebuild emits
/// `IDERunDestination` etc.).
#[derive(Copy, Clone)]
enum SimctlNoise {
    /// `xcrun simctl boot` — "Unable to boot device in current state: Booted"
    /// fires when the sim is already up, which is the normal case
    /// after the first `whisker run`.
    Boot,
    /// `xcrun simctl install` / `launch` — generally low-noise but
    /// can emit POSIX prefixes; treat anything matching the known
    /// boilerplate as suppressed. `simctl launch
    /// --terminate-running-process` is also routed through here
    /// (the previous separate `simctl terminate` step was rolled
    /// into the launch flag — see `ios_install_and_launch`).
    Other,
    /// `xcodebuild` — `[MT] IDERunDestination`, the date-time
    /// preamble lines, and the post-build "xcframework written"
    /// confirmation all belong here.
    Xcodebuild,
    /// `adb install -r` — stdout banners "Performing Streamed
    /// Install" and "Success" duplicate the `install_step.done()`
    /// row our UI already prints.
    AdbInstall,
    /// `adb shell am start` — stdout banner `Starting: Intent
    /// { cmp=... }` duplicates what the launch step's label
    /// already says.
    AdbAmStart,
}

impl SimctlNoise {
    fn is_benign(&self, line: &str) -> bool {
        // Lines common to several Apple tools.
        if line.contains("An error was encountered processing the command")
            || line.contains("Underlying error (domain=")
            || line.starts_with("    The request to terminate")
        {
            return true;
        }
        match self {
            SimctlNoise::Boot => {
                line.contains("Unable to boot device in current state: Booted")
                    || line.starts_with("(code=405)")
            }
            SimctlNoise::Other => false,
            SimctlNoise::Xcodebuild => whisker_build::ios::is_xcodebuild_noise(line),
            SimctlNoise::AdbInstall | SimctlNoise::AdbAmStart => false,
        }
    }

    fn is_benign_stdout(&self, line: &str) -> bool {
        // `-quiet` does not fully silence xcodebuild, so its stdout needs the
        // benign filter too.
        if matches!(self, SimctlNoise::Xcodebuild) && whisker_build::ios::is_xcodebuild_noise(line)
        {
            return true;
        }
        match self {
            // Duplicates what `step.done(...)` already reports.
            SimctlNoise::Other => line.contains(": ") && line.chars().any(|c| c.is_ascii_digit()),
            // Subsumed by the `install` step's ✓ row.
            SimctlNoise::AdbInstall => line == "Performing Streamed Install" || line == "Success",
            // "Starting: Intent { cmp=… }" duplicates the launch
            // step's label.
            SimctlNoise::AdbAmStart => line.starts_with("Starting: Intent {"),
            _ => false,
        }
    }
}

fn adb() -> Command {
    Command::new(adb_program())
}

/// Resolves `adb` the way `whisker doctor` does: `PATH` first, then the SDK's `platform-tools`.
fn adb_program() -> PathBuf {
    let on_path = std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join("adb").is_file()));
    if !on_path && let Ok(sdk) = crate::hotpatch::android_ndk::android_home() {
        let bundled = sdk.join("platform-tools/adb");
        if bundled.is_file() {
            return bundled;
        }
    }
    PathBuf::from("adb")
}

/// Write the dev token property and verify it landed by reading it
/// back, retrying on mismatch. Only the `getprop` round-trip proves
/// the write: `adb shell` exit codes are historically unreliable, and
/// emulator quick-boot snapshots restore a previous session's value —
/// a silently-failed `setprop` then leaves that stale secret in place
/// and the app fails the hot-reload handshake for the whole session.
async fn deliver_android_dev_token(token: &str) -> bool {
    for attempt in 0..5 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
        let mut setprop_cmd = adb();
        setprop_cmd.args(["shell", "setprop", "debug.whisker_dev_token", token]);
        let _ = run_filtered(setprop_cmd, SimctlNoise::Other).await;
        let read_back = adb()
            .args(["shell", "getprop", "debug.whisker_dev_token"])
            .output()
            .await;
        if let Ok(out) = read_back {
            if String::from_utf8_lossy(&out.stdout).trim() == token {
                return true;
            }
        }
    }
    false
}

/// Bridge the device to the dev server and hand it this session's secret.
///
/// Both must succeed before the app starts: its receiver dials
/// `127.0.0.1:9876`, which only reaches `dev_port` on the host through
/// `adb reverse` (the host side follows `--bind`), and it reads the
/// secret once per connection attempt from `debug.whisker_dev_token`.
/// The app process doesn't inherit adb-set environment variables, hence
/// the system property. Every adb call goes through `run_filtered` so its
/// stdio can't bypass the TUI's capture pipe.
async fn android_prepare_dev_channel(dev_port: u16, dev_token: Option<&str>) -> Result<()> {
    let mut reverse_cmd = adb();
    let device_port = crate::ANDROID_DEVICE_PORT;
    reverse_cmd.args([
        "reverse",
        &format!("tcp:{device_port}"),
        &format!("tcp:{dev_port}"),
    ]);
    let reverse = run_filtered(reverse_cmd, SimctlNoise::Other).await?;
    anyhow::ensure!(
        reverse.success(),
        "`adb reverse tcp:{device_port} tcp:{dev_port}` failed ({reverse}); the app could not \
         reach the hot-reload server"
    );
    if let Some(token) = dev_token {
        anyhow::ensure!(
            deliver_android_dev_token(token).await,
            "could not set debug.whisker_dev_token on the device; the app could not \
             authenticate to the hot-reload server"
        );
    }
    Ok(())
}

async fn android_install_and_launch(
    p: &AndroidParams,
    dev_port: u16,
    dev_token: Option<&str>,
) -> Result<()> {
    let apk = p
        .project_dir
        .join("app/build/outputs/apk/debug/app-debug.apk");
    if !apk.is_file() {
        anyhow::bail!("APK missing at {}", apk.display());
    }

    android_prepare_dev_channel(dev_port, dev_token).await?;

    let install_step = whisker_build::ui::step(
        whisker_build::ui::OperationKind::Install,
        apk.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "app-debug.apk".into()),
    );
    let mut install_cmd = adb();
    install_cmd.args(["install", "-r"]).arg(&apk);
    let install = run_filtered(install_cmd, SimctlNoise::AdbInstall)
        .await
        .context("spawn adb install")?;
    if !install.success() {
        install_step.fail(format!("{install}"));
        anyhow::bail!("adb install -r {} failed ({install})", apk.display());
    }
    install_step.done("");

    android_launch(p).await
}

async fn android_launch(p: &AndroidParams) -> Result<()> {
    // force-stop first, so the relaunch really re-bootstraps.
    let mut stop_cmd = adb();
    stop_cmd.args(["shell", "am", "force-stop", &p.application_id]);
    let _ = run_filtered(stop_cmd, SimctlNoise::Other).await;

    let component = format!("{}/{}", p.application_id, p.launcher_activity);
    let launch_step =
        whisker_build::ui::step(whisker_build::ui::OperationKind::Launch, component.clone());
    let mut launch_cmd = adb();
    // Launcher action/category so a later launcher tap resumes this task instead of stacking a new Activity.
    launch_cmd.args([
        "shell",
        "am",
        "start",
        "-a",
        "android.intent.action.MAIN",
        "-c",
        "android.intent.category.LAUNCHER",
        "-n",
        &component,
    ]);
    let launch = run_filtered(launch_cmd, SimctlNoise::AdbAmStart)
        .await
        .context("spawn adb am start")?;
    if !launch.success() {
        launch_step.fail(format!("{launch}"));
        anyhow::bail!("adb am start {component} failed ({launch})");
    }
    launch_step.done("");
    Ok(())
}

async fn ios_install_and_launch(
    p: &IosParams,
    workspace_root: &std::path::Path,
    package: &str,
    capture: Option<&CaptureShims>,
    features: &[String],
    dev_port: u16,
    dev_token: Option<&str>,
) -> Result<()> {
    let xcode_project = p.project_dir.join(format!("{}.xcodeproj", p.scheme));
    if !xcode_project.is_dir() {
        anyhow::bail!(
            "Xcode project missing at {} — run xcodegen first",
            xcode_project.display()
        );
    }
    let derived = workspace_root
        .join("target/.whisker/ios-derived")
        .join(package);

    let xc_step = whisker_build::ui::step(
        whisker_build::ui::OperationKind::Xcodebuild,
        p.scheme.clone(),
    );
    let mut xc_cmd = Command::new("xcodebuild");
    use_current_whisker_cli(&mut xc_cmd)?;
    xc_cmd
        .arg("-project")
        .arg(&xcode_project)
        .args(["-scheme", &p.scheme])
        .args(["-configuration", "Debug"])
        .args(["-destination", "generic/platform=iOS Simulator"])
        .arg("-derivedDataPath")
        .arg(&derived)
        // Whisker's codegen plugin and compiler macros have separate trust prompts that headless builds cannot answer.
        .arg("-skipPackagePluginValidation")
        .arg("-skipMacroValidation")
        // The dev loop never opens the project in Xcode, so its index store is dead weight on disk.
        .arg("COMPILER_INDEX_STORE_ENABLE=NO")
        // The generic destination otherwise requests every Simulator slice, forcing an extra Rust target install.
        .arg(format!(
            "ARCHS={}",
            whisker_build::ios::host_simulator_arch()
        ))
        .args(["-quiet", "build"]);
    // No `WHISKER_IOS_RUNTIME` / `WHISKER_IOS_MACROS` injection:
    // WhiskerRuntime and the codegen plugin resolve from the remote
    // `whisker` SwiftPM package (`whisker_build::ios::WHISKER_IOS_SPM_URL`).
    // The framework is produced by a Build Phase inside this very
    // xcodebuild run, so the capture envs have to ride along on it:
    // they propagate xcodebuild → shell Build Phase → `whisker
    // build-ios` subprocess → cargo, where the shims intercept rustc
    // and the linker. `None` (not `HotPatchMode::HotReload`) runs
    // xcodebuild without them and the loop falls back to full
    // reloads.
    if let Some(c) = capture {
        let sim_triple = "aarch64-apple-ios-sim";
        for (k, v) in whisker_build::capture_env_vars_for_triple(c, Some(sim_triple)) {
            xc_cmd.env(k, v);
        }
    }
    // Forward cargo features through to the Build Phase's
    // `whisker build-ios` invocation as a space-separated list. The
    // pbxproj's shell script expands each entry into `--features <feat>`
    // before invoking the binary. `whisker run` puts `whisker/hot-reload`
    // here so the user dylib carries the dev-runtime WebSocket client;
    // without that the app never reports `aslr_reference` and every
    // patch falls through to a full reload + relaunch.
    if !features.is_empty() {
        xc_cmd.env("WHISKER_FEATURES", features.join(" "));
    }
    let xc_status = run_filtered(xc_cmd, SimctlNoise::Xcodebuild)
        .await
        .context("spawn xcodebuild")?;
    if !xc_status.success() {
        xc_step.fail(format!("{xc_status}"));
        anyhow::bail!("xcodebuild build failed ({xc_status})");
    }
    xc_step.done("");
    whisker_build::ui::section("Launch");

    let app_path = derived
        .join("Build/Products/Debug-iphonesimulator")
        .join(format!("{}.app", p.scheme));
    if !app_path.is_dir() {
        anyhow::bail!(
            "expected {}.app missing under {} after build",
            p.scheme,
            derived.display()
        );
    }

    // Best-effort boot of either the caller's override or the first
    // available iPhone simctl knows about. "Already booted" stderr
    // is filtered as a benign noise pattern.
    let device = p
        .device_override
        .clone()
        .or_else(pick_available_iphone)
        .unwrap_or_else(|| "iPhone 17 Pro".into());
    let boot_step = whisker_build::ui::step(whisker_build::ui::OperationKind::Boot, device.clone());
    let mut boot_cmd = Command::new("xcrun");
    boot_cmd.args(["simctl", "boot", &device]);
    let _ = run_filtered(boot_cmd, SimctlNoise::Boot).await;
    boot_step.done("");

    let install_step = whisker_build::ui::step(
        whisker_build::ui::OperationKind::Install,
        format!("{}.app", p.scheme),
    );
    let mut install_cmd = Command::new("xcrun");
    install_cmd
        .args(["simctl", "install", "booted"])
        .arg(&app_path);
    let install = run_filtered(install_cmd, SimctlNoise::Other)
        .await
        .context("spawn simctl install")?;
    if !install.success() {
        install_step.fail(format!("{install}"));
        anyhow::bail!("simctl install {} failed ({install})", app_path.display());
    }
    install_step.done("");

    ios_launch(p, dev_port, dev_token).await
}

async fn ios_launch(p: &IosParams, dev_port: u16, dev_token: Option<&str>) -> Result<()> {
    // `SIMCTL_CHILD_<NAME>` shows up as `<NAME>` inside the launched
    // app's env — that's how the dev-runtime finds us.
    //
    // `--terminate-running-process` atomically kills the previous
    // instance (so the runtime re-bootstraps and reconnects the dev
    // WebSocket) and launches the fresh build. A separate `simctl
    // terminate` first would print `Simulator device failed to
    // terminate <bundle>.` on every cold start, where the app is
    // installed but not running; the flag handles that silently.
    let launch_step = whisker_build::ui::step(
        whisker_build::ui::OperationKind::Launch,
        p.bundle_id.clone(),
    );
    let mut launch_cmd = Command::new("xcrun");
    launch_cmd
        .args([
            "simctl",
            "launch",
            "--terminate-running-process",
            "booted",
            &p.bundle_id,
        ])
        // The Simulator shares the host loopback, so it dials the
        // dev-server's bind port directly. Honors `--bind <port>`.
        .env(
            "SIMCTL_CHILD_WHISKER_DEV_ADDR",
            format!("127.0.0.1:{dev_port}"),
        );
    // Deliver the dev-session token as an env var the launched app
    // inherits (`SIMCTL_CHILD_<NAME>` → `<NAME>` in the child).
    if let Some(token) = dev_token {
        launch_cmd.env("SIMCTL_CHILD_WHISKER_DEV_TOKEN", token);
    }
    let launch = run_filtered(launch_cmd, SimctlNoise::Other)
        .await
        .context("spawn simctl launch")?;
    if !launch.success() {
        launch_step.fail(format!("{launch}"));
        anyhow::bail!("simctl launch {} failed ({launch})", p.bundle_id);
    }
    launch_step.done("");
    Ok(())
}

/// Make an Xcode build launched by this CLI call the exact same executable
/// from its Rust build phase. This matters for `cargo run`: a bare `whisker`
/// lookup may otherwise select an older cargo-installed binary from `PATH`.
/// Xcode builds started independently keep the generated script's `whisker`
/// fallback.
fn use_current_whisker_cli(command: &mut Command) -> Result<()> {
    let executable = std::env::current_exe().context("resolve current Whisker CLI executable")?;
    command.env("WHISKER_CLI", executable);
    Ok(())
}

/// Best-effort pick of an iPhone simulator that's installed on this
/// machine. `pick_available_iphone()` returns `None` if simctl isn't
/// available or the output doesn't parse; the caller substitutes a
/// hard-coded default.
fn pick_available_iphone() -> Option<String> {
    let out = std::process::Command::new("xcrun")
        .args(["simctl", "list", "devices", "available"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    for line in text.lines() {
        let trimmed = line.trim();
        // Lines look like:  iPhone 17 Pro (UDID...) (Shutdown)
        let Some((name, _rest)) = trimmed.split_once(" (") else {
            continue;
        };
        if name.starts_with("iPhone ") {
            return Some(name.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn android_params() -> AndroidParams {
        AndroidParams {
            project_dir: PathBuf::from("/tmp/x"),
            application_id: "rs.whisker.examples.helloworld".into(),
            launcher_activity: ".MainActivity".into(),
            abi: "arm64-v8a".into(),
        }
    }

    #[test]
    fn installer_for_android_without_params_errors() {
        let inst = Installer::new(
            Target::Android,
            None,
            None,
            None,
            None,
            None,
            PathBuf::new(),
            "x".into(),
            None,
            Vec::new(),
            9876,
            None,
        );
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = rt
            .block_on(async { inst.install_and_launch().await })
            .unwrap_err();
        assert!(err.to_string().contains("AndroidParams"), "got: {err:#}");
    }

    #[test]
    fn installer_for_ios_without_params_errors() {
        let inst = Installer::new(
            Target::IosSimulator,
            None,
            None,
            None,
            None,
            None,
            PathBuf::new(),
            "x".into(),
            None,
            Vec::new(),
            9876,
            None,
        );
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = rt
            .block_on(async { inst.install_and_launch().await })
            .unwrap_err();
        assert!(err.to_string().contains("IosParams"), "got: {err:#}");
    }

    #[test]
    fn android_install_errors_when_apk_missing() {
        let p = android_params();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = rt
            .block_on(async { android_install_and_launch(&p, 9876, None).await })
            .unwrap_err();
        assert!(err.to_string().contains("APK missing"), "got: {err:#}");
    }

    #[test]
    fn xcodebuild_uses_the_current_whisker_cli() {
        let mut command = Command::new("xcodebuild");
        use_current_whisker_cli(&mut command).unwrap();

        let configured = command
            .as_std()
            .get_envs()
            .find_map(|(key, value)| (key == "WHISKER_CLI").then_some(value))
            .flatten();
        assert_eq!(
            configured,
            Some(std::env::current_exe().unwrap().as_os_str())
        );
    }
}
