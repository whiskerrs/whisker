//! The on-disk store: recipients parsing, age encrypt/decrypt, and
//! the bootstrap that mints the one-and-only secret key.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use age::secrecy::ExposeSecret;
use age::x25519;
use anyhow::{Context, Result, anyhow, bail};

/// Handle to an opened `credentials/` directory. Holds the parsed
/// recipient (public key) list; never holds the secret key.
pub struct Store {
    dir: PathBuf,
    recipients: Vec<x25519::Recipient>,
}

/// The age secret key (`AGE-SECRET-KEY-1…`), parsed. Wrapped so the
/// rest of the workspace never touches `age` types directly, and so
/// the raw string can be dropped as early as possible.
pub struct Identity(x25519::Identity);

impl Identity {
    /// Parse a pasted / env-provided secret key. Trims whitespace —
    /// clipboard copies and CI secret UIs love trailing newlines.
    pub fn parse(s: &str) -> Result<Self> {
        s.trim()
            .parse::<x25519::Identity>()
            .map(Self)
            .map_err(|e| anyhow!("not a valid age secret key (AGE-SECRET-KEY-…): {e}"))
    }

    /// Read the identity from [`crate::IDENTITY_ENV`] if set.
    /// `Ok(None)` means "not set" (caller falls back to prompting);
    /// a set-but-invalid value is an error, not a silent fallback —
    /// a typo'd CI secret should fail loudly, not interactively hang.
    pub fn from_env() -> Result<Option<Self>> {
        match std::env::var(crate::IDENTITY_ENV) {
            Ok(v) if !v.trim().is_empty() => Self::parse(&v)
                .map(Some)
                .with_context(|| format!("${} is set but invalid", crate::IDENTITY_ENV)),
            _ => Ok(None),
        }
    }

    /// Does this secret key correspond to one of the store's
    /// recipients? Lets callers reject a wrong-repo key *before*
    /// attempting decryption, with a message better than age's
    /// generic "no matching keys".
    pub fn matches(&self, store: &Store) -> bool {
        let pk = self.0.to_public().to_string();
        store.recipients.iter().any(|r| r.to_string() == pk)
    }
}

impl Store {
    /// `<project_root>/credentials`.
    pub fn dir_for(project_root: &Path) -> PathBuf {
        project_root.join(crate::DIR_NAME)
    }

    /// A store exists once `recipients.txt` does. The directory
    /// alone doesn't count — half-created state from an aborted
    /// bootstrap must read as "not bootstrapped".
    pub fn exists(project_root: &Path) -> bool {
        Self::dir_for(project_root)
            .join(crate::RECIPIENTS_FILE)
            .is_file()
    }

    pub fn open(project_root: &Path) -> Result<Self> {
        let dir = Self::dir_for(project_root);
        let path = dir.join(crate::RECIPIENTS_FILE);
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let recipients =
            parse_recipients(&text).with_context(|| format!("parse {}", path.display()))?;
        if recipients.is_empty() {
            bail!(
                "{} contains no age public keys — re-run `whisker credential ios` or `android` to bootstrap",
                path.display()
            );
        }
        Ok(Self { dir, recipients })
    }

    /// Create the store: generate a fresh key pair, write the public
    /// half to `recipients.txt`, and return the secret half as a
    /// string. **This is the only moment the secret exists outside
    /// the caller's memory** — the CLI shows it once and forgets it.
    pub fn bootstrap(project_root: &Path) -> Result<(Self, String)> {
        let dir = Self::dir_for(project_root);
        let path = dir.join(crate::RECIPIENTS_FILE);
        if path.exists() {
            bail!("credentials store already exists ({})", path.display());
        }
        std::fs::create_dir_all(&dir).with_context(|| format!("mkdir {}", dir.display()))?;
        let identity = x25519::Identity::generate();
        let recipient = identity.to_public();
        let contents = format!(
            "# Whisker credential recipients — age PUBLIC keys. Safe to commit.\n\
             # The matching secret key (AGE-SECRET-KEY-…) decrypts this store;\n\
             # it lives in your password manager / CI secrets, never in the repo.\n\
             {recipient}\n"
        );
        std::fs::write(&path, contents).with_context(|| format!("write {}", path.display()))?;
        let secret = identity.to_string().expose_secret().to_string();
        Ok((
            Self {
                dir,
                recipients: vec![recipient],
            },
            secret,
        ))
    }

    /// Does `<rel>.age` exist? `rel` is the plaintext-relative path
    /// (e.g. `android/com.example.app/keystore.jks`).
    pub fn has(&self, rel: &str) -> bool {
        self.age_path(rel).is_file()
    }

    /// Encrypt `plaintext` to **all** recipients and write
    /// `<dir>/<rel>.age`. Needs no secret key — see module docs.
    pub fn put(&self, rel: &str, plaintext: &[u8]) -> Result<()> {
        let path = self.age_path(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("mkdir {}", parent.display()))?;
        }
        let encryptor = age::Encryptor::with_recipients(
            self.recipients.iter().map(|r| r as &dyn age::Recipient),
        )
        .context("build age encryptor")?;
        let mut ciphertext = Vec::new();
        let mut writer = encryptor
            .wrap_output(&mut ciphertext)
            .context("start age encryption")?;
        writer.write_all(plaintext).context("encrypt payload")?;
        writer.finish().context("finalize age ciphertext")?;
        std::fs::write(&path, ciphertext).with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }

    /// Decrypt `<rel>.age`. Callers should have checked
    /// [`Identity::matches`] first for a friendlier error, but a
    /// mismatched key still fails safely here.
    pub fn get(&self, rel: &str, identity: &Identity) -> Result<Vec<u8>> {
        let path = self.age_path(rel);
        let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let decryptor = age::Decryptor::new(&bytes[..])
            .with_context(|| format!("{} is not an age file", path.display()))?;
        let mut reader = decryptor
            .decrypt(std::iter::once(&identity.0 as &dyn age::Identity))
            .with_context(|| {
                format!(
                    "decrypt {} — wrong WHISKER_CREDENTIALS_KEY?",
                    path.display()
                )
            })?;
        let mut plaintext = Vec::new();
        reader
            .read_to_end(&mut plaintext)
            .with_context(|| format!("decrypt {}", path.display()))?;
        Ok(plaintext)
    }

    fn age_path(&self, rel: &str) -> PathBuf {
        self.dir.join(format!("{rel}.age"))
    }
}

/// Parse `recipients.txt`: one age public key per line, `#` comments
/// and blank lines ignored.
fn parse_recipients(text: &str) -> Result<Vec<x25519::Recipient>> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let recipient = line
            .parse::<x25519::Recipient>()
            .map_err(|e| anyhow!("line {}: not an age public key: {e}", i + 1))?;
        out.push(recipient);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("whisker-credentials-test-")
            .tempdir()
            .expect("tempdir")
    }

    #[test]
    fn bootstrap_then_roundtrip() {
        let root = tmp();
        assert!(!Store::exists(root.path()));
        let (store, secret) = Store::bootstrap(root.path()).expect("bootstrap");
        assert!(Store::exists(root.path()));
        assert!(secret.starts_with("AGE-SECRET-KEY-1"));

        store
            .put("android/com.example.app/keystore.json", b"{\"k\":1}")
            .unwrap();
        assert!(store.has("android/com.example.app/keystore.json"));

        // Re-open (fresh parse of recipients.txt) and decrypt.
        let reopened = Store::open(root.path()).expect("open");
        let identity = Identity::parse(&secret).expect("parse identity");
        assert!(identity.matches(&reopened));
        let plain = reopened
            .get("android/com.example.app/keystore.json", &identity)
            .expect("decrypt");
        assert_eq!(plain, b"{\"k\":1}");
    }

    #[test]
    fn put_works_without_secret_key() {
        // A clone that only has recipients.txt (no identity anywhere)
        // must still be able to add credentials.
        let root = tmp();
        let (_, secret) = Store::bootstrap(root.path()).unwrap();
        drop(secret);
        let store = Store::open(root.path()).unwrap();
        store.put("ios/default/asc.json", b"payload").unwrap();
        assert!(store.has("ios/default/asc.json"));
    }

    #[test]
    fn wrong_identity_is_detected_before_decrypt() {
        let root_a = tmp();
        let root_b = tmp();
        let (store_a, _secret_a) = Store::bootstrap(root_a.path()).unwrap();
        let (_store_b, secret_b) = Store::bootstrap(root_b.path()).unwrap();
        let wrong = Identity::parse(&secret_b).unwrap();
        assert!(!wrong.matches(&store_a));
        store_a.put("ios/default/asc.json", b"x").unwrap();
        assert!(store_a.get("ios/default/asc.json", &wrong).is_err());
    }

    #[test]
    fn bootstrap_refuses_to_overwrite() {
        let root = tmp();
        Store::bootstrap(root.path()).unwrap();
        assert!(Store::bootstrap(root.path()).is_err());
    }

    #[test]
    fn recipients_parser_skips_comments_and_blanks() {
        let (_, secret) = {
            let root = tmp();
            Store::bootstrap(root.path()).unwrap()
        };
        // Build a recipients file with noise around a real key.
        let identity = Identity::parse(&secret).unwrap();
        let pk = identity.0.to_public().to_string();
        let parsed = parse_recipients(&format!("# comment\n\n{pk}\n  \n")).expect("parse");
        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn invalid_recipient_line_errors_with_line_number() {
        let err = parse_recipients("# ok\nnot-a-key\n").unwrap_err();
        assert!(err.to_string().contains("line 2"));
    }
}
