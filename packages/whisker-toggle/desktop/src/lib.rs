//! Desktop Host implementation for `whisker-toggle`.

use whisker_desktop::{
    DesktopEventEmitter, DesktopNativeEvent, DesktopViewDefinition, ModuleDefinition,
    WhiskerModule, WhiskerValue,
};

#[derive(Debug)]
struct ToggleDesktopView {
    checked: bool,
    disabled: bool,
    events: DesktopEventEmitter,
}

struct ToggleModule;

/// Declares the Desktop implementation independently from the Rust schema.
#[WhiskerModule]
impl WhiskerModule for ToggleModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new()
            .name("whisker-toggle:WhiskerToggle")
            .view(
                DesktopViewDefinition::new("whisker.toggle/Toggle", |events| ToggleDesktopView {
                    checked: false,
                    disabled: false,
                    events,
                })
                .prop(
                    "checked",
                    |view, value| {
                        let WhiskerValue::Bool(value) = value else {
                            unreachable!("Desktop Host validates Toggle property shapes")
                        };
                        view.checked = *value;
                    },
                    |view| view.checked = false,
                )
                .prop(
                    "disabled",
                    |view, value| {
                        let WhiskerValue::Bool(value) = value else {
                            unreachable!("Desktop Host validates Toggle property shapes")
                        };
                        view.disabled = *value;
                    },
                    |view| view.disabled = false,
                )
                .event("change")
                .command("setChecked", |view, parameters| {
                    let WhiskerValue::Bool(checked) = parameters else {
                        unreachable!("Desktop Host validates Toggle command parameters")
                    };
                    view.checked = *checked;
                    view.events.emit(DesktopNativeEvent {
                        event: "change".into(),
                        detail: WhiskerValue::map([("checked", WhiskerValue::Bool(view.checked))]),
                    });
                }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definition_is_an_independent_host_catalog() {
        let definition = __whisker_module_definition();
        assert_eq!(definition.factories().len(), 1);
    }
}
