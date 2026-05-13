import UIKit
import Lynx
// LyraMobile re-exports the C ABI of `lyra_bridge.h` (see its
// module.modulemap), so `lyra_bridge_engine_attach` etc. are visible
// from this single import.
import LyraMobile

/// Hosts the Lyra runtime on iOS.
///
/// Swift only attaches the engine and hands it to the Rust runtime via
/// `lyra_mobile_app_main`. The element tree, the diff engine, and reactive
/// state all live in Rust.
///
/// Render loop:
///   - A `CADisplayLink` is the heartbeat. It starts paused.
///   - Rust calls back into `requestFrameTrampoline` whenever a signal
///     update marks the tree dirty, which unpauses the link.
///   - On each vsync tick we call `lyra_mobile_tick`. The Rust runtime
///     returns `true` once it has nothing further to render; we pause
///     the link until the next signal update.
///
/// So idle apps consume zero per-frame wakeups while interactive updates
/// land on the next display refresh with no `Timer` jitter.
public final class LyraView: LynxView {

    private var engine: OpaquePointer?
    private var displayLink: CADisplayLink?

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

        let selfPtr = Unmanaged.passUnretained(self).toOpaque()
        lyra_mobile_app_main(
            UnsafeMutableRawPointer(engine),
            LyraView.requestFrameTrampoline,
            selfPtr
        )

        let link = CADisplayLink(target: self, selector: #selector(handleDisplayLink(_:)))
        link.isPaused = true
        link.add(to: .main, forMode: .common)
        self.displayLink = link
    }

    public required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported")
    }

    deinit {
        displayLink?.invalidate()
        if let engine = engine {
            lyra_bridge_engine_release(engine)
        }
    }

    @objc private func handleDisplayLink(_ link: CADisplayLink) {
        guard let engine = engine else { return }
        let idle = lyra_mobile_tick(UnsafeMutableRawPointer(engine))
        if idle {
            link.isPaused = true
        }
    }

    /// C-ABI entry point Rust calls into when a signal marks the tree
    /// dirty. `userData` is the LyraView pointer set up in `init`.
    private static let requestFrameTrampoline:
        @convention(c) (UnsafeMutableRawPointer?) -> Void = { userData in
        guard let userData = userData else { return }
        let view = Unmanaged<LyraView>.fromOpaque(userData).takeUnretainedValue()
        // The display link must be touched from the main run loop. In our
        // current iOS setup the runtime/Lynx-TASM thread already *is* the
        // main thread, so this is the synchronous fast path; the
        // `DispatchQueue.main.async` branch is a safety net for future
        // multi-threaded TASM setups.
        if Thread.isMainThread {
            view.displayLink?.isPaused = false
        } else {
            DispatchQueue.main.async {
                view.displayLink?.isPaused = false
            }
        }
    }

    public override func onEnterForeground() {
        super.onEnterForeground()
    }

    public override func onEnterBackground() {
        super.onEnterBackground()
    }
}
