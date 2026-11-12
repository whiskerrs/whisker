//! Recipient consent is stored outside the repository that supplies the keys.

use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use whisker_credentials::Store;

const EXPECTED: &str = "WHISKER_CREDENTIALS_RECIPIENTS";

fn normalize(keys: impl Iterator<Item = String>) -> Vec<String> {
    let mut keys: Vec<_> = keys.filter(|key| !key.is_empty()).collect();
    keys.sort();
    keys.dedup();
    keys
}

fn trust_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .context("user home is required for credential recipient trust")?;
    Ok(PathBuf::from(home).join(".whisker/credential-recipients.json"))
}

fn verify_with(
    path: &Path,
    project: &str,
    keys: &[String],
    confirm: impl FnOnce(&[String], &[String]) -> Result<bool>,
) -> Result<()> {
    let mut records: BTreeMap<String, Vec<String>> = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("parse recipient trust record")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(error) => return Err(error).context("read recipient trust record"),
    };
    let previous = records.get(project).cloned().unwrap_or_default();
    if previous == keys {
        return Ok(());
    }
    ensure!(
        confirm(&previous, keys)?,
        "credential recipients were not accepted; nothing was saved"
    );
    records.insert(project.to_owned(), keys.to_vec());
    let parent = path.parent().context("recipient trust parent directory")?;
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(parent)
        .context("create recipient trust directory")?;
    let temporary = parent.join(format!(
        "recipients-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let result = (|| -> Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&serde_json::to_vec_pretty(&records)?)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.context("save recipient trust record")
}

pub(super) fn verify(project_root: &Path, store: &Store) -> Result<()> {
    let keys = store.recipient_keys();
    if let Ok(expected) = std::env::var(EXPECTED) {
        let expected = normalize(
            expected
                .split(|c: char| c == ',' || c.is_whitespace())
                .map(str::to_owned),
        );
        ensure!(
            !expected.is_empty() && expected == keys,
            "credential recipients do not match ${EXPECTED}; nothing was saved"
        );
        return Ok(());
    }
    if !super::prompt::is_interactive() {
        bail!(
            "set ${EXPECTED} to the independently verified recipient keys before saving credentials non-interactively"
        );
    }
    let project = project_root
        .canonicalize()
        .context("resolve credential project root")?;
    let project = project
        .to_str()
        .context("credential project path must be UTF-8")?;
    verify_with(&trust_path()?, project, &keys, |previous, current| {
        println!(
            "Credential encryption recipients need review. Every listed key can decrypt new credentials."
        );
        for key in current.iter().filter(|key| !previous.contains(key)) {
            println!("  Added: {key}");
        }
        for key in previous.iter().filter(|key| !current.contains(key)) {
            println!("  Removed: {key}");
        }
        super::prompt::confirm("Trust this exact recipient set?")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipient_changes_require_consent_and_rejection_preserves_trust() {
        let root =
            std::env::temp_dir().join(format!("whisker-recipient-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("trust.json");
        let first = vec!["first".to_owned()];
        assert!(verify_with(&path, "project", &first, |_, _| Ok(false)).is_err());
        assert!(!path.exists());
        verify_with(&path, "project", &first, |old, new| {
            assert!(old.is_empty());
            assert_eq!(new, first);
            Ok(true)
        })
        .unwrap();
        verify_with(&path, "project", &first, |_, _| {
            panic!("unchanged keys prompted")
        })
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let second = vec!["attacker".to_owned(), "first".to_owned()];
        assert!(
            verify_with(&path, "project", &second, |old, new| {
                assert_eq!(old, first);
                assert_eq!(new, second);
                Ok(false)
            })
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(verify_with(&path, "another-project", &first, |_, _| Ok(false)).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
