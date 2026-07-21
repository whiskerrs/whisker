// Resolves the app's per-directory sandbox paths from Foundation.
//
// Plain helper — no Whisker / Lynx types. The DSL module that exposes
// it to Rust lives in `PathsModule.swift`. Returns plain filesystem
// paths (not file:// URLs) so the Rust side can use them with std::fs
// directly. The directories are not created here — a returned path may
// not exist yet; the caller creates it with std::fs::create_dir_all.

import Foundation

enum Paths {
    /// The four per-app directories, keyed to match the Rust side's
    /// `cache` / `document` / `support` / `temp` lookup.
    static func directories() -> [String: String] {
        [
            "cache": search(.cachesDirectory),
            "document": search(.documentDirectory),
            "support": search(.applicationSupportDirectory),
            "temp": NSTemporaryDirectory(),
        ]
    }

    /// First path for `directory` in the user domain, or the temp dir
    /// as a last resort (the search only returns nil in exotic sandbox
    /// configurations).
    private static func search(_ directory: FileManager.SearchPathDirectory) -> String {
        NSSearchPathForDirectoriesInDomains(directory, .userDomainMask, true).first
            ?? NSTemporaryDirectory()
    }
}
