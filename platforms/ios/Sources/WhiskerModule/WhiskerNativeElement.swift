import QuartzCore
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

/** Event source shared by class-backed module Views and built-in controls. */
public protocol WhiskerEventSource: AnyObject {
    func installWhiskerEventSink(_ sink: ((String, WhiskerValue) -> Void)?)
}

/** Runtime-facing contract implemented by module-owned native Views. */
public protocol WhiskerNativeElement: WhiskerEventSource {
    static func makeWhiskerView() -> WhiskerPlatformView
}

/** A module View whose frame a `WhiskerFrameHost` runs. */
public protocol WhiskerFrameClient: AnyObject {
    func runWhiskerFrame(_ timestamp: CFTimeInterval)
}

/** Implemented by the View that owns a Whisker runtime; runs requested View frames before its tick. */
public protocol WhiskerFrameHost: AnyObject {
    func requestViewFrame(_ view: WhiskerFrameClient)
}

/** Lightweight native-element base class with no renderer SDK dependency. */
open class WhiskerUI<V: WhiskerPlatformView>: WhiskerPlatformView, WhiskerNativeElement, WhiskerFrameClient {
    private var nativeView: V!
    private var eventSink: ((String, WhiskerValue) -> Void)?
    private var frameRequested = false

    public required override init(frame: CGRect) {
        super.init(frame: frame)
        let nativeView = createView()
        self.nativeView = nativeView
        nativeView.frame = bounds
        nativeView.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        addSubview(nativeView)
    }

    public required init?(coder: NSCoder) { nil }

    open func createView() -> V {
        fatalError("WhiskerUI subclasses must implement createView()")
    }

    public func view() -> V { nativeView }

    public class func makeWhiskerView() -> WhiskerPlatformView {
        self.init(frame: .zero)
    }

    public func installWhiskerEventSink(_ sink: ((String, WhiskerValue) -> Void)?) {
        eventSink = sink
    }

    public func emitWhiskerEvent(_ name: String, detail: WhiskerValue = .null) {
        eventSink?(name, detail)
    }

    /// Calls `onFrame` once on the next display frame, before Whisker applies that frame's updates.
    public func requestFrame() {
        guard !frameRequested else { return }
        frameRequested = true
        if window != nil { forwardFrameRequest() }
    }

    /// Runs outside Whisker's commit, so it may call into Rust; what it publishes lands in this frame.
    open func onFrame(timestamp: CFTimeInterval) {}

    public func runWhiskerFrame(_ timestamp: CFTimeInterval) {
        guard frameRequested, window != nil else { return }
        frameRequested = false
        onFrame(timestamp: timestamp)
    }

    #if canImport(UIKit)
    open override func didMoveToWindow() {
        super.didMoveToWindow()
        if frameRequested, window != nil { forwardFrameRequest() }
    }
    #else
    open override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if frameRequested, window != nil { forwardFrameRequest() }
    }
    #endif

    private func forwardFrameRequest() {
        var ancestor = superview
        while let view = ancestor {
            if let host = view as? WhiskerFrameHost {
                host.requestViewFrame(self)
                return
            }
            ancestor = view.superview
        }
        DispatchQueue.main.async { [weak self] in
            self?.runWhiskerFrame(CACurrentMediaTime())
        }
    }
}

/** Event helper used by module-owned native controls. */
public enum WhiskerCustomEvent {
    public static func dispatch<V>(
        from ui: WhiskerUI<V>,
        name: String,
        params: [AnyHashable: Any]? = nil
    ) {
        let values = (params ?? [:]).reduce(into: [String: WhiskerValue]()) { result, entry in
            result[String(describing: entry.key)] = WhiskerValue.from(nsObject: entry.value)
        }
        ui.emitWhiskerEvent(name, detail: .map(values))
    }
}
