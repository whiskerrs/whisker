// `whisker-input` ModuleDefinition (iOS).
//
// Mirrors `whisker-video`'s `VideoModule` shape for the `View(...)` +
// `Prop(...)` + `Function(...)` + `Events(...)` DSL surface. The
// codegen plugin discovers this `Module` subclass and emits a
// registration block in `WhiskerInput+Generated.swift` that:
//
//   - Reads `definitionLazy.view!.viewClass` (== `WhiskerInputView`).
//   - Calls `LynxComponentRegistry.registerUI(viewClass, withName:
//     "whisker-input:Input")`.
//   - Calls `module.registerWithLynx()` so all `Prop(...)` setters +
//     `Function(...)` methods install via the Obj-C-runtime path.
//
// The `WhiskerInputView` Lynx UI subclass lives in `InputView.swift`.
// Same split on Android (`InputModule.kt` + `WhiskerInputView.kt`).
//
// ## Events
//
// Events are declared inside the `View(...)` block per the DSL contract
// (`Events(...)` is legal both at module-level and inside a view block).
// The actual dispatch goes through `WhiskerCustomEvent.dispatch(from:
// name:params:)` called by the view's delegate methods — see
// `InputView.swift`. `Events(...)` here is declaration-only metadata.
//
// ## CSS text-style props
//
// `color`, `font-size`, `font-weight`, and `text-align` arrive from
// Lynx's style cascade as explicit `Prop` entries rather than through
// the base-class `LynxUI` inheritance chain. The base class handles
// generic view props (background-color, border-radius, opacity), but
// it does NOT forward text-style values into the backing UITextField /
// UITextView — those live in the custom view layer and must be set
// explicitly. This mirrors the same approach: each CSS text prop is
// listed as a `Prop(...)` in the DSL definition and forwarded to the
// corresponding setter on `WhiskerInputView`.

import WhiskerModule

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
                // Colour props arrive either as a parsed Lynx ARGB int
                // (when set via the CSS cascade / `SetRawInlineStyles`) or
                // as a raw CSS string (when set as a plain attribute). Pass
                // the whole `WhiskerValue` through so the view can resolve
                // both forms — see `WhiskerInputView.resolveColor(_:)`.
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
                // `color`, `font-size`, `font-weight`, `text-align` flow
                // through Lynx's style cascade and DO reach this custom UI's
                // prop setters on iOS (Lynx dispatches the element's prop
                // bundle through `LynxPropsProcessor.updateProp:withKey:forUI:`,
                // which resolves our registered `set<Cap>:requestReset:`
                // selectors via the Obj-C runtime — the same channel the
                // base class's background/border setters use).
                //
                // CRITICAL: Lynx delivers these as ALREADY-PARSED values, not
                // CSS strings — `color` is an ARGB int (NSNumber → `.int`),
                // `font-size` a resolved point CGFloat (`.float`/`.int`),
                // `font-weight` a `LynxFontWeightType` enum int, `text-align`
                // a `LynxTextAlignType` enum int. We forward the whole
                // `WhiskerValue` so the view can decode the numeric form
                // (falling back to string parsing when set as a plain attr).
                // The earlier `value.asString ?? ""` form silently dropped
                // every numeric value → black text / default font (bug).

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

                // ---- Events ----------------------------------------------
                //
                // Declaration-only: dispatch goes through
                // `WhiskerCustomEvent.dispatch(from:name:params:)` in
                // `InputView.swift`. Listed here so the codegen / docs
                // scanner knows the full event surface.

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
                    // Args arrive as `{ "args": [<WhiskerValue.map { "value": ... }>] }` via
                    // `WhiskerValue.fromNSDictionary`; the first positional arg is the map
                    // the Rust side sent as `WhiskerValue::map([("value", ...)])`.
                    if case .map(let m) = args.first, let s = m["value"]?.asString {
                        view.setValue(s)
                    }
                    return .null
                }
                Function("getValue") { (view: WhiskerInputView, _: [WhiskerValue]) -> WhiskerValue in
                    // Returns `{ "value": "<current text>" }` — matches the
                    // `GetValueResult` struct the Rust side deserializes into.
                    return .map(["value": .string(view.currentText())])
                }
            }
        }
    }
}
