import UIKit
import Lynx
// WhiskerDriver re-exports the C ABI of `whisker_bridge.h` (see its
// module.modulemap), so `whisker_bridge_engine_attach` etc. are visible
// from this single import.
import WhiskerDriver

/// Hosts the Whisker runtime on iOS.
///
/// Swift only attaches the engine and hands it to the Rust runtime via
/// `whisker_app_main`. The element tree, the diff engine, and reactive
/// state all live in Rust.
///
/// Render loop:
///   - A `CADisplayLink` is the heartbeat. It starts paused.
///   - Rust calls back into `requestFrameTrampoline` whenever a signal
///     update marks the tree dirty, which unpauses the link.
///   - On each vsync tick we call `whisker_tick`. The Rust runtime returns
///     `true` once it has nothing further to render; we pause the link
///     until the next signal update.
///
/// So idle apps consume zero per-frame wakeups while interactive updates
/// land on the next display refresh with no `Timer` jitter.
public final class WhiskerView: LynxView {

    private var engine: OpaquePointer?
    private var displayLink: CADisplayLink?

    public override init(frame: CGRect) {
        super.init(builderBlock: { builder in
            builder.frame = frame
        })

        let viewPtr = Unmanaged.passUnretained(self).toOpaque()
        guard let engine = whisker_bridge_engine_attach(viewPtr) else {
            NSLog("[WhiskerView] whisker_bridge_engine_attach returned NULL")
            return
        }
        self.engine = engine

        // IMPORTANT: create the CADisplayLink BEFORE calling
        // `whisker_app_main`. The Rust bootstrap registers
        // `requestFrameTrampoline` as the host-wake callback and
        // then synchronously runs the user's `app()` — which calls
        // `resource(fetch_stories)` → `spawn_local(future)` →
        // `host_wake::wake_runtime()` → this trampoline.
        //
        // If `displayLink` is still nil at that point (because we
        // hadn't created it yet), the `view.displayLink?.isPaused`
        // optional-chaining no-ops and the wake is silently
        // dropped. The future then never gets polled — no
        // CADisplayLink → no `whisker_tick` → no
        // `run_until_stalled` → fetch sits unscheduled — and the
        // app is stuck on the loading banner forever.
        //
        // Creating the link upfront makes the trampoline's unpause
        // actually take effect.
        let link = CADisplayLink(target: self, selector: #selector(handleDisplayLink(_:)))
        link.isPaused = true
        link.add(to: .main, forMode: .common)
        self.displayLink = link

        let selfPtr = Unmanaged.passUnretained(self).toOpaque()
        whisker_app_main(
            UnsafeMutableRawPointer(engine),
            WhiskerView.requestFrameTrampoline,
            selfPtr
        )
    }

    public required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported")
    }

    deinit {
        displayLink?.invalidate()
        if let engine = engine {
            whisker_bridge_engine_release(engine)
        }
    }

    @objc private func handleDisplayLink(_ link: CADisplayLink) {
        guard let engine = engine else { return }
        let idle = whisker_tick(UnsafeMutableRawPointer(engine))
        if idle {
            link.isPaused = true
        }
    }

    /// C-ABI entry point Rust calls into when a signal marks the tree
    /// dirty. `userData` is the WhiskerView pointer set up in `init`.
    private static let requestFrameTrampoline:
        @convention(c) (UnsafeMutableRawPointer?) -> Void = { userData in
        guard let userData = userData else { return }
        let view = Unmanaged<WhiskerView>.fromOpaque(userData).takeUnretainedValue()
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
