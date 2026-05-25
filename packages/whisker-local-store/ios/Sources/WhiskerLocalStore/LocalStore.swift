// UserDefaults-backed persistent string store. Persists across
// launches, doesn't sync across devices.
//
// Plain helper — no Whisker / Lynx types. The DSL module that
// exposes it to Rust lives in `LocalStoreModule.swift`.

import Foundation

enum LocalStore {
    /// Persist `value` under `key`. Always succeeds on iOS
    /// (UserDefaults has no failure channel); returns `true` to
    /// match the Rust wrapper's `Result<bool, _>` shape.
    static func save(_ key: String, _ value: String) -> Bool {
        UserDefaults.standard.set(value, forKey: key)
        return true
    }

    /// Read `key`; `nil` on miss (→ `Option::None` on the Rust side).
    static func load(_ key: String) -> String? {
        UserDefaults.standard.string(forKey: key)
    }

    /// Drop `key`'s entry.
    static func remove(_ key: String) {
        UserDefaults.standard.removeObject(forKey: key)
    }
}
