// `whisker-input` ModuleDefinition (iOS).
//
// The SwiftPM build plugin discovers this Module subclass and emits its
// Whisker registration entry. Android uses the same ModuleDefinition shape.

import WhiskerModule

@WhiskerModule
public final class InputModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("Input")
            View("whisker-input:Input", WhiskerInputView.self) {

                // ---- Value + placeholder ----------------------------------

                Prop("value") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setValue(value.asString ?? "")
                }
                Prop("placeholder") { (view: WhiskerInputView, value: WhiskerValue) in
                    view.setPlaceholder(value.asString ?? "")
                }
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

                TextStyle { (view: WhiskerInputView, style: WhiskerTextStyle) in
                    view.applyTextStyle(style)
                }

                // Declaration-only metadata for the codegen / docs scanner;
                // dispatch happens in `InputView.swift`.
                Events("input", "change", "focus", "blur", "submit")

                // ---- One-way View commands -------------------------------

                Command("focus") { (view: WhiskerInputView, _: WhiskerValue) in
                    view.focusField()
                }
                Command("blur") { (view: WhiskerInputView, _: WhiskerValue) in
                    view.blurField()
                }
                Command("clear") { (view: WhiskerInputView, _: WhiskerValue) in
                    view.clearField()
                }
                Command("setValue") { (view: WhiskerInputView, parameters: WhiskerValue) in
                    if case .map(let m) = parameters, let s = m["value"]?.asString {
                        view.setValue(s)
                    }
                }
            }
        }
    }
}
