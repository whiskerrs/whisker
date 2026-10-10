//! Local image files named by `store.rs`.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// Resolve `file` (relative to the app's `Cargo.toml`) and check it
/// is an image a store will take, returning its path and MIME type.
pub(crate) fn image(root: &Path, file: &str, what: &str) -> Result<(PathBuf, &'static str)> {
    let path = root.join(file);
    let mime = match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        _ => bail!("store.rs: {what} `{file}` must be a .png, .jpg or .jpeg file"),
    };
    if !path.is_file() {
        bail!(
            "store.rs: {what} `{file}` does not exist (looked for {})",
            path.display()
        );
    }
    Ok((path, mime))
}

pub(crate) fn sha256_hex(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(ring::digest::digest(&ring::digest::SHA256, &bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
