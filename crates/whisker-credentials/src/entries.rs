//! Typed entry payloads + the store layout paths, plus the
//! build-facing resolvers that turn "give me signing inputs for this
//! identifier" into materialized files.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::materialize::MaterializedDir;
use crate::store::{Identity, Store};

/// App Store Connect **Team** API key (Admin role). One entry powers
/// everything iOS: automatic signing, cloud-managed distribution,
/// bundle-id auto-registration, and (later) build upload.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AscKey {
    /// The `AuthKey_<key_id>.p8` contents (PKCS#8 PEM).
    pub p8_pem: String,
    pub key_id: String,
    pub issuer_id: String,
    /// Resolved at wizard time from the key itself (bundleIds
    /// `seedId`) so builds never have to ask the user for it.
    pub team_id: String,
}

/// Android upload-keystore metadata; the keystore bytes themselves
/// live in a sibling binary entry ([`android_keystore_rel`]).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct KeystoreMeta {
    pub store_password: String,
    pub key_password: String,
    pub key_alias: String,
}

/// `ios/<bundle_id or "default">/asc.json`
pub fn ios_asc_rel(bundle_id: Option<&str>) -> String {
    format!("ios/{}/asc.json", bundle_id.unwrap_or("default"))
}

/// `android/<application_id>/keystore.jks`
pub fn android_keystore_rel(application_id: &str) -> String {
    format!("android/{application_id}/keystore.jks")
}

/// `android/<application_id>/keystore.json`
pub fn android_meta_rel(application_id: &str) -> String {
    format!("android/{application_id}/keystore.json")
}

/// Decrypted, on-disk iOS signing inputs for one build. Paths live
/// inside a [`MaterializedDir`] owned by the caller — they vanish
/// when the build's guard drops.
pub struct IosSigning {
    /// `AuthKey_<key_id>.p8` — the filename shape Apple's tools
    /// conventionally search for.
    pub key_path: PathBuf,
    pub key_id: String,
    pub issuer_id: String,
    pub team_id: String,
}

/// Decrypted, on-disk Android signing inputs for one build.
pub struct AndroidSigning {
    pub keystore_path: PathBuf,
    pub store_password: String,
    pub key_password: String,
    pub key_alias: String,
}

impl Store {
    /// Which entry would an iOS build for `bundle_id` use, if any?
    /// Exact per-bundle override wins; otherwise the team-wide
    /// `default` entry.
    pub fn resolve_ios_rel(&self, bundle_id: &str) -> Option<String> {
        let exact = ios_asc_rel(Some(bundle_id));
        if self.has(&exact) {
            return Some(exact);
        }
        let default = ios_asc_rel(None);
        self.has(&default).then_some(default)
    }

    /// Decrypt + stage the iOS signing inputs for `bundle_id`.
    /// `Ok(None)` = no entry (caller runs the wizard or errors with
    /// the exact `whisker credential ios` invocation to run).
    pub fn ios_signing(
        &self,
        bundle_id: &str,
        identity: &Identity,
        dir: &MaterializedDir,
    ) -> Result<Option<IosSigning>> {
        let Some(rel) = self.resolve_ios_rel(bundle_id) else {
            return Ok(None);
        };
        let json = self.get(&rel, identity)?;
        let key: AscKey = serde_json::from_slice(&json)
            .with_context(|| format!("parse decrypted {rel} as AscKey JSON"))?;
        let key_path = dir.write(&format!("AuthKey_{}.p8", key.key_id), key.p8_pem.as_bytes())?;
        Ok(Some(IosSigning {
            key_path,
            key_id: key.key_id,
            issuer_id: key.issuer_id,
            team_id: key.team_id,
        }))
    }

    /// Decrypt + stage the Android signing inputs for
    /// `application_id`. Exact-match only — upload keystores are
    /// per-app assets (see crate docs).
    pub fn android_signing(
        &self,
        application_id: &str,
        identity: &Identity,
        dir: &MaterializedDir,
    ) -> Result<Option<AndroidSigning>> {
        let jks_rel = android_keystore_rel(application_id);
        let meta_rel = android_meta_rel(application_id);
        if !self.has(&jks_rel) || !self.has(&meta_rel) {
            return Ok(None);
        }
        let meta: KeystoreMeta = serde_json::from_slice(&self.get(&meta_rel, identity)?)
            .with_context(|| format!("parse decrypted {meta_rel} as KeystoreMeta JSON"))?;
        let jks = self.get(&jks_rel, identity)?;
        let keystore_path = dir.write(&format!("{application_id}.jks"), &jks)?;
        Ok(Some(AndroidSigning {
            keystore_path,
            store_password: meta.store_password,
            key_password: meta.key_password,
            key_alias: meta.key_alias,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store, Identity) {
        let root = tempfile::Builder::new()
            .prefix("whisker-credentials-test-")
            .tempdir()
            .unwrap();
        let (store, secret) = Store::bootstrap(root.path()).unwrap();
        let identity = Identity::parse(&secret).unwrap();
        (root, store, identity)
    }

    #[test]
    fn ios_resolution_falls_back_to_default() {
        let (_root, store, _id) = store();
        store.put(&ios_asc_rel(None), b"{}").unwrap();
        assert_eq!(
            store.resolve_ios_rel("com.example.app"),
            Some("ios/default/asc.json".to_string())
        );
        store
            .put(&ios_asc_rel(Some("com.example.app")), b"{}")
            .unwrap();
        assert_eq!(
            store.resolve_ios_rel("com.example.app"),
            Some("ios/com.example.app/asc.json".to_string())
        );
    }

    #[test]
    fn ios_signing_materializes_p8_with_apple_filename() {
        let (_root, store, identity) = store();
        let key = AscKey {
            p8_pem: "-----BEGIN PRIVATE KEY-----\nMAo=\n-----END PRIVATE KEY-----\n".into(),
            key_id: "ABC123XYZ".into(),
            issuer_id: "57246542-96fe-1a63-e053-0824d011072a".into(),
            team_id: "ABCDE12345".into(),
        };
        store
            .put(
                &ios_asc_rel(None),
                serde_json::to_vec(&key).unwrap().as_slice(),
            )
            .unwrap();

        let dir = MaterializedDir::new().unwrap();
        let signing = store
            .ios_signing("com.example.app", &identity, &dir)
            .unwrap()
            .expect("entry present");
        assert!(signing.key_path.ends_with("AuthKey_ABC123XYZ.p8"));
        assert_eq!(
            std::fs::read_to_string(&signing.key_path).unwrap(),
            key.p8_pem
        );
        assert_eq!(signing.team_id, "ABCDE12345");
    }

    #[test]
    fn android_signing_requires_both_entries() {
        let (_root, store, identity) = store();
        let dir = MaterializedDir::new().unwrap();
        store
            .put(&android_keystore_rel("com.example.app"), b"jksbytes")
            .unwrap();
        // meta missing → not resolvable yet
        assert!(
            store
                .android_signing("com.example.app", &identity, &dir)
                .unwrap()
                .is_none()
        );
        let meta = KeystoreMeta {
            store_password: "sp".into(),
            key_password: "kp".into(),
            key_alias: "upload".into(),
        };
        store
            .put(
                &android_meta_rel("com.example.app"),
                serde_json::to_vec(&meta).unwrap().as_slice(),
            )
            .unwrap();
        let signing = store
            .android_signing("com.example.app", &identity, &dir)
            .unwrap()
            .expect("both entries present");
        assert_eq!(std::fs::read(&signing.keystore_path).unwrap(), b"jksbytes");
        assert_eq!(signing.key_alias, "upload");
        // exact-match only: a different application id resolves to none
        assert!(
            store
                .android_signing("com.example.other", &identity, &dir)
                .unwrap()
                .is_none()
        );
    }
}
