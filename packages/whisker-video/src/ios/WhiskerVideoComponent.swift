// `whisker-video:Video` platform component on iOS — Phase 7-Φ.H.2.6
// sample. Backed by AVPlayer.
//
// Demonstrates:
//   - `@WhiskerComponent` for tag registration (namespaced as
//     `whisker-video:Video` — Phase 7-Φ.H.2.1).
//   - `@WhiskerProp("src")` for declarative prop dispatch from
//     Lynx's reflection layer — emits the `__lynx_prop_config__src`
//     class method around the `setSrc:requestReset:` setter so
//     `src=` in `render!` reaches `setSrc`. Phase 7-Φ.H.2.7 follow-up.
//   - `@WhiskerUIMethod` for imperative methods Rust dispatches
//     via `ElementRef<T>` (`video_ref.play()` etc.). Phase
//     7-Φ.H.2.2 + .3 + .7.

import AVKit
import UIKit
import WhiskerComponents
import WhiskerModuleApi

@WhiskerComponent("Video")
@objc(WhiskerVideoComponent)
public final class WhiskerVideoComponent: WhiskerUI<UIView> {
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

    // MARK: - Lynx prop dispatch

    @WhiskerProp("src")
    @objc public func setSrc(_ value: NSString, requestReset: Bool) {
        guard let url = URL(string: value as String) else { return }
        // Tear down any prior player + layer so a `src=` change
        // rebuilds cleanly.
        playerLayer?.removeFromSuperlayer()

        let p = AVPlayer(url: url)
        let layer = AVPlayerLayer(player: p)
        layer.videoGravity = .resizeAspectFill
        layer.backgroundColor = UIColor.black.cgColor

        let hostView: UIView = self.view()
        // setSrc can fire before Lynx assigns the host view its
        // computed frame (the first dispatch happens during
        // initial-mount prop application). Give the layer a
        // sensible default rect; `frameDidChange` resizes once
        // layout completes.
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

    // MARK: - @WhiskerUIMethod handlers

    @WhiskerUIMethod
    public func play(_ args: [WhiskerValue]) -> WhiskerValue {
        player?.play()
        return .null
    }

    @WhiskerUIMethod
    public func pause(_ args: [WhiskerValue]) -> WhiskerValue {
        player?.pause()
        return .null
    }

    @WhiskerUIMethod
    public func seek(_ args: [WhiskerValue]) -> WhiskerValue {
        guard case .float(let seconds) = args.first ?? .null else {
            return .error("seek: expected first arg to be a Float (position in seconds)")
        }
        let time = CMTime(seconds: seconds, preferredTimescale: 600)
        player?.seek(to: time)
        return .null
    }
}
