// Whisker module view hosting a UIImageView + Kingfisher-driven URL
// loading. Registration is driven by `ImageModule`'s `definition()`, not
// by annotations on this class.
//
// `@objc(WhiskerImageView)` pins the Obj-C class name to the bare
// `WhiskerImageView` so the codegen plugin's `NSClassFromString` lookup
// finds it under either the SwiftPM-target-prefixed form
// (`whisker_image.WhiskerImageView`) or the bare form.
//
// CSS presentation, including border radius and clipping, is owned by the
// common Host wrapper around this module view.

import Foundation
import Kingfisher
import UIKit
import WhiskerModule

@objc(WhiskerImageView)
public final class WhiskerImageView: WhiskerUI<UIImageView> {

    private var currentSrc: String?
    private var currentHeaders: [String: String] = [:]
    private var loadScheduled = false

    @objc public override func createView() -> UIImageView {
        let v = UIImageView()
        // `aspectFill` is the module default, and needs
        // `clipsToBounds` so the overflowing edges don't paint beyond the
        // element's frame.
        v.contentMode = .scaleAspectFill
        v.clipsToBounds = true
        return v
    }

    /// Backing of the `src` prop. Kingfisher tracks the in-flight
    /// request against the image view, so a later load cancels the
    /// earlier one automatically.
    public func setSrc(_ value: String) {
        // Kingfisher would short-circuit an unchanged src through its
        // cache anyway, but asking the cache and rebuilding the request
        // still costs something on every benign re-render.
        if currentSrc == value { return }
        currentSrc = value
        scheduleLoad()
    }

    /// Backing of the `headers` prop: a JSON object of request
    /// headers. Re-fetches, because a host that answers differently
    /// per header answers differently per change.
    public func setHeaders(_ json: String) {
        let parsed = Self.parseHeaders(json)
        if parsed == currentHeaders { return }
        currentHeaders = parsed
        scheduleLoad()
    }

    /// Props of one update arrive as consecutive setter calls in one
    /// main-thread turn; loading from each setter would start a headerless
    /// download when `src` lands first, which the headered retry then joins.
    private func scheduleLoad() {
        if loadScheduled { return }
        loadScheduled = true
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.loadScheduled = false
            self.load()
        }
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
            // Clear the previous image so a bad URL doesn't leave a stale
            // shot on screen.
            imageView.kf.cancelDownloadTask()
            imageView.image = nil
            return
        }

        // `.cacheOriginalImage` keeps the decoded original alongside the
        // resized variant, so a `mode` change — which doesn't reload —
        // doesn't redecode from disk. `.scaleFactor` is what gets 2x / 3x
        // bitmaps on Retina.
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

        // `whisker-asset` resolves bundled assets to `file://` URLs.
        // Kingfisher's default `URL` source would load them through
        // `URLSession`, bypassing the local-file fast path.
        if url.isFileURL {
            let provider = LocalFileImageDataProvider(fileURL: url)
            imageView.kf.setImage(with: .provider(provider), options: options, completionHandler: done)
        } else {
            // Two requests that differ only by header are two different
            // resources: Kingfisher keys its cache by URL alone, so
            // without a composed key a header change hands back the
            // answer to the old one.
            let resource = KF.ImageResource(
                downloadURL: url,
                cacheKey: Self.cacheKey(for: url, headers: currentHeaders)
            )
            imageView.kf.setImage(with: resource, options: options, completionHandler: done)
        }
    }

    /// URL plus the headers that shape the answer. Shared with `prefetch`
    /// so a prefetched image is found by the view that later shows it.
    private static func cacheKey(for url: URL, headers: [String: String]) -> String {
        guard !headers.isEmpty else { return url.absoluteString }
        let joined = headers
            .sorted { $0.key < $1.key }
            .map { "\($0.key)=\($0.value)" }
            .joined(separator: ";")
        return "\(url.absoluteString)|\(joined)"
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
        let resources = targets.map {
            KF.ImageResource(downloadURL: $0, cacheKey: cacheKey(for: $0, headers: headers))
        }
        ImagePrefetcher(resources: resources, options: options).start()
    }

    /// Shared with the module's `prefetch`, which has no view to ask.
    static func headers(from json: String) -> [String: String] {
        parseHeaders(json)
    }

    /// Backing of the `mode` prop, mapping its stable wire
    /// strings onto `UIView.ContentMode`.
    ///
    /// `clipsToBounds` stays on for every mode, including the `aspectFit` /
    /// `scaleToFill` cases that don't overflow: it is also what makes
    /// the common presentation wrapper clip the painted bitmap.
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
