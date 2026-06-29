# whisker-secure-store

Hardware-backed secure key-value store for Whisker apps. For **secrets**
— OAuth tokens, signing / DPoP private keys, API keys — that must never
sit in plaintext. For non-secret app data use
[`whisker-local-store`](../whisker-local-store) instead.

```rust
use whisker_secure_store::WhiskerSecureStore;

WhiskerSecureStore::save("session".into(), session_json)?;
let restored = WhiskerSecureStore::load("session".into())?; // Option<String>
WhiskerSecureStore::remove("session".into())?;
```

## Backends

| Platform | Storage | Key protection |
|----------|---------|----------------|
| **iOS** | Keychain (`kSecClassGenericPassword`) | Secure Enclave / device passcode; `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` (not iCloud-synced, not migrated to a new device) |
| **Android** | AES-256-GCM via **Google Tink**, ciphertext in app-private `SharedPreferences` | Tink keyset envelope-encrypted by an **Android Keystore** master key (hardware-backed where available) |

Android uses Tink rather than the now-deprecated
`androidx.security:security-crypto` (`EncryptedSharedPreferences`):
Tink — maintained by Google's security team — owns the payload crypto
(consistent across OEMs, upgradeable), while Keystore only protects the
keyset-wrapping key. `minSdk` is **23** (Keystore master key requirement).

## API

`save(key, value) -> Result<bool>`, `load(key) -> Result<Option<String>>`,
`remove(key) -> Result<()>`. Keep values small — secrets, not blobs.

Unlike `whisker-local-store`, a secure store can genuinely fail (a
Keychain `OSStatus`, a Keystore exception, a Tink decrypt failure after
a credential reset). Those surface as `Err(WhiskerModuleError)`; treat a
`load` error like a miss that requires re-authentication.
