//! full reload install + relaunch.
//!
//! After a successful cold-rebuild, the freshly-built artifact has to
//! land on the target and start (re-bootstrapping the dev-runtime so
//! it dials the dev-server back). For Android we shell out to `adb`;
//! for iOS Simulator to `xcrun simctl`.
//!
//! Application identity (bundle id, applicationId, launcher activity,
//! scheme, …) is **not** baked in here. The cli passes those as
//! `Config::android` / `Config::ios` after reading the user's
//! `whisker.rs::configure(&mut Config)`, so this module has zero
//! knowledge of which example or external user crate is in play.

use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::process::Command;

use crate::{AndroidParams, IosParams, Target};
use whisker_build::CaptureShims;

pub struct Installer {
    target: Target,
    android: Option<AndroidParams>,
    ios: Option<IosParams>,
    workspace_root: PathBuf,
    package: String,
    /// hot reload capture shims for hot-reload. When `Some`, the
    /// xcodebuild Command in [`ios_install_and_launch`] gets the
    /// `RUSTC_WORKSPACE_WRAPPER` + `CARGO_TARGET_*_LINKER` +
    /// `CARGO_TARGET_*_RUSTFLAGS` env vars set so the Step-7
    /// Build Phase's cargo invocation runs as a fat capture build.
    /// Pre-Step-7 the dev-server primed capture via a separate
    /// `build_xcframework_with` call in `builder.rs`; that call now
    /// produces an artifact xcodebuild's Build Phase rebuilds anyway,
    /// so the capture wiring moves here.
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
}

impl Installer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        target: Target,
        android: Option<AndroidParams>,
        ios: Option<IosParams>,
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
            workspace_root,
            package,
            capture,
            features,
            dev_port,
            dev_token,
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
        // Anything that survived the filter is real output — surface
        // it as a warning so the user notices but the curated layout
        // isn't drowned.
        whisker_build::ui::warn(trimmed);
    }
    // Stdout from these tools is usually empty or low-noise (e.g.
    // `simctl launch` prints `<bundle_id>: <pid>`); echo it through
    // info() at debug-grade.
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
/// underlying compiler still emits diagnostics — which under Xcode
/// with iOS 26 SDK + Lynx's pre-iOS-26 framework headers means a
/// hundreds-of-lines deprecation cascade (`'mainScreen' is
/// deprecated`, `'screens' is deprecated`, …) on every build. None
/// of it is actionable by Whisker users (the headers ship from
/// upstream Lynx), so we filter it as benign here.
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
    // Under `--verbose` / `WHISKER_VERBOSE=1`, let every line
    // through — that's the explicit "I want to see the full
    // underlying tool output" mode, including the deprecation
    // chain we'd otherwise suppress.
    if whisker_build::ui::is_verbose() {
        return false;
    }

    let line = raw.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '·');

    // Always surface real errors. Check this first so we don't
    // accidentally suppress a `warning:`-prefixed error message.
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

    // Diagnostic chain: warnings, notes, source line listings,
    // `N warnings generated.` summary.
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
    //   `217 | #import "LynxBackgroundInfo.h"`
    //   `    | ^`
    //   `56 |`        (empty source line for context)
    // After trimming leading whitespace + all leading digits, the
    // remainder always starts with `|` (with or without trailing
    // content). Multi-digit line numbers were the gap that earlier
    // single-char `strip_prefix` filters missed.
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
        // For xcodebuild, also fold stdout through the benign filter
        // — `-quiet` doesn't fully silence it on iOS 26 SDK + Lynx
        // pre-iOS-26 headers, so `mainScreen` deprecations etc. land
        // on stdout depending on Xcode version.
        if matches!(self, SimctlNoise::Xcodebuild) && is_benign_xcodebuild_line(line) {
            return true;
        }
        match self {
            // `simctl launch` always reports `<bundle_id>: <pid>` on
            // success; it duplicates info our `step.done(...)`
            // already covers.
            SimctlNoise::Other => line.contains(": ") && line.chars().any(|c| c.is_ascii_digit()),
            // `adb install -r`: two stdout lines on success — both
            // are subsumed by the `install` step's ✓ row.
            SimctlNoise::AdbInstall => line == "Performing Streamed Install" || line == "Success",
            // `adb shell am start -n <component>`: the one stdout
            // line "Starting: Intent { cmp=… }" duplicates the
            // launch step's label.
            SimctlNoise::AdbAmStart => line.starts_with("Starting: Intent {"),
            _ => false,
        }
    }
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

    // adb reverse — bridge device `127.0.0.1:9876` → host `dev_port` so
    // the on-device dev-runtime can reach our WebSocket without knowing
    // the emulator-gateway IP (10.0.2.2). The device keeps dialing its
    // default 9876 (`WHISKER_DEV_ADDR` fallback); only the host side of
    // the mapping follows `--bind`, so a custom `--bind` port keeps hot
    // reload working. Best-effort: it might already be set from a
    // previous run, or the device might be a non-emulator that doesn't
    // need it. Routed through `run_filtered` rather than `.status()` so
    // its stdio doesn't bypass the TUI's stderr-capture pipe and overlay
    // the live region — same reason every other adb call below now uses
    // it.
    let mut reverse_cmd = Command::new("adb");
    reverse_cmd.args(["reverse", "tcp:9876", &format!("tcp:{dev_port}")]);
    let _ = run_filtered(reverse_cmd, SimctlNoise::Other).await;

    // Deliver the dev-session token. The app process doesn't inherit
    // adb-set env vars, so we stash it in a `debug.*` system property
    // (settable over adb) that the device-side `whisker-dev-runtime`
    // reads via `__system_property_get`. Without a matching token the
    // dev-server refuses to ship patches to the app.
    if let Some(token) = dev_token {
        let mut setprop_cmd = Command::new("adb");
        setprop_cmd.args(["shell", "setprop", "debug.whisker_dev_token", token]);
        let _ = run_filtered(setprop_cmd, SimctlNoise::Other).await;
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

    // adb shell am force-stop  (so the relaunch actually re-bootstraps).
    // `force-stop` is silent on success; route through `run_filtered`
    // anyway so any error preamble lands in scrollback rather than on
    // top of the live region.
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
    // NB: WhiskerRuntime + the codegen plugin now resolve from the
    // remote `whisker` SwiftPM package (see whisker_build::ios::
    // WHISKER_IOS_SPM_URL), so the old `WHISKER_IOS_RUNTIME` /
    // `WHISKER_IOS_MACROS` env injection that pointed module manifests at
    // `platforms/ios` is gone — modules no longer read it.
    // hot reload capture wiring (hot-reload). Pre-Step-7 the dev-server
    // ran a separate `build_xcframework_with` call to prime the rustc
    // + linker capture caches before xcodebuild touched the framework.
    // Step 7's Build Phase produces the framework during xcodebuild
    // itself, so the capture envs need to ride along here — they
    // propagate xcodebuild → shell Build Phase → `whisker build-ios`
    // subprocess → cargo, where the shims actually intercept rustc +
    // linker. Capture is opt-in (`HotPatchMode::HotReload`); when
    // `None`, xcodebuild runs without the shims and the loop falls
    // back to full reloads.
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
    // `--terminate-running-process` makes simctl atomically kill the
    // previous instance (so the runtime re-bootstraps + reconnects
    // the dev WebSocket) and immediately launch the fresh build. We
    // used to do this as two steps — `simctl terminate` followed by
    // `simctl launch` — but the terminate call emits
    // `Simulator device failed to terminate <bundle>.` to stderr
    // whenever the app exists on the simulator but isn't actually
    // running (which is every cold start, and also the "user
    // backgrounded the app between rebuilds" case). The flag bundles
    // both operations and handles the not-running case silently.
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

// ============================================================================
// Tests
// ============================================================================

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
