//! Build the CNG Windows/Linux Cargo executables and assemble their distribution.
use crate::Profile;
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
};
use whisker_cng::desktop::{distribution_files, load_build_plan};
use whisker_plugin::{
    FileEntry,
    project::{ExecutableSource, ProjectPath},
};

/// A generated Windows or Linux project and its build output location.
pub struct DesktopBuild<'a> {
    pub project_dir: &'a Path,
    pub target_dir: &'a Path,
    pub profile: Profile,
}
/// Compile selected Cargo binaries, copy prebuilt helpers, and return the
/// distribution root. Cross compilation requires the target toolchain/sysroot.
pub fn build_app(inputs: &DesktopBuild<'_>) -> Result<PathBuf> {
    let project = inputs.project_dir.canonicalize()?;
    let target = if inputs.target_dir.is_absolute() {
        inputs.target_dir.to_path_buf()
    } else {
        std::env::current_dir()?.join(inputs.target_dir)
    };
    let plan = load_build_plan(&project)?;
    let files = distribution_files(&project, &plan)?;
    let mut executables = BTreeMap::new();
    let triple = plan
        .cargo_selection
        .target
        .as_deref()
        .context("desktop target missing")?;
    for (id, executable) in &plan.executables {
        let path = match &executable.source {
            ExecutableSource::Prebuilt(path) => project.join(path.as_str()),
            ExecutableSource::Cargo(rust) => {
                let manifest = project.join(rust.manifest.as_str());
                ensure!(manifest.is_file(), "missing desktop Cargo manifest");
                let mut command =
                    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
                command
                    .current_dir(&project)
                    .arg("build")
                    .arg("--manifest-path")
                    .arg(&manifest)
                    .arg("--package")
                    .arg(&rust.package)
                    .arg("--bin")
                    .arg(&rust.target)
                    .arg("--target-dir")
                    .arg(&target)
                    .arg("--target")
                    .arg(triple);
                if let Some(flag) = inputs.profile.cargo_flag() {
                    command.arg(flag);
                }
                if !rust.default_features {
                    command.arg("--no-default-features");
                }
                for feature in &rust.features {
                    command.arg("--features").arg(feature);
                }
                let step = crate::ui::step(
                    crate::ui::OperationKind::Compile,
                    format!("{} ({triple})", rust.target),
                );
                let status = step
                    .pipe(&mut command)
                    .context("build desktop Cargo binary")?;
                ensure!(status.success(), "desktop Cargo build failed ({status})");
                step.done("");
                let suffix = if plan.platform == whisker_cng::GenerationTarget::Windows {
                    ".exe"
                } else {
                    ""
                };
                target
                    .join(triple)
                    .join(inputs.profile.dir_name())
                    .join(format!("{}{suffix}", rust.target))
            }
        };
        // Snapshot all bytes and reject links before touching the old distribution.
        for ancestor in path.ancestors() {
            if let Ok(meta) = std::fs::symlink_metadata(ancestor) {
                ensure!(
                    !meta.file_type().is_symlink(),
                    "symlink executable input {}",
                    ancestor.display()
                );
            }
        }
        let meta = std::fs::metadata(&path)
            .with_context(|| format!("read executable {id}: {}", path.display()))?;
        ensure!(meta.is_file(), "desktop executable must be a regular file");
        let entry = FileEntry::binary(&std::fs::read(&path)?);
        #[cfg(unix)]
        let entry = {
            use std::os::unix::fs::PermissionsExt;
            let mut entry = entry;
            entry.mode = Some(meta.permissions().mode() & 0o777 | 0o111);
            entry
        };
        executables.insert(executable.destination.clone(), entry);
    }
    let out = target.join("dist").join(inputs.profile.dir_name());
    assemble(&out, files, executables)?;
    Ok(out)
}
fn assemble(
    out: &Path,
    mut files: BTreeMap<ProjectPath, FileEntry>,
    executables: BTreeMap<ProjectPath, FileEntry>,
) -> Result<()> {
    for (path, entry) in executables {
        ensure!(
            files.insert(path, entry).is_none(),
            "executable output collision"
        );
    }
    for ancestor in out.ancestors() {
        if let Ok(meta) = std::fs::symlink_metadata(ancestor) {
            ensure!(
                !meta.file_type().is_symlink(),
                "symlink distribution output"
            );
        }
    }
    let parent = out.parent().context("distribution needs a parent")?;
    std::fs::create_dir_all(parent)?;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let temporary = parent.join(format!(
        ".whisker-dist-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir(&temporary)?;
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let guard = Cleanup(temporary);
    for (path, entry) in files {
        let path = guard.0.join(path.as_str());
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, entry.to_bytes()?)?;
        #[cfg(unix)]
        if let Some(mode) = entry.mode {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))?;
        }
    }
    // Build and input failures preserve the previous output. Publishing itself
    // follows the macOS builder's replace-directory contract.
    if out.exists() {
        std::fs::remove_dir_all(out)?;
    }
    std::fs::rename(&guard.0, out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    use whisker_cng::CargoSelection;
    use whisker_cng::desktop::{DesktopProjectInputs, sync_project};
    use whisker_plugin::project::*;
    struct Root(PathBuf);
    impl Root {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "whisker-desktop-build-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path.canonicalize().unwrap())
        }
    }
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn path(value: &str) -> ProjectPath {
        ProjectPath::new(value).unwrap()
    }
    #[test]
    fn distribution_contains_only_declared_outputs_and_failure_preserves_previous_build() {
        let root = Root::new();
        for windows in [false, true] {
            let executable = DesktopExecutable {
                source: ExecutableSource::Prebuilt(path("input/bin")),
                destination: path(if windows { "demo.exe" } else { "bin/demo" }),
            };
            let files: ProjectFiles = [
                (
                    path("input/bin"),
                    ProjectFile::Generated {
                        entry: FileEntry::binary(b"executable fixture"),
                    },
                ),
                (
                    path("input/data"),
                    ProjectFile::Generated {
                        entry: FileEntry::binary(b"assets"),
                    },
                ),
                (
                    path("input/private"),
                    ProjectFile::Generated {
                        entry: FileEntry::binary(b"staging only"),
                    },
                ),
            ]
            .into();
            let resources = vec![Resource {
                source: path("input/data"),
                destination: path("share/data"),
                kind: ResourceKind::File,
            }];
            let project = if windows {
                ProjectIr::Windows(WindowsProjectIr {
                    application: "app".into(),
                    executables: [(
                        "app".into(),
                        WindowsExecutable {
                            executable,
                            manifest: None,
                            icon: None,
                            version_info: None,
                            resource_scripts: vec![],
                        },
                    )]
                    .into(),
                    files,
                    resources,
                    ..Default::default()
                })
            } else {
                ProjectIr::Linux(LinuxProjectIr {
                    app_id: "test.demo".into(),
                    application: "app".into(),
                    executables: [("app".into(), executable)].into(),
                    files,
                    resources,
                    ..Default::default()
                })
            };
            let project_dir = root.0.join(if windows { "windows" } else { "linux" });
            sync_project(
                &project_dir,
                &DesktopProjectInputs {
                    project,
                    app_crate_dir: None,
                    cargo_selection: Default::default(),
                },
            )
            .unwrap();
            let target = project_dir.join("target");
            let input = DesktopBuild {
                project_dir: &project_dir,
                target_dir: &target,
                profile: Profile::Release,
            };
            let out = build_app(&input).unwrap();
            assert_eq!(std::fs::read(out.join("share/data")).unwrap(), b"assets");
            assert!(!out.join("input/private").exists());
            let exe = out.join(if windows { "demo.exe" } else { "bin/demo" });
            assert_eq!(std::fs::read(&exe).unwrap(), b"executable fixture");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_ne!(
                    std::fs::metadata(&exe).unwrap().permissions().mode() & 0o111,
                    0
                );
            }
            std::fs::write(out.join("stale"), b"old").unwrap();
            build_app(&input).unwrap();
            assert!(!out.join("stale").exists());
            std::fs::remove_file(project_dir.join("input/data")).unwrap();
            assert!(build_app(&input).is_err());
            assert_eq!(std::fs::read(out.join("share/data")).unwrap(), b"assets");
            assert!(exe.is_file());
        }
    }
    // Native CI exercises a real Cargo package with bin-level required features
    // and a deliberately invalid default feature. No Whisker runtime is needed.
    #[test]
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn cargo_selection_builds_and_runs_the_declared_binary() {
        let root = Root::new();
        let windows = cfg!(target_os = "windows");
        let build = RustBuild {
            manifest: path("Cargo.toml"),
            package: "fixture".into(),
            target: "fixture-bin".into(),
            kind: RustArtifactKind::Bin,
            features: vec!["selected".into()],
            default_features: false,
        };
        let executable = DesktopExecutable {
            source: ExecutableSource::Cargo(build),
            destination: path(if windows { "app.exe" } else { "bin/app" }),
        };
        let files = [
            (path("Cargo.toml"),ProjectFile::Generated { entry:FileEntry::text("[workspace]\n[package]\nname='fixture'\nversion='0.0.0'\nedition='2024'\n[[bin]]\nname='fixture-bin'\npath='main.rs'\nrequired-features=['selected']\n[features]\ndefault=['broken']\nbroken=[]\nselected=[]\n") }),
            (path("main.rs"),ProjectFile::Generated { entry:FileEntry::text("#[cfg(feature=\"broken\")] compile_error!(\"default enabled\"); fn main() { println!(\"built selected binary\"); }") }),
        ].into();
        let project = if windows {
            ProjectIr::Windows(WindowsProjectIr {
                application: "app".into(),
                executables: [(
                    "app".into(),
                    WindowsExecutable {
                        executable,
                        manifest: None,
                        icon: None,
                        version_info: None,
                        resource_scripts: vec![],
                    },
                )]
                .into(),
                files,
                ..Default::default()
            })
        } else {
            ProjectIr::Linux(LinuxProjectIr {
                app_id: "test.fixture".into(),
                application: "app".into(),
                executables: [("app".into(), executable)].into(),
                files,
                ..Default::default()
            })
        };
        let host = Command::new("rustc").arg("-vV").output().unwrap();
        let host = String::from_utf8(host.stdout)
            .unwrap()
            .lines()
            .find_map(|l| l.strip_prefix("host: ").map(str::to_owned))
            .unwrap();
        sync_project(
            &root.0,
            &DesktopProjectInputs {
                project,
                app_crate_dir: None,
                cargo_selection: CargoSelection {
                    target: Some(host),
                    ..Default::default()
                },
            },
        )
        .unwrap();
        let out = build_app(&DesktopBuild {
            project_dir: &root.0,
            target_dir: &root.0.join("target"),
            profile: Profile::Debug,
        })
        .unwrap();
        let output = Command::new(out.join(if windows { "app.exe" } else { "bin/app" }))
            .current_dir(std::env::temp_dir())
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"built selected binary\n");
    }
}

#[cfg(test)]
mod cross_tests {
    use super::*;
    use whisker_cng::{Config, GenerationTarget, ProjectEngine, desktop::*};
    use whisker_plugin::project::*;
    /// This intentionally tiny no_std binary needs no Windows SDK libraries.
    /// Run with an MSVC target, llvm-rc, and lld-link configured in the environment.
    #[test]
    #[ignore = "requires Windows Rust target, resource compiler and linker"]
    fn windows_cargo_links_resources_into_a_real_pe_image() {
        let root = std::env::temp_dir().join(format!("whisker-pe-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let mut config = Config::default();
        config.name("日本語 App").bundle_id("test.resources");
        let mut inputs = inputs_from(
            &config,
            GenerationTarget::Windows,
            "fixture".into(),
            root.clone(),
            "\"0.14\"".into(),
        )
        .unwrap();
        inputs.cargo_selection.target = Some("x86_64-pc-windows-msvc".into());
        let selection = inputs.cargo_selection.clone();
        let empty = inputs.empty_project().unwrap();
        let mut project = ProjectEngine::with_windows_application(inputs)
            .compose(&Config::default(), &empty)
            .unwrap()
            .project;
        let ProjectIr::Windows(w) = &mut project else {
            panic!()
        };
        for (path, text) in [
            (
                "Cargo.toml",
                "[workspace]\n[package]\nname='fixture-whisker-windows'\nversion='0.0.0'\nedition='2024'\n[profile.dev]\npanic='abort'\n",
            ),
            (
                "src/main.rs",
                "#![no_std]\n#![no_main]\n#[panic_handler] fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }\n#[unsafe(no_mangle)] pub extern \"C\" fn main() -> i32 { 42 }\n",
            ),
        ] {
            w.files.insert(
                ProjectPath::new(path).unwrap(),
                ProjectFile::Generated {
                    entry: FileEntry::text(text),
                },
            );
        }
        sync_project(
            &root,
            &DesktopProjectInputs {
                project,
                app_crate_dir: Some(root.clone()),
                cargo_selection: selection,
            },
        )
        .unwrap();
        let out = build_app(&DesktopBuild {
            project_dir: &root,
            target_dir: &root.join("target"),
            profile: Profile::Debug,
        })
        .unwrap();
        let bytes = std::fs::read(out.join("fixture-whisker-windows.exe")).unwrap();
        assert!(bytes.starts_with(b"MZ"));
        let unicode: Vec<u8> = "日本語 App"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert!(bytes.windows(unicode.len()).any(|window| window == unicode));
        assert!(
            bytes
                .windows(b"assemblyIdentity".len())
                .any(|window| window == b"assemblyIdentity")
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
