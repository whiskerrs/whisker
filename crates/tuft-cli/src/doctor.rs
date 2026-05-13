//! `tuft doctor` — environment health check.
//!
//! Mirrors `expo doctor` / `flutter doctor` in spirit: walk the local
//! machine for the toolchains and artifacts Tuft needs, report each as
//! ok / warning / error, and exit non-zero if any error is found.
//!
//! Each check is a pure inspection — no side effects (no installs, no
//! downloads). The user runs the fix themselves; we only diagnose.

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(clap::Args)]
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
    let mut report = Report::default();

    print_section("Rust toolchain");
    report.extend(check_rust());

    if !args.no_android {
        print_section("Android");
        report.extend(check_android());
    }

    if !args.no_ios {
        print_section("iOS");
        report.extend(check_ios());
    }

    if !args.no_lynx {
        print_section("Lynx artifacts");
        report.extend(check_lynx());
    }

    println!();
    report.print_summary();
    if report.has_errors() {
        std::process::exit(1);
    }
    Ok(())
}

// ----- Output helpers --------------------------------------------------------

const C_OK: &str = "\x1b[32m";
const C_WARN: &str = "\x1b[33m";
const C_ERR: &str = "\x1b[31m";
const C_DIM: &str = "\x1b[2m";
const C_RESET: &str = "\x1b[0m";

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
    fn emit(&self) {
        let (sym, col) = match self.status {
            Status::Ok => ("✓", C_OK),
            Status::Warn => ("⚠", C_WARN),
            Status::Err => ("✗", C_ERR),
        };
        let detail = if self.detail.is_empty() {
            String::new()
        } else {
            format!("  {C_DIM}{}{C_RESET}", self.detail)
        };
        println!("  {col}{sym}{C_RESET} {}{detail}", self.name);
    }
}

#[derive(Default)]
struct Report {
    ok: usize,
    warn: usize,
    err: usize,
}

impl Report {
    fn extend(&mut self, checks: Vec<Check>) {
        for c in checks {
            c.emit();
            match c.status {
                Status::Ok => self.ok += 1,
                Status::Warn => self.warn += 1,
                Status::Err => self.err += 1,
            }
        }
    }
    fn has_errors(&self) -> bool {
        self.err > 0
    }
    fn print_summary(&self) {
        let total = self.ok + self.warn + self.err;
        println!(
            "{total} checks: {C_OK}{}✓{C_RESET}  {C_WARN}{}⚠{C_RESET}  {C_ERR}{}✗{C_RESET}",
            self.ok, self.warn, self.err
        );
    }
}

fn print_section(name: &str) {
    println!("\n{name}");
    println!("{C_DIM}{:─<width$}{C_RESET}", "", width = name.len());
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
                        format!("{line} — Tuft requires 1.85+"),
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

    let installed = run_capture("rustup", &["target", "list", "--installed"])
        .unwrap_or_default();
    let installed: Vec<&str> = installed.lines().map(str::trim).collect();
    for triple in &[
        "aarch64-linux-android",
        "aarch64-apple-ios",
        "aarch64-apple-ios-sim",
        "x86_64-apple-ios",
    ] {
        if installed.iter().any(|t| t == triple) {
            out.push(Check::ok(
                format!("rustup target {triple}"),
                "installed",
            ));
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
        out.push(Check::ok(
            "NDK 21.1.6352462",
            ndk.display().to_string(),
        ));
    } else {
        out.push(Check::err(
            "NDK 21.1.6352462",
            "missing — `sdkmanager 'ndk;21.1.6352462'`",
        ));
    }

    // JDK 11 — Lynx's gradle wrapper (6.7.1) refuses anything newer.
    let jdk11 = resolve_jdk11();
    match jdk11 {
        Some(p) => out.push(Check::ok("JDK 11", p.display().to_string())),
        None => out.push(Check::warn(
            "JDK 11",
            "not found (set TUFT_JAVA11_HOME) — required for Lynx AAR build only",
        )),
    }

    // adb — required for `tuft run android` / install workflows.
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
    if let Some(p) = std::env::var_os("TUFT_JAVA11_HOME").map(PathBuf::from) {
        if p.is_dir() {
            return Some(p);
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    for cand in [
        home.join("work/java11/jdk-11.0.25+9/Contents/Home"),
        home.join("work/java11/jdk-11.0.25+9"),
        PathBuf::from("/Library/Java/JavaVirtualMachines/temurin-11.jdk/Contents/Home"),
    ] {
        if cand.is_dir() {
            return Some(cand);
        }
    }
    None
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
        Ok(s) => out.push(Check::ok(
            "Xcode (xcode-select -p)",
            s.trim().to_string(),
        )),
        Err(_) => out.push(Check::err(
            "Xcode (xcode-select -p)",
            "Xcode command-line tools not configured — `xcode-select --install`",
        )),
    }

    match run_capture("pod", &["--version"]) {
        Ok(s) => out.push(Check::ok(
            "CocoaPods (pod)",
            format!("v{}", s.trim()),
        )),
        Err(_) => out.push(Check::warn(
            "CocoaPods (pod)",
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
                "not inside a Tuft workspace — skipping Lynx checks",
            ));
            return out;
        }
    };
    let target = ws.join("target");

    let lynx_src = target.join("lynx-src");
    if lynx_src.join("platform/android/lynx_android").is_dir() {
        let head = run_capture_in(
            &lynx_src,
            "git",
            &["log", "-1", "--pretty=%h %s"],
        )
        .unwrap_or_default();
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
    let aars = ["LynxBase.aar", "LynxTrace.aar", "LynxAndroid.aar", "ServiceAPI.aar"];
    if aars.iter().all(|a| aar_dir.join(a).is_file()) {
        out.push(Check::ok(
            "Lynx Android AARs",
            format!("4 files at {}", aar_dir.display()),
        ));
    } else {
        out.push(Check::warn(
            "Lynx Android AARs",
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
            "Lynx iOS xcframeworks",
            format!("4 frameworks at {}", ios_dir.display()),
        ));
    } else {
        out.push(Check::warn(
            "Lynx iOS xcframeworks",
            "missing — `cargo xtask ios build-lynx-frameworks`",
        ));
    }

    let headers = target.join("lynx-headers");
    if headers.join("Lynx").is_dir() && headers.join("LynxBase").is_dir() {
        out.push(Check::ok(
            "Lynx staged headers",
            headers.display().to_string(),
        ));
    } else {
        out.push(Check::warn(
            "Lynx staged headers",
            "missing — produced as a side effect of `build-lynx-frameworks`",
        ));
    }

    out
}

/// Best-effort workspace root: walk up from CWD looking for a Cargo.toml
/// whose `[workspace]` table mentions tuft-driver-sys (cheap heuristic).
fn workspace_root() -> Option<PathBuf> {
    let mut cur = std::env::current_dir().ok()?;
    loop {
        let cargo = cur.join("Cargo.toml");
        if cargo.is_file() {
            if let Ok(txt) = std::fs::read_to_string(&cargo) {
                if txt.contains("[workspace]") && txt.contains("tuft-driver-sys") {
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
