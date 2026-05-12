import UIKit
import Lynx
import LyraBridge
import LyraMobile

/// Hosts the Lyra runtime on iOS.
///
/// **Phase 2**: now inherits from `LynxView` so we ride on Lynx's iOS SDK
/// for surface management, vsync, lifecycle, touch dispatch, and
/// accessibility. We don't load a `.lynx` template — instead we drop a
/// plain UIKit overlay that displays a greeting fetched from Rust via
/// the LyraMobile C ABI.
///
/// Phase 3 will replace the overlay with Element PAPI calls driven by
/// the Rust runtime through the C++ bridge.
public final class LyraView: LynxView {

    /// `LynxView`'s designated initializer is `init(builderBlock:)`.
    /// We funnel everything through it so the Lynx engine is configured
    /// correctly even when callers pass just a frame.
    public override init(frame: CGRect) {
        super.init(builderBlock: { builder in
            builder.frame = frame
        })
        backgroundColor = .systemBackground
        // Phase 3a smoke test: prove the C++/Obj-C++ bridge is reachable.
        lyra_bridge_log_hello()
        // Phase 3b: hand our LynxView to the bridge so it can resolve the
        // engine proxy and dispatch a task onto the Lynx TASM thread.
        let viewPtr = Unmanaged.passUnretained(self).toOpaque()
        let ok = lyra_bridge_dispatch_log(viewPtr)
        NSLog("[LyraView] bridge dispatch_log returned \(ok)")
        installGreetingLabel()
    }

    public required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported")
    }

    private func installGreetingLabel() {
        let label = UILabel()
        label.text = greetingFromRust()
        label.font = .systemFont(ofSize: 32, weight: .semibold)
        label.textColor = .label
        label.translatesAutoresizingMaskIntoConstraints = false
        addSubview(label)

        NSLayoutConstraint.activate([
            label.centerXAnchor.constraint(equalTo: centerXAnchor),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    /// Calls into the Rust static library via the C ABI exposed in
    /// `lyra-mobile`. The pointer references static storage on the Rust
    /// side, so no free is required.
    private func greetingFromRust() -> String {
        guard let cstr = lyra_mobile_greeting() else {
            return "<null from Rust>"
        }
        return String(cString: cstr)
    }

    /// Forward foreground transitions to the Rust runtime once wired.
    public override func onEnterForeground() {
        super.onEnterForeground()
    }

    /// Forward background transitions to the Rust runtime once wired.
    public override func onEnterBackground() {
        super.onEnterBackground()
    }
}
