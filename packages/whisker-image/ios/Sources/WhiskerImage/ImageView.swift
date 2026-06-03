// Lynx UI subclass hosting a UIImageView + Kingfisher-driven URL
// loading. A plain `WhiskerUI<UIImageView>` subclass — no Whisker
// annotations; registration is driven by `ImageModule`'s
// `definition()` (see `ImageModule.swift`).
//
// `@objc(WhiskerImageView)` pins the Obj-C class name to the bare
// `WhiskerImageView` so the codegen plugin's `NSClassFromString`
// lookup can find it under either the SwiftPM-target-prefixed form
// (`whisker_image.WhiskerImageView`) or the bare form.
//
// ## Corners — iOS does it natively
//
// CSS `border-radius` Just Works on iOS without any subclass-side
// help. Lynx iOS dispatches props via the Obj-C runtime (the
// `LYNX_PROP_DEFINE("border-radius", setBorderRadius, …)` macro on
// `LynxUI`), so the base class's setter resolves through the normal
// class hierarchy and reaches every custom subclass. The base then
// updates `_backgroundManager.borderRadius` and the `clipOnBorderRadius`
// flag (default `YES`) clips the view's CALayer — UIImageView's
// bitmap is rounded along with it.
//
// Android can't piggy-back on this because it uses APT-generated
// `$$PropsSetter` static dispatch tables that don't extend to
// runtime-registered custom modules. See `WhiskerImageView.kt` for
// the corresponding workaround.

import Foundation
import Kingfisher
import UIKit
import WhiskerModule

@objc(WhiskerImageView)
public final class WhiskerImageView: WhiskerUI<UIImageView> {

    private var currentSrc: String?

    @objc public override func createView() -> UIImageView {
        let v = UIImageView()
        // Default to `aspectFill` to match the Lynx `mode` default;
        // `setMode(_:)` flips it as soon as a non-default value
        // lands. `clipsToBounds` is required for `aspectFill` so the
        // overflowing edges don't paint beyond the element's frame.
        v.contentMode = .scaleAspectFill
        v.clipsToBounds = true
        return v
    }

    /// Backing of the `src` prop. Kicks off a Kingfisher fetch on
    /// the image view itself — Kingfisher tracks the in-flight
    /// request against the view, so a second `setSrc` cancels the
    /// first automatically.
    public func setSrc(_ value: String) {
        // No-op on equal — avoids re-fetching on a benign re-render
        // (e.g. a parent re-renders but the src signal didn't
        // actually change). Kingfisher would itself short-circuit
        // via its cache, but the work to ask the cache + recreate
        // the request is non-zero.
        if currentSrc == value { return }
        currentSrc = value

        let imageView: UIImageView = self.view()
        guard let url = URL(string: value) else {
            // Bad URL — clear any previous image so the element
            // doesn't keep showing a stale shot.
            imageView.kf.cancelDownloadTask()
            imageView.image = nil
            return
        }

        // Kingfisher request. Options worth turning on by default:
        //   - `.transition(.fade(0.2))` — soft fade-in avoids the
        //     flash from placeholder to image. 200ms matches iOS-
        //     system convention.
        //   - `.cacheOriginalImage` — store the decoded original
        //     alongside the resized variant so a `mode` change
        //     (which usually doesn't trigger a reload) doesn't have
        //     to redecode from disk.
        //   - `.scaleFactor(UIScreen.main.scale)` — request 2x / 3x
        //     bitmaps on Retina so the rendered image is sharp.
        let options: KingfisherOptionsInfo = [
            .transition(.fade(0.2)),
            .cacheOriginalImage,
            .scaleFactor(UIScreen.main.scale),
        ]
        imageView.kf.setImage(with: url, options: options)
    }

    /// Backing of the `mode` prop. Maps the Lynx-convention mode
    /// strings onto `UIView.ContentMode`. Unknown values fall back
    /// to `aspectFill` (the most common choice for square artwork).
    ///
    /// `clipsToBounds = true` is also what makes Lynx's
    /// CSS-driven `cornerRadius` actually clip the painted bitmap.
    /// We keep the flag enabled even for the `aspectFit` /
    /// `scaleToFill` cases where the scale itself wouldn't
    /// otherwise need clipping.
    public func setMode(_ value: String) {
        let imageView: UIImageView = self.view()
        switch value {
        case "aspectFill":
            imageView.contentMode = .scaleAspectFill
        case "aspectFit":
            imageView.contentMode = .scaleAspectFit
        case "scaleToFill":
            imageView.contentMode = .scaleToFill
        case "center":
            imageView.contentMode = .center
        default:
            imageView.contentMode = .scaleAspectFill
        }
        imageView.clipsToBounds = true
    }
}
