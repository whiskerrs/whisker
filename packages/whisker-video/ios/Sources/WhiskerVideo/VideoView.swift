// Whisker module view hosting AVPlayer + AVPlayerLayer. Registration is
// driven by `VideoModule`'s `definition()`, not by annotations here.
//
// `@objc(VideoView)` pins the Obj-C class name to the bare
// `VideoView` so the codegen plugin's `NSClassFromString` lookup
// can find it under either the SwiftPM-target-prefixed form
// (`whisker_video.VideoView`) or the bare form.

import AVKit
import UIKit
import WhiskerModule

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
    /// The Host fires this after applying the element's computed frame —
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
        // setSrc can fire before the Host assigns the view its computed
        // frame — the first dispatch happens during initial-mount prop
        // application — so the layer needs a placeholder rect until
        // `frameDidChange` resizes it.
        layer.frame = hostView.bounds.isEmpty
            ? CGRect(x: 0, y: 0, width: 400, height: 200)
            : hostView.bounds
        hostView.layer.addSublayer(layer)

        self.player = p
        self.playerLayer = layer
        // TODO: expose this as an `autoplay` attribute instead of
        // unconditionally starting playback.
        p.play()
    }

    public func play()  { player?.play()  }
    public func pause() { player?.pause() }
    public func seek(_ seconds: Double) {
        let time = CMTime(seconds: seconds, preferredTimescale: 600)
        player?.seek(to: time)
    }
}
