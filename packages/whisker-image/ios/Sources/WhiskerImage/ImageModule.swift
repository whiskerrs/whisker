// `whisker-image` ModuleDefinition (iOS).
//
// The codegen plugin scans this Swift target for any concrete subclass
// of `Module` and emits a registration block in
// `<Target>+Generated.swift`, which registers the view and module functions
// with Whisker's Host registry.
//
// The `WhiskerImageView` module view this references lives in
// `ImageView.swift`. Same split on Android (`ImageModule.kt` +
// `WhiskerImageView.kt`).

import Foundation
import WhiskerModule

@WhiskerModule
public final class ImageModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("Image")

            AsyncFunction("prefetch") { (args: [WhiskerValue], promise: WhiskerPromise) in
                let urls: [String] = {
                    guard case .array(let items)? = args.first else { return [] }
                    return items.compactMap { $0.asString }
                }()
                let headers = args.count > 1 ? (args[1].asString ?? "") : ""
                DispatchQueue.main.async {
                    WhiskerImageView.prefetch(
                        urls: urls,
                        headers: WhiskerImageView.headers(from: headers)
                    )
                    promise.resolve(.null)
                }
            }
            AsyncFunction("configureCache") { (args: [WhiskerValue], promise: WhiskerPromise) in
                let value = args.first?.asString ?? ""
                ManagedImageCache.shared.queue.async {
                    do { try ManagedImageCache.shared.configure(value); promise.resolve(.null) }
                    catch { promise.reject(error.localizedDescription) }
                }
            }
            AsyncFunction("removeCached") { (args: [WhiskerValue], promise: WhiskerPromise) in
                let value = args.first?.asString ?? ""
                ManagedImageCache.shared.queue.async {
                    do { try ManagedImageCache.shared.remove(value); promise.resolve(.null) }
                    catch { promise.reject(error.localizedDescription) }
                }
            }
            View("whisker-image:Image", WhiskerImageView.self) {
                Prop("src") { (view: WhiskerImageView, value: WhiskerValue) in
                    view.setSrc(value.asString ?? "")
                }
                Prop("mode") { (view: WhiskerImageView, value: WhiskerValue) in
                    view.setMode(value.asString ?? "aspectFill")
                }
                Prop("headers") { (view: WhiskerImageView, value: WhiskerValue) in
                    view.setHeaders(value.asString ?? "")
                }
                Prop("cache") { (view: WhiskerImageView, value: WhiskerValue) in
                    view.setCache(value.asString ?? "")
                }
                Events("load", "error")
            }
        }
    }
}
