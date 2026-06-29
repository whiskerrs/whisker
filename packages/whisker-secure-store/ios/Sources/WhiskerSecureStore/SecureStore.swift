// Keychain-backed secure string store. Persists across launches,
// hardware-protected, not iCloud-synced, not migrated to a new device
// or unencrypted backup.
//
// Plain helper — no Whisker / Lynx types. The DSL module that exposes
// it to Rust lives in `SecureStoreModule.swift`.

import Foundation
import Security

/// Carries a failure message out of `SecureStore`. `Result`'s `Failure`
/// must conform to `Error`, and `String` doesn't — so we wrap it.
struct SecureStoreError: Error {
    let message: String
}

enum SecureStore {
    /// Namespaces our items in the shared Keychain via `kSecAttrService`,
    /// so keys can't collide with another component's generic passwords.
    private static let service = "WhiskerSecureStore"

    /// `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`: the value is
    /// readable once the device has been unlocked at least once since
    /// boot (so background refreshes work), never leaves this device
    /// (no iCloud Keychain sync), and is excluded from encrypted
    /// backups' device migration. A sensible default for auth tokens.
    private static let accessible = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly

    private static func fail(_ message: String) -> SecureStoreError {
        SecureStoreError(message: message)
    }

    /// Encrypt + persist `value` under `key` (upsert). `.success(true)`
    /// on success; `.failure` carries the `OSStatus`.
    static func save(_ key: String, _ value: String) -> Result<Bool, SecureStoreError> {
        guard let data = value.data(using: .utf8) else {
            return .failure(fail("WhiskerSecureStore.save: value is not valid UTF-8"))
        }
        let match: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
        ]
        // Try to update an existing item first; if there's none, add.
        let updateStatus = SecItemUpdate(
            match as CFDictionary,
            [kSecValueData as String: data] as CFDictionary
        )
        if updateStatus == errSecSuccess {
            return .success(true)
        }
        if updateStatus == errSecItemNotFound {
            var add = match
            add[kSecValueData as String] = data
            add[kSecAttrAccessible as String] = accessible
            let addStatus = SecItemAdd(add as CFDictionary, nil)
            if addStatus == errSecSuccess {
                return .success(true)
            }
            return .failure(fail("WhiskerSecureStore.save: SecItemAdd failed (OSStatus \(addStatus))"))
        }
        return .failure(fail("WhiskerSecureStore.save: SecItemUpdate failed (OSStatus \(updateStatus))"))
    }

    /// Read + decrypt `key`. `.success(nil)` on miss (→ `Option::None`
    /// on the Rust side); `.failure` on a real Keychain error.
    static func load(_ key: String) -> Result<String?, SecureStoreError> {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        if status == errSecItemNotFound {
            return .success(nil)
        }
        if status != errSecSuccess {
            return .failure(fail("WhiskerSecureStore.load: SecItemCopyMatching failed (OSStatus \(status))"))
        }
        guard let data = item as? Data, let value = String(data: data, encoding: .utf8) else {
            return .failure(fail("WhiskerSecureStore.load: stored value is not valid UTF-8"))
        }
        return .success(value)
    }

    /// Drop `key`'s entry. A missing item is success (idempotent).
    static func remove(_ key: String) -> Result<Void, SecureStoreError> {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
        ]
        let status = SecItemDelete(query as CFDictionary)
        if status == errSecSuccess || status == errSecItemNotFound {
            return .success(())
        }
        return .failure(fail("WhiskerSecureStore.remove: SecItemDelete failed (OSStatus \(status))"))
    }
}
