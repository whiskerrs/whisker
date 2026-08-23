package rs.whisker.toggle

import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerModule
import rs.whisker.runtime.WhiskerValue

/** Independently compiled Android declaration, negotiated with Rust by name. */
@WhiskerModule
public class ToggleModule : Module() {
    override fun definition(): ModuleDefinition = ModuleDefinition {
        View("whisker.toggle/Toggle", ToggleView::class.java) {
            Prop("checked") { view: ToggleView, value ->
                view.setChecked(value.asBool() ?: false)
            }
            Prop("disabled") { view: ToggleView, value ->
                view.setDisabled(value.asBool() ?: false)
            }
            Events("change")
        }
        Function("echo") { args -> args.firstOrNull() ?: WhiskerValue.Null }
        AsyncFunction("echoAsync") { args, promise ->
            promise.resolve(args.firstOrNull() ?: WhiskerValue.Null)
        }
        Events("ready")
        OnStartObserving("ready") {
            sendEvent("ready", WhiskerValue.Str("android-ready"))
        }
    }
}
