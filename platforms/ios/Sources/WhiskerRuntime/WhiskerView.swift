import UIKit
import Lynx
// WhiskerDriver re-exports the C ABI of `whisker_bridge.h` (see its
// module.modulemap), so `whisker_bridge_engine_attach` etc. are visible
// from this single import.
import WhiskerCBridge

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
    private var displayLinkProxy: DisplayLinkProxy?

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
        //
        // Route through a weak proxy rather than `target: self`. A
        // CADisplayLink added to a run loop is retained by that run
        // loop, and the link strongly retains its `target`; `target:
        // self` therefore forms a retain cycle (run loop → link → view)
        // that keeps the WhiskerView — and the whole Rust engine it
        // owns — alive forever. `deinit` would never run, so
        // `invalidate()` / `whisker_bridge_engine_release` would never
        // fire (a per-view leak). The proxy holds the view weakly, so
        // the cycle is broken and `deinit` runs when the hierarchy lets
        // go of the view.
        let proxy = DisplayLinkProxy(target: self)
        self.displayLinkProxy = proxy
        let link = CADisplayLink(target: proxy, selector: #selector(DisplayLinkProxy.tick(_:)))
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

    fileprivate func handleDisplayLink(_ link: CADisplayLink) {
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

    // MARK: - Safe-area broadcast

    /// Called by UIKit whenever the host view's `safeAreaInsets`
    /// recompute — on first layout after attach, on rotation, on
    /// multitasking split-screen resize, on notch / Dynamic Island
    /// reveal. We re-broadcast through `NotificationCenter` so the
    /// `whisker-safe-area:SafeArea` module can pick the change up
    /// without holding a direct reference to this view.
    ///
    /// Loose-coupling through the notification keeps the runtime
    /// agnostic of the safe-area module: the consumer crate is opt-
    /// in. Apps that don't depend on `whisker-safe-area` pay only the
    /// cost of one `NotificationCenter.post` per insets change, which
    /// is rare and cheap.
    public override func safeAreaInsetsDidChange() {
        super.safeAreaInsetsDidChange()
        NotificationCenter.default.post(
            name: WhiskerView.safeAreaInsetsDidChangeNotification,
            object: self,
            userInfo: [WhiskerView.safeAreaInsetsKey: safeAreaInsets]
        )
    }

    /// Posted on every `safeAreaInsetsDidChange()` fire. `object` is
    /// the firing `WhiskerView`; `userInfo[safeAreaInsetsKey]` is a
    /// `UIEdgeInsets`. Loose-coupling hook for the `whisker-safe-area`
    /// module.
    public static let safeAreaInsetsDidChangeNotification =
        Notification.Name("WhiskerViewSafeAreaInsetsDidChange")

    /// `userInfo` key carrying the `UIEdgeInsets` payload of
    /// [`safeAreaInsetsDidChangeNotification`].
    public static let safeAreaInsetsKey = "WhiskerViewSafeAreaInsets"
}

/// Weak forwarding target for the `CADisplayLink`, so the link (and the
/// run loop that retains it) does not strongly retain the
/// `WhiskerView`. See the comment at the link's creation in `init` for
/// why a direct `target: self` would leak the view and its engine.
private final class DisplayLinkProxy {
    weak var target: WhiskerView?

    init(target: WhiskerView) {
        self.target = target
    }

    @objc func tick(_ link: CADisplayLink) {
        // If the view has been deallocated the weak ref is nil and the
        // tick is a no-op; the view's `deinit` will have invalidated the
        // link by then anyway.
        target?.handleDisplayLink(link)
    }
}
