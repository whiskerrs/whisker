//! Native consumer build/link tests.
//!
//! These deliberately enter through the generated Gradle/Xcode projects. A
//! passing Cargo cross-build is insufficient: the final app artifact must
//! contain the Rust application library and its stable Host entry points.

use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use whisker_dev_server::Target;

const PACKAGE: &str = "mobile-link-test";
const ANDROID_ABI: &str = "arm64-v8a";

pub fn run(root: &Path, host: &str) -> Result<()> {
    let fixture = root.join("tests/mobile-link-test");
    let target = match host {
        "android" => Target::Android,
        "ios" => Target::IosSimulator,
        _ => bail!("unknown mobile Host {host:?}; expected android or ios"),
    };

    let gen_dir = sync_project(root, &fixture, target)?;
    build_cli(root)?;
    match host {
        "android" => android(root, &gen_dir),
        "ios" => ios(root, &gen_dir),
        _ => unreachable!("validated above"),
    }
}

fn sync_project(root: &Path, fixture: &Path, target: Target) -> Result<PathBuf> {
    let manifest_path = fixture.join("Cargo.toml");
    let manifest = whisker_cli::manifest::resolve(Some(&manifest_path))
        .context("resolve the standalone mobile link test app")?;
    ensure!(manifest.package == PACKAGE, "unexpected link test package");

    let platform_dir = match target {
        Target::Android => "android",
        Target::IosSimulator => "ios",
        _ => unreachable!("mobile-only link test"),
    };
    let gen_dir = fixture.join("gen").join(platform_dir);
    if gen_dir.exists() {
        fs::remove_dir_all(&gen_dir)
            .with_context(|| format!("remove stale {}", gen_dir.display()))?;
    }

    let sync = whisker_cli::platforms::sync_for_target(
        target,
        &manifest.config,
        &manifest.crate_dir,
        root,
        &manifest.package,
    )
    .with_context(|| format!("generate {platform_dir} consumer project"))?;
    ensure!(
        sync.regenerated,
        "clean link test project was not regenerated"
    );

    Ok(sync.gen_dir)
}

fn build_cli(root: &Path) -> Result<()> {
    super::run(Command::new(super::cargo()).current_dir(root).args([
        "build",
        "-p",
        "whisker-cli",
        "--bin",
        "whisker",
    ]))
    .context("build the native-project build tool")
}

fn android(root: &Path, gen_dir: &Path) -> Result<()> {
    let wrapper = if cfg!(windows) {
        gen_dir.join("gradlew.bat")
    } else {
        gen_dir.join("gradlew")
    };
    let mut command = Command::new(&wrapper);
    command
        .current_dir(gen_dir)
        .args(["--no-daemon", ":app:assembleDebug"]);
    prepend_cli_to_path(root, &mut command)?;
    super::run(&mut command).context("Gradle consumer build")?;

    let apk = gen_dir.join("app/build/outputs/apk/debug/app-debug.apk");
    ensure!(apk.is_file(), "Gradle did not produce {}", apk.display());
    let file = fs::File::open(&apk).with_context(|| format!("open {}", apk.display()))?;
    let mut archive = zip::ZipArchive::new(file).context("open generated APK")?;
    let member = format!("lib/{ANDROID_ABI}/libmobile_link_test.so");
    let mut library = archive
        .by_name(&member)
        .with_context(|| format!("APK does not contain {member}"))?;
    ensure!(library.size() > 0, "{member} is empty");

    let inspection_dir = root.join("target/xtask/mobile-link-test/android");
    fs::create_dir_all(&inspection_dir)?;
    let library_path = inspection_dir.join("libmobile_link_test.so");
    let mut bytes = Vec::with_capacity(library.size() as usize);
    library.read_to_end(&mut bytes)?;
    fs::write(&library_path, bytes)?;

    let toolchain = whisker_build::android::resolve_toolchain(ANDROID_ABI, 24)?;
    let readelf = toolchain
        .ar
        .parent()
        .context("NDK llvm-ar has no parent")?
        .join("llvm-readelf");
    let symbols = super::capture(
        Command::new(readelf)
            .args(["--dyn-syms", "--wide"])
            .arg(&library_path),
    )?;
    assert_symbols(
        &symbols,
        &[
            "JNI_OnLoad",
            "Java_rs_whisker_runtime_WhiskerView_nativeCreate",
            "whisker_view_create",
            "whisker_view_tick",
            "whisker_view_destroy",
        ],
        &library_path,
    )
}

#[cfg(target_os = "macos")]
fn ios(root: &Path, gen_dir: &Path) -> Result<()> {
    let available = super::simctl_devices(&["list", "devices", "available", "-j"])?;
    let device = super::first_iphone(&available, false).context("no available iPhone Simulator")?;
    let destination = format!("platform=iOS Simulator,id={device}");
    let derived_data = root.join("target/xtask/mobile-link-test/ios-derived");
    if derived_data.exists() {
        fs::remove_dir_all(&derived_data)
            .with_context(|| format!("remove stale {}", derived_data.display()))?;
    }

    let project = gen_dir.join("MobileLinkTest.xcodeproj");
    let mut command = Command::new("xcodebuild");
    command
        .current_dir(gen_dir)
        .arg("-quiet")
        .arg("-project")
        .arg(&project)
        .args([
            "-scheme",
            "MobileLinkTest",
            "-configuration",
            "Debug",
            "-destination",
            &destination,
            "-derivedDataPath",
        ])
        .arg(&derived_data)
        .args(["CODE_SIGNING_ALLOWED=NO", "build"]);
    prepend_cli_to_path(root, &mut command)?;
    super::run(&mut command).context("Xcode consumer build")?;

    let app = derived_data.join("Build/Products/Debug-iphonesimulator/MobileLinkTest.app");
    let framework = app.join("Frameworks/WhiskerDriver.framework/WhiskerDriver");
    ensure!(app.is_dir(), "Xcode did not produce {}", app.display());
    ensure!(
        framework.is_file(),
        "built app does not embed {}",
        framework.display()
    );
    let symbols = super::capture(Command::new("nm").args(["-gU"]).arg(&framework))?;
    assert_symbols(
        &symbols,
        &[
            "_whisker_view_create",
            "_whisker_view_tick",
            "_whisker_view_destroy",
            "_whisker_aslr_anchor",
        ],
        &framework,
    )
}

#[cfg(not(target_os = "macos"))]
fn ios(_root: &Path, _gen_dir: &Path) -> Result<()> {
    bail!("iOS mobile link test requires macOS and Xcode")
}

fn prepend_cli_to_path(root: &Path, command: &mut Command) -> Result<()> {
    let cli_dir = root.join("target/debug");
    let path = env::var_os("PATH").unwrap_or_default();
    let joined = env::join_paths(std::iter::once(cli_dir).chain(env::split_paths(&path)))
        .context("construct PATH containing the local whisker CLI")?;
    command.env("PATH", joined);
    Ok(())
}

fn assert_symbols(output: &str, expected: &[&str], artifact: &Path) -> Result<()> {
    let symbols = output.split_ascii_whitespace().collect::<Vec<_>>();
    let missing = expected
        .iter()
        .copied()
        .filter(|symbol| !symbols.contains(symbol))
        .collect::<Vec<_>>();
    ensure!(
        missing.is_empty(),
        "{} is missing exported symbols: {}",
        artifact.display(),
        missing.join(", ")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_assertion_reports_every_missing_entry() {
        let error = assert_symbols(
            "0000 whisker_view_create",
            &["whisker_view_create", "whisker_view_tick", "JNI_OnLoad"],
            Path::new("libapp.so"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("whisker_view_tick, JNI_OnLoad"));
    }

    #[test]
    fn symbol_assertion_does_not_accept_a_prefix_match() {
        assert_symbols(
            "0000 whisker_view_create_wrapper",
            &["whisker_view_create"],
            Path::new("libapp.so"),
        )
        .unwrap_err();
    }
}
