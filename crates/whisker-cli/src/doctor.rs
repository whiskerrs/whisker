//! `whisker doctor` — environment health check.
//!
//! Mirrors `expo doctor` / `flutter doctor` in spirit: walk the local
//! machine for the toolchains and artifacts Whisker needs, report each as
//! ok / warning / error, and exit non-zero if any error is found.
//!
//! Each check is a pure inspection — no side effects (no installs, no
//! downloads). The user runs the fix themselves; we only diagnose.
//!
//! ## Output style
//! Section spinners ("Probing Android …") while each group runs, then
//! a fixed-width-aligned list with a small ✓/⚠/✗ glyph at the left.
//! Plain scrollback text — no boxes, no TUI takeover — so the result
//! is easy to copy/paste into an issue or hand to an AI assistant.

use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// Skip the iOS section even on macOS hosts.
    #[arg(long)]
    pub no_ios: bool,
    /// Skip the Android section.
    #[arg(long)]
    pub no_android: bool,
    /// Skip the Lynx-artifact section.
    #[arg(long)]
    pub no_lynx: bool,
}

pub fn run(args: Args) -> Result<()> {
    println!("{BOLD}whisker doctor{RESET}\n");

    let mut report = Report::default();

    report.add_section("Rust toolchain", check_rust);

    if !args.no_android {
        report.add_section("Android", check_android);
    }
    if !args.no_ios {
        report.add_section("iOS", check_ios);
    }
    if !args.no_lynx {
        report.add_section("Lynx artifacts", check_lynx);
    }

    report.print_summary();
    if report.has_errors() {
        std::process::exit(1);
    }
    Ok(())
}

// ----- Style tokens ---------------------------------------------------------

const C_OK: &str = "\x1b[32m";
const C_WARN: &str = "\x1b[33m";
const C_ERR: &str = "\x1b[31m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

#[derive(Clone, Copy)]
enum Status {
    Ok,
    Warn,
    Err,
}

struct Check {
    name: String,
    status: Status,
    detail: String,
}

impl Check {
    fn ok(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: Status::Ok,
            detail: detail.into(),
        }
    }
    fn warn(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: Status::Warn,
            detail: detail.into(),
        }
    }
    fn err(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: Status::Err,
            detail: detail.into(),
        }
    }
}

#[derive(Default)]
struct Report {
    ok: usize,
    warn: usize,
    err: usize,
}

impl Report {
    fn add_section<F: FnOnce() -> Vec<Check>>(&mut self, name: &str, body: F) {
        let pb = section_spinner(name);
        let checks = body();
        let (n_ok, n_warn, n_err) = tally(&checks);
        let summary = format!(
            "{n_ok}✓  {n_warn}⚠  {n_err}✗",
            n_ok = n_ok,
            n_warn = n_warn,
            n_err = n_err,
        );
        pb.finish_and_clear();

        // Section header (bold)
        println!("{BOLD}{name}{RESET}  {DIM}{summary}{RESET}");

        // Compute aligned width of names (clamped so very long entries
        // don't push detail off-screen).
        let name_w = checks
            .iter()
            .map(|c| visible_width(&c.name))
            .max()
            .unwrap_or(0)
            .min(40);

        for c in &checks {
            let (glyph, col) = match c.status {
                Status::Ok => ("✓", C_OK),
                Status::Warn => ("⚠", C_WARN),
                Status::Err => ("✗", C_ERR),
            };
            let pad = name_w.saturating_sub(visible_width(&c.name));
            let detail = if c.detail.is_empty() {
                String::new()
            } else {
                format!("  {DIM}{}{RESET}", c.detail)
            };
            println!(
                "  {col}{glyph}{RESET}  {name}{pad}{detail}",
                name = c.name,
                pad = " ".repeat(pad),
            );
        }
        println!();

        self.ok += n_ok;
        self.warn += n_warn;
        self.err += n_err;
    }

    fn has_errors(&self) -> bool {
        self.err > 0
    }

    fn print_summary(&self) {
        let total = self.ok + self.warn + self.err;
        match (self.err, self.warn) {
            (0, 0) => println!("{C_OK}{BOLD}all {total} checks passed{RESET}"),
            (0, w) => println!(
                "{total} checks: {C_OK}{}✓{RESET}  {C_WARN}{w}⚠{RESET}",
                self.ok
            ),
            (e, w) => println!(
                "{total} checks: {C_OK}{}✓{RESET}  {C_WARN}{w}⚠{RESET}  {C_ERR}{e}✗{RESET}",
                self.ok
            ),
        }
    }
}

fn tally(checks: &[Check]) -> (usize, usize, usize) {
    let (mut o, mut w, mut e) = (0, 0, 0);
    for c in checks {
        match c.status {
            Status::Ok => o += 1,
            Status::Warn => w += 1,
            Status::Err => e += 1,
        }
    }
    (o, w, e)
}

fn section_spinner(name: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan}  {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.set_message(format!("Probing {name} …"));
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

/// Visible width of a string ignoring ANSI escapes. Good enough for
/// our short ASCII labels — no full Unicode width tables required.
fn visible_width(s: &str) -> usize {
    let mut w = 0;
    let mut in_esc = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_esc = true;
            continue;
        }
        if in_esc {
            if c.is_ascii_alphabetic() {
                in_esc = false;
            }
            continue;
        }
        w += 1;
    }
    w
}

// ----- Rust toolchain --------------------------------------------------------

fn check_rust() -> Vec<Check> {
    let mut out = Vec::new();

    match run_capture("rustc", &["--version"]) {
        Ok(s) => {
            let line = s.lines().next().unwrap_or("").trim().to_string();
            if let Some(v) = parse_rustc_version(&line) {
                if v >= (1, 85) {
                    out.push(Check::ok("rustc", line));
                } else {
                    out.push(Check::err(
                        "rustc",
                        format!("{line} — Whisker requires 1.85+"),
                    ));
                }
            } else {
                out.push(Check::warn("rustc", line));
            }
        }
        Err(_) => out.push(Check::err("rustc", "not on PATH")),
    }

    match run_capture("cargo", &["--version"]) {
        Ok(s) => out.push(Check::ok(
            "cargo",
            s.lines().next().unwrap_or("").trim().to_string(),
        )),
        Err(_) => out.push(Check::err("cargo", "not on PATH")),
    }

    let installed = run_capture("rustup", &["target", "list", "--installed"]).unwrap_or_default();
    let installed: Vec<&str> = installed.lines().map(str::trim).collect();
    for triple in &[
        "aarch64-linux-android",
        "aarch64-apple-ios",
        "aarch64-apple-ios-sim",
        "x86_64-apple-ios",
    ] {
        if installed.iter().any(|t| t == triple) {
            out.push(Check::ok(format!("rustup target {triple}"), "installed"));
        } else {
            out.push(Check::warn(
                format!("rustup target {triple}"),
                format!("missing — `rustup target add {triple}`"),
            ));
        }
    }

    out
}

fn parse_rustc_version(s: &str) -> Option<(u32, u32)> {
    let rest = s.strip_prefix("rustc ")?;
    let v = rest.split_whitespace().next()?;
    let mut it = v.split('.');
    let major: u32 = it.next()?.parse().ok()?;
    let minor: u32 = it.next()?.parse().ok()?;
    Some((major, minor))
}

// ----- Android ---------------------------------------------------------------

fn check_android() -> Vec<Check> {
    let mut out = Vec::new();

    let android_home = std::env::var_os("ANDROID_HOME")
        .or_else(|| std::env::var_os("ANDROID_SDK_ROOT"))
        .map(PathBuf::from);
    let android_home = match android_home {
        Some(p) if p.is_dir() => {
            out.push(Check::ok("ANDROID_HOME", p.display().to_string()));
            p
        }
        Some(p) => {
            out.push(Check::err(
                "ANDROID_HOME",
                format!("{} does not exist", p.display()),
            ));
            return out;
        }
        None => {
            out.push(Check::err(
                "ANDROID_HOME",
                "not set (`export ANDROID_HOME=$HOME/Library/Android/sdk`)",
            ));
            return out;
        }
    };

    // NDK 21.1.6352462 — pinned because Lynx's gn/ninja toolchain
    // requires that exact version (build_lynx_aar bails otherwise).
    let ndk = android_home.join("ndk/21.1.6352462");
    if ndk.is_dir() {
        out.push(Check::ok("NDK 21.1.6352462", ndk.display().to_string()));
    } else {
        out.push(Check::err(
            "NDK 21.1.6352462",
            "missing — `sdkmanager 'ndk;21.1.6352462'`",
        ));
    }

    // JDK 11 — Lynx's gradle wrapper (6.7.1) refuses anything newer.
    match resolve_jdk11() {
        Some(p) => out.push(Check::ok("JDK 11", p.display().to_string())),
        None => out.push(Check::warn(
            "JDK 11",
            "not found (set WHISKER_JAVA11_HOME) — required for Lynx AAR build only",
        )),
    }

    // adb — required for `whisker run android` / install workflows.
    match which("adb").or_else(|| {
        let cand = android_home.join("platform-tools/adb");
        cand.is_file().then_some(cand)
    }) {
        Some(p) => out.push(Check::ok("adb", p.display().to_string())),
        None => out.push(Check::warn(
            "adb",
            "not on PATH (add $ANDROID_HOME/platform-tools)",
        )),
    }

    out
}

fn resolve_jdk11() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("WHISKER_JAVA11_HOME").map(PathBuf::from) {
        if p.is_dir() {
            return Some(p);
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    [
        home.join("work/java11/jdk-11.0.25+9/Contents/Home"),
        home.join("work/java11/jdk-11.0.25+9"),
        PathBuf::from("/Library/Java/JavaVirtualMachines/temurin-11.jdk/Contents/Home"),
    ]
    .into_iter()
    .find(|cand| cand.is_dir())
}

// ----- iOS -------------------------------------------------------------------

fn check_ios() -> Vec<Check> {
    let mut out = Vec::new();
    if !cfg!(target_os = "macos") {
        out.push(Check::warn(
            "host OS",
            "iOS builds require macOS — skipping",
        ));
        return out;
    }

    match run_capture("xcode-select", &["-p"]) {
        Ok(s) => out.push(Check::ok("Xcode", s.trim().to_string())),
        Err(_) => out.push(Check::err(
            "Xcode",
            "command-line tools not configured — `xcode-select --install`",
        )),
    }

    match run_capture("pod", &["--version"]) {
        Ok(s) => out.push(Check::ok("CocoaPods", format!("v{}", s.trim()))),
        Err(_) => out.push(Check::warn(
            "CocoaPods",
            "not on PATH — needed for `cargo xtask ios build-lynx-frameworks`",
        )),
    }

    match run_capture("xcodegen", &["--version"]) {
        Ok(s) => out.push(Check::ok(
            "xcodegen",
            s.lines().next().unwrap_or("").trim().to_string(),
        )),
        Err(_) => out.push(Check::warn(
            "xcodegen",
            "not on PATH — needed for `cargo xtask ios build-lynx-frameworks`",
        )),
    }

    match run_capture("xcrun", &["simctl", "help"]) {
        Ok(_) => out.push(Check::ok("xcrun simctl", "available")),
        Err(_) => out.push(Check::err(
            "xcrun simctl",
            "not available — required for Simulator launches",
        )),
    }

    out
}

// ----- Lynx artifacts (workspace-local) --------------------------------------

fn check_lynx() -> Vec<Check> {
    let mut out = Vec::new();
    let ws = match workspace_root() {
        Some(p) => p,
        None => {
            out.push(Check::warn(
                "workspace",
                "not inside a Whisker workspace — skipping Lynx checks",
            ));
            return out;
        }
    };
    let target = ws.join("target");

    let lynx_src = target.join("lynx-src");
    if lynx_src.join("platform/android/lynx_android").is_dir() {
        let head =
            run_capture_in(&lynx_src, "git", &["log", "-1", "--pretty=%h %s"]).unwrap_or_default();
        out.push(Check::ok(
            "lynx-src",
            head.lines().next().unwrap_or("checked out").to_string(),
        ));
    } else {
        out.push(Check::warn(
            "lynx-src",
            "not bootstrapped — `git clone … target/lynx-src && tools/hab sync`",
        ));
    }

    let aar_dir = target.join("lynx-android");
    let aars = [
        "LynxBase.aar",
        "LynxTrace.aar",
        "LynxAndroid.aar",
        "ServiceAPI.aar",
    ];
    if aars.iter().all(|a| aar_dir.join(a).is_file()) {
        out.push(Check::ok(
            "Android AARs",
            format!("4 files at {}", short_path(&aar_dir)),
        ));
    } else {
        out.push(Check::warn(
            "Android AARs",
            "missing — `cargo xtask android build-lynx-aar`",
        ));
    }

    let ios_dir = target.join("lynx-ios");
    let xcfs = [
        "Lynx.xcframework",
        "LynxBase.xcframework",
        "LynxServiceAPI.xcframework",
        "PrimJS.xcframework",
    ];
    if xcfs.iter().all(|x| ios_dir.join(x).is_dir()) {
        out.push(Check::ok(
            "iOS xcframeworks",
            format!("4 frameworks at {}", short_path(&ios_dir)),
        ));
    } else {
        out.push(Check::warn(
            "iOS xcframeworks",
            "missing — `cargo xtask ios build-lynx-frameworks`",
        ));
    }

    let headers = target.join("lynx-headers");
    if headers.join("Lynx").is_dir() && headers.join("LynxBase").is_dir() {
        out.push(Check::ok("Staged C++ headers", short_path(&headers)));
    } else {
        out.push(Check::warn(
            "Staged C++ headers",
            "missing — produced as a side effect of `build-lynx-frameworks`",
        ));
    }

    out
}

fn short_path(p: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    short_path_with_home(p, home.as_deref())
}

/// Pure-function core of [`short_path`]: replace a leading `home` prefix
/// with `~`. Factored out so unit tests don't have to mutate `$HOME`.
fn short_path_with_home(p: &Path, home: Option<&Path>) -> String {
    if let Some(home) = home {
        if let Ok(rest) = p.strip_prefix(home) {
            return format!("~/{}", rest.display());
        }
    }
    p.display().to_string()
}

/// Best-effort workspace root: walk up from CWD looking for a Cargo.toml
/// whose `[workspace]` table mentions whisker-driver-sys (cheap heuristic).
fn workspace_root() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    workspace_root_from(&cwd)
}

/// Pure-function core of [`workspace_root`]: start the upward walk from
/// the supplied directory rather than the process CWD. Factored out so
/// unit tests can drive it with a tempdir.
fn workspace_root_from(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        let cargo = cur.join("Cargo.toml");
        if cargo.is_file() {
            if let Ok(txt) = std::fs::read_to_string(&cargo) {
                if txt.contains("[workspace]") && txt.contains("whisker-driver-sys") {
                    return Some(cur);
                }
            }
        }
        if !cur.pop() {
            return None;
        }
    }
}

// ----- Tiny helpers ----------------------------------------------------------

fn run_capture(cmd: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(cmd).args(args).output()?;
    if !out.status.success() {
        anyhow::bail!("{cmd} exited {}", out.status);
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn run_capture_in(dir: &Path, cmd: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(cmd).args(args).current_dir(dir).output()?;
    if !out.status.success() {
        anyhow::bail!("{cmd} exited {}", out.status);
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn which(cmd: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = dir.join(cmd);
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // ----- parse_rustc_version ------------------------------------------------

    #[test]
    fn parse_rustc_version_extracts_major_minor() {
        assert_eq!(
            parse_rustc_version("rustc 1.91.0 (f8297e351 2025-10-28)"),
            Some((1, 91)),
        );
    }

    #[test]
    fn parse_rustc_version_handles_no_metadata() {
        assert_eq!(parse_rustc_version("rustc 1.85.2"), Some((1, 85)));
    }

    #[test]
    fn parse_rustc_version_handles_pre_release_channel() {
        // Real-world nightly: "rustc 1.93.0-nightly (abc 2026-01-01)"
        assert_eq!(
            parse_rustc_version("rustc 1.93.0-nightly (abcdef 2026-01-01)"),
            Some((1, 93)),
        );
    }

    #[test]
    fn parse_rustc_version_rejects_garbage() {
        assert_eq!(parse_rustc_version(""), None);
        assert_eq!(parse_rustc_version("cargo 1.91.0"), None);
        assert_eq!(parse_rustc_version("rustc not-a-version"), None);
        assert_eq!(parse_rustc_version("rustc 1"), None);
    }

    // ----- visible_width ------------------------------------------------------

    #[test]
    fn visible_width_counts_plain_ascii() {
        assert_eq!(visible_width(""), 0);
        assert_eq!(visible_width("hello"), 5);
    }

    #[test]
    fn visible_width_ignores_ansi_color_escapes() {
        // "\x1b[32m✓\x1b[0m" should report 1 visible char (✓).
        assert_eq!(visible_width("\x1b[32m✓\x1b[0m"), 1);
        // Mixed: "  ✓  hello" -> 10 visible chars.
        assert_eq!(visible_width("  \x1b[32m✓\x1b[0m  hello"), 10);
    }

    #[test]
    fn visible_width_ignores_long_ansi_sequences() {
        // 38;5;n colour selector
        assert_eq!(visible_width("\x1b[38;5;208mhi\x1b[0m"), 2);
    }

    // ----- Check / Status constructors ----------------------------------------

    #[test]
    fn check_constructors_set_status() {
        assert!(matches!(Check::ok("n", "d").status, Status::Ok));
        assert!(matches!(Check::warn("n", "d").status, Status::Warn));
        assert!(matches!(Check::err("n", "d").status, Status::Err));
    }

    #[test]
    fn check_constructors_store_strings() {
        let c = Check::ok("rustc", "1.91.0");
        assert_eq!(c.name, "rustc");
        assert_eq!(c.detail, "1.91.0");
    }

    // ----- tally --------------------------------------------------------------

    #[test]
    fn tally_counts_each_status_bucket() {
        let checks = vec![
            Check::ok("a", ""),
            Check::ok("b", ""),
            Check::warn("c", ""),
            Check::err("d", ""),
            Check::err("e", ""),
            Check::err("f", ""),
        ];
        assert_eq!(tally(&checks), (2, 1, 3));
    }

    #[test]
    fn tally_of_empty_is_all_zero() {
        assert_eq!(tally(&[]), (0, 0, 0));
    }

    // ----- Report::has_errors -------------------------------------------------

    #[test]
    fn report_has_errors_only_when_err_nonzero() {
        let mut r = Report::default();
        assert!(!r.has_errors());
        r.warn = 5;
        assert!(!r.has_errors(), "warnings alone don't constitute errors");
        r.err = 1;
        assert!(r.has_errors());
    }

    // ----- short_path_with_home -----------------------------------------------

    #[test]
    fn short_path_with_home_substitutes_tilde() {
        let home = PathBuf::from("/home/itome");
        assert_eq!(
            short_path_with_home(Path::new("/home/itome/projects/whisker"), Some(&home),),
            "~/projects/whisker",
        );
    }

    #[test]
    fn short_path_with_home_leaves_unrelated_paths_alone() {
        let home = PathBuf::from("/home/itome");
        assert_eq!(
            short_path_with_home(Path::new("/etc/hosts"), Some(&home)),
            "/etc/hosts",
        );
    }

    #[test]
    fn short_path_with_home_none_returns_full_path() {
        assert_eq!(short_path_with_home(Path::new("/tmp/x"), None), "/tmp/x",);
    }

    #[test]
    fn short_path_with_home_does_not_match_overlapping_prefix() {
        // `/home/itome2` must not get its `/home/itome` prefix stripped.
        let home = PathBuf::from("/home/itome");
        assert_eq!(
            short_path_with_home(Path::new("/home/itome2/work"), Some(&home)),
            "/home/itome2/work",
        );
    }

    // ----- workspace_root_from ------------------------------------------------

    fn write_workspace_marker(dir: &Path) {
        fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/whisker-driver-sys\"]\n",
        )
        .unwrap();
    }

    #[test]
    fn workspace_root_from_finds_root_at_start_dir() {
        let tmp = tempdir();
        write_workspace_marker(tmp.path());
        assert_eq!(workspace_root_from(tmp.path()).as_deref(), Some(tmp.path()),);
    }

    #[test]
    fn workspace_root_from_walks_up_to_find_root() {
        let tmp = tempdir();
        write_workspace_marker(tmp.path());
        let nested = tmp.path().join("crates/whisker-cli/src");
        fs::create_dir_all(&nested).unwrap();
        assert_eq!(workspace_root_from(&nested).as_deref(), Some(tmp.path()),);
    }

    #[test]
    fn workspace_root_from_ignores_unrelated_cargo_tomls() {
        // A non-workspace Cargo.toml in the start dir must NOT match —
        // we only want the one with `[workspace]` + whisker-driver-sys.
        let tmp = tempdir();
        fs::write(tmp.path().join("Cargo.toml"), "[package]\nname = \"foo\"\n").unwrap();
        assert_eq!(workspace_root_from(tmp.path()), None);
    }

    #[test]
    fn workspace_root_from_returns_none_when_no_root_above() {
        // Bare tempdir, no Cargo.toml anywhere on the path.
        let tmp = tempdir();
        assert_eq!(workspace_root_from(tmp.path()), None);
    }

    // ----- tempdir helper -----------------------------------------------------
    //
    // The test suite is too small to justify pulling in the `tempfile`
    // crate as a dev-dependency; this hand-rolled helper is enough.

    struct TempDir(PathBuf);
    impl TempDir {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn tempdir() -> TempDir {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let p = std::env::temp_dir().join(format!("whisker-cli-test-{pid}-{n}"));
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}
