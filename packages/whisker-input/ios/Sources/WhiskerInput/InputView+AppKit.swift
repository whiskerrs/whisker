#if !canImport(UIKit) && canImport(AppKit)
import AppKit
import WhiskerModule

/// The AppKit counterpart of the UIKit `WhiskerInputView`: an `NSTextField` (or `NSSecureTextField`)
/// for single-line input and an `NSTextView` for multiline, rebuilt when either mode flips.
@objc(WhiskerInputView)
public final class WhiskerInputView: WhiskerUI<NSView> {
    private lazy var containerView: InputContainerView = {
        let view = InputContainerView()
        view.onLayout = { [weak self] in self?.layoutField() }
        return view
    }()

    private var textField: NSTextField?
    private var scrollView: NSScrollView?
    private var textView: NSTextView?

    private var cachedText = ""
    private var cachedPlaceholder = ""
    private var cachedPlaceholderColor = NSColor.placeholderTextColor
    private var cachedCaretColor: NSColor?
    private var cachedSelectionColor: NSColor?
    private var cachedMultiline = false
    private var cachedSecure = false
    private var cachedEditable = true
    private var cachedAutoFocus = false
    private var cachedMaxLength = 0
    private var cachedAutocorrect = true
    private var cachedSpellCheck = true
    private var cachedTextColor = NSColor.labelColor
    private var typography = InputTypography(
        family: nil, size: 13, weight: 400, italic: false, lineHeight: nil, letterSpacing: 0)
    private var cachedTextAlignment: NSTextAlignment = .natural
    private var built = false
    // Set while the Host writes the text, so the change is not echoed back as user input.
    private var silent = false

    @objc public override func createView() -> NSView {
        rebuild()
        return containerView
    }

    private func rebuild() {
        if let field = textField {
            field.delegate = nil
            field.removeFromSuperview()
            textField = nil
        }
        if let scroll = scrollView {
            textView?.delegate = nil
            scroll.removeFromSuperview()
            scrollView = nil
            textView = nil
        }
        built = true
        if cachedMultiline {
            let scroll = NSScrollView()
            scroll.drawsBackground = false
            scroll.hasVerticalScroller = false
            scroll.autohidesScrollers = true
            scroll.borderType = .noBorder
            scroll.frame = containerView.bounds
            scroll.autoresizingMask = [.width, .height]
            let view = InputTextView()
            view.autoresizingMask = [.width]
            view.isVerticallyResizable = true
            view.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: .greatestFiniteMagnitude)
            view.textContainer?.widthTracksTextView = true
            view.onFocus = { [weak self] in self?.emitFocus() }
            view.onBlur = { [weak self] in self?.emitBlur() }
            view.onSubmit = { [weak self] in
                guard let self else { return }
                self.emitSubmit(self.currentText())
            }
            scroll.documentView = view
            view.drawsBackground = false
            view.isRichText = false
            view.textContainerInset = .zero
            view.textContainer?.lineFragmentPadding = 0
            view.allowsUndo = true
            view.delegate = self
            containerView.addSubview(scroll)
            scrollView = scroll
            textView = view
        } else {
            let field: NSTextField = cachedSecure ? InputSecureField() : InputField()
            (field as? InputFieldEvents)?.onFocus = { [weak self] in self?.emitFocus() }
            (field as? InputFieldEvents)?.onBlur = { [weak self] in self?.emitBlur() }
            field.isBordered = false
            field.isBezeled = false
            field.drawsBackground = false
            field.focusRingType = .none
            field.usesSingleLineMode = true
            field.cell?.wraps = false
            field.cell?.isScrollable = true
            field.delegate = self
            containerView.addSubview(field)
            textField = field
        }
        applyAll()
        containerView.needsLayout = true
    }

    private func layoutField() {
        guard let field = textField else { return }
        let bounds = containerView.bounds
        let height = min(bounds.height, field.intrinsicContentSize.height)
        field.frame = NSRect(
            x: 0, y: ((bounds.height - height) / 2).rounded(), width: bounds.width, height: height)
    }

    private func applyAll() {
        applyText(cachedText)
        applyPlaceholder()
        applyAppearance()
        applyBehaviour()
    }

    private func currentText() -> String {
        textField?.stringValue ?? textView?.string ?? ""
    }

    private func applyText(_ text: String) {
        guard currentText() != text else { return }
        silent = true
        if let field = textField {
            field.stringValue = text
        } else if let view = textView {
            view.string = text
            applyAppearance()
        }
        silent = false
    }

    private func applyPlaceholder() {
        textField?.placeholderAttributedString = NSAttributedString(
            string: cachedPlaceholder,
            attributes: [.foregroundColor: cachedPlaceholderColor, .font: typography.font])
    }

    private func applyAppearance() {
        var attributes = typography.attributes(alignment: cachedTextAlignment)
        attributes[.foregroundColor] = cachedTextColor
        if let field = textField {
            containerView.needsLayout = true
            field.font = typography.font
            field.textColor = cachedTextColor
            field.alignment = cachedTextAlignment
            if let editor = field.currentEditor() as? NSTextView {
                applyCaret(to: editor)
            }
        }
        if let view = textView {
            view.font = typography.font
            view.textColor = cachedTextColor
            view.alignment = cachedTextAlignment
            let selection = view.selectedRanges
            view.textStorage?.setAttributes(
                attributes, range: NSRange(location: 0, length: view.textStorage?.length ?? 0))
            view.typingAttributes = attributes
            view.selectedRanges = selection
            applyCaret(to: view)
        }
        applyPlaceholder()
    }

    private func applyCaret(to view: NSTextView) {
        view.insertionPointColor = cachedCaretColor ?? cachedTextColor
        if let selection = cachedSelectionColor {
            view.selectedTextAttributes = [.backgroundColor: selection]
        }
    }

    private func applyBehaviour() {
        textField?.isEditable = cachedEditable
        textField?.isSelectable = true
        if let view = textView {
            view.isEditable = cachedEditable
            view.isSelectable = true
            view.isAutomaticSpellingCorrectionEnabled = cachedAutocorrect
            view.isContinuousSpellCheckingEnabled = cachedSpellCheck
        }
        if cachedAutoFocus {
            // `makeFirstResponder` does nothing until the view is in a window, which it isn't yet on this pass.
            DispatchQueue.main.async { [weak self] in self?.focusField() }
        }
    }

    // MARK: - Property setters

    public func setValue(_ text: String) {
        cachedText = text
        applyText(text)
    }

    public func setPlaceholder(_ text: String) {
        cachedPlaceholder = text
        applyPlaceholder()
    }

    public func setPlaceholderColor(_ value: WhiskerValue) {
        cachedPlaceholderColor = Self.resolveColor(value) ?? .placeholderTextColor
        applyPlaceholder()
    }

    public func setCaretColor(_ value: WhiskerValue) {
        cachedCaretColor = Self.resolveColor(value)
        applyAppearance()
    }

    public func setSelectionColor(_ value: WhiskerValue) {
        cachedSelectionColor = Self.resolveColor(value)
        applyAppearance()
    }

    public func setMultiline(_ multiline: Bool) {
        guard multiline != cachedMultiline || !built else { return }
        cachedMultiline = multiline
        rebuild()
    }

    public func setLines(_ lines: Int64) {}

    public func setSecure(_ secure: Bool) {
        guard secure != cachedSecure else { return }
        cachedSecure = secure
        if !cachedMultiline && built { rebuild() }
    }

    public func setEditable(_ editable: Bool) {
        cachedEditable = editable
        applyBehaviour()
    }

    public func setAutoFocus(_ autoFocus: Bool) {
        cachedAutoFocus = autoFocus
        if autoFocus && built {
            DispatchQueue.main.async { [weak self] in self?.focusField() }
        }
    }

    public func setMaxLength(_ count: Int64) {
        cachedMaxLength = max(0, Int(clamping: count))
    }

    public func setKeyboardType(_ type: String) {}
    public func setReturnKey(_ key: String) {}
    public func setAutoCapitalize(_ mode: String) {}

    public func setAutocorrect(_ enabled: Bool) {
        cachedAutocorrect = enabled
        applyBehaviour()
    }

    public func setSpellCheck(_ enabled: Bool) {
        cachedSpellCheck = enabled
        applyBehaviour()
    }

    public func applyTextStyle(_ style: WhiskerTextStyle) {
        cachedTextColor = style.color
        typography = InputTypography(style)
        cachedTextAlignment = switch style.alignment {
        case .left: .left
        case .right: .right
        case .center: .center
        case .start, .end: .natural
        }
        applyAppearance()
    }

    // MARK: - Commands

    public func focusField() {
        guard !isFocused, let target: NSView = textField ?? textView,
              containerView.window?.makeFirstResponder(target) == true
        else { return }
        if let editor = textField?.currentEditor() {
            editor.selectedRange = NSRange(location: (editor.string as NSString).length, length: 0)
        }
    }

    public func blurField() {
        if isFocused { containerView.window?.makeFirstResponder(nil) }
    }

    private var isFocused: Bool {
        textField?.currentEditor() != nil
            || (textView.map { containerView.window?.firstResponder === $0 } ?? false)
    }

    public func clearField() {
        applyText("")
        emitInput("")
    }

    // MARK: - Events

    private func emitFocus() {
        WhiskerCustomEvent.dispatch(from: self, name: "focus", params: [:])
    }

    private func emitBlur() {
        WhiskerCustomEvent.dispatch(from: self, name: "blur", params: [:])
        emitChange(currentText())
    }

    private func emitInput(_ text: String) {
        cachedText = text
        WhiskerCustomEvent.dispatch(from: self, name: "input", params: ["value": text])
    }

    private func emitChange(_ text: String) {
        WhiskerCustomEvent.dispatch(from: self, name: "change", params: ["value": text])
    }

    private func emitSubmit(_ text: String) {
        WhiskerCustomEvent.dispatch(from: self, name: "submit", params: ["value": text])
        emitChange(text)
    }

    private func userEdited() {
        guard !silent else { return }
        var text = currentText()
        if cachedMaxLength > 0 && text.count > cachedMaxLength {
            text = String(text.prefix(cachedMaxLength))
            silent = true
            textField?.stringValue = text
            textView?.string = text
            silent = false
        }
        emitInput(text)
    }

    private static func resolveColor(_ value: WhiskerValue) -> NSColor? {
        guard case .string(let text) = value, !text.isEmpty else { return nil }
        return parseCssColor(text)
    }
}

extension WhiskerInputView: NSTextFieldDelegate, NSTextViewDelegate {
    public func controlTextDidBeginEditing(_ notification: Notification) {
        if let editor = notification.userInfo?["NSFieldEditor"] as? NSTextView {
            applyCaret(to: editor)
        }
    }

    public func controlTextDidChange(_ notification: Notification) {
        userEdited()
    }

    public func control(
        _ control: NSControl, textView: NSTextView, doCommandBy selector: Selector
    ) -> Bool {
        guard selector == #selector(NSResponder.insertNewline(_:)) else { return false }
        emitSubmit(currentText())
        return true
    }

    public func textDidChange(_ notification: Notification) {
        userEdited()
    }
}

private protocol InputFieldEvents: AnyObject {
    var onFocus: (() -> Void)? { get set }
    var onBlur: (() -> Void)? { get set }
}

// The delegate's begin-editing callbacks wait for the first keystroke, so focus is reported from the responder chain.
private final class InputField: NSTextField, InputFieldEvents {
    var onFocus: (() -> Void)?
    var onBlur: (() -> Void)?

    override func becomeFirstResponder() -> Bool {
        let accepted = super.becomeFirstResponder()
        if accepted { onFocus?() }
        return accepted
    }

    override func textDidEndEditing(_ notification: Notification) {
        super.textDidEndEditing(notification)
        onBlur?()
    }
}

private final class InputSecureField: NSSecureTextField, InputFieldEvents {
    var onFocus: (() -> Void)?
    var onBlur: (() -> Void)?

    override func becomeFirstResponder() -> Bool {
        let accepted = super.becomeFirstResponder()
        if accepted { onFocus?() }
        return accepted
    }

    override func textDidEndEditing(_ notification: Notification) {
        super.textDidEndEditing(notification)
        onBlur?()
    }
}

private final class InputTextView: NSTextView {
    var onFocus: (() -> Void)?
    var onBlur: (() -> Void)?
    var onSubmit: (() -> Void)?

    override func becomeFirstResponder() -> Bool {
        let accepted = super.becomeFirstResponder()
        if accepted { onFocus?() }
        return accepted
    }

    override func resignFirstResponder() -> Bool {
        let accepted = super.resignFirstResponder()
        if accepted { onBlur?() }
        return accepted
    }

    override func keyDown(with event: NSEvent) {
        let isReturn = event.keyCode == 36 || event.keyCode == 76
        if isReturn && event.modifierFlags.contains(.command) {
            onSubmit?()
        } else {
            super.keyDown(with: event)
        }
    }
}

private final class InputContainerView: NSView {
    var onLayout: (() -> Void)?

    override func layout() {
        super.layout()
        onLayout?()
    }
}

/// Parses `#RGB`, `#RRGGBB`, `#RRGGBBAA`, `rgb()` and `rgba()`, plus a few named colours.
private func parseCssColor(_ raw: String) -> NSColor? {
    let text = raw.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    if text.hasPrefix("#") {
        let hex = String(text.dropFirst())
        guard let value = UInt32(hex, radix: 16) else { return nil }
        func channel(_ shift: UInt32, _ mask: UInt32 = 0xFF) -> CGFloat {
            CGFloat((value >> shift) & mask) / CGFloat(mask)
        }
        switch hex.count {
        case 3: return NSColor(srgbRed: channel(8, 0xF), green: channel(4, 0xF), blue: channel(0, 0xF), alpha: 1)
        case 6: return NSColor(srgbRed: channel(16), green: channel(8), blue: channel(0), alpha: 1)
        case 8: return NSColor(srgbRed: channel(24), green: channel(16), blue: channel(8), alpha: channel(0))
        default: return nil
        }
    }
    if text.hasPrefix("rgb"), let open = text.firstIndex(of: "("), let close = text.lastIndex(of: ")") {
        let parts = text[text.index(after: open)..<close].split(separator: ",").map {
            Double($0.trimmingCharacters(in: .whitespaces))
        }
        guard parts.count >= 3, let r = parts[0], let g = parts[1], let b = parts[2] else { return nil }
        let alpha = parts.count >= 4 ? (parts[3] ?? 1) : 1
        return NSColor(srgbRed: r / 255, green: g / 255, blue: b / 255, alpha: alpha)
    }
    switch text {
    case "black": return .black
    case "white": return .white
    case "red": return .red
    case "green": return NSColor(srgbRed: 0, green: 128.0 / 255, blue: 0, alpha: 1)
    case "blue": return .blue
    case "gray", "grey": return .gray
    case "transparent": return .clear
    default: return nil
    }
}
#endif
