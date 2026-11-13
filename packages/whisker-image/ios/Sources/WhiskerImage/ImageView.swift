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
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif
import WhiskerModule

#if canImport(UIKit)
public typealias WhiskerImageContentView = UIImageView
#else
/// `NSImageView` has no aspect-fill scaling, so the image is shown through
/// the layer's contents gravity instead.
public final class WhiskerImageContentView: NSImageView {
    enum ContentMode { case scaleAspectFill, scaleAspectFit, scaleToFill, center }

    var contentMode = ContentMode.scaleAspectFill {
        didSet { updateContents() }
    }

    public override var image: NSImage? {
        didSet { updateContents() }
    }

    public override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
    }

    public required init?(coder: NSCoder) { nil }

    public override var wantsUpdateLayer: Bool { true }

    public override func updateLayer() {
        updateContents()
    }

    private func updateContents() {
        guard let layer else { return }
        layer.contents = image?.cgImage(forProposedRect: nil, context: nil, hints: nil)
        layer.contentsGravity = switch contentMode {
        case .scaleAspectFill: .resizeAspectFill
        case .scaleAspectFit: .resizeAspect
        case .scaleToFill: .resize
        case .center: .center
        }
    }
}
#endif

@objc(WhiskerImageView)
public final class WhiskerImageView: WhiskerUI<WhiskerImageContentView> {

    private var currentSrc: String?
    private var currentHeaders: [String: String] = [:]
    private var loadScheduled = false
    #if canImport(UIKit)
    private var request: WhiskerImageRequest?
    private var task: WhiskerImageTask?
    private var shown: WhiskerLoadedImage?
    private var decodesAtViewSize = true
    private var awaitedLayout = false

    public override func layoutSubviews() {
        super.layoutSubviews()
        guard let request, request.targetPixelSize != .zero else { return }
        let needed = targetPixelSize()
        // A view that grew past what was decoded reloads only while the source has more pixels to give.
        let outgrown = needed.width > request.targetPixelSize.width + 1 ||
            needed.height > request.targetPixelSize.height + 1
        let exhausted = shown.map {
            $0.pixelSize.width >= $0.naturalSize.width && $0.pixelSize.height >= $0.naturalSize.height
        } ?? false
        if outgrown, !exhausted { scheduleLoad() }
    }

    private func targetPixelSize() -> CGSize {
        guard decodesAtViewSize else { return .zero }
        let scale = Self.screenScale
        return CGSize(
            width: (bounds.width * scale).rounded(.up),
            height: (bounds.height * scale).rounded(.up)
        )
    }
    #endif

    @objc public override func createView() -> WhiskerImageContentView {
        let v = WhiskerImageContentView()
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

    #if canImport(UIKit)
    private func load() {
        guard let value = currentSrc else { return }
        let imageView: WhiskerImageContentView = self.view()
        guard let url = value.hasPrefix("/") ? URL(fileURLWithPath: value) : URL(string: value) else {
            task?.cancel()
            task = nil
            request = nil
            shown = nil
            imageView.image = nil
            return
        }
        let target = targetPixelSize()
        if decodesAtViewSize, target.width <= 0 || target.height <= 0, !awaitedLayout {
            // Props arrive before the first layout; one more turn lets the frame land.
            awaitedLayout = true
            scheduleLoad()
            return
        }
        let next = WhiskerImageRequest(
            url: url,
            cacheKey: WhiskerImagePipeline.cacheKey(for: url, headers: currentHeaders),
            headers: currentHeaders,
            targetPixelSize: target.width > 0 && target.height > 0 ? target : .zero
        )
        if next == request { return }
        task?.cancel()
        task = nil
        if next.cacheKey != request?.cacheKey {
            shown = nil
            imageView.image = nil
        }
        request = next
        if let cached = WhiskerImagePipeline.shared.cached(next) {
            show(cached, animated: false)
            return
        }
        task = WhiskerImagePipeline.shared.load(next, scale: Self.screenScale) { [weak self] result, downloaded in
            guard let self, self.request == next else { return }
            self.task = nil
            switch result {
            case .success(let loaded):
                self.show(loaded, animated: downloaded)
            case .failure(let error):
                WhiskerCustomEvent.dispatch(
                    from: self,
                    name: "error",
                    params: ["error": error.localizedDescription]
                )
            }
        }
    }

    private func show(_ loaded: WhiskerLoadedImage, animated: Bool) {
        let imageView: WhiskerImageContentView = self.view()
        shown = loaded
        if animated {
            UIView.transition(
                with: imageView,
                duration: 0.2,
                options: [.transitionCrossDissolve, .allowUserInteraction]
            ) { imageView.image = loaded.image }
        } else {
            imageView.image = loaded.image
        }
        WhiskerCustomEvent.dispatch(
            from: self,
            name: "load",
            params: ["width": loaded.naturalSize.width, "height": loaded.naturalSize.height]
        )
    }
    #else
    private func load() {
        guard let value = currentSrc else { return }

        let imageView: WhiskerImageContentView = self.view()
        // macOS bundles resolve `whisker-asset` paths to absolute file paths rather than URLs.
        guard let url = value.hasPrefix("/") ? URL(fileURLWithPath: value) : URL(string: value) else {
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
            .scaleFactor(Self.screenScale),
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
                        "width": Self.pixelSize(of: loaded.image).width,
                        "height": Self.pixelSize(of: loaded.image).height,
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

    private static func pixelSize(of image: KFCrossPlatformImage) -> CGSize {
        guard let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
            return image.size
        }
        return CGSize(width: cgImage.width, height: cgImage.height)
    }
    #endif

    private static var screenScale: CGFloat {
        #if canImport(UIKit)
        UIScreen.main.scale
        #else
        NSScreen.main?.backingScaleFactor ?? 2
        #endif
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
        #if canImport(UIKit)
        WhiskerImagePipeline.shared.prefetch(urls: targets, headers: headers)
        #else
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
        #endif
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
        let imageView: WhiskerImageContentView = self.view()
        #if canImport(UIKit)
        // `center` shows the source at its own size, so it is the one mode decoded in full.
        let atViewSize = value != "center"
        if atViewSize != decodesAtViewSize {
            decodesAtViewSize = atViewSize
            if currentSrc != nil { scheduleLoad() }
        }
        #endif
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
