import UIKit

final class WhiskerTextSelection: NSObject, UITextViewDelegate {
    weak var label: WhiskerTextLabel?
    let view: UITextView
    private var updating = false
    private var previousSource = ""
    private var lastSelection: WhiskerValue?
    private var previousRange = NSRange(location: 0, length: 0)
    private var direction = "forward"

    init(label: WhiskerTextLabel) {
        self.label = label
        let layout = WhiskerParagraphLayout(text: NSAttributedString(string: ""), width: label.bounds.width, maxLines: label.numberOfLines, overflow: label.richOverflow)
        view = WhiskerSelectableTextView(frame: .zero, textContainer: layout.container)
        super.init()
        (view as? WhiskerSelectableTextView)?.label = label
        view.delegate = self
        view.isAccessibilityElement = false
        view.isEditable = false
        view.isSelectable = true
        view.isScrollEnabled = false
        view.backgroundColor = .clear
        view.textContainerInset = .zero
        view.contentInset = .zero
        label.isUserInteractionEnabled = true
        label.addSubview(view)
        synchronize()
    }

    func synchronize() {
        guard let label else { return }
        updating = true
        defer { updating = false }
        let text = label.preparedParagraphLayout(width: label.bounds.width)?.storage ?? label.attributedText ?? NSAttributedString(string: label.text ?? "")
        let previous = view.selectedRange
        let source = label.richParagraph?.source ?? text.string
        let sameSource = previousSource == source
        previousSource = source
        if !view.textStorage.isEqual(to: text) {
            view.textStorage.setAttributedString(text)
            view.selectedRange = sameSource && NSMaxRange(previous) <= text.length ? previous : NSRange(location: 0, length: 0)
        }
        view.frame = label.bounds
        view.textContainer.size = CGSize(width: label.bounds.width, height: .greatestFiniteMagnitude)
        view.textContainer.maximumNumberOfLines = label.numberOfLines
        view.textContainer.lineBreakMode = label.richOverflow == .ellipsis ? .byTruncatingTail : .byWordWrapping
        view.layoutManager.ensureLayout(for: view.textContainer)
        if !sameSource { emitSelection() }
    }

    func textViewDidChangeSelection(_ textView: UITextView) {
        if !updating { emitSelection() }
    }

    func emitSelection() {
        let limit = label?.richParagraph?.visibleEnd ?? view.textStorage.length
        let range = NSIntersectionRange(view.selectedRange, NSRange(location: 0, length: limit))
        if range.length == 0 { direction = "forward" }
        else if range.location != previousRange.location && NSMaxRange(range) == NSMaxRange(previousRange) { direction = "backward" }
        else if NSMaxRange(range) != NSMaxRange(previousRange) { direction = "forward" }
        previousRange = range
        let value = WhiskerValue.map(["revision": .int(Int64(label?.preparedContent ?? 0)), "start": .int(Int64(range.location)), "end": .int(Int64(NSMaxRange(range))), "direction": .string(direction)])
        if value != lastSelection { lastSelection = value; label?.textEventSink?("selectionchange", value) }
    }

    deinit { view.removeFromSuperview() }
}

private final class WhiskerSelectableTextView: UITextView {
    weak var label: WhiskerTextLabel?
    override func copy(_ sender: Any?) {
        guard NSMaxRange(selectedRange) <= textStorage.length else { return }
        UIPasteboard.general.string = label?.richParagraph?.copyText(selectedRange) ?? (textStorage.string as NSString).substring(with: selectedRange)
    }
}

extension WhiskerTextLabel {
    func setSelectable(_ selectable: Bool) {
        if selectable, selectionController == nil { selectionController = WhiskerTextSelection(label: self) }
        if !selectable { selectionController = nil }
        setNeedsDisplay()
    }

    func setTextSelection(_ value: WhiskerValue) {
        guard case .map(let fields) = value, matchesRevision(fields),
              let range = textRange(fields), let selectionController else { return }
        selectionController.view.selectedRange = range
        selectionController.emitSelection()
    }

    func clearTextSelection(_ value: WhiskerValue) {
        guard case .map(let fields) = value, matchesRevision(fields), let selectionController else { return }
        selectionController.view.selectedRange = NSRange(location: 0, length: 0)
        selectionController.emitSelection()
    }

    func queryText(_ value: WhiskerValue) {
        guard case .map(let fields) = value, case .int(let id) = fields["id"] else { return }
        var reply: [String: WhiskerValue] = ["id": .int(id)]
        if !matchesRevision(fields) { reply["error"] = .string("stale-layout") }
        else if fields["kind"]?.asString == "selectedText" {
            let selected = selectionController?.view.selectedRange ?? NSRange(location: 0, length: 0)
            let source = richParagraph?.source ?? (richContent ?? attributedText)?.string ?? text ?? ""
            if selected.location <= (source as NSString).length && selected.length <= (source as NSString).length - selected.location {
                reply["text"] = .string(richParagraph?.copyText(selected) ?? (source as NSString).substring(with: selected))
            } else { reply["error"] = .string("invalid-range") }
        } else if fields["kind"]?.asString == "boundingRects", let range = textRange(fields) {
            let layout: WhiskerParagraphLayout
            if let view = selectionController?.view {
                layout = WhiskerParagraphLayout(storage: view.textStorage, manager: view.layoutManager, container: view.textContainer)
            } else {
                layout = preparedParagraphLayout(width: bounds.width) ?? WhiskerParagraphLayout(text: attributedText ?? NSAttributedString(string: text ?? ""), width: bounds.width, maxLines: numberOfLines, overflow: richOverflow)
            }
            reply["rects"] = .array(layout.selectionRects(range).map { rect in
                .map(["x": .float(Double(rect.minX)), "y": .float(Double(rect.minY)),
                      "width": .float(Double(rect.width)), "height": .float(Double(rect.height))])
            })
        } else { reply["error"] = .string("invalid-range") }
        textEventSink?("textqueryresult", .map(reply))
    }

    private func matchesRevision(_ fields: [String: WhiskerValue]) -> Bool {
        guard case .int(let revision) = fields["revision"], revision > 0 else { return false }
        return UInt64(revision) == preparedContent
    }

    private func textRange(_ fields: [String: WhiskerValue]) -> NSRange? {
        guard case .int(let start) = fields["start"], case .int(let end) = fields["end"], start >= 0, end >= start else { return nil }
        let source = (richParagraph?.source ?? (richContent ?? attributedText)?.string ?? text ?? "") as NSString
        guard end <= source.length else { return nil }
        let units = Array((source as String).utf16)
        for offset in [Int(start), Int(end)] where offset > 0 && offset < units.count {
            if (0xD800...0xDBFF).contains(units[offset - 1]) && (0xDC00...0xDFFF).contains(units[offset]) { return nil }
        }
        let range = NSRange(location: Int(start), length: Int(end - start))
        let normalized = range.length == 0 ? range : source.rangeOfComposedCharacterSequences(for: range)
        let limit = richParagraph?.visibleEnd ?? source.length
        let first = min(normalized.location, limit)
        return NSRange(location: first, length: min(NSMaxRange(normalized), limit) - first)
    }
}

extension WhiskerParagraphLayout {
    func selectionRects(_ characters: NSRange) -> [CGRect] {
        let glyphs = manager.glyphRange(forCharacterRange: characters, actualCharacterRange: nil)
        var rects: [CGRect] = []
        manager.enumerateLineFragments(forGlyphRange: manager.glyphRange(for: container)) { _, _, _, line, _ in
            let truncated = self.manager.truncatedGlyphRange(inLineFragmentForGlyphAt: line.location)
            let visible = NSRange(location: line.location, length: (truncated.location == NSNotFound ? NSMaxRange(line) : truncated.location) - line.location)
            let intersection = NSIntersectionRange(glyphs, visible)
            guard intersection.length > 0 else { return }
            self.manager.enumerateEnclosingRects(forGlyphRange: intersection, withinSelectedGlyphRange: NSRange(location: NSNotFound, length: 0), in: self.container) { rect, _ in rects.append(rect) }
        }
        return rects
    }
}
