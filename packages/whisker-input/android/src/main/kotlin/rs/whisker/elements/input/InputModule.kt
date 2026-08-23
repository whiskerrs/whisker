// `whisker-input` ModuleDefinition (Android).
//
// KSP scans this module's sources for `@WhiskerModule`
// and emits the registration block into
// `WhiskerInputBehaviors.registerAll()`.
//
// The `WhiskerInputView` Lynx UI subclass this references lives in
// `WhiskerInputView.kt`. Matching iOS files live under
// `packages/whisker-input/ios/Sources/WhiskerInput/`.

package rs.whisker.elements.input

import rs.whisker.runtime.Module
import rs.whisker.runtime.WhiskerModule
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerValue


@WhiskerModule
class InputModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("Input")
        View(WhiskerInputView::class.java) {
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

            // ---- callable UI methods ---------------------------------------

            Function("focus") { view: WhiskerInputView, _ ->
                view.focusField()
                WhiskerValue.Null
            }
            Function("blur") { view: WhiskerInputView, _ ->
                view.blurField()
                WhiskerValue.Null
            }
            // `clear` fires `input` so the bound signal sees the change as
            // though the user had typed it.
            Function("clear") { view: WhiskerInputView, _ ->
                view.clearField()
                WhiskerValue.Null
            }
            // The view applies the cursor-diff guard and suppresses the
            // resulting afterTextChanged, which is not a user edit.
            Function("setValue") { view: WhiskerInputView, args ->
                // The Lynx bridge packs args as { "args": [map] }; the map
                // carries { "value": "<text>" }.
                val map = (args.getOrNull(0) as? WhiskerValue.Map)?.value
                val text = map?.get("value")?.asString() ?: ""
                view.setValueExternal(text)
                WhiskerValue.Null
            }
            // TODO: Android result-returning custom element methods need
            // the `invoke_async` bridge path in `lynx_native_renderer.cc`,
            // compiled iOS-only in Lynx 3.8.0-whisker.1 (memory note
            // `whisker_element_method_results_need_async`). Until the fork
            // wires it, Rust receives a DispatchFailed error here.
            Function("getValue") { view: WhiskerInputView, _ ->
                WhiskerValue.Map(
                    mapOf("value" to WhiskerValue.Str(view.currentText()))
                )
            }
        }
    }
}
