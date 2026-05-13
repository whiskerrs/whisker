//! Extract `jni/<abi>/*.so` from every AAR under `target/lynx-android/`
//! into `target/lynx-android-unpacked/jni/<abi>/`.
//!
//! Two consumers need this layout:
//!
//! 1. `examples/<x>/build.rs` (Lyra's C++ bridge build) passes the path
//!    as a `-L` so the linker can resolve `liblynx.so` /
//!    `liblynxbase.so` at compile time.
//! 2. `cargo xtask android build-example` copies the .so files into the
//!    app's `jniLibs/<abi>/` so they end up bundled in the APK.

use anyhow::{Context, Result};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use crate::paths;

#[derive(clap::Args)]
pub struct UnpackArgs {
    /// AAR source directory (default: `target/lynx-android`).
    #[arg(long)]
    pub src: Option<PathBuf>,

    /// Output directory (default: `target/lynx-android-unpacked`).
    #[arg(long)]
    pub dest: Option<PathBuf>,
}

pub fn run(args: UnpackArgs) -> Result<()> {
    let root = paths::workspace_root()?;
    let src = args
        .src
        .unwrap_or_else(|| root.join("target/lynx-android"));
    let dest = args
        .dest
        .unwrap_or_else(|| root.join("target/lynx-android-unpacked"));
    run_with(&src, &dest)
}

/// Library-style entrypoint used by `build-example` to ensure the
/// `jni/` tree exists without going through clap.
pub fn run_with(src: &Path, dest: &Path) -> Result<()> {
    if !src.is_dir() {
        anyhow::bail!(
            "AAR source not found: {} \n\
             (run `cargo xtask android build-lynx-aar` first, or drop \
             AARs in manually — expected LynxBase.aar, LynxTrace.aar, \
             LynxAndroid.aar, ServiceAPI.aar).",
            src.display()
        );
    }

    if dest.exists() {
        fs::remove_dir_all(dest)
            .with_context(|| format!("clear stale {}", dest.display()))?;
    }
    let dest_jni = dest.join("jni");
    fs::create_dir_all(&dest_jni).context("create dest/jni")?;

    let mut found_any = false;
    for entry in fs::read_dir(src).with_context(|| format!("read {}", src.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("aar") {
            continue;
        }
        found_any = true;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("(unknown)");
        println!("==> unpacking {name}");
        extract_jni(&path, &dest_jni)
            .with_context(|| format!("unpack {}", path.display()))?;
    }
    if !found_any {
        anyhow::bail!("no AARs in {}", src.display());
    }

    println!("\n✅ unpacked to {}", dest.display());
    if let Ok(entries) = fs::read_dir(&dest_jni) {
        for entry in entries.flatten() {
            println!("  {}", entry.file_name().to_string_lossy());
        }
    }
    Ok(())
}

fn extract_jni(aar: &Path, dest_jni: &Path) -> Result<()> {
    let file = File::open(aar).context("open AAR")?;
    let mut archive = zip::ZipArchive::new(file).context("read AAR as zip")?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        // Reject anything that escapes the archive via `..`.
        let raw_path = match entry.enclosed_name() {
            Some(p) => p.to_path_buf(),
            None => continue,
        };
        let rest = match raw_path.strip_prefix("jni") {
            Ok(r) => r,
            Err(_) => continue,
        };
        if rest.as_os_str().is_empty() {
            // The `jni/` directory itself; nothing to write.
            continue;
        }
        let out = dest_jni.join(rest);
        if entry.is_dir() {
            fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut sink = File::create(&out).with_context(|| format!("write {}", out.display()))?;
        io::copy(&mut entry, &mut sink)?;
    }
    Ok(())
}
