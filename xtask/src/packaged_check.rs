//! Compiles the crates.io tarballs, not the workspace, for a mobile target.
//!
//! `cargo publish` verifies each package on the Linux publish runner, where
//! iOS- and Android-only code is compiled out, so a source file the tarball
//! is missing only fails once an app builds it for a phone.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};

const ANDROID_ABI: &str = "arm64-v8a";

struct Package {
    name: String,
    version: String,
    mobile: bool,
}

pub fn run(root: &Path, host: &str) -> Result<()> {
    let target = match host {
        "android" => "aarch64-linux-android",
        "ios" => "aarch64-apple-ios-sim",
        _ => bail!("unknown mobile Host {host:?}; expected android or ios"),
    };
    let work = root.join("target/xtask/packaged");
    if work.exists() {
        fs::remove_dir_all(&work).with_context(|| format!("remove stale {}", work.display()))?;
    }
    let packages = published_packages(root)?;
    let sources = unpack(root, &work, &packages)?;
    let consumer = work.join("consumer");
    fs::create_dir_all(consumer.join("src"))?;
    fs::write(consumer.join("src/lib.rs"), "")?;
    fs::write(
        consumer.join("Cargo.toml"),
        consumer_manifest(&packages, &sources),
    )?;

    let mut check = Command::new(super::cargo());
    // A fresh target dir: tarballs carry fixed mtimes, so a reused one
    // would treat the unpacked sources as already checked.
    check
        .current_dir(&consumer)
        .args(["check", "--target", target, "--target-dir"])
        .arg(work.join("target"));
    if host == "android" {
        let toolchain = whisker_build::android::resolve_toolchain(ANDROID_ABI, 24)?;
        let triple = target.replace('-', "_");
        check
            .env(format!("CC_{triple}"), &toolchain.clang)
            .env(format!("CXX_{triple}"), &toolchain.clang_cpp)
            .env(format!("AR_{triple}"), &toolchain.ar)
            .env("ANDROID_NDK_HOME", &toolchain.ndk);
    }
    super::run(&mut check).with_context(|| format!("check the packaged crates for {target}"))?;
    let lock = fs::read_to_string(consumer.join("Cargo.lock"))?;
    let registry = from_registry(&lock, &packages)?;
    ensure!(
        registry.is_empty(),
        "resolved from crates.io instead of the tarballs (a patch went unused): {}",
        registry.join(", ")
    );
    Ok(())
}

/// Published workspace crates the lockfile took from a registry, which
/// happens when a tarball's version doesn't satisfy a dependent's requirement.
fn from_registry(lock: &str, packages: &[Package]) -> Result<Vec<String>> {
    let lock: toml_edit::DocumentMut = lock.parse().context("parse consumer Cargo.lock")?;
    let mut found = Vec::new();
    for entry in lock["package"]
        .as_array_of_tables()
        .context("Cargo.lock packages")?
    {
        let name = entry["name"].as_str().context("locked package name")?;
        if entry.contains_key("source") && packages.iter().any(|each| each.name == name) {
            found.push(format!(
                "{name} {}",
                entry["version"].as_str().unwrap_or("?")
            ));
        }
    }
    Ok(found)
}

fn published_packages(root: &Path) -> Result<Vec<Package>> {
    let metadata = super::capture(Command::new(super::cargo()).current_dir(root).args([
        "metadata",
        "--no-deps",
        "--format-version",
        "1",
    ]))?;
    let metadata: serde_json::Value = serde_json::from_str(&metadata)?;
    let packages_dir = root.join("packages");
    let mut packages = Vec::new();
    for package in metadata["packages"].as_array().context("packages")? {
        if package["publish"] == serde_json::json!([]) {
            continue;
        }
        let name = package["name"].as_str().context("package name")?.to_owned();
        let manifest = Path::new(package["manifest_path"].as_str().context("manifest path")?);
        packages.push(Package {
            mobile: is_mobile(&name, manifest.starts_with(&packages_dir)),
            version: package["version"].as_str().context("version")?.to_owned(),
            name,
        });
    }
    Ok(packages)
}

/// The crates a mobile app links: the framework, the mobile Driver, and every
/// module except its Rust Host halves.
fn is_mobile(name: &str, is_module: bool) -> bool {
    matches!(name, "whisker" | "whisker-driver")
        || (is_module
            && !["-web", "-linux", "-desktop"]
                .iter()
                .any(|suffix| name.ends_with(suffix)))
}

fn unpack(root: &Path, work: &Path, packages: &[Package]) -> Result<PathBuf> {
    let target_dir = work.join("target");
    let mut package = Command::new(super::cargo());
    package
        .current_dir(root)
        .args(["package", "--no-verify", "--allow-dirty", "--target-dir"])
        .arg(&target_dir);
    for each in packages {
        package.args(["-p", &each.name]);
    }
    super::run(&mut package).context("package the published crates")?;

    let sources = work.join("crates");
    fs::create_dir_all(&sources)?;
    for each in packages {
        let tarball = target_dir
            .join("package")
            .join(format!("{}-{}.crate", each.name, each.version));
        super::run(
            Command::new("tar")
                .arg("xzf")
                .arg(&tarball)
                .arg("-C")
                .arg(&sources),
        )
        .with_context(|| format!("unpack {}", tarball.display()))?;
    }
    Ok(sources)
}

fn consumer_manifest(packages: &[Package], sources: &Path) -> String {
    let mut manifest = String::from(
        "[package]\nname = \"packaged-consumer\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[workspace]\n\n[dependencies]\n",
    );
    for each in packages.iter().filter(|each| each.mobile) {
        manifest.push_str(&format!("{} = \"={}\"\n", each.name, each.version));
    }
    manifest.push_str("\n[patch.crates-io]\n");
    for each in packages {
        let path = sources.join(format!("{}-{}", each.name, each.version));
        manifest.push_str(&format!("{} = {{ path = {:?} }}\n", each.name, path));
    }
    manifest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobile_set_is_the_framework_driver_and_mobile_modules() {
        assert!(is_mobile("whisker", false));
        assert!(is_mobile("whisker-driver", false));
        assert!(is_mobile("whisker-input", true));
        assert!(!is_mobile("whisker-input-web", true));
        assert!(!is_mobile("whisker-image-linux", true));
        assert!(!is_mobile("whisker-local-store-desktop", true));
        assert!(!is_mobile("whisker-cli", false));
        assert!(!is_mobile("whisker-web", false));
    }

    #[test]
    fn registry_copies_of_workspace_crates_are_reported() {
        let packages = [Package {
            name: "whisker".into(),
            version: "1.2.3".into(),
            mobile: true,
        }];
        let lock = r#"
            [[package]]
            name = "whisker"
            version = "1.2.3"

            [[package]]
            name = "whisker"
            version = "1.2.0"
            source = "registry+https://github.com/rust-lang/crates.io-index"

            [[package]]
            name = "serde"
            version = "1.0.0"
            source = "registry+https://github.com/rust-lang/crates.io-index"
        "#;
        assert_eq!(from_registry(lock, &packages).unwrap(), ["whisker 1.2.0"]);
    }

    #[test]
    fn consumer_depends_on_mobile_crates_and_patches_every_package() {
        let packages = [
            Package {
                name: "whisker".into(),
                version: "1.2.3".into(),
                mobile: true,
            },
            Package {
                name: "whisker-cli".into(),
                version: "1.2.3".into(),
                mobile: false,
            },
        ];
        let manifest = consumer_manifest(&packages, Path::new("/w"));
        assert!(manifest.contains("[dependencies]\nwhisker = \"=1.2.3\"\n\n"));
        assert!(manifest.contains("whisker-cli = { path = \"/w/whisker-cli-1.2.3\" }"));
    }
}
