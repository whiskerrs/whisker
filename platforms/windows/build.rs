//! Restores the `Whisker.WindowsHost` NuGet package and links it into Windows executables.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Pinned `Whisker.WindowsHost` release; bumped with the `windows-host-v*` tag.
const WHISKER_WINDOWS_HOST_VERSION: &str = "0.1.0";
const WINDOWS_APP_SDK_VERSION: &str = "2.6.5";
const NUGET_ORG: &str = "https://api.nuget.org/v3/index.json";

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=NUGET");
    println!("cargo::rerun-if-env-changed=WHISKER_WINDOWS_HOST_FEED");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let (platform, rid) = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => ("x64", "win-x64"),
        Ok("aarch64") => ("ARM64", "win-arm64"),
        other => panic!("the Windows Host supports x86_64 and aarch64, not {other:?}"),
    };
    let out = std::env::var("OUT_DIR").unwrap();
    // nuget.exe rejects verbatim `\\?\` paths, which cargo inherits from a canonicalized target dir.
    let out = PathBuf::from(out.strip_prefix(r"\\?\").unwrap_or(&out));
    // Versioned packages are immutable, so builds share one short directory that stays under MAX_PATH.
    let packages = std::env::var_os("LOCALAPPDATA")
        .map(|local| PathBuf::from(local).join(r"whisker\nuget"))
        .unwrap_or_else(|| out.join("packages"));
    restore(&out, &packages);
    let host = packages.join(format!(
        "Whisker.WindowsHost.{WHISKER_WINDOWS_HOST_VERSION}"
    ));
    let foundation = packages.join(format!(
        "Microsoft.WindowsAppSDK.Foundation.{WINDOWS_APP_SDK_VERSION}"
    ));
    println!(
        "cargo::rustc-link-search=native={}",
        host.join("build/native/lib").join(platform).display()
    );
    println!(
        "cargo::rustc-link-search=native={}",
        foundation.join("lib/native").join(platform).display()
    );
    for library in [
        "static=WhiskerHost",
        "Microsoft.WindowsAppRuntime.Bootstrap",
        "windowsapp",
        "user32",
    ] {
        println!("cargo::rustc-link-lib={library}");
    }
    let bootstrap = foundation
        .join("runtimes")
        .join(rid)
        .join("native/Microsoft.WindowsAppRuntime.Bootstrap.dll");
    // OUT_DIR is `<target>/<triple?>/<profile>/build/<pkg>-<hash>/out`; the executable lands in `<profile>`.
    let profile_dir = out.ancestors().nth(3).unwrap();
    std::fs::copy(
        &bootstrap,
        profile_dir.join("Microsoft.WindowsAppRuntime.Bootstrap.dll"),
    )
    .unwrap_or_else(|error| panic!("copy {}: {error}", bootstrap.display()));
}

fn restore(out: &Path, packages: &Path) {
    let mut config = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<packages>\n");
    for (id, version) in [
        ("Whisker.WindowsHost", WHISKER_WINDOWS_HOST_VERSION),
        (
            "Microsoft.WindowsAppSDK.Foundation",
            WINDOWS_APP_SDK_VERSION,
        ),
    ] {
        config += &format!(
            "  <package id=\"{id}\" version=\"{version}\" targetFramework=\"native\" />\n"
        );
    }
    config += "</packages>\n";
    let path = out.join("packages.config");
    std::fs::write(&path, config).unwrap();
    let mut command = Command::new(std::env::var_os("NUGET").unwrap_or_else(|| "nuget".into()));
    command
        .arg("restore")
        .arg(&path)
        .arg("-PackagesDirectory")
        .arg(packages)
        .arg("-NonInteractive");
    if let Some(feed) = std::env::var_os("WHISKER_WINDOWS_HOST_FEED") {
        command
            .arg("-Source")
            .arg(feed)
            .args(["-Source", NUGET_ORG]);
    }
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("run nuget.exe (install it or set NUGET): {error}"));
    assert!(status.success(), "nuget restore failed ({status})");
}
