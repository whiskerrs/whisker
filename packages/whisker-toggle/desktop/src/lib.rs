//! Desktop Host implementation for `whisker-toggle`.

use whisker_desktop::{DesktopViewDefinition, ModuleDefinition, WhiskerModule, WhiskerValue};

#[derive(Debug, Default)]
struct ToggleDesktopView {
    checked: bool,
    disabled: bool,
}

struct ToggleModule;

/// Declares the Desktop implementation independently from the Rust schema.
#[WhiskerModule]
impl WhiskerModule for ToggleModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new().view(
            DesktopViewDefinition::new("whisker.toggle/Toggle", ToggleDesktopView::default)
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
                .event("change"),
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
