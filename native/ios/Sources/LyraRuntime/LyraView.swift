import UIKit
import Lynx
import LyraBridge
import LyraMobile

// Demo-only tick increment exported by `examples/hello-world`. Will go
// away once tap-driven events flow on iOS.
@_silgen_name("hello_world_tick_signal")
private func hello_world_tick_signal()

/// Hosts the Lyra runtime on iOS.
///
/// **Phase 4–8**: Swift only attaches the engine and hands it to the
/// Rust runtime via `lyra_mobile_app_main`. The element tree, the diff
/// engine, and (eventually) reactive state all live in Rust.
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
        // Hand control to Rust. The runtime dispatches to the Lynx TASM
        // thread internally, so this returns immediately.
        lyra_mobile_app_main(UnsafeMutableRawPointer(engine))

        // Tick once per second. Until tap-driven events flow on iOS the
        // demo crate exposes `hello_world_tick_signal` to bump its
        // counter externally; Swift calls it before each `lyra_mobile_tick`
        // so the signal is dirty by the time `Runtime::frame` runs.
        tickTimer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) {
            [weak self] _ in
            guard let engine = self?.engine else { return }
            hello_world_tick_signal()
            lyra_mobile_tick(UnsafeMutableRawPointer(engine))
        }
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

    public override func onEnterForeground() {
        super.onEnterForeground()
    }

    public override func onEnterBackground() {
        super.onEnterBackground()
    }
}
