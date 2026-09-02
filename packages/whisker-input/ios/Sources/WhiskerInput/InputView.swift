// Whisker module element hosting a UITextField (single-line) or UITextView
// (multiline) behind a unified interface. Registration is driven by
// `InputModule`'s `definition()` — no annotations required here.
//
// `@objc(WhiskerInputView)` pins the Obj-C class name to the bare,
// SwiftPM-target-unprefixed form, matching the sibling view modules.
//
// ## Single-line vs multiline
//
// `WhiskerUI<UIView>` hosts a transparent `containerView` holding either
// a `UITextField` or a `UITextView` pinned to its bounds. Because the
// two are different UIKit classes, switching `multiline` tears the
// control down and rebuilds it — so every prop is cached and re-applied
// across the swap (props may arrive in any order relative to
// `multiline`, which itself arrives after `createView`).
//
// ## Cursor-preservation diff
//
// `setValue(_:)` writes the control's text only when it differs from
// what is displayed. Rust sets `value`, the view fires `input`, Rust
// sets `value` again with the text the view just reported — writing
// unconditionally would jump the insertion point to the end on every
// keystroke.

import Foundation
import UIKit
import WhiskerModule

/// A container `UIView` that invokes `onDetach` when it leaves its
/// window, so `WhiskerInputView` can resign first responder on teardown
/// and a removed input never lingers as the keyboard target.
private final class DetachAwareView: UIView {
    var onDetach: (() -> Void)?

    override func willMove(toWindow newWindow: UIWindow?) {
        super.willMove(toWindow: newWindow)
        // A single↔multiline swap replaces the *inner* control, not this
        // container, so a nil window really is an unmount.
        if newWindow == nil {
            onDetach?()
        }
    }
}

@objc(WhiskerInputView)
public final class WhiskerInputView: WhiskerUI<UIView> {

    // MARK: - Hosted controls

    /// Transparent container that fills the module element; holds either the
    /// `textField` or `textView` as a subview.
    ///
    /// A [`DetachAwareView`] so it can resign focus on unmount. UIKit
    /// auto-resigns a first responder removed from its window anyway, but
    /// doing it explicitly dismisses the IME promptly and keeps parity with
    /// Android's `onViewDetachedFromWindow` hook.
    private lazy var containerView: UIView = {
        let v = DetachAwareView()
        v.backgroundColor = .clear
        v.onDetach = { [weak self] in self?.blurField() }
        return v
    }()

    /// Mutually exclusive with `textView` — exactly one is non-nil once a
    /// control has been built.
    private var textField: UITextField?

    /// Mutually exclusive with `textField`.
    private var textView: UITextView?

    private var isMultiline: Bool = false

    // MARK: - Cached prop state
    //
    // Every prop is cached so `applyAllCachedProps()` can reinstate the
    // full state onto a freshly built control after a single↔multiline
    // switch.

    private var cachedText: String = ""
    private var cachedPlaceholder: String = ""
    private var cachedPlaceholderColor: UIColor = UIColor(white: 0.6, alpha: 1)
    private var cachedCaretColor: UIColor = UIColor.systemBlue  // tintColor default
    private var cachedSelectionColor: UIColor? = nil    // nil = use caret color
    private var cachedSecure: Bool = false
    private var cachedEditable: Bool = true
    private var cachedAutoFocus: Bool = false
    private var cachedMaxLength: Int = 0                // 0 = unset
    private var cachedKeyboardType: UIKeyboardType = .default
    private var cachedReturnKeyType: UIReturnKeyType = .default
    private var cachedAutoCapitalize: UITextAutocapitalizationType = .sentences
    // `.default` rather than `.yes` for the enabled case, so UIKit keeps
    // its contextual behaviour — it already disables autocorrect on URL /
    // email keyboards. Only `false` forces `.no`.
    private var cachedAutocorrect: UITextAutocorrectionType = .default
    private var cachedSpellCheck: UITextSpellCheckingType = .default
    private var cachedTextColor: UIColor = .label
    private var cachedFontSize: CGFloat = 17
    private var cachedFontWeight: UIFont.Weight = .regular
    private var cachedTextAlignment: NSTextAlignment = .natural

    /// Distinguishes "no control yet" from "control exists, possibly needs
    /// a mode switch" in `ensureControl`.
    private var controlBuilt: Bool = false

    // MARK: - WhiskerUI lifecycle

    @objc public override func createView() -> UIView {
        // Default to single-line so the first render shows a working field
        // even though `multiline` hasn't arrived yet.
        ensureControl(multiline: false)
        return containerView
    }

    // MARK: - Control builder

    /// Build the hosted control for `multiline`, or switch to it if a
    /// control of the other mode is already live, re-applying every cached
    /// prop so no state is lost across the switch.
    private func ensureControl(multiline: Bool) {
        if controlBuilt && isMultiline == multiline { return }

        // Clear the delegate and targets as well as dropping the refs, so a
        // torn-down control can't deliver events after the switch.
        if let tf = textField {
            tf.delegate = nil
            tf.removeTarget(self, action: nil, for: .allEvents)
            tf.removeFromSuperview()
            textField = nil
        }
        if let tv = textView {
            tv.delegate = nil
            tv.removeFromSuperview()
            textView = nil
        }

        isMultiline = multiline
        controlBuilt = true

        if multiline {
            buildTextView()
        } else {
            buildTextField()
        }

        // Pin the fresh control to the container's current bounds.
        let bounds = containerView.bounds
        textField?.frame = bounds
        textView?.frame = bounds

        applyAllCachedProps()
    }

    private func buildTextField() {
        let tf = PaddedTextField()
        tf.borderStyle = .none
        tf.backgroundColor = .clear
        tf.addTarget(self, action: #selector(textFieldDidChange(_:)), for: .editingChanged)
        tf.addTarget(self, action: #selector(textFieldDidEndOnExit(_:)), for: .editingDidEndOnExit)
        tf.delegate = self
        tf.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        containerView.addSubview(tf)
        textField = tf
    }

    private func buildTextView() {
        let tv = UITextView()
        tv.backgroundColor = .clear
        // Zero `lineFragmentPadding` so horizontal CSS padding lands
        // exactly, rather than adding to the container's own padding.
        tv.textContainerInset = .zero
        tv.textContainer.lineFragmentPadding = 0
        // A UITextView vertically centers content shorter than its bounds
        // unless scrolling is enabled, so scroll-on is what top-aligns the
        // text; `.never` then stops the safe area pushing the first line
        // off the top edge.
        tv.isScrollEnabled = true
        tv.contentInsetAdjustmentBehavior = .never
        tv.returnKeyType = .default
        tv.delegate = self
        tv.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        containerView.addSubview(tv)
        textView = tv
    }

    // MARK: - Apply all cached props to the current active control

    private func applyAllCachedProps() {
        applyText(cachedText)
        applyPlaceholder()
        applyColors()
        applyFont()
        applyTextAlignment()
        applyBehaviour()
    }

    /// Apply current text to whichever control is active. The equality
    /// check is what preserves the cursor position.
    private func applyText(_ s: String) {
        if let tf = textField {
            if tf.text != s { tf.text = s }
        } else if let tv = textView {
            if tv.text != s { tv.text = s }
        }
    }

    private func applyPlaceholder() {
        if let tf = textField {
            tf.attributedPlaceholder = NSAttributedString(
                string: cachedPlaceholder,
                attributes: [.foregroundColor: cachedPlaceholderColor]
            )
        }
        // UITextView has no native placeholder; unsupported for now.
    }

    private func applyColors() {
        // UIKit exposes no independent selection color: `tintColor` drives
        // both the cursor and the selection highlight, so `selection-color`
        // wins the shared tint when set and `caret-color` is the fallback.
        let tint = cachedSelectionColor ?? cachedCaretColor
        if let tf = textField {
            tf.textColor = cachedTextColor
            tf.tintColor = tint
        } else if let tv = textView {
            tv.textColor = cachedTextColor
            tv.tintColor = tint
        }
    }

    private func applyFont() {
        let font = UIFont.systemFont(ofSize: cachedFontSize, weight: cachedFontWeight)
        textField?.font = font
        textView?.font = font
    }

    private func applyTextAlignment() {
        textField?.textAlignment = cachedTextAlignment
        textView?.textAlignment = cachedTextAlignment
    }

    private func applyBehaviour() {
        if let tf = textField {
            tf.isSecureTextEntry = cachedSecure
            tf.isEnabled = cachedEditable
            tf.keyboardType = cachedKeyboardType
            tf.returnKeyType = cachedReturnKeyType
            tf.autocapitalizationType = cachedAutoCapitalize
            tf.autocorrectionType = cachedAutocorrect
            tf.spellCheckingType = cachedSpellCheck
        }
        if let tv = textView {
            tv.isEditable = cachedEditable
            tv.keyboardType = cachedKeyboardType
            tv.returnKeyType = cachedReturnKeyType
            tv.autocapitalizationType = cachedAutoCapitalize
            tv.autocorrectionType = cachedAutocorrect
            tv.spellCheckingType = cachedSpellCheck
            // UITextView has no `isSecureTextEntry`; `secure` is a no-op
            // for multiline.
        }
        if cachedAutoFocus {
            // `becomeFirstResponder` does nothing until the view is in a
            // window, which it isn't yet on this pass.
            DispatchQueue.main.async { [weak self] in
                self?.textField?.becomeFirstResponder()
                self?.textView?.becomeFirstResponder()
            }
        }
    }

    // MARK: - Public setters (called by InputModule's Prop closures)

    // ---- Value ----------------------------------------------------------

    /// External value write (from the Rust signal).
    public func setValue(_ s: String) {
        cachedText = s
        applyText(s)
    }

    // ---- Placeholder ----------------------------------------------------

    public func setPlaceholder(_ s: String) {
        cachedPlaceholder = s
        applyPlaceholder()
    }

    public func setPlaceholderColor(_ value: WhiskerValue) {
        cachedPlaceholderColor = Self.resolveColor(value) ?? UIColor(white: 0.6, alpha: 1)
        applyPlaceholder()
    }

    // ---- Cursor / selection colours ------------------------------------

    public func setCaretColor(_ value: WhiskerValue) {
        cachedCaretColor = Self.resolveColor(value) ?? .systemBlue
        applyColors()
    }

    public func setSelectionColor(_ value: WhiskerValue) {
        // Empty string / null means unset — fall back to the caret color.
        if case .string(let s) = value, s.isEmpty {
            cachedSelectionColor = nil
        } else if case .null = value {
            cachedSelectionColor = nil
        } else {
            cachedSelectionColor = Self.resolveColor(value)
        }
        applyColors()
    }

    // ---- Layout mode ---------------------------------------------------

    public func setMultiline(_ multiline: Bool) {
        ensureControl(multiline: multiline)
    }

    public func setLines(_ lines: Int64) {
        // Deliberately inert: `UITextView` has no visible-line-count API,
        // and CSS `height` / `min-height` is the authoritative sizing.
        _ = lines
    }

    // ---- Input behaviour -----------------------------------------------

    public func setSecure(_ secure: Bool) {
        cachedSecure = secure
        textField?.isSecureTextEntry = cachedSecure
        // UITextView doesn't support secure entry.
    }

    public func setEditable(_ editable: Bool) {
        cachedEditable = editable
        textField?.isEnabled = cachedEditable
        textView?.isEditable = cachedEditable
    }

    public func setAutoFocus(_ autoFocus: Bool) {
        cachedAutoFocus = autoFocus
        if cachedAutoFocus && controlBuilt {
            DispatchQueue.main.async { [weak self] in
                self?.textField?.becomeFirstResponder()
                self?.textView?.becomeFirstResponder()
            }
        }
    }

    public func setMaxLength(_ count: Int64) {
        cachedMaxLength = max(0, Int(clamping: count)) // enforced in delegate callbacks
    }

    // ---- Keyboard / return key -----------------------------------------

    public func setKeyboardType(_ s: String) {
        cachedKeyboardType = Self.mapKeyboardType(s)
        textField?.keyboardType = cachedKeyboardType
        textView?.keyboardType = cachedKeyboardType
    }

    public func setReturnKey(_ s: String) {
        cachedReturnKeyType = Self.mapReturnKeyType(s)
        textField?.returnKeyType = cachedReturnKeyType
        textView?.returnKeyType = cachedReturnKeyType
    }

    public func setAutoCapitalize(_ s: String) {
        cachedAutoCapitalize = Self.mapAutoCapitalize(s)
        textField?.autocapitalizationType = cachedAutoCapitalize
        textView?.autocapitalizationType = cachedAutoCapitalize
        // A keyboard trait changed while the keyboard is up only takes
        // effect on the next presentation unless the input views reload.
        if textField?.isFirstResponder == true { textField?.reloadInputViews() }
        if textView?.isFirstResponder == true { textView?.reloadInputViews() }
    }

    public func setAutocorrect(_ enabled: Bool) {
        cachedAutocorrect = enabled ? .default : .no
        textField?.autocorrectionType = cachedAutocorrect
        textView?.autocorrectionType = cachedAutocorrect
        if textField?.isFirstResponder == true { textField?.reloadInputViews() }
        if textView?.isFirstResponder == true { textView?.reloadInputViews() }
    }

    public func setSpellCheck(_ enabled: Bool) {
        cachedSpellCheck = enabled ? .default : .no
        textField?.spellCheckingType = cachedSpellCheck
        textView?.spellCheckingType = cachedSpellCheck
        if textField?.isFirstResponder == true { textField?.reloadInputViews() }
        if textView?.isFirstResponder == true { textView?.reloadInputViews() }
    }

    // ---- Resolved text style ------------------------------------------

    public func applyTextStyle(_ style: WhiskerTextStyle) {
        cachedTextColor = style.color
        cachedFontSize = style.fontSize
        cachedFontWeight = Self.mapFontWeight(style.fontWeight)
        cachedTextAlignment = switch style.alignment {
        case .left: .left
        case .right: .right
        case .center: .center
        case .start, .end: .natural
        }
        applyColors()
        applyFont()
        applyTextAlignment()
    }

    // MARK: - Imperative handle targets (called by InputModule's Command handlers)

    /// Focus the field and raise the keyboard.
    public func focusField() {
        textField?.becomeFirstResponder()
        textView?.becomeFirstResponder()
    }

    /// Resign focus and dismiss the keyboard.
    public func blurField() {
        textField?.resignFirstResponder()
        textView?.resignFirstResponder()
    }

    /// Clear the text and emit `input`, so the bound signal ends up where
    /// it would be had the user deleted every character.
    public func clearField() {
        applyText("")
        cachedText = ""
        emitInput("")
    }

    // MARK: - Event emission helpers

    /// The event params — `{ "value": "<text>" }`.
    ///
    /// IMPORTANT: do NOT wrap this in a `detail` key. Host event encoding
    /// places the dispatched `params` under `detail` in the event body, and
    /// the Rust `InputEvent { detail: {
    /// value } }` reads `body.detail`. Wrapping here double-nests, and
    /// every `on_input` / `on_change` / `on_submit` delivers an empty
    /// string.
    private func detailPayload(_ text: String) -> [AnyHashable: Any] {
        return ["value": text]
    }

    // The emitters below dispatch SYNCHRONOUSLY, which is only safe because
    // the Rust renderer is re-entrancy-safe: renderer methods take `&self`
    // and keep mutable state behind narrowly scoped interior mutability.
    // That matters because UIKit delegate callbacks can fire during native
    // teardown on a hot-reload remount, while `remove_child`
    // is still on the Rust stack — a re-entrant `dispatch_event` is granted
    // rather than aborting on "RefCell already borrowed". Deferring a
    // runloop tick instead would cost every event a tick of latency.

    private func emitInput(_ text: String) {
        // Update owned state before dispatch so controlled props observe it.
        cachedText = text
        WhiskerCustomEvent.dispatch(from: self, name: "input", params: detailPayload(text))
    }

    private func emitChange(_ text: String) {
        WhiskerCustomEvent.dispatch(from: self, name: "change", params: detailPayload(text))
    }

    private func emitFocus() {
        WhiskerCustomEvent.dispatch(from: self, name: "focus", params: [:])
    }

    private func emitBlur() {
        WhiskerCustomEvent.dispatch(from: self, name: "blur", params: [:])
    }

    private func emitSubmit(_ text: String) {
        WhiskerCustomEvent.dispatch(from: self, name: "submit", params: detailPayload(text))
    }

    // MARK: - UITextField action targets

    @objc private func textFieldDidChange(_ sender: UITextField) {
        let text = sender.text ?? ""
        emitInput(text)
    }

    @objc private func textFieldDidEndOnExit(_ sender: UITextField) {
        let text = sender.text ?? ""
        emitSubmit(text)
        emitChange(text)
    }

    // MARK: - Mapping helpers

    private static func mapKeyboardType(_ s: String) -> UIKeyboardType {
        switch s {
        case "number":  return .numberPad
        case "decimal": return .decimalPad
        case "email":   return .emailAddress
        case "phone":   return .phonePad
        case "url":     return .URL
        default:        return .default
        }
    }

    private static func mapReturnKeyType(_ s: String) -> UIReturnKeyType {
        switch s {
        case "done":   return .done
        case "go":     return .go
        case "next":   return .next
        case "search": return .search
        case "send":   return .send
        default:       return .default
        }
    }

    private static func mapAutoCapitalize(_ s: String) -> UITextAutocapitalizationType {
        switch s {
        case "none":       return .none
        case "words":      return .words
        case "characters": return .allCharacters
        default:           return .sentences
        }
    }

    private static func mapFontWeight(_ value: Int) -> UIFont.Weight {
        switch value {
        case ...150: .ultraLight
        case ...250: .thin
        case ...350: .light
        case ...450: .regular
        case ...550: .medium
        case ...650: .semibold
        case ...750: .bold
        case ...850: .heavy
        default: .black
        }
    }

    /// Resolve a module color property from a packed ARGB value or CSS string.
    private static func resolveColor(_ value: WhiskerValue) -> UIColor? {
        if case .string(let s) = value {
            return parseCssColor(s)
        }
        // A literal 0 is fully-transparent black — a legitimate value, not
        // "unset".
        if let argb = value.asInt {
            return colorFromARGB(UInt32(truncatingIfNeeded: argb))
        }
        return nil
    }

    /// Build a `UIColor` from Whisker's `0xAARRGGBB` packed integer.
    private static func colorFromARGB(_ argb: UInt32) -> UIColor {
        let a = CGFloat((argb >> 24) & 0xFF) / 255
        let r = CGFloat((argb >> 16) & 0xFF) / 255
        let g = CGFloat((argb >>  8) & 0xFF) / 255
        let b = CGFloat(argb & 0xFF) / 255
        return UIColor(red: r, green: g, blue: b, alpha: a)
    }
}

// MARK: - UITextFieldDelegate

extension WhiskerInputView: UITextFieldDelegate {

    public func textFieldDidBeginEditing(_ textField: UITextField) {
        emitFocus()
    }

    public func textFieldDidEndEditing(_ textField: UITextField) {
        let text = textField.text ?? ""
        emitBlur()
        emitChange(text)
    }

    /// Enforce `max-length` on UITextField.
    public func textField(
        _ textField: UITextField,
        shouldChangeCharactersIn range: NSRange,
        replacementString string: String
    ) -> Bool {
        guard cachedMaxLength > 0 else { return true }
        let current = (textField.text ?? "") as NSString
        let proposed = current.replacingCharacters(in: range, with: string)
        return proposed.count <= cachedMaxLength
    }
}

// MARK: - UITextViewDelegate

extension WhiskerInputView: UITextViewDelegate {

    public func textViewDidBeginEditing(_ textView: UITextView) {
        emitFocus()
    }

    public func textViewDidEndEditing(_ textView: UITextView) {
        let text = textView.text ?? ""
        emitBlur()
        emitChange(text)
    }

    public func textViewDidChange(_ textView: UITextView) {
        let text = textView.text ?? ""
        emitInput(text)
    }

    /// Enforce `max-length` on UITextView.
    ///
    /// Return must NOT be intercepted here: in a multiline area it is a
    /// normal newline, and `submit` is a single-line-only concept handled
    /// by the UITextField `editingDidEndOnExit` path. `max-length` still
    /// counts a newline as a character, so it is rejected past the limit
    /// like any other.
    public func textView(
        _ textView: UITextView,
        shouldChangeTextIn range: NSRange,
        replacementText text: String
    ) -> Bool {
        guard cachedMaxLength > 0 else { return true }
        let current = (textView.text ?? "") as NSString
        let proposed = current.replacingCharacters(in: range, with: text)
        return proposed.count <= cachedMaxLength
    }
}

// MARK: - CSS colour parser

/// Best-effort CSS colour parser. Handles `#RGB`, `#RRGGBB`,
/// `#RRGGBBAA`, `rgb(r, g, b)`, `rgba(r, g, b, a)`, and a handful of
/// named colours. Returns `nil` on parse failure so callers can fall
/// back to their cached default.
private func parseCssColor(_ raw: String) -> UIColor? {
    let s = raw.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !s.isEmpty else { return nil }

    // ---- Hex -----------------------------------------------------------
    if s.hasPrefix("#") {
        let hex = String(s.dropFirst())
        switch hex.count {
        case 3:
            if let n = UInt32(hex, radix: 16) {
                let r = (n >> 8) & 0xF
                let g = (n >> 4) & 0xF
                let b = n & 0xF
                return UIColor(
                    red:   CGFloat(r * 17) / 255,
                    green: CGFloat(g * 17) / 255,
                    blue:  CGFloat(b * 17) / 255,
                    alpha: 1
                )
            }
        case 6:
            if let n = UInt32(hex, radix: 16) {
                return UIColor(
                    red:   CGFloat((n >> 16) & 0xFF) / 255,
                    green: CGFloat((n >>  8) & 0xFF) / 255,
                    blue:  CGFloat(n & 0xFF) / 255,
                    alpha: 1
                )
            }
        case 8:
            if let n = UInt32(hex, radix: 16) {
                return UIColor(
                    red:   CGFloat((n >> 24) & 0xFF) / 255,
                    green: CGFloat((n >> 16) & 0xFF) / 255,
                    blue:  CGFloat((n >>  8) & 0xFF) / 255,
                    alpha: CGFloat(n & 0xFF) / 255
                )
            }
        default: break
        }
        return nil
    }

    // ---- rgb() / rgba() ------------------------------------------------
    let lower = s.lowercased()
    if lower.hasPrefix("rgb") {
        guard let open = s.firstIndex(of: "("),
              let close = s.lastIndex(of: ")") else { return nil }
        let inner = String(s[s.index(after: open)..<close])
        let parts = inner.split(separator: ",").map {
            $0.trimmingCharacters(in: .whitespaces)
        }
        guard parts.count >= 3,
              let r = Double(parts[0]),
              let g = Double(parts[1]),
              let b = Double(parts[2]) else { return nil }
        let a = parts.count >= 4 ? (Double(parts[3]) ?? 1.0) : 1.0
        return UIColor(
            red:   CGFloat(r) / 255,
            green: CGFloat(g) / 255,
            blue:  CGFloat(b) / 255,
            alpha: CGFloat(a)
        )
    }

    // ---- Named colours -------------------------------------------------
    switch lower {
    case "black":       return .black
    case "white":       return .white
    case "red":         return .red
    case "green":       return UIColor(red: 0, green: 128.0/255, blue: 0, alpha: 1)
    case "blue":        return .blue
    case "gray", "grey":return .gray
    case "transparent": return .clear
    default:            return nil
    }
}

// MARK: - Padded single-line field

/// `UITextField` that insets its text, editing, and placeholder rects by
/// `textInsets`. Plain `UITextField` has no built-in content inset, so
/// overriding the three rect hooks is the only way to honor CSS padding on
/// a single-line field — the multiline `UITextView` gets the same effect
/// for free via `textContainerInset`.
///
/// `clearButtonRect` is deliberately left at the default so a clear button
/// would still track the right edge.
private final class PaddedTextField: UITextField {

    var textInsets: UIEdgeInsets = .zero {
        didSet {
            guard textInsets != oldValue else { return }
            setNeedsLayout()
        }
    }

    override func textRect(forBounds bounds: CGRect) -> CGRect {
        return bounds.inset(by: textInsets)
    }

    override func editingRect(forBounds bounds: CGRect) -> CGRect {
        return bounds.inset(by: textInsets)
    }

    override func placeholderRect(forBounds bounds: CGRect) -> CGRect {
        return bounds.inset(by: textInsets)
    }
}
