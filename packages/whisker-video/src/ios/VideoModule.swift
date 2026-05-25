// Phase L-3 — `whisker-video` migrated to the new
// `ModuleDefinition` DSL on iOS.
//
// Replaces the pre-L-3 `@WhiskerComponent("Video")`-annotated
// `WhiskerVideoComponent` class plus its `@WhiskerProp` /
// `@WhiskerUIMethod` members. The Lynx tag stays
// `whisker-video:Video`; the DSL's `Prop("src") { ... }`,
// `Function("play") { ... }`, etc. expand into the same Lynx-
// visible setters / methods via the SwiftPM codegen plugin +
// Obj-C-runtime install (L-2b).
//
// Same shape on Android — `VideoModule.kt` next to this file.

import AVKit
import UIKit
import WhiskerModuleApi

/// Lynx UI subclass hosting AVPlayer + AVPlayerLayer. Stays a
/// plain [WhiskerUI] subclass — no annotations; registration is
/// driven by [VideoModule] below.
///
/// `@objc(VideoView)` pins the Obj-C class name to the bare
/// `VideoView` so the codegen plugin's `NSClassFromString` lookup
/// can find it under either the SwiftPM-target-prefixed form
/// (`whisker_video.VideoView`) or the bare form.
@objc(VideoView)
public final class VideoView: WhiskerUI<UIView> {
    private var player: AVPlayer?
    private var playerLayer: AVPlayerLayer?

    @objc public override func createView() -> UIView {
        let v = UIView()
        v.backgroundColor = .black
        return v
    }

    /// Keep the AVPlayerLayer sized to the host UIView's bounds.
    /// Lynx fires this after computing the FiberElement's frame —
    /// `self.view().bounds` is authoritative here.
    @objc public override func frameDidChange() {
        super.frameDidChange()
        playerLayer?.frame = self.view().bounds
    }

    /// Backing of the `src` prop.
    public func setSrc(_ value: String) {
        guard let url = URL(string: value) else { return }
        // Tear down any prior player + layer so a `src=` change
        // rebuilds cleanly.
        playerLayer?.removeFromSuperlayer()

        let p = AVPlayer(url: url)
        let layer = AVPlayerLayer(player: p)
        layer.videoGravity = .resizeAspectFill
        layer.backgroundColor = UIColor.black.cgColor

        let hostView: UIView = self.view()
        // setSrc can fire before Lynx assigns the host view its
        // computed frame (first dispatch happens during initial-
        // mount prop application). Give the layer a sensible default
        // rect; `frameDidChange` resizes once layout completes.
        layer.frame = hostView.bounds.isEmpty
            ? CGRect(x: 0, y: 0, width: 400, height: 200)
            : hostView.bounds
        hostView.layer.addSublayer(layer)

        self.player = p
        self.playerLayer = layer
        // Autoplay so the demo shows motion immediately. A real
        // module would expose this via an `autoplay` attribute.
        p.play()
    }

    public func play()  { player?.play()  }
    public func pause() { player?.pause() }
    public func seek(_ seconds: Double) {
        let time = CMTime(seconds: seconds, preferredTimescale: 600)
        player?.seek(to: time)
    }
}

/// DSL-driven module. The SwiftPM codegen plugin (L-3 addition)
/// discovers this class by spotting the `: WhiskerModule`
/// inheritance, then emits a registration block in
/// `<Target>+Generated.swift` that:
///
///   - Reads `definitionLazy.view!.viewClass` (== `VideoView`).
///   - Calls `LynxComponentRegistry.registerUI(viewClass, withName:
///     "whisker-video:Video")`.
///   - Calls `module.registerWithLynx()` so the DSL's
///     `Prop("src")` + `Function("play"/"pause"/"seek")` install
///     via the Obj-C-runtime path (L-2b).
@objc(VideoModule)
public final class VideoModule: WhiskerModule {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("Video")
            View(VideoView.self) {
                Prop("src") { (view: VideoView, value: String) in
                    view.setSrc(value)
                }
                Function("play")  { (view: VideoView) in view.play()  }
                Function("pause") { (view: VideoView) in view.pause() }
                Function("seek")  { (view: VideoView, seconds: Double) in
                    view.seek(seconds)
                }
            }
        }
    }
}
