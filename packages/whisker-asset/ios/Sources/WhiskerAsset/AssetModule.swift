// `whisker-asset` Module (iOS). View-less, startup-only.
//
// Installs the runtime asset-resolution base so `asset!("images/x.png")`
// — which lowers to `whisker_asset::resolve("images/x.png")` in Rust —
// composes a directly-loadable `file://` URL into the app bundle.
//
// ## Why native, and why here
//
// The iOS base is the app bundle's `whisker_assets/` folder reference
// (placed there by the whisker-asset build plugin, Phase 2). Its
// absolute path is `Bundle.main.bundlePath`, which is **only known at
// runtime** — so unlike Android (a fixed `file:///android_asset/…`
// constant the Rust side installs itself), iOS MUST hand the path to
// Rust from native code at launch.
//
// We pass a `file://` **URL string** (not a bare filesystem path) so
// the resolved value Kingfisher receives in `WhiskerImageView.setSrc`
// is directly loadable as a `URL`.
//
// ## Startup hook
//
// There is no explicit `OnCreate` in the `ModuleDefinition` DSL. The
// framework reads each `Module` subclass's `definitionLazy` exactly
// once at registration time (app launch, before the first render —
// see `WhiskerModuleCodegen` + `Module.definitionLazy`). Running the
// base install inside `definition()` is therefore the idiomatic
// once-at-startup hook: it fires before any `resolve` call and is
// memoised so it never re-runs.

import Foundation
import WhiskerModule

/// Rust C-ABI export, defined in `packages/whisker-asset/src/lib.rs`:
///
/// ```c
/// void whisker_asset_set_ios_base(const uint8_t *ptr, size_t len);
/// ```
///
/// Exported from the app's `cdylib` (the `#[whisker::main]` crate
/// transitively links `whisker-asset` because `asset!(…)` expands to
/// `::whisker_asset::resolve(…)`). `@_silgen_name` binds the raw C
/// symbol by name — resolved by the dynamic linker at load — without
/// needing the symbol in a C header / module map. `ptr`/`len` describe
/// a non-NUL-terminated UTF-8 byte buffer; the bytes are copied by
/// Rust, so a borrow for the duration of the call is sufficient.
@_silgen_name("whisker_asset_set_ios_base")
private func whisker_asset_set_ios_base(_ ptr: UnsafePointer<UInt8>?, _ len: Int)

public final class AssetModule: Module {

    public override func definition() -> ModuleDefinition {
        // Side-effect at registration time: install the asset base.
        // Done before returning the (empty) definition so it runs
        // exactly once, at launch, ahead of any render / resolve.
        Self.installBase()

        return ModuleDefinition {
            Name("WhiskerAsset")
        }
    }

    /// Compose `file://<bundlePath>/whisker_assets` and hand it to the
    /// Rust resolver via the C export.
    ///
    /// The `whisker_assets` segment matches the iOS namespace the
    /// build plugin bundles folder-referenced assets under
    /// (`gen/ios/whisker_assets/<rel>` → `<bundle>/whisker_assets/<rel>`),
    /// and `IosDir`'s `"{dir}/{rel}"` composition then yields
    /// `file://<bundle>/whisker_assets/<rel>`.
    static func installBase() {
        let base = "file://\(Bundle.main.bundlePath)/whisker_assets"
        // `utf8` view is contiguous for a Swift `String`; copy into an
        // Array so we have a stable pointer for the duration of the
        // call (Rust copies the bytes, so the buffer can drop after).
        let bytes = Array(base.utf8)
        bytes.withUnsafeBufferPointer { buf in
            whisker_asset_set_ios_base(buf.baseAddress, buf.count)
        }
    }
}
