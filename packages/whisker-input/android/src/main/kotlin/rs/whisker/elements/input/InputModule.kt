// `whisker-input` ModuleDefinition (Android). KSP discovers the annotated
// module and generates its registration entry.

package rs.whisker.elements.input

import rs.whisker.runtime.Module
import rs.whisker.runtime.WhiskerModule
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerValue


@WhiskerModule
class InputModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("Input")
        View("whisker-input:Input", WhiskerInputView::class.java) {
            // ---- text-content props ----------------------------------------

            Prop("value") { view: WhiskerInputView, value ->
                view.setValue(value.asString() ?: "")
            }
            Prop("placeholder") { view: WhiskerInputView, value ->
                view.setPlaceholder(value.asString() ?: "")
            }
            Prop("placeholder-color") { view: WhiskerInputView, value ->
                view.setPlaceholderColor(value.asString() ?: "")
            }
            Prop("caret-color") { view: WhiskerInputView, value ->
                view.applyCaretColor(value.asString() ?: "")
            }
            Prop("selection-color") { view: WhiskerInputView, value ->
                view.setSelectionColor(value.asString() ?: "")
            }

            // ---- behaviour props -------------------------------------------

            Prop("multiline") { view: WhiskerInputView, value ->
                view.setMultiline(value.asString() ?: "false")
            }
            Prop("lines") { view: WhiskerInputView, value ->
                view.setLines(value.asString() ?: "0")
            }
            Prop("secure") { view: WhiskerInputView, value ->
                view.setSecure(value.asString() ?: "false")
            }
            Prop("editable") { view: WhiskerInputView, value ->
                view.setEditable(value.asString() ?: "true")
            }
            Prop("auto-focus") { view: WhiskerInputView, value ->
                view.setAutoFocus(value.asString() ?: "false")
            }
            Prop("max-length") { view: WhiskerInputView, value ->
                view.setMaxLength(value.asString() ?: "0")
            }
            Prop("keyboard-type") { view: WhiskerInputView, value ->
                view.setKeyboardType(value.asString() ?: "default")
            }
            Prop("return-key") { view: WhiskerInputView, value ->
                view.setReturnKey(value.asString() ?: "default")
            }
            Prop("auto-capitalize") { view: WhiskerInputView, value ->
                view.setAutoCapitalize(value.asString() ?: "sentences")
            }
            Prop("autocorrect") { view: WhiskerInputView, value ->
                view.setAutocorrect(value.asString() ?: "true")
            }
            Prop("spell-check") { view: WhiskerInputView, value ->
                view.setSpellCheck(value.asString() ?: "true")
            }

            // Declaration-only metadata (parity with the iOS module);
            // actual dispatch is the imperative WhiskerCustomEvent path
            // inside WhiskerInputView. Documents the emittable set.
            Events("input", "change", "focus", "blur", "submit")

            TextStyle { view: WhiskerInputView, style ->
                view.applyTextStyle(style)
            }

            // ---- one-way View commands -------------------------------------

            Command("focus") { view: WhiskerInputView, _ ->
                view.focusField()
            }
            Command("blur") { view: WhiskerInputView, _ ->
                view.blurField()
            }
            // `clear` fires `input` so the bound signal sees the change as
            // though the user had typed it.
            Command("clear") { view: WhiskerInputView, _ ->
                view.clearField()
            }
            // The view applies the cursor-diff guard and suppresses the
            // resulting afterTextChanged, which is not a user edit.
            Command("setValue") { view: WhiskerInputView, parameters ->
                val map = (parameters as? WhiskerValue.Map)?.value
                val text = map?.get("value")?.asString() ?: ""
                view.setValueExternal(text)
            }
        }
    }
}
