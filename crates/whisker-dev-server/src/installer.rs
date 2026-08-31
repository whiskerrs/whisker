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
use std::path::PathBuf;
use tokio::process::Command;

use crate::{AndroidParams, IosParams, MacosParams, Target};
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
    /// Shared dev-session token to deliver to the device so its
    /// `hello` is accepted by the server's auth gate. iOS gets it via
    /// `SIMCTL_CHILD_WHISKER_DEV_TOKEN`; Android via
    /// `adb shell setprop debug.whisker_dev_token`. `None` = token-less.
    dev_token: Option<String>,
    macos_child: tokio::sync::Mutex<Option<tokio::process::Child>>,
}

impl Installer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        target: Target,
        android: Option<AndroidParams>,
        ios: Option<IosParams>,
        macos: Option<MacosParams>,
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
            workspace_root,
            package,
            capture,
            features,
            dev_port,
            dev_token,
            macos_child: tokio::sync::Mutex::new(None),
        }
    }

    pub async fn install_and_launch(&self) -> Result<()> {
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
                let executable = params
                    .target_dir
                    .join("bundles/debug")
                    .join(format!("{}.app", params.app_name))
                    .join("Contents/MacOS")
                    .join(&params.binary_name);
                if !executable.is_file() {
                    anyhow::bail!(
                        "macOS Host executable is missing at {}",
                        executable.display()
                    );
                }
                let mut slot = self.macos_child.lock().await;
                if let Some(mut previous) = slot.take() {
                    let _ = previous.kill().await;
                    let _ = previous.wait().await;
                }
                let mut command = Command::new(&executable);
                command
                    .current_dir(executable.parent().expect("bundle executable has a parent"))
                    .env("WHISKER_DEV_ADDR", format!("127.0.0.1:{}", self.dev_port))
                    .kill_on_drop(true);
                if let Some(token) = &self.dev_token {
                    command.env("WHISKER_DEV_TOKEN", token);
                }
                *slot = Some(
                    command
                        .spawn()
                        .with_context(|| format!("launch macOS Host {}", executable.display()))?,
                );
                Ok(())
            }
            Target::Web => {
                anyhow::bail!("Web launch is driven by the CNG-generated Trunk project")
            }
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
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().context("spawn child")?;
    // Track the PID so `whisker run`'s hard-exit quit path can SIGTERM
    // an in-flight xcodebuild / simctl instead of orphaning it. The
    // guard unregisters when this fn returns.
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

/// `xcodebuild`'s `-quiet` flag silences progress chatter but the
/// underlying compiler still emits diagnostics. Framework deprecation
/// cascades can produce hundreds of non-actionable lines on every build, so
/// the concise UI filters warning chains while verbose mode preserves them.
///
/// Approach: drop anything that looks like a clang / xcodebuild
/// warning chain — the `warning:` line, the `note:` follow-ups,
/// the source-line listings (`  217 |`, `      |   ^`), the
/// "in file included from" / "N warnings generated." summaries,
/// and the `[MT] IDERunDestination` IDE chatter.
///
/// Real errors are preserved: lines containing `error:` /
/// `fatal error:` / `** BUILD FAILED **` always fall through.
fn is_benign_xcodebuild_line(raw: &str) -> bool {
    // `--verbose` means "show me the full tool output", deprecation
    // chain included.
    if whisker_build::ui::is_verbose() {
        return false;
    }

    let line = raw.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '·');

    // First, so a `warning:`-prefixed error isn't suppressed below.
    if line.starts_with("error:")
        || line.contains(" error:")
        || line.starts_with("fatal error:")
        || line.starts_with("** BUILD FAILED")
        || line.starts_with("** BUILD INTERRUPTED")
    {
        return false;
    }

    // `2026-05-21 18:21:52.770 xcodebuild[54160:34595363] [MT] IDERunDestination …`
    if raw.starts_with("20") && raw.contains("xcodebuild[") && raw.contains("] [MT] ") {
        return true;
    }

    // The xcframework command's success line.
    if line.starts_with("xcframework successfully written out to:") {
        return true;
    }

    // Diagnostic chain: warnings, notes, source listings, summary.
    if line.starts_with("warning:")
        || line.contains(" warning:")
        || line.starts_with("note:")
        || line.contains(" note:")
        || line.starts_with("In file included from")
        || line.ends_with(" warnings generated.")
        || line.ends_with(" warning generated.")
    {
        return true;
    }

    // Source-line listings rendered alongside the warning chain:
    //   `217 | #import "FrameworkHeader.h"`
    //   `    | ^`
    //   `56 |`        (empty source line for context)
    // After trimming leading whitespace and ALL leading digits, the
    // remainder always starts with `|` — trimming a single character
    // would miss multi-digit line numbers.
    let after_digits = line
        .trim_start()
        .trim_start_matches(|c: char| c.is_ascii_digit() || c.is_ascii_whitespace());
    if after_digits.starts_with('|') {
        return true;
    }

    false
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
            SimctlNoise::Xcodebuild => is_benign_xcodebuild_line(line),
            SimctlNoise::AdbInstall | SimctlNoise::AdbAmStart => false,
        }
    }

    fn is_benign_stdout(&self, line: &str) -> bool {
        // `-quiet` does not fully silence xcodebuild, so its stdout needs the
        // benign filter too.
        if matches!(self, SimctlNoise::Xcodebuild) && is_benign_xcodebuild_line(line) {
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

/// Write the dev token property and verify it landed by reading it
/// back, retrying on mismatch. Only the `getprop` round-trip proves
/// the write: `adb shell` exit codes are historically unreliable, and
/// emulator quick-boot snapshots restore a previous session's value —
/// a silently-failed `setprop` then leaves that stale token in place
/// and the server rejects every hello for the whole session.
async fn deliver_android_dev_token(token: &str) -> bool {
    for attempt in 0..5 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
        let mut setprop_cmd = Command::new("adb");
        setprop_cmd.args(["shell", "setprop", "debug.whisker_dev_token", token]);
        let _ = run_filtered(setprop_cmd, SimctlNoise::Other).await;
        let read_back = Command::new("adb")
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

    // Bridge device `127.0.0.1:9876` → host `dev_port` so the
    // dev-runtime reaches the WebSocket without knowing the
    // emulator-gateway IP. The device always dials its default 9876;
    // only the host side follows `--bind`, so a custom port still hot
    // reloads. Best-effort — a previous run may have set it, and a
    // physical device doesn't need it at all. Every adb call goes
    // through `run_filtered` so its stdio can't bypass the TUI's
    // capture pipe and overlay the live region.
    let mut reverse_cmd = Command::new("adb");
    reverse_cmd.args(["reverse", "tcp:9876", &format!("tcp:{dev_port}")]);
    let _ = run_filtered(reverse_cmd, SimctlNoise::Other).await;

    // Deliver the dev-session token. The app process doesn't inherit
    // adb-set env vars, so we stash it in a `debug.*` system property
    // (settable over adb) that the device-side `whisker-dev-runtime`
    // reads via `__system_property_get`. Without a matching token the
    // dev-server refuses to ship patches to the app.
    if let Some(token) = dev_token {
        if !deliver_android_dev_token(token).await {
            whisker_build::ui::warn(format!(
                "dev token did not land on the device — the app will keep being \
                 rejected by the hot-reload server (0 clients). Fix manually: \
                 adb shell setprop debug.whisker_dev_token {token}"
            ));
        }
    }

    let install_step = whisker_build::ui::step(
        "install",
        apk.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "app-debug.apk".into()),
    );
    let mut install_cmd = Command::new("adb");
    install_cmd.args(["install", "-r"]).arg(&apk);
    let install = run_filtered(install_cmd, SimctlNoise::AdbInstall)
        .await
        .context("spawn adb install")?;
    if !install.success() {
        install_step.fail(format!("{install}"));
        anyhow::bail!("adb install -r {} failed ({install})", apk.display());
    }
    install_step.done("");

    // force-stop first, so the relaunch really re-bootstraps.
    let mut stop_cmd = Command::new("adb");
    stop_cmd.args(["shell", "am", "force-stop", &p.application_id]);
    let _ = run_filtered(stop_cmd, SimctlNoise::Other).await;

    let component = format!("{}/{}", p.application_id, p.launcher_activity);
    let launch_step = whisker_build::ui::step("launch", component.clone());
    let mut launch_cmd = Command::new("adb");
    launch_cmd.args(["shell", "am", "start", "-n", &component]);
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

    let xc_step = whisker_build::ui::step("xcodebuild", p.scheme.clone());
    let mut xc_cmd = Command::new("xcodebuild");
    xc_cmd
        .arg("-project")
        .arg(&xcode_project)
        .args(["-scheme", &p.scheme])
        .args(["-configuration", "Debug"])
        .args(["-destination", "generic/platform=iOS Simulator"])
        .arg("-derivedDataPath")
        .arg(&derived)
        // The WhiskerModuleCodegenPlugin is a SwiftPM build-tool plugin;
        // Xcode gates plugins behind an interactive trust prompt a
        // headless build can't answer, so skip validation (it ships from
        // Whisker's own `whisker` SPM package).
        .arg("-skipPackagePluginValidation")
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
    let boot_step = whisker_build::ui::step("boot", device.clone());
    let mut boot_cmd = Command::new("xcrun");
    boot_cmd.args(["simctl", "boot", &device]);
    let _ = run_filtered(boot_cmd, SimctlNoise::Boot).await;
    boot_step.done("");

    let install_step = whisker_build::ui::step("install", format!("{}.app", p.scheme));
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

    // `SIMCTL_CHILD_<NAME>` shows up as `<NAME>` inside the launched
    // app's env — that's how the dev-runtime finds us.
    //
    // `--terminate-running-process` atomically kills the previous
    // instance (so the runtime re-bootstraps and reconnects the dev
    // WebSocket) and launches the fresh build. A separate `simctl
    // terminate` first would print `Simulator device failed to
    // terminate <bundle>.` on every cold start, where the app is
    // installed but not running; the flag handles that silently.
    let launch_step = whisker_build::ui::step("launch", p.bundle_id.clone());
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
}
