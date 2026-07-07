// Lynx UI subclass hosting a UITextField (single-line) or UITextView
// (multiline) behind a unified interface. Registration is driven by
// `InputModule`'s `definition()` — no annotations required here.
//
// `@objc(WhiskerInputView)` pins the Obj-C class name so the codegen
// plugin's `NSClassFromString` lookup can find it regardless of whether
// the SwiftPM-target prefix (`whisker_input.WhiskerInputView`) or the
// bare form is used.
//
// ## Single-line vs multiline
//
// The backing control is chosen lazily. `WhiskerUI<UIView>` hosts a
// transparent `containerView` that holds either a `UITextField` or a
// `UITextView` pinned to its bounds. The `multiline` prop (default
// false) triggers a rebuild of the hosted control the first time it
// changes; subsequent changes to `multiline` after the control is
// built are treated as no-ops (matching native-input semantics: the
// field type is set once at construction time). Consumers that need to
// switch between single/multiline should remount the component.
//
// ## Event body shape
//
// All value-carrying events use `{ "detail": { "value": "<text>" } }`
// so the Rust `InputEvent` struct's `#[serde(default)]` decode sees
// the expected shape. Focus / blur emit `{ "detail": {} }`.
//
// ## Cursor-preservation diff
//
// `setValue(_:)` only calls `field.text = ...` / `textView.text = ...`
// when the incoming string differs from what the control currently
// displays. This avoids a cursor-jump on the two-way-binding round
// trip (Rust sets `value`, view fires `input`, Rust sets `value` again
// with the same text the view just reported — without the guard this
// causes the insertion point to jump to the end on every keystroke).
//
// ## CSS text-style props
//
// `color`, `font-size`, `font-weight`, `text-align` are received as
// `Prop(...)` callbacks from the `InputModule` definition. They are
// applied immediately to whichever control is currently active and
// cached so the next control swap re-applies them.

import Foundation
import UIKit
import WhiskerModule

@objc(WhiskerInputView)
/// A container `UIView` that invokes `onDetach` when it leaves its
/// window (a real unmount). Used by [`WhiskerInputView`] to resign the
/// field's first responder on teardown so a removed input never lingers
/// as the keyboard target.
private final class DetachAwareView: UIView {
    var onDetach: (() -> Void)?

    override func willMove(toWindow newWindow: UIWindow?) {
        super.willMove(toWindow: newWindow)
        // `nil` newWindow = the view is being removed from the hierarchy.
        // (A single↔multiline control swap replaces the *inner* control,
        // not this container, so it doesn't spuriously trigger here.)
        if newWindow == nil {
            onDetach?()
        }
    }
}

public final class WhiskerInputView: WhiskerUI<UIView> {

    // MARK: - Hosted controls

    /// Transparent container that fills the LynxUI frame; holds
    /// either the `textField` or `textView` as a subview.
    ///
    /// A [`DetachAwareView`] so it can resign focus on unmount. UIKit
    /// already auto-resigns a first responder that's removed from its
    /// window, but doing it explicitly also dismisses the IME promptly
    /// and keeps parity with the Android `onViewDetachedFromWindow`
    /// hook. Navigation-driven dismissal is handled up front by
    /// whisker-router; this covers non-navigation unmounts (a
    /// conditionally-rendered field removed while focused).
    private lazy var containerView: UIView = {
        let v = DetachAwareView()
        v.backgroundColor = .clear
        v.onDetach = { [weak self] in self?.blurField() }
        return v
    }()

    /// The live single-line field, when the current mode is single-line.
    /// Mutually exclusive with `textView` — exactly one is non-nil once a
    /// control has been built.
    private var textField: UITextField?

    /// The live multiline area, when the current mode is multiline.
    /// Mutually exclusive with `textField`.
    private var textView: UITextView?

    /// Whether the view is currently in multiline mode. Determines which
    /// control is hosted in `containerView`. `setMultiline` rebuilds the
    /// control whenever the requested mode differs from this.
    private var isMultiline: Bool = false

    // MARK: - Cached prop state
    //
    // All mutable props are cached so they can be re-applied whenever the
    // hosted control is (re)built — both on the initial build and when
    // `setMultiline` switches between single-line and multiline. Props can
    // arrive in any order relative to `multiline`, so we cache everything
    // and `applyAllCachedProps()` reinstates the full state onto the new
    // control after a switch.

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
    // Default `.sentences` matches UIKit's own UITextField/UITextView
    // default, so a field that never sets `auto-capitalize` behaves
    // exactly as before this prop existed.
    private var cachedAutoCapitalize: UITextAutocapitalizationType = .sentences
    // `.default` (not `.yes`) for the enabled case so UIKit keeps its
    // contextual behaviour — e.g. it already disables autocorrect on URL
    // / email keyboards. `false` forces `.no`.
    private var cachedAutocorrect: UITextAutocorrectionType = .default
    private var cachedSpellCheck: UITextSpellCheckingType = .default
    private var cachedTextColor: UIColor = .label
    private var cachedFontSize: CGFloat = 17
    private var cachedFontWeight: UIFont.Weight = .regular
    private var cachedTextAlignment: NSTextAlignment = .natural

    /// Computed CSS padding (left/top/right/bottom) read from the base
    /// `LynxUI.padding`. Insets the text inside both controls so the text
    /// doesn't sit flush against the element edges. Defaults to `.zero`
    /// (flush) so a field with no CSS padding matches Android's 0 default.
    private var cachedPadding: UIEdgeInsets = .zero

    /// True once a control (UITextField or UITextView) has been built.
    /// Distinguishes "no control yet" (initial build path) from "control
    /// exists, possibly needs a mode switch" in `ensureControl`.
    private var controlBuilt: Bool = false

    // MARK: - LynxUI lifecycle

    @objc public override func createView() -> UIView {
        // The host container is created once; the inner control
        // (`UITextField` or `UITextView`) is built lazily in
        // `ensureControl()`. Default to single-line so the very first
        // render shows a working text field even if `multiline` hasn't
        // arrived yet.
        ensureControl(multiline: false)
        return containerView
    }

    @objc public override func frameDidChange() {
        super.frameDidChange()
        // `self.view()` is the `containerView` — Lynx already set its
        // frame to the computed element bounds. Propagate that frame to
        // the hosted UITextField / UITextView so they fill the element.
        let bounds = self.view().bounds
        textField?.frame = bounds
        textView?.frame = bounds
        // Padding is resolved during layout, so this hook (which fires
        // after layout) is the authoritative point to read it.
        syncPadding()
    }

    /// Fired by Lynx after a batch of prop / style updates has been
    /// applied. We use it as a belt-and-braces fallback for `font-size`:
    /// the base `LynxUI` exposes the computed `fontSize` (a resolved
    /// point value) for ANY element, so even if the `font-size` prop
    /// dispatch didn't reach our setter, we still pick up the cascaded
    /// value here. `color` / `font-weight` / `text-align` have no
    /// base-class computed accessor, so those rely on the `Prop` setters.
    @objc public override func propsDidUpdate() {
        super.propsDidUpdate()
        let computed = self.fontSize
        if computed > 0 && abs(computed - cachedFontSize) > 0.01 {
            cachedFontSize = computed
            applyFont()
        }
        // `padding` may be resolved by now on a props-only update too.
        syncPadding()
    }

    /// Read the base `LynxUI.padding` (computed CSS padding — shorthand,
    /// units, and per-side longhands all already resolved to point insets
    /// by Lynx's layout) and apply it to the live control when it changed.
    /// This is the single source of truth for the field's text inset.
    private func syncPadding() {
        let p = self.padding
        if p != cachedPadding {
            cachedPadding = p
            applyPadding()
        }
    }

    // MARK: - Control builder

    /// Build the hosted control for `multiline`, or switch to it if a
    /// control of the other mode is already live.
    ///
    /// `createView()` calls this once with the default single-line mode
    /// so the element always shows a working field. `setMultiline(...)`
    /// then calls it again once the real `multiline` value arrives (props
    /// are applied after `createView`); if that differs from the current
    /// mode we tear down the old control and rebuild as the other kind,
    /// re-applying all cached props so no state (text / colours / font /
    /// alignment / behaviour) is lost across the switch.
    private func ensureControl(multiline: Bool) {
        // Already in the requested mode → nothing to do.
        if controlBuilt && isMultiline == multiline { return }

        // Tear down the existing control (if any) before building the new
        // one. Removing from the superview drops UIKit's strong ref; we
        // also nil our own refs and clear the delegate / targets so a
        // stale control can't deliver events after the switch.
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

        // Pin the freshly-built control to the container's current bounds
        // (frameDidChange may have already fired before this switch).
        let bounds = containerView.bounds
        textField?.frame = bounds
        textView?.frame = bounds

        applyAllCachedProps()
    }

    private func buildTextField() {
        // `PaddedTextField` insets text / placeholder by `textInsets`;
        // plain UITextField has no built-in inset, so a subclass is the
        // only way to honor CSS padding on a single-line field.
        let tf = PaddedTextField()
        tf.borderStyle = .none
        tf.backgroundColor = .clear
        // `autocorrectionType` / `spellCheckingType` are applied from the
        // cached prop state in `applyBehaviour` (called via
        // `applyAllCachedProps` right after this build).
        tf.addTarget(self, action: #selector(textFieldDidChange(_:)), for: .editingChanged)
        tf.addTarget(self, action: #selector(textFieldDidEndOnExit(_:)), for: .editingDidEndOnExit)
        tf.delegate = self
        containerView.addSubview(tf)
        textField = tf
    }

    private func buildTextView() {
        let tv = UITextView()
        tv.backgroundColor = .clear
        // Start flush; `applyPadding()` sets the real CSS-padding inset.
        // `lineFragmentPadding = 0` so horizontal padding equals exactly
        // the CSS value (otherwise the container adds its own padding).
        tv.textContainerInset = .zero
        tv.textContainer.lineFragmentPadding = 0
        // Top-align the text. A UITextView vertically centers its content
        // whenever the content height is shorter than the bounds UNLESS
        // scrolling is enabled (scroll-enabled views pin content to the
        // top). Force scroll on, and disable the system content-inset
        // adjustment so the safe-area / nav-bar doesn't push the first
        // line down off the top edge.
        tv.isScrollEnabled = true
        tv.contentInsetAdjustmentBehavior = .never
        // Default the keyboard's return key to insert a newline (the
        // multiline contract); `setReturnKey` may override the label.
        tv.returnKeyType = .default
        tv.delegate = self
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
        applyPadding()
    }

    /// Inset the text by the computed CSS padding on whichever control is
    /// live. UITextField routes through the `PaddedTextField` subclass's
    /// `textInsets`; UITextView uses `textContainerInset` directly (with
    /// `lineFragmentPadding = 0` already set so horizontal padding is
    /// exact, not padding + the container's default fragment padding).
    private func applyPadding() {
        if let tf = textField as? PaddedTextField {
            tf.textInsets = cachedPadding
        }
        if let tv = textView {
            tv.textContainerInset = cachedPadding
        }
    }

    /// Apply current text to whichever control is active, without
    /// cursor-jump (no-op when text hasn't changed).
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
        // UITextView has no native placeholder; skip for v1
        // (a label-overlay approach would be the follow-up).
    }

    private func applyColors() {
        // iOS uses `tintColor` for both the cursor and selection highlight
        // (UITextField / UITextView do not expose independent
        // selection-color control via a public API). When `selection-color`
        // is set it "wins" for that combined tint; otherwise fall back to
        // `caret-color`. Both paths leave `caret-color` as the effective
        // cursor color — best-effort, single tint shared across cursor and
        // selection on this platform.
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
            // UITextView has no `isSecureTextEntry`; secure flag is a no-op
            // for multiline (passwords are always single-line in practice).
        }
        if cachedAutoFocus {
            // Defer to the next run-loop so the view has been attached to
            // the window before we try to become first responder.
            DispatchQueue.main.async { [weak self] in
                self?.textField?.becomeFirstResponder()
                self?.textView?.becomeFirstResponder()
            }
        }
    }

    // MARK: - Public setters (called by InputModule's Prop closures)

    // ---- Value ----------------------------------------------------------

    /// External value write (from Rust signal). Only re-sets the field's
    /// displayed text when it differs from the current value — this is
    /// the cursor-preservation diff guard described in the class comment.
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
        // Empty string / null → unset → fall back to caret color.
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

    public func setMultiline(_ s: String) {
        let want = (s == "true")
        // `multiline` is always sent by the Rust outer component, and it
        // arrives AFTER `createView()` has built the default single-line
        // field. `ensureControl` switches the control to a UITextView when
        // `want` is true (and back to a UITextField if it ever flips),
        // re-applying all cached props so the field lands in the right mode
        // with its full state intact.
        ensureControl(multiline: want)
    }

    public func setLines(_ s: String) {
        // `lines` is a best-effort visible-line hint for multiline areas.
        // CSS `height` / `min-height` from the Rust style prop is
        // authoritative in v1; we store this for potential future use.
        // (`UITextView` doesn't expose a "visible line count" API.)
        _ = Int(s) ?? 0
    }

    // ---- Input behaviour -----------------------------------------------

    public func setSecure(_ s: String) {
        cachedSecure = (s == "true")
        textField?.isSecureTextEntry = cachedSecure
        // UITextView doesn't support secure entry.
    }

    public func setEditable(_ s: String) {
        cachedEditable = (s != "false")
        textField?.isEnabled = cachedEditable
        textView?.isEditable = cachedEditable
    }

    public func setAutoFocus(_ s: String) {
        cachedAutoFocus = (s == "true")
        if cachedAutoFocus && controlBuilt {
            DispatchQueue.main.async { [weak self] in
                self?.textField?.becomeFirstResponder()
                self?.textView?.becomeFirstResponder()
            }
        }
    }

    public func setMaxLength(_ s: String) {
        cachedMaxLength = Int(s) ?? 0   // enforced in delegate callbacks
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
        // Changing the autocapitalization type while the keyboard is up
        // only takes effect on the next keyboard presentation; UIKit
        // exposes `reloadInputViews()` to apply it live.
        if textField?.isFirstResponder == true { textField?.reloadInputViews() }
        if textView?.isFirstResponder == true { textView?.reloadInputViews() }
    }

    public func setAutocorrect(_ s: String) {
        // `.default` (not `.yes`) for the enabled case — see the cached
        // property's declaration.
        cachedAutocorrect = (s == "false") ? .no : .default
        textField?.autocorrectionType = cachedAutocorrect
        textView?.autocorrectionType = cachedAutocorrect
        if textField?.isFirstResponder == true { textField?.reloadInputViews() }
        if textView?.isFirstResponder == true { textView?.reloadInputViews() }
    }

    public func setSpellCheck(_ s: String) {
        cachedSpellCheck = (s == "false") ? .no : .default
        textField?.spellCheckingType = cachedSpellCheck
        textView?.spellCheckingType = cachedSpellCheck
        if textField?.isFirstResponder == true { textField?.reloadInputViews() }
        if textView?.isFirstResponder == true { textView?.reloadInputViews() }
    }

    // ---- CSS text-style props ------------------------------------------
    //
    // These arrive from Lynx's CSS cascade ALREADY PARSED (not as CSS
    // strings): `color` is an ARGB int, `font-size` a resolved point
    // value, `font-weight` a `LynxFontWeightType` enum int, `text-align`
    // a `LynxTextAlignType` enum int. Each setter decodes the numeric
    // form first and falls back to string parsing so it still works if
    // the value is ever delivered as a plain-string attribute.

    public func setTextColor(_ value: WhiskerValue) {
        cachedTextColor = Self.resolveColor(value) ?? .label
        applyColors()
    }

    public func setFontSize(_ value: WhiskerValue) {
        // Numeric form (Lynx cascade): a resolved point size — use as-is.
        if let n = value.asDouble, n > 0 {
            cachedFontSize = CGFloat(n)
            applyFont()
            return
        }
        // String form (plain attr): "16px", "16", "1.5em" → strip a
        // trailing `px` and read the leading number. Unknown units leave
        // the cached size unchanged rather than regressing the font.
        if let s = value.asString {
            let stripped = s.hasSuffix("px") ? String(s.dropLast(2)) : s
            if let pt = Double(stripped.trimmingCharacters(in: .whitespaces)), pt > 0 {
                cachedFontSize = CGFloat(pt)
                applyFont()
            }
        }
    }

    public func setFontWeight(_ value: WhiskerValue) {
        // Numeric form: `LynxFontWeightType` enum (Normal=0, Bold=1,
        // 100=2 … 900=10). String form: a CSS keyword / numeric weight.
        if let i = value.asInt {
            cachedFontWeight = Self.mapLynxFontWeightEnum(Int(i))
        } else if let s = value.asString {
            cachedFontWeight = Self.mapFontWeight(s)
        }
        applyFont()
    }

    public func setTextAlign(_ value: WhiskerValue) {
        // Numeric form: `LynxTextAlignType` enum (Left=0, Center=1,
        // Right=2, Start=3, End=4, Justify=5). String form: CSS keyword.
        if let i = value.asInt {
            cachedTextAlignment = Self.mapLynxTextAlignEnum(Int(i))
        } else if let s = value.asString {
            cachedTextAlignment = Self.mapTextAlignment(s)
        }
        applyTextAlignment()
    }

    // MARK: - Imperative handle targets (called by InputModule's Function closures)

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

    /// Clear the text to empty AND emit `input` so the bound signal
    /// updates (the same outcome as the user deleting all characters).
    public func clearField() {
        applyText("")
        cachedText = ""
        emitInput("")
    }

    /// Synchronous read of the current text for `getValue`.
    public func currentText() -> String {
        return cachedText
    }

    // MARK: - Event emission helpers

    /// The event params — `{ "value": "<text>" }`.
    ///
    /// IMPORTANT: do NOT wrap this in a `detail` key. Lynx's
    /// `generateEventBody` (iOS) / the Android event reporter already
    /// places the dispatched `params` UNDER a `detail` key in the event
    /// body (`{ type, target, currentTarget, detail: <params> }`). The
    /// Rust `InputEvent { detail: { value } }` then reads `body.detail`.
    /// Wrapping here too would double-nest (`detail: { detail: { value }}`)
    /// and the value would never reach the handler — every `on_input` /
    /// `on_change` / `on_submit` would deliver an empty string.
    private func detailPayload(_ text: String) -> [AnyHashable: Any] {
        return ["value": text]
    }

    // These dispatch SYNCHRONOUSLY. UIKit delegate callbacks
    // (`textFieldDidEndEditing`, `textViewDidChange`, …) can fire during
    // Lynx's native teardown on a hot-reload remount, *while*
    // `remove_child` is on the stack inside Rust's renderer. Previously
    // that re-entered `dispatch_event` → a second `with_renderer` borrow
    // → "RefCell already borrowed" panic, so we deferred a runloop tick
    // (`DispatchQueue.main.async`) to dodge it — which is exactly the
    // one-tick-late delivery of whisker #3.
    //
    // The Rust renderer is now re-entrancy-safe: `DynRenderer` methods
    // take `&self`, `BridgeRenderer` holds its state behind per-field
    // `RefCell`s with FFI-scoped borrows, and `with_renderer` takes a
    // SHARED borrow — so a synchronous re-entrant `dispatch_event` during
    // teardown is granted instead of aborting. See
    // `crates/whisker-runtime/src/view/renderer.rs` (`with_renderer`) and
    // `crates/whisker-driver/src/lynx/renderer.rs` (BridgeRenderer). With
    // that fix the deferral is no longer needed, and removing it collapses
    // the one-tick delay: `on_input` / `on_change` / `on_submit` now
    // deliver on the same tick the user interacts.

    private func emitInput(_ text: String) {
        // `cachedText` is owned state — keep it correct before dispatch so
        // `getValue` / the cursor-preservation diff stay consistent.
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
        // "Done" / return key on a UITextField. Emit both submit and change.
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

    private static func mapFontWeight(_ s: String) -> UIFont.Weight {
        switch s.lowercased() {
        case "100", "thin":        return .ultraLight
        case "200", "extralight":  return .thin
        case "300", "light":       return .light
        case "400", "normal":      return .regular
        case "500", "medium":      return .medium
        case "600", "semibold":    return .semibold
        case "700", "bold":        return .bold
        case "800", "extrabold":   return .heavy
        case "900", "black":       return .black
        default:                   return .regular
        }
    }

    private static func mapTextAlignment(_ s: String) -> NSTextAlignment {
        switch s.lowercased() {
        case "left":    return .left
        case "right":   return .right
        case "center":  return .center
        case "justify": return .justified
        default:        return .natural
        }
    }

    /// `LynxTextAlignType` enum → `NSTextAlignment`. Values per
    /// `LynxCSSType.h`: Left=0, Center=1, Right=2, Start=3, End=4,
    /// Justify=5. Start/End map to `.natural` (UIKit resolves per the
    /// writing direction).
    private static func mapLynxTextAlignEnum(_ i: Int) -> NSTextAlignment {
        switch i {
        case 0:  return .left
        case 1:  return .center
        case 2:  return .right
        case 5:  return .justified
        default: return .natural   // Start(3) / End(4) / unknown
        }
    }

    /// Numeric `font-weight` → `UIFont.Weight`. Lynx normally delivers
    /// the `LynxFontWeightType` enum (Normal=0, Bold=1, 100=2 … 900=10,
    /// per `LynxAutoGenCSSType.h`). As a belt-and-braces guard we also
    /// accept a raw CSS weight number (100…900) since those ranges don't
    /// overlap the 0…10 enum range — whichever form arrives resolves
    /// correctly.
    private static func mapLynxFontWeightEnum(_ i: Int) -> UIFont.Weight {
        switch i {
        // LynxFontWeightType enum indices.
        case 0:   return .regular     // Normal
        case 1:   return .bold        // Bold
        case 2:   return .ultraLight  // 100
        case 3:   return .thin        // 200
        case 4:   return .light       // 300
        case 5:   return .regular     // 400
        case 6:   return .medium      // 500
        case 7:   return .semibold    // 600
        case 8:   return .bold        // 700
        case 9:   return .heavy       // 800
        case 10:  return .black       // 900
        // Raw CSS numeric weights (in case Lynx forwards the literal).
        case 100: return .ultraLight
        case 200: return .thin
        case 300: return .light
        case 400: return .regular
        case 500: return .medium
        case 600: return .semibold
        case 700: return .bold
        case 800: return .heavy
        case 900: return .black
        default:  return .regular
        }
    }

    /// Resolve a colour prop value to a `UIColor`. Lynx's CSS cascade
    /// delivers a parsed colour as an ARGB integer (`0xAARRGGBB`,
    /// arriving as `.int` after the NSNumber → WhiskerValue conversion);
    /// a plain-string attribute delivers a CSS string. Returns `nil` on
    /// an unrecognised / empty value so callers keep their default.
    private static func resolveColor(_ value: WhiskerValue) -> UIColor? {
        // Numeric ARGB (Lynx cascade). `.int`/`.float` both coerce via
        // `asInt`. A literal 0 is fully-transparent black — a legitimate
        // value, so we DON'T treat it as "unset" here.
        if case .string(let s) = value {
            return parseCssColor(s)
        }
        if let argb = value.asInt {
            return colorFromARGB(UInt32(truncatingIfNeeded: argb))
        }
        return nil
    }

    /// Build a `UIColor` from a Lynx `0xAARRGGBB` packed integer.
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

    /// Enforce `max-length` on UITextField. The standard delegate
    /// intercept: return `false` when adding `string` would push
    /// the count over `cachedMaxLength`.
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
    /// Return is a normal newline in a multiline area — we do NOT
    /// intercept `\n` here. The field inserts the line break naturally,
    /// and `submit` is a single-line-only concept (handled by the
    /// UITextField `editingDidEndOnExit` path). Intercepting the newline
    /// (the previous behaviour) both suppressed the line break the user
    /// expected AND fired a spurious `submit` on every Return.
    ///
    /// `max-length` still counts newlines as characters, so a `\n` that
    /// would exceed the limit is rejected like any other character.
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
///
/// Mirrors `parseCssColor` from `whisker-svg/ios/Sources/WhiskerSvg/
/// WhiskerSvgView.swift`, extended with `rgb()`/`rgba()` support for
/// the colour-string inputs the Input view handles.
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
        // Grab the content inside the outer parens.
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
/// `textInsets`. Plain `UITextField` has no built-in content inset, so a
/// subclass overriding the three rect hooks is the only way to honor CSS
/// padding on a single-line field — the multiline `UITextView` gets the
/// same effect for free via `textContainerInset`.
///
/// `clearButtonRect` is intentionally left at the default so the clear
/// button (when present) still tracks the right edge sensibly; the field
/// doesn't enable a clear button today, so it's a non-issue in practice.
private final class PaddedTextField: UITextField {

    /// CSS padding (left/top/right/bottom). Setting it re-lays-out so the
    /// new inset takes effect immediately.
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
