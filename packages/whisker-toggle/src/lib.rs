//! Minimal external element used to exercise RFC0004 end to end.
//!
//! This crate is the platform-neutral half of the module: it owns only the
//! authoring API, schema, and generated IDs. Target definitions
//! live beside it in `desktop/` and `web/`, matching the native source-tree
//! split used by Android and iOS modules.

use whisker::Signal;
use whisker::event::CustomEvent;

/// Fully-qualified service name used by [`whisker::PlatformModule`].
///
/// This is distinct from the `whisker.toggle/Toggle` element name: a module
/// may expose multiple elements alongside functions and events.
pub const MODULE_NAME: &str = concat!(env!("CARGO_PKG_NAME"), ":WhiskerToggle");

/// Rust-side authoring component for the external Toggle element.
#[whisker::module_element(
    name = "whisker.toggle/Toggle",
    measurement = None,
    commands = [("setChecked", Bool)],
)]
pub fn toggle(
    checked: Signal<bool>,
    disabled: Signal<bool>,
    style: whisker::Style,
    on_change: CustomEvent,
) {
}

/// Element schemas exported by this package for surface bootstrap.
#[doc(hidden)]
pub fn __whisker_element_module_definition() -> whisker::ElementModuleDefinition {
    whisker::ElementModuleDefinition::new(
        env!("CARGO_PKG_NAME"),
        [toggle_schema::element_provider()],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_versionless_and_covers_the_full_toggle_contract() {
        assert_eq!(MODULE_NAME, "whisker-toggle:WhiskerToggle");
        let provider = toggle_schema::element_provider();
        assert_eq!(provider.schema, toggle_schema::schema());
        assert_eq!(provider.authoring, whisker::ElementAuthoringBinding::Named);
        assert_eq!(provider.schema.name, toggle_schema::NAME);
        assert_eq!(provider.schema.name, "whisker.toggle/Toggle");
        assert!(!provider.schema.name.contains('@'));
        assert_eq!(provider.schema.properties.len(), 2);
        assert_eq!(provider.schema.events.len(), 1);
        assert_eq!(provider.schema.commands.len(), 1);
        assert_eq!(provider.schema.commands[0].name, "setChecked");
        assert_eq!(provider.schema.events[0].detail, None);
        assert_eq!(provider.schema.validate(), Ok(()));
    }
}
