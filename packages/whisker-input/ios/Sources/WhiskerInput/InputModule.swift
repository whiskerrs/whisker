// `whisker-input` ModuleDefinition (iOS).
//
// The codegen plugin discovers this `Module` subclass and emits a
// registration block in `WhiskerInput+Generated.swift` that registers
// `definitionLazy.view!.viewClass` with `LynxComponentRegistry` under
// "whisker-input:Input", then calls `module.registerWithLynx()` so every
// `Prop(...)` setter and `Function(...)` method installs via the
// Obj-C-runtime path.
//
// The `WhiskerInputView` Lynx UI subclass lives in `InputView.swift`;
// Android splits the same way (`InputModule.kt` + `WhiskerInputView.kt`).
//
// ## CSS text-style props
//
// `LynxUI` forwards generic view props (background-color, border-radius,
// opacity) on its own, but NOT text-style values — those never reach the
// backing UITextField / UITextView. So `color`, `font-size`,
// `font-weight`, and `text-align` are declared as explicit `Prop` entries
// here and forwarded to the view's setters.

import WhiskerModule

@WhiskerModule
public final class InputModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("Input")
            View(WhiskerInputView.self) {

                // ---- Value + placeholder ----------------------------------

                Prop("value") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setValue(value.asString ?? "")
                }
                Prop("placeholder") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setPlaceholder(value.asString ?? "")
                }
                // Colour props arrive as a parsed Lynx ARGB int via the CSS
                // cascade, or as a raw CSS string when set as a plain
                // attribute — so the whole `WhiskerValue` goes through and
                // `WhiskerInputView.resolveColor(_:)` picks the form.
                Prop("placeholder-color") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setPlaceholderColor(value)
                }

                // ---- Cursor / selection colours --------------------------

                Prop("caret-color") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setCaretColor(value)
                }
                Prop("selection-color") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setSelectionColor(value)
                }

                // ---- Layout mode -----------------------------------------

                Prop("multiline") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setMultiline(value.asString ?? "false")
                }
                Prop("lines") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setLines(value.asString ?? "0")
                }

                // ---- Input behaviour -------------------------------------

                Prop("secure") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setSecure(value.asString ?? "false")
                }
                Prop("editable") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setEditable(value.asString ?? "true")
                }
                Prop("auto-focus") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setAutoFocus(value.asString ?? "false")
                }
                Prop("max-length") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setMaxLength(value.asString ?? "0")
                }

                // ---- Keyboard / return key -------------------------------

                Prop("keyboard-type") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setKeyboardType(value.asString ?? "default")
                }
                Prop("return-key") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setReturnKey(value.asString ?? "default")
                }
                Prop("auto-capitalize") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setAutoCapitalize(value.asString ?? "sentences")
                }
                Prop("autocorrect") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setAutocorrect(value.asString ?? "true")
                }
                Prop("spell-check") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setSpellCheck(value.asString ?? "true")
                }

                // ---- CSS text-style props --------------------------------
                //
                // These do reach a custom UI's prop setters on iOS: Lynx
                // resolves the registered `set<Cap>:requestReset:` selectors
                // through the Obj-C runtime, the same channel the base
                // class's background/border setters use.
                //
                // CRITICAL: Lynx delivers ALREADY-PARSED values, not CSS
                // strings — `color` is an ARGB int, `font-size` a resolved
                // point CGFloat, `font-weight` a `LynxFontWeightType` enum
                // int, `text-align` a `LynxTextAlignType` enum int. The whole
                // `WhiskerValue` must be forwarded; an `asString ?? ""`
                // coercion here silently drops every numeric value.

                Prop("color") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setTextColor(value)
                }
                Prop("font-size") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setFontSize(value)
                }
                Prop("font-weight") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setFontWeight(value)
                }
                Prop("text-align") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setTextAlign(value)
                }

                // Declaration-only metadata for the codegen / docs scanner;
                // dispatch happens in `InputView.swift`.
                Events("input", "change", "focus", "blur", "submit")

                // ---- Imperative methods ----------------------------------

                Function("focus") { (view: WhiskerInputView, _: [WhiskerValue]) -> WhiskerValue in
                    view.focusField()
                    return .null
                }
                Function("blur") { (view: WhiskerInputView, _: [WhiskerValue]) -> WhiskerValue in
                    view.blurField()
                    return .null
                }
                Function("clear") { (view: WhiskerInputView, _: [WhiskerValue]) -> WhiskerValue in
                    view.clearField()
                    return .null
                }
                Function("setValue") { (view: WhiskerInputView, args: [WhiskerValue]) -> WhiskerValue in
                    // The first positional arg is the map the Rust side sent
                    // as `WhiskerValue::map([("value", ...)])`.
                    if case .map(let m) = args.first, let s = m["value"]?.asString {
                        view.setValue(s)
                    }
                    return .null
                }
                Function("getValue") { (view: WhiskerInputView, _: [WhiskerValue]) -> WhiskerValue in
                    // Shape must match the Rust `GetValueResult` struct.
                    return .map(["value": .string(view.currentText())])
                }
            }
        }
    }
}
