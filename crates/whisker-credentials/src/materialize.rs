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
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static STAGING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Remove live plaintext staging directories before a process exits without unwinding
///
/// Call after stopping build children. Only directories registered by this process
/// are removed; this does not scan shared temporary storage or clean other processes.
pub fn cleanup_staging() -> Result<()> {
    let mut staging = STAGING.lock().unwrap_or_else(|error| error.into_inner());
    let mut failure = None;
    staging.retain(|path| match std::fs::remove_dir_all(path) {
        Ok(()) => false,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            failure = Some(error);
            true
        }
    });
    match failure {
        Some(error) => Err(error).context("remove plaintext credential staging"),
        None => Ok(()),
    }
}

/// Guard owning the staging directory. Keep it alive for the whole
/// build; everything under it disappears on drop.
pub struct MaterializedDir {
    dir: tempfile::TempDir,
}

impl MaterializedDir {
    pub fn new() -> Result<Self> {
        let mut staging = STAGING.lock().unwrap_or_else(|error| error.into_inner());
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
        staging.push(dir.path().to_path_buf());
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
        let _staging = STAGING.lock().unwrap_or_else(|error| error.into_inner());
        anyhow::ensure!(
            std::path::Path::new(name).components().count() == 1
                && matches!(
                    std::path::Path::new(name).components().next(),
                    Some(std::path::Component::Normal(_))
                ),
            "credential staging names must be single file names"
        );
        let path = self.dir.path().join(name);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&path)
            .context("create credential staging file")?;
        file.write_all(bytes)
            .context("write credential staging file")?;
        Ok(path)
    }
}

impl Drop for MaterializedDir {
    fn drop(&mut self) {
        let mut staging = STAGING.lock().unwrap_or_else(|error| error.into_inner());
        staging.retain(|path| path != self.dir.path());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_cleanup_removes_live_staging_without_affecting_other_temp_dirs() {
        let unrelated = tempfile::tempdir().unwrap();
        let unrelated_file = unrelated.path().join("keep");
        std::fs::write(&unrelated_file, b"keep").unwrap();
        let staged = MaterializedDir::new().unwrap();
        let path = staged.write("secret.p8", b"secret").unwrap();
        cleanup_staging().unwrap();
        assert!(!path.exists());
        assert!(!staged.dir_path().exists());
        assert!(unrelated_file.exists());
        assert!(staged.write("secret.p8", b"secret").is_err());
    }

    #[test]
    fn staging_never_overwrites_existing_paths() {
        let dir = MaterializedDir::new().unwrap();
        let path = dir.write("secret.p8", b"original").unwrap();
        assert!(dir.write("secret.p8", b"replacement").is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"original");
    }

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
