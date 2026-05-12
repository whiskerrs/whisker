import UIKit
import Lynx
import LyraBridge
import LyraMobile

/// Hosts the Lyra runtime on iOS.
///
/// **Phase 3c**: drives Lynx's Element PAPI directly from the Obj-C++
/// bridge to render text — no `.lynx` template, no UIKit overlay.
public final class LyraView: LynxView {

    public override init(frame: CGRect) {
        super.init(builderBlock: { builder in
            builder.frame = frame
        })

        let viewPtr = Unmanaged.passUnretained(self).toOpaque()
        let greeting = String(cString: lyra_mobile_greeting())
        let ok = lyra_bridge_render_text(viewPtr, greeting)
        NSLog("[LyraView] bridge render_text(\"\(greeting)\") returned \(ok)")
    }

    public required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported")
    }

    public override func onEnterForeground() {
        super.onEnterForeground()
    }

    public override func onEnterBackground() {
        super.onEnterBackground()
    }
}
