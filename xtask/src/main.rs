use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, bail};

mod mobile_abi;
mod mobile_link_test;
mod rust_host_link_test;

#[cfg(test)]
mod host_sdk_surface;

fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    match (arguments.next().as_deref(), arguments.next().as_deref()) {
        (Some("host-conformance"), Some(host)) if arguments.next().is_none() => {
            host_conformance(host)
        }
        (Some("mobile-abi"), Some(mode)) if arguments.next().is_none() => {
            mobile_abi::run(&workspace_root()?, mode)
        }
        (Some("mobile-link-test"), Some(host)) if arguments.next().is_none() => {
            mobile_link_test::run(&workspace_root()?, host)
        }
        (Some("rust-host-link-test"), Some(host)) if arguments.next().is_none() => {
            rust_host_link_test::run(&workspace_root()?, host)
        }
        _ => bail!(
            "usage: cargo xtask host-conformance <desktop|web|android|ios>\n       cargo xtask mobile-abi <generate|check>\n       cargo xtask mobile-link-test <android|ios>\n       cargo xtask rust-host-link-test <desktop|web>"
        ),
    }
}

fn host_conformance(host: &str) -> Result<()> {
    let root = workspace_root()?;
    match host {
        "desktop" => run(Command::new(cargo()).current_dir(&root).args([
            "test",
            "-p",
            "whisker-desktop",
            "--features",
            "host-conformance",
        ])),
        "web" => web(&root),
        "android" => android(&root),
        "ios" => ios(&root),
        _ => bail!("unknown Host {host:?}; expected desktop, web, android, or ios"),
    }
}

fn web(root: &Path) -> Result<()> {
    let chromedriver = chromedriver(root)?;
    run(Command::new("wasm-pack")
        .current_dir(root.join("platforms/web"))
        .args(["test", "--headless", "--chrome", "--chromedriver"])
        .arg(chromedriver))
    .context("Web conformance requires wasm-pack and Chrome")
}

fn chromedriver(root: &Path) -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("CHROMEDRIVER") {
        return Ok(PathBuf::from(path));
    }

    let chrome = chrome_binary().context(
        "could not find Chrome; set GOOGLE_CHROME_BIN to Chrome or CHROMEDRIVER to a compatible driver",
    )?;
    let chrome_version = capture(Command::new(&chrome).arg("--version"))?;
    let chrome_version = parse_version(&chrome_version)
        .with_context(|| format!("parse Chrome version from {chrome_version:?}"))?;
    let chrome_major = version_major(&chrome_version)?;

    if let Some(path) = find_on_path(chromedriver_binary()) {
        if driver_major(&path).as_deref() == Some(chrome_major.as_str()) {
            return Ok(path);
        }
    }

    let platform = chromedriver_platform()?;
    let cache = root
        .join("target/xtask/chromedriver")
        .join(&chrome_major)
        .join(platform);
    let driver = cache.join(chromedriver_binary());
    if driver.is_file() && driver_major(&driver).as_deref() == Some(chrome_major.as_str()) {
        return Ok(driver);
    }

    let version_url = format!(
        "https://googlechromelabs.github.io/chrome-for-testing/LATEST_RELEASE_{chrome_major}"
    );
    let driver_version = capture(Command::new("curl").args([
        "--fail",
        "--location",
        "--silent",
        "--show-error",
        &version_url,
    ]))
    .with_context(|| format!("find a ChromeDriver release for Chrome {chrome_version}"))?;
    let driver_version = driver_version.trim();
    parse_version(driver_version)
        .with_context(|| format!("invalid ChromeDriver release {driver_version:?}"))?;

    eprintln!(
        "Chrome {chrome_version} detected; installing compatible ChromeDriver {driver_version}"
    );
    fs::create_dir_all(&cache).with_context(|| format!("create {}", cache.display()))?;
    let url = format!(
        "https://storage.googleapis.com/chrome-for-testing-public/{driver_version}/{platform}/chromedriver-{platform}.zip"
    );
    let archive_path = cache.join(format!("chromedriver.zip.download-{}", std::process::id()));
    run(Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--output",
        ])
        .arg(&archive_path)
        .arg(&url))
    .with_context(|| format!("download ChromeDriver {driver_version}"))?;
    let bytes = fs::read(&archive_path).context("read ChromeDriver archive")?;
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).context("open ChromeDriver archive")?;
    let member = format!("chromedriver-{platform}/{}", chromedriver_binary());
    let mut source = archive
        .by_name(&member)
        .with_context(|| format!("find {member} in ChromeDriver archive"))?;
    let temporary = cache.join(format!(
        "{}.download-{}",
        chromedriver_binary(),
        std::process::id()
    ));
    {
        let mut output = fs::File::create(&temporary)
            .with_context(|| format!("create {}", temporary.display()))?;
        io::copy(&mut source, &mut output).context("extract ChromeDriver")?;
    }
    make_executable(&temporary)?;
    fs::rename(&temporary, &driver).with_context(|| format!("install {}", driver.display()))?;
    fs::remove_file(&archive_path).with_context(|| format!("remove {}", archive_path.display()))?;
    Ok(driver)
}

fn chrome_binary() -> Option<PathBuf> {
    for variable in ["GOOGLE_CHROME_BIN", "CHROME"] {
        if let Some(path) = std::env::var_os(variable) {
            return Some(PathBuf::from(path));
        }
    }

    if let Some(path) = default_chrome_locations()
        .into_iter()
        .find(|path| path.is_file())
    {
        return Some(path);
    }

    [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "chrome",
    ]
    .into_iter()
    .find_map(find_on_path)
}

#[cfg(target_os = "macos")]
fn default_chrome_locations() -> Vec<PathBuf> {
    vec![PathBuf::from(
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    )]
}

#[cfg(target_os = "windows")]
fn default_chrome_locations() -> Vec<PathBuf> {
    ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .map(|base| base.join("Google/Chrome/Application/chrome.exe"))
        .collect()
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn default_chrome_locations() -> Vec<PathBuf> {
    Vec::new()
}

fn parse_version(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .find(|part| {
            let mut components = part.split('.');
            components.clone().count() >= 3
                && components.all(|component| component.parse::<u32>().is_ok())
        })
        .map(str::to_owned)
}

fn version_major(version: &str) -> Result<String> {
    version
        .split('.')
        .next()
        .filter(|major| major.parse::<u32>().is_ok())
        .map(str::to_owned)
        .with_context(|| format!("invalid version {version:?}"))
}

fn driver_major(path: &Path) -> Option<String> {
    let output = Command::new(path).arg("--version").output().ok()?;
    output.status.success().then_some(())?;
    let version = parse_version(&String::from_utf8(output.stdout).ok()?)?;
    version_major(&version).ok()
}

fn find_on_path(binary: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|directory| directory.join(binary))
        .find(|path| path.is_file())
}

fn chromedriver_binary() -> &'static str {
    if cfg!(windows) {
        "chromedriver.exe"
    } else {
        "chromedriver"
    }
}

fn chromedriver_platform() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("mac-arm64"),
        ("macos", "x86_64") => Ok("mac-x64"),
        ("linux", "x86_64") => Ok("linux64"),
        ("windows", "x86_64") => Ok("win64"),
        (os, architecture) => {
            bail!("ChromeDriver is unavailable for {os}/{architecture}")
        }
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn android(root: &Path) -> Result<()> {
    let wrapper = if cfg!(windows) {
        root.join("platforms/android/gradle-plugin/gradlew.bat")
    } else {
        root.join("platforms/android/gradle-plugin/gradlew")
    };
    run(Command::new(wrapper)
        .current_dir(root.join("platforms/android"))
        .arg("--project-dir")
        .arg(root.join("platforms/android"))
        .arg(":runtime:connectedDebugAndroidTest"))
}

#[cfg(target_os = "macos")]
fn ios(root: &Path) -> Result<()> {
    let package_root = root.join("platforms/ios");
    let sdk = capture(Command::new("xcrun").args(["--sdk", "iphonesimulator", "--show-sdk-path"]))?;
    let triple = match std::env::consts::ARCH {
        "aarch64" => "arm64-apple-ios-simulator",
        "x86_64" => "x86_64-apple-ios-simulator",
        architecture => bail!("unsupported macOS architecture {architecture}"),
    };
    run(Command::new("swift")
        .current_dir(&package_root)
        .arg("build")
        .arg("--package-path")
        .arg(&package_root)
        .arg("--build-tests")
        .arg("--triple")
        .arg(triple)
        .arg("--sdk")
        .arg(sdk.trim()))?;

    let booted = simctl_devices(&["list", "devices", "booted", "-j"])?;
    let (device, needs_boot) = match first_iphone(&booted, true) {
        Some(device) => (device, false),
        None => {
            let available = simctl_devices(&["list", "devices", "available", "-j"])?;
            (
                first_iphone(&available, false).context("no available iPhone Simulator")?,
                true,
            )
        }
    };
    if needs_boot {
        run(Command::new("xcrun").args(["simctl", "boot", &device]))?;
    }
    run(Command::new("xcrun").args(["simctl", "bootstatus", &device, "-b"]))?;

    let developer_dir = capture(Command::new("xcode-select").arg("-p"))?;
    let xctest = Path::new(developer_dir.trim())
        .join("Platforms/iPhoneSimulator.platform/Developer/Library/Xcode/Agents/xctest");
    let bundle = package_root
        .join(".build")
        .join(triple)
        .join("debug/WhiskerPackageTests.xctest");
    run(Command::new("xcrun")
        .arg("simctl")
        .arg("spawn")
        .arg(device)
        .arg(xctest)
        .arg(bundle))
}

#[cfg(not(target_os = "macos"))]
fn ios(_root: &Path) -> Result<()> {
    bail!("iOS Host conformance requires macOS and Xcode")
}

#[cfg(target_os = "macos")]
fn simctl_devices(arguments: &[&str]) -> Result<serde_json::Value> {
    let output = capture(Command::new("xcrun").arg("simctl").args(arguments))?;
    serde_json::from_str(&output).context("decode simctl device list")
}

#[cfg(target_os = "macos")]
fn first_iphone(value: &serde_json::Value, require_booted: bool) -> Option<String> {
    value
        .get("devices")?
        .as_object()?
        .values()
        .find_map(|devices| {
            devices.as_array()?.iter().find_map(|device| {
                let name = device.get("name")?.as_str()?;
                let state = device.get("state")?.as_str()?;
                let available = device
                    .get("isAvailable")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true);
                (name.starts_with("iPhone") && available && (!require_booted || state == "Booted"))
                    .then(|| device.get("udid")?.as_str().map(str::to_owned))?
            })
        })
}

fn workspace_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask must be inside the workspace")?
        .canonicalize()
        .context("resolve workspace root")
}

fn cargo() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

fn run(command: &mut Command) -> Result<()> {
    let description = describe(command);
    let status = command
        .status()
        .with_context(|| format!("start {description}"))?;
    if !status.success() {
        bail!("{description} exited with {status}");
    }
    Ok(())
}

fn capture(command: &mut Command) -> Result<String> {
    let description = describe(command);
    let Output {
        status,
        stdout,
        stderr,
    } = command
        .output()
        .with_context(|| format!("start {description}"))?;
    if !status.success() {
        bail!(
            "{description} exited with {status}: {}",
            String::from_utf8_lossy(&stderr).trim()
        );
    }
    String::from_utf8(stdout).with_context(|| format!("decode output from {description}"))
}

fn describe(command: &Command) -> String {
    std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(OsStr::to_string_lossy)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn selects_a_booted_available_iphone() {
        let devices = serde_json::json!({
            "devices": {
                "runtime": [
                    { "name": "iPad Pro", "state": "Booted", "udid": "ipad", "isAvailable": true },
                    { "name": "iPhone 17", "state": "Shutdown", "udid": "off", "isAvailable": true },
                    { "name": "iPhone 17 Pro", "state": "Booted", "udid": "phone", "isAvailable": true }
                ]
            }
        });
        assert_eq!(first_iphone(&devices, true).as_deref(), Some("phone"));
    }

    #[test]
    fn parses_browser_and_driver_versions() {
        assert_eq!(
            parse_version("Google Chrome 151.0.7922.170\n").as_deref(),
            Some("151.0.7922.170")
        );
        assert_eq!(
            parse_version("ChromeDriver 151.0.7922.138 (revision)").as_deref(),
            Some("151.0.7922.138")
        );
        assert_eq!(parse_version("Google Chrome dev"), None);
    }
}
