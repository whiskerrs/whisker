//! Plaintext staging for the duration of one build.
//!
//! xcodebuild and gradle take *file paths* (an `AuthKey_….p8`, a
//! `.jks`), so fully in-memory secrets aren't possible. The
//! compromise: decrypted bytes land in a fresh `0700` temp
//! directory whose lifetime is tied to this guard — dropped (or
//! unwound on panic) → directory and contents are deleted. Build
//! code must never copy these files anywhere else, in particular
//! not into `gen/` or the credentials dir.

use anyhow::{Context, Result};
use std::path::PathBuf;

/// Guard owning the staging directory. Keep it alive for the whole
/// build; everything under it disappears on drop.
pub struct MaterializedDir {
    dir: tempfile::TempDir,
}

impl MaterializedDir {
    pub fn new() -> Result<Self> {
        let dir = tempfile::Builder::new()
            .prefix("whisker-credentials-")
            .tempdir()
            .context("create credential staging dir")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
                .context("chmod 700 credential staging dir")?;
        }
        Ok(Self { dir })
    }

    /// The staging directory itself — for tools that need to *create*
    /// a file in the protected area (e.g. `keytool -genkeypair`
    /// writing a fresh keystore) rather than receive bytes.
    pub fn dir_path(&self) -> &std::path::Path {
        self.dir.path()
    }

    /// Write `bytes` as `<staging>/<name>` with owner-only
    /// permissions. Returns the absolute path to hand to
    /// xcodebuild / gradle.
    pub fn write(&self, name: &str, bytes: &[u8]) -> Result<PathBuf> {
        let path = self.dir.path().join(name);
        std::fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .with_context(|| format!("chmod 600 {}", path.display()))?;
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_owner_only_and_deleted_on_drop() {
        let staged;
        {
            let dir = MaterializedDir::new().unwrap();
            staged = dir.write("secret.p8", b"key material").unwrap();
            assert!(staged.is_file());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&staged).unwrap().permissions().mode();
                assert_eq!(mode & 0o777, 0o600);
                let dir_mode = std::fs::metadata(staged.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode();
                assert_eq!(dir_mode & 0o777, 0o700);
            }
        }
        assert!(!staged.exists(), "staging dir must vanish on drop");
    }
}
