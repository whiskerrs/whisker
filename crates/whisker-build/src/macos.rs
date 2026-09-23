//! Build and bundle a CNG-generated Cargo-based macOS project.
//!
//! Both `whisker build macos` and `whisker run desktop` call this module. The
//! generated `gen/macos` tree is therefore the single platform project rather
//! than a development-only launcher.

use anyhow::{Context, Result, bail, ensure};
use std::path::{Path, PathBuf};

use crate::{Profile, ui};

/// Inputs needed to compile and assemble one macOS `.app` bundle.
pub struct MacosBuild<'a> {
    /// Generated `gen/macos` project directory.
    pub project_dir: &'a Path,
    /// Cargo target directory shared by run and build.
    pub target_dir: &'a Path,
    /// Human-readable `.app` bundle name.
    pub app_name: &'a str,
    /// Generated Cargo binary/package name.
    pub binary_name: &'a str,
    /// Cargo build profile.
    pub profile: Profile,
    /// Generated Host features enabled for this build.
    pub features: &'a [String],
    /// Optional hot-patch capture envelope for development builds.
    pub capture: Option<&'a crate::CaptureShims>,
}

/// Compiles the generated project and returns the assembled `.app` path.
pub fn build_app(inputs: &MacosBuild<'_>) -> Result<PathBuf> {
    let plan = whisker_cng::macos::load_build_plan(inputs.project_dir)?;
    ensure!(
        plan.app_name == inputs.app_name && plan.rust.target == inputs.binary_name,
        "macOS build/run names do not match the generated plan; regenerate gen/macos"
    );
    let manifest = inputs.project_dir.join(plan.rust.manifest.as_str());
    ensure!(
        manifest.is_file(),
        "generated macOS Cargo manifest is missing"
    );
    let step = ui::step(
        ui::OperationKind::Compile,
        format!("{} ({:?})", inputs.binary_name, inputs.profile),
    );
    let selection = whisker_cng::CargoSelection::load_project(inputs.project_dir)?;
    let triple = selection
        .as_ref()
        .and_then(|selection| selection.target.as_deref());
    let mut command = std::process::Command::new("cargo");
    command
        .arg("build")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--target-dir")
        .arg(inputs.target_dir)
        .arg("--package")
        .arg(&plan.rust.package)
        .arg("--bin")
        .arg(&plan.rust.target)
        .env("MACOSX_DEPLOYMENT_TARGET", &plan.minimum_system_version);
    if let Some(triple) = triple {
        command.arg("--target").arg(triple);
    }
    if matches!(inputs.profile, Profile::Release) {
        command.arg("--release");
    }
    if !plan.rust.default_features {
        command.arg("--no-default-features");
    }
    let features: Vec<_> = plan
        .rust
        .features
        .iter()
        .chain(inputs.features)
        .cloned()
        .collect();
    if !features.is_empty() {
        command.arg("--features").arg(features.join(","));
    }
    if let Some(capture) = inputs.capture {
        for (key, value) in crate::capture_env_vars_all_crates(capture) {
            command.env(key, value);
        }
    }
    let status = step
        .pipe(&mut command)
        .context("spawn cargo for macOS Host")?;
    if !status.success() {
        step.fail(status.to_string());
        bail!("cargo build for macOS Host failed ({status})");
    }
    step.done("");

    let profile_dir = match inputs.profile {
        Profile::Debug => "debug",
        Profile::Release => "release",
    };
    let artifacts = triple.map_or_else(
        || inputs.target_dir.to_path_buf(),
        |triple| inputs.target_dir.join(triple),
    );
    let executable = artifacts.join(profile_dir).join(inputs.binary_name);
    if !executable.is_file() {
        bail!(
            "macOS Host executable missing after cargo build: {}",
            executable.display()
        );
    }

    let bundle = inputs
        .target_dir
        .join("bundles")
        .join(profile_dir)
        .join(format!("{}.app", inputs.app_name));
    assemble_bundle(inputs.project_dir, &executable, &bundle)?;
    Ok(bundle)
}

/// Stage and validate every input, then build/sign a replacement before removing
/// the previous bundle. Native tool failures therefore leave the old app usable.
fn assemble_bundle(project_dir: &Path, executable: &Path, bundle: &Path) -> Result<()> {
    let plan = whisker_cng::macos::load_build_plan(project_dir)?;
    let files = whisker_cng::macos::bundle_files(project_dir, &plan)?;
    let entitlements = whisker_cng::macos::signing_entitlements(project_dir, &plan)?;
    let icons = plan
        .icons
        .iter()
        .map(|icon| whisker_cng::macos::icon_files(project_dir, icon))
        .collect::<Result<Vec<_>>>()?;
    ensure!(executable.is_file(), "missing macOS executable");
    for ancestor in bundle.ancestors() {
        if let Ok(meta) = std::fs::symlink_metadata(ancestor) {
            ensure!(
                !meta.file_type().is_symlink(),
                "refusing symlink bundle output: {}",
                ancestor.display()
            );
        }
    }
    let parent = bundle.parent().context("bundle needs a parent directory")?;
    std::fs::create_dir_all(parent)?;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let temp_path = parent.join(format!(
        ".whisker-bundle-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir(&temp_path)?;
    let temp = TempDirectory(temp_path);
    let staged = temp.0.join(format!("{}.app", plan.app_name));
    std::fs::create_dir_all(staged.join("Contents/MacOS"))?;
    std::fs::create_dir_all(staged.join("Contents/Resources"))?;
    std::fs::copy(
        executable,
        staged.join("Contents/MacOS").join(&plan.rust.target),
    )?;
    for (path, entry) in files {
        write_file(&staged.join(path.as_str()), &entry.to_bytes()?, entry.mode)?;
    }
    for (index, (icon, files)) in plan.icons.iter().zip(icons).enumerate() {
        let root = temp.0.join(format!("icon-{index}"));
        for (path, entry) in files {
            write_file(&root.join(path.as_str()), &entry.to_bytes()?, entry.mode)?;
        }
        let output = staged.join(icon.destination.as_str());
        std::fs::create_dir_all(output.parent().unwrap())?;
        let result = std::process::Command::new("iconutil")
            .args(["--convert", "icns", "--output"])
            .arg(output)
            .arg(root.join(icon.source.as_str()))
            .output()
            .context("compile macOS iconset")?;
        ensure!(
            result.status.success(),
            "compile macOS iconset: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let signing = temp.0.join("Entitlements.plist");
    std::fs::write(&signing, entitlements)?;
    let result = std::process::Command::new("codesign")
        .args(["--force", "--sign", "-", "--entitlements"])
        .arg(signing)
        .arg(&staged)
        .output()
        .context("ad-hoc sign macOS bundle")?;
    ensure!(
        result.status.success(),
        "sign macOS bundle: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    if bundle.exists() {
        std::fs::remove_dir_all(bundle)?;
    }
    std::fs::rename(staged, bundle)?;
    Ok(())
}
fn write_file(path: &Path, bytes: &[u8], mode: Option<u32>) -> Result<()> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(())
}
struct TempDirectory(PathBuf);
impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    #[test]
    fn cargo_features_bundle_resources_and_entitlements_are_applied() {
        let root = std::env::temp_dir().join(format!("whisker-macos-build-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let root = TempDirectory(root.canonicalize().unwrap());
        let project = root.0.join("project");
        let target = root.0.join("target");
        std::fs::create_dir_all(project.join(".whisker")).unwrap();
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(project.join("Cargo.toml"), "[workspace]\n[package]\nname='fixture'\nversion='0.0.0'\nedition='2024'\n[features]\ndefault=['unwanted']\nunwanted=[]\nselected=[]\n").unwrap();
        std::fs::write(
            project.join("src/main.rs"),
            r#"
#[cfg(any(feature="unwanted",not(feature="selected")))] compile_error!("wrong Cargo selection");
fn main() {
    let exe = std::env::current_exe().unwrap();
    let data = exe.parent().unwrap().parent().unwrap().join("Resources/data/message.txt");
    assert_eq!(std::fs::read_to_string(data).unwrap(),"payload");
}
"#,
        )
        .unwrap();
        std::fs::write(project.join("Info.plist"), r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>fixture</string><key>CFBundleIdentifier</key><string>test.whisker.fixture</string><key>CFBundlePackageType</key><string>APPL</string></dict></plist>"#).unwrap();
        std::fs::write(project.join("Entitlements.plist"), r#"<?xml version="1.0"?><plist version="1.0"><dict><key>com.apple.security.get-task-allow</key><true/></dict></plist>"#).unwrap();
        std::fs::write(project.join("input.txt"), "payload").unwrap();
        std::fs::write(project.join("private.txt"), "private").unwrap();
        std::fs::write(project.join(".whisker/macos-build.json"), r#"{
            "version":1,"app_name":"Fixture","minimum_system_version":"12.0",
            "rust":{"manifest":"Cargo.toml","package":"fixture","target":"fixture","kind":"bin","features":["selected"],"default_features":false},
            "entitlements":"Entitlements.plist","icons":[],
            "files":{"Contents/Info.plist":"Info.plist","Contents/Resources/data/message.txt":"input.txt"}
        }"#).unwrap();
        let inputs = MacosBuild {
            project_dir: &project,
            target_dir: &target,
            app_name: "Fixture",
            binary_name: "fixture",
            profile: Profile::Debug,
            features: &[],
            capture: None,
        };
        let bundle = build_app(&inputs).unwrap();
        assert!(!bundle.join("Contents/Resources/private.txt").exists());
        assert!(
            std::process::Command::new(bundle.join("Contents/MacOS/fixture"))
                .current_dir("/")
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("codesign")
                .args(["--verify", "--strict"])
                .arg(&bundle)
                .status()
                .unwrap()
                .success()
        );
        let ent = std::process::Command::new("codesign")
            .args(["--display", "--entitlements", ":-"])
            .arg(&bundle)
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&ent.stdout).contains("com.apple.security.get-task-allow"));
        std::fs::write(bundle.join("stale.txt"), "stale").unwrap();
        build_app(&inputs).unwrap();
        assert!(!bundle.join("stale.txt").exists());
        std::fs::remove_file(project.join("input.txt")).unwrap();
        assert!(assemble_bundle(&project, &target.join("debug/fixture"), &bundle).is_err());
        assert!(bundle.join("Contents/Resources/data/message.txt").is_file());
    }
}
