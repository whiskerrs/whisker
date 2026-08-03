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
    private var currentHeaders: [String: String] = [:]

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
        load()
    }

    /// Backing of the `headers` prop: a JSON object of request
    /// headers. Re-fetches, because a host that answers differently
    /// per header answers differently per change.
    public func setHeaders(_ json: String) {
        let parsed = Self.parseHeaders(json)
        if parsed == currentHeaders { return }
        currentHeaders = parsed
        load()
    }

    private static func parseHeaders(_ json: String) -> [String: String] {
        guard !json.isEmpty,
              let data = json.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            return [:]
        }
        return object.compactMapValues { $0 as? String }
    }

    private func load() {
        guard let value = currentSrc else { return }

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
        var options: KingfisherOptionsInfo = [
            .transition(.fade(0.2)),
            .cacheOriginalImage,
            .scaleFactor(UIScreen.main.scale),
        ]

        // Hot-link protection is the reason: those hosts answer 403
        // unless the request carries the `Referer` their own pages
        // send.
        if !currentHeaders.isEmpty {
            let headers = currentHeaders
            options.append(.requestModifier(AnyModifier { request in
                var request = request
                for (name, value) in headers {
                    request.setValue(value, forHTTPHeaderField: name)
                }
                return request
            }))
        }

        // `whisker-asset` resolves bundled assets to a
        // `file://<bundle>/whisker_assets/<rel>` URL (the iOS base is
        // installed from native at launch — see `AssetModule.swift`).
        // Kingfisher's default `URL` source treats every URL as a
        // network resource (`URLSession`), which does technically load
        // `file://` URLs but bypasses the local-file fast path and the
        // dedicated provider. Route `file://` URLs through
        // `LocalFileImageDataProvider` explicitly so on-disk bundle
        // assets load via the documented local path; http(s) URLs
        // keep the normal network source.
        // The outcome is reported either way: a page that 403s is
        // otherwise a blank the app never hears about.
        let done: (Result<RetrieveImageResult, KingfisherError>) -> Void = { [weak self] result in
            guard let self else { return }
            switch result {
            case .success(let loaded):
                WhiskerCustomEvent.dispatch(
                    from: self,
                    name: "load",
                    params: [
                        "width": loaded.image.size.width * loaded.image.scale,
                        "height": loaded.image.size.height * loaded.image.scale,
                    ]
                )
            case .failure(let error):
                WhiskerCustomEvent.dispatch(
                    from: self,
                    name: "error",
                    params: ["error": error.localizedDescription]
                )
            }
        }

        if url.isFileURL {
            let provider = LocalFileImageDataProvider(fileURL: url)
            imageView.kf.setImage(with: .provider(provider), options: options, completionHandler: done)
        } else {
            // Two requests that differ only by header are two different
            // resources: Kingfisher keys its cache by URL alone, so
            // without a composed key a header change hands back the
            // answer to the old one.
            let resource = KF.ImageResource(downloadURL: url, cacheKey: cacheKey(for: url))
            imageView.kf.setImage(with: resource, options: options, completionHandler: done)
        }
    }

    /// URL plus the headers that shape the answer.
    private func cacheKey(for url: URL) -> String {
        guard !currentHeaders.isEmpty else { return url.absoluteString }
        let headers = currentHeaders
            .sorted { $0.key < $1.key }
            .map { "\($0.key)=\($0.value)" }
            .joined(separator: ";")
        return "\(url.absoluteString)|\(headers)"
    }

    /// Warms Kingfisher's cache. Static because prefetching belongs to
    /// no particular view — the pages after the one on screen have no
    /// element yet.
    static func prefetch(urls: [String], headers: [String: String]) {
        let targets = urls.compactMap(URL.init(string:)).filter { !$0.isFileURL }
        guard !targets.isEmpty else { return }
        var options: KingfisherOptionsInfo = [.cacheOriginalImage]
        if !headers.isEmpty {
            options.append(.requestModifier(AnyModifier { request in
                var request = request
                for (name, value) in headers {
                    request.setValue(value, forHTTPHeaderField: name)
                }
                return request
            }))
        }
        ImagePrefetcher(urls: targets, options: options).start()
    }

    /// Shared with the module's `prefetch`, which has no view to ask.
    static func headers(from json: String) -> [String: String] {
        parseHeaders(json)
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
