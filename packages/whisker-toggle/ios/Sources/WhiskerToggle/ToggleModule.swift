import WhiskerModule

/// Independently compiled iOS declaration, negotiated with Rust by name.
@WhiskerModule
public final class ToggleModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("WhiskerToggle")
            View("whisker.toggle/Toggle", ToggleView.self) {
                Prop("checked") { (view: ToggleView, value: WhiskerValue) in
                    view.setChecked(value.asBool ?? false)
                }
                Prop("disabled") { (view: ToggleView, value: WhiskerValue) in
                    view.setDisabled(value.asBool ?? false)
                }
                Events("change")
                Command("setChecked") { (view: ToggleView, parameters: WhiskerValue) in
                    view.setChecked(parameters.asBool ?? false)
                }
            }
            Function("echo") { (args: [WhiskerValue]) -> WhiskerValue in
                args.first ?? .null
            }
            AsyncFunction("echoAsync") { (args: [WhiskerValue], promise: WhiskerPromise) in
                promise.resolve(args.first ?? .null)
            }
            Events("ready")
            OnStartObserving("ready") { [weak self] in
                self?.sendEvent("ready", .string("ios-ready"))
            }
        }
    }
}
