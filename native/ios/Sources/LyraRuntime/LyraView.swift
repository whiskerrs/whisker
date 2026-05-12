import UIKit
import Lynx
import LyraBridge
import LyraMobile

// Demo-only tap shim exported by `examples/hello-world`. Until Lynx's
// per-element event listener delivery is unblocked, the host attaches a
// UITapGestureRecognizer to the whole LyraView and dispatches taps
// through this single FFI entry point. Once tap events flow inside Lynx
// the Rust side's `on_tap:` closure handles it directly and this shim
// goes away.
@_silgen_name("hello_world_handle_tap")
private func hello_world_handle_tap()

/// Hosts the Lyra runtime on iOS.
///
/// **Phase 4–8**: Swift only attaches the engine and hands it to the
/// Rust runtime via `lyra_mobile_app_main`. The element tree, the diff
/// engine, and reactive state all live in Rust.
public final class LyraView: LynxView {

    private var engine: OpaquePointer?
    private var tickTimer: Timer?

    public override init(frame: CGRect) {
        super.init(builderBlock: { builder in
            builder.frame = frame
        })

        let viewPtr = Unmanaged.passUnretained(self).toOpaque()
        guard let engine = lyra_bridge_engine_attach(viewPtr) else {
            NSLog("[LyraView] lyra_bridge_engine_attach returned NULL")
            return
        }
        self.engine = engine
        lyra_mobile_app_main(UnsafeMutableRawPointer(engine))

        // Tick at ~30Hz so tap-driven signal updates feel immediate. The
        // tick is cheap when nothing's dirty (it short-circuits inside
        // `Runtime::frame`).
        tickTimer = Timer.scheduledTimer(withTimeInterval: 1.0 / 30.0, repeats: true) {
            [weak self] _ in
            guard let engine = self?.engine else { return }
            lyra_mobile_tick(UnsafeMutableRawPointer(engine))
        }

        // Global tap gesture as a stand-in for per-element Lynx events.
        let tap = UITapGestureRecognizer(target: self, action: #selector(handleTap))
        tap.cancelsTouchesInView = false
        addGestureRecognizer(tap)
    }

    public required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported")
    }

    deinit {
        tickTimer?.invalidate()
        if let engine = engine {
            lyra_bridge_engine_release(engine)
        }
    }

    @objc private func handleTap() {
        hello_world_handle_tap()
    }

    public override func onEnterForeground() {
        super.onEnterForeground()
    }

    public override func onEnterBackground() {
        super.onEnterBackground()
    }
}
