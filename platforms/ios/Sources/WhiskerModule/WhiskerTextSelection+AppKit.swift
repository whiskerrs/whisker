#if !canImport(UIKit) && canImport(AppKit)
import AppKit

final class WhiskerTextSelection: NSObject, NSTextViewDelegate {
    weak var label: WhiskerTextLabel?
    let view: NSTextView
    private var updating = false
    private var previousSource = ""
    private var lastSelection: WhiskerValue?
    private var previousRange = NSRange(location: 0, length: 0)
    private var direction = "forward"

    init(label: WhiskerTextLabel) {
        self.label = label
        let layout = WhiskerParagraphLayout(text: NSAttributedString(string: ""), width: label.bounds.width, maxLines: label.numberOfLines, overflow: label.richOverflow)
        let view = WhiskerSelectableTextView(frame: .zero, textContainer: layout.container)
        self.view = view
        super.init()
        view.label = label
        view.delegate = self
        view.setAccessibilityElement(false)
        view.isEditable = false
        view.isSelectable = true
        view.drawsBackground = false
        view.textContainerInset = .zero
        view.isVerticallyResizable = false
        view.isHorizontallyResizable = false
        label.addSubview(view)
        synchronize()
    }

    var selectedRange: NSRange {
        get { view.selectedRange() }
        set { view.setSelectedRange(newValue) }
    }

    var paragraphLayout: WhiskerParagraphLayout {
        WhiskerParagraphLayout(storage: view.textStorage!, manager: view.layoutManager!, container: view.textContainer!)
    }

    func synchronize() {
        guard let label, let storage = view.textStorage, let container = view.textContainer else { return }
        updating = true
        defer { updating = false }
        let text = label.preparedParagraphLayout(width: label.bounds.width)?.storage ?? label.attributedText ?? NSAttributedString(string: label.text ?? "")
        let previous = view.selectedRange()
        let source = label.richParagraph?.source ?? text.string
        let sameSource = previousSource == source
        previousSource = source
        if !storage.isEqual(to: text) {
            storage.setAttributedString(text)
            view.setSelectedRange(sameSource && NSMaxRange(previous) <= text.length ? previous : NSRange(location: 0, length: 0))
        }
        view.frame = label.bounds
        container.size = CGSize(width: label.bounds.width, height: .greatestFiniteMagnitude)
        container.maximumNumberOfLines = label.numberOfLines
        container.lineBreakMode = label.richOverflow == .ellipsis ? .byTruncatingTail : .byWordWrapping
        view.layoutManager?.ensureLayout(for: container)
        if !sameSource { emitSelection() }
    }

    func textViewDidChangeSelection(_ notification: Notification) {
        if !updating { emitSelection() }
    }

    func emitSelection() {
        let limit = label?.richParagraph?.visibleEnd ?? (view.textStorage?.length ?? 0)
        let range = NSIntersectionRange(view.selectedRange(), NSRange(location: 0, length: limit))
        if range.length == 0 { direction = "forward" }
        else if range.location != previousRange.location && NSMaxRange(range) == NSMaxRange(previousRange) { direction = "backward" }
        else if NSMaxRange(range) != NSMaxRange(previousRange) { direction = "forward" }
        previousRange = range
        let value = WhiskerValue.map(["revision": .int(Int64(label?.preparedContent ?? 0)), "start": .int(Int64(range.location)), "end": .int(Int64(NSMaxRange(range))), "direction": .string(direction)])
        if value != lastSelection { lastSelection = value; label?.textEventSink?("selectionchange", value) }
    }

    deinit { view.removeFromSuperview() }
}

private final class WhiskerSelectableTextView: NSTextView {
    weak var label: WhiskerTextLabel?

    override var isFlipped: Bool { true }

    override func copy(_ sender: Any?) {
        guard let storage = textStorage, NSMaxRange(selectedRange()) <= storage.length else { return }
        let text = label?.richParagraph?.copyText(selectedRange()) ?? (storage.string as NSString).substring(with: selectedRange())
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }
}
#endif
