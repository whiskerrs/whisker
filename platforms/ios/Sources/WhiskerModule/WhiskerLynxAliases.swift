import UIKit

/** Runtime-facing contract implemented by module-owned native Views. */
public protocol WhiskerNativeElement: AnyObject {
    static func makeWhiskerView() -> UIView
    func installWhiskerEventSink(_ sink: ((String, WhiskerValue) -> Void)?)
}

/** Lightweight native-element base class with no renderer SDK dependency. */
open class WhiskerUI<V: UIView>: UIView, WhiskerNativeElement {
    private var nativeView: V!
    private var eventSink: ((String, WhiskerValue) -> Void)?

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

    public class func makeWhiskerView() -> UIView {
        self.init(frame: .zero)
    }

    public func installWhiskerEventSink(_ sink: ((String, WhiskerValue) -> Void)?) {
        eventSink = sink
    }

    public func emitWhiskerEvent(_ name: String, detail: WhiskerValue = .null) {
        eventSink?(name, detail)
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
