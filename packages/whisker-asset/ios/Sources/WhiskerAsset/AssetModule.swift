// `whisker-asset` Module (iOS). View-less, startup-only.
//
// Installs the runtime asset-resolution base so `asset!("images/x.png")`
// — which lowers to `whisker_asset::resolve("images/x.png")` in Rust —
// composes a directly-loadable `file://` URL into the app bundle.
//
// ## Why native, and why here
//
// The iOS base is the app bundle's `whisker_assets/` folder reference,
// placed there by the whisker-asset build plugin. Its absolute path is
// `Bundle.main.bundlePath`, which is only known at RUNTIME — so unlike
// Android, where the Rust side installs a fixed `file:///android_asset/…`
// constant itself, iOS must hand the path to Rust from native at launch.
//
// The value is a `file://` URL STRING, not a bare filesystem path, so
// what Kingfisher receives in `WhiskerImageView.setSrc` is directly
// loadable as a `URL`.
//
// ## Startup hook
//
// The DSL has no explicit `OnCreate`, but the framework reads each
// `Module` subclass's `definitionLazy` exactly once at registration time
// — app launch, before the first render. Installing the base inside
// `definition()` therefore runs ahead of any `resolve` call and is
// memoised against re-running.

import Foundation
import WhiskerModule

/// Rust C-ABI export, defined in `packages/whisker-asset/src/lib.rs`:
///
/// ```c
/// void whisker_asset_set_ios_base(const uint8_t *ptr, size_t len);
/// ```
///
/// Exported from the app's `cdylib`, since the `#[whisker::main]` crate
/// transitively links `whisker-asset` via `asset!(…)`. `@_silgen_name`
/// binds the raw C symbol by name, resolved by the dynamic linker at
/// load, without needing it in a C header / module map. `ptr`/`len`
/// describe a non-NUL-terminated UTF-8 buffer; Rust copies the bytes, so
/// a borrow for the duration of the call is enough.
@_silgen_name("whisker_asset_set_ios_base")
private func whisker_asset_set_ios_base(_ ptr: UnsafePointer<UInt8>?, _ len: Int)

@WhiskerModule
public final class AssetModule: Module {

    public override func definition() -> ModuleDefinition {
        Self.installBase()

        return ModuleDefinition {
            Name("WhiskerAsset")
        }
    }

    /// Compose `file://<bundlePath>/whisker_assets` and hand it to the Rust
    /// resolver via the C export.
    ///
    /// The `whisker_assets` segment must match the namespace the build
    /// plugin bundles folder-referenced assets under, so that `IosDir`'s
    /// `"{dir}/{rel}"` composition yields
    /// `file://<bundle>/whisker_assets/<rel>`.
    static func installBase() {
        let base = "file://\(Bundle.main.bundlePath)/whisker_assets"
        // Copied into an Array for a stable pointer across the call.
        let bytes = Array(base.utf8)
        bytes.withUnsafeBufferPointer { buf in
            whisker_asset_set_ios_base(buf.baseAddress, buf.count)
        }
    }
}
