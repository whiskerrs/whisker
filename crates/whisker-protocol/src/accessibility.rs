//! Host-independent accessibility semantics attached to scene nodes.

use whisker_value::WhiskerValue;

/// Semantic role exposed to platform accessibility services.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AccessibilityRole {
    /// A generic group without a more specific role.
    Group,
    /// Static text.
    Text,
    /// A pressable button.
    Button,
    /// A navigational link.
    Link,
    /// An image or illustration.
    Image,
    /// A heading in the content hierarchy.
    Header,
    /// A checkbox with a checked state.
    Checkbox,
    /// A radio option with a checked state.
    Radio,
    /// A binary switch.
    Switch,
    /// An adjustable value such as a slider.
    Adjustable,
    /// A search input.
    SearchBox,
    /// A tab in a tab list.
    Tab,
}

impl AccessibilityRole {
    /// Stable protocol spelling used by the mobile value envelope.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Group => "group",
            Self::Text => "text",
            Self::Button => "button",
            Self::Link => "link",
            Self::Image => "image",
            Self::Header => "header",
            Self::Checkbox => "checkbox",
            Self::Radio => "radio",
            Self::Switch => "switch",
            Self::Adjustable => "adjustable",
            Self::SearchBox => "searchbox",
            Self::Tab => "tab",
        }
    }
}

/// Checked state for controls that are not merely binary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AccessibilityChecked {
    /// The control is not checked.
    Unchecked,
    /// The control is checked.
    Checked,
    /// A partially checked aggregate value.
    Mixed,
}

impl AccessibilityChecked {
    /// Stable protocol spelling used by the mobile value envelope.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unchecked => "false",
            Self::Checked => "true",
            Self::Mixed => "mixed",
        }
    }
}

/// Dynamic state announced alongside an accessibility node.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccessibilityState {
    /// Optional disabled state for the associated control.
    pub disabled: Option<bool>,
    /// Optional selected state for the associated item.
    pub selected: Option<bool>,
    /// Optional checked state for checkable controls.
    pub checked: Option<AccessibilityChecked>,
    /// Optional expanded/collapsed state for disclosure controls.
    pub expanded: Option<bool>,
}

impl AccessibilityState {
    /// Creates an empty state description.
    pub const fn new() -> Self {
        Self {
            disabled: None,
            selected: None,
            checked: None,
            expanded: None,
        }
    }

    /// Sets the disabled state.
    pub const fn disabled(mut self, value: bool) -> Self {
        self.disabled = Some(value);
        self
    }

    /// Sets the selected state.
    pub const fn selected(mut self, value: bool) -> Self {
        self.selected = Some(value);
        self
    }

    /// Sets the checked state.
    pub const fn checked(mut self, value: AccessibilityChecked) -> Self {
        self.checked = Some(value);
        self
    }

    /// Sets the expanded/collapsed state.
    pub const fn expanded(mut self, value: bool) -> Self {
        self.expanded = Some(value);
        self
    }
}

/// Complete semantic accessibility description for one scene node.
///
/// This is a common scene contract, not an element-specific module property.
/// Consequently built-in and custom elements receive the same behavior.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Accessibility {
    /// Human-readable name announced for the element.
    pub label: Option<String>,
    /// Additional usage guidance announced after the label.
    pub hint: Option<String>,
    /// Semantic role of the element.
    pub role: Option<AccessibilityRole>,
    /// Stable identifier intended for UI automation and accessibility tools.
    pub identifier: Option<String>,
    /// Whether this node and its descendants are hidden from accessibility.
    pub hidden: bool,
    /// Whether this node represents a modal region.
    pub modal: bool,
    /// Dynamic semantic state.
    pub state: AccessibilityState,
}

impl Accessibility {
    /// Creates an empty semantic description.
    pub const fn new() -> Self {
        Self {
            label: None,
            hint: None,
            role: None,
            identifier: None,
            hidden: false,
            modal: false,
            state: AccessibilityState::new(),
        }
    }

    /// Sets the announced label.
    pub fn label(mut self, value: impl Into<String>) -> Self {
        self.label = Some(value.into());
        self
    }

    /// Sets the announced hint.
    pub fn hint(mut self, value: impl Into<String>) -> Self {
        self.hint = Some(value.into());
        self
    }

    /// Sets the semantic role.
    pub const fn role(mut self, value: AccessibilityRole) -> Self {
        self.role = Some(value);
        self
    }

    /// Sets the automation/accessibility identifier.
    pub fn identifier(mut self, value: impl Into<String>) -> Self {
        self.identifier = Some(value.into());
        self
    }

    /// Hides or reveals this semantic subtree.
    pub const fn hidden(mut self, value: bool) -> Self {
        self.hidden = value;
        self
    }

    /// Marks or unmarks this node as a modal region.
    pub const fn modal(mut self, value: bool) -> Self {
        self.modal = value;
        self
    }

    /// Replaces the dynamic state description.
    pub const fn state(mut self, value: AccessibilityState) -> Self {
        self.state = value;
        self
    }

    /// Converts this semantic object into the universal mobile ABI value.
    pub fn to_value(&self) -> WhiskerValue {
        WhiskerValue::map([
            (
                "label",
                self.label
                    .clone()
                    .map(WhiskerValue::String)
                    .unwrap_or(WhiskerValue::Null),
            ),
            (
                "hint",
                self.hint
                    .clone()
                    .map(WhiskerValue::String)
                    .unwrap_or(WhiskerValue::Null),
            ),
            (
                "role",
                self.role
                    .map(|role| WhiskerValue::String(role.as_str().to_owned()))
                    .unwrap_or(WhiskerValue::Null),
            ),
            (
                "identifier",
                self.identifier
                    .clone()
                    .map(WhiskerValue::String)
                    .unwrap_or(WhiskerValue::Null),
            ),
            ("hidden", WhiskerValue::Bool(self.hidden)),
            ("modal", WhiskerValue::Bool(self.modal)),
            (
                "state",
                WhiskerValue::map([
                    (
                        "disabled",
                        match self.state.disabled {
                            Some(value) => WhiskerValue::Bool(value),
                            None => WhiskerValue::Null,
                        },
                    ),
                    (
                        "selected",
                        match self.state.selected {
                            Some(value) => WhiskerValue::Bool(value),
                            None => WhiskerValue::Null,
                        },
                    ),
                    (
                        "checked",
                        self.state
                            .checked
                            .map(|checked| WhiskerValue::String(checked.as_str().to_owned()))
                            .unwrap_or(WhiskerValue::Null),
                    ),
                    (
                        "expanded",
                        self.state
                            .expanded
                            .map(WhiskerValue::Bool)
                            .unwrap_or(WhiskerValue::Null),
                    ),
                ]),
            ),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessibility_has_a_stable_value_shape() {
        let value = Accessibility::new()
            .label("Play")
            .role(AccessibilityRole::Button)
            .state(
                AccessibilityState::new()
                    .disabled(true)
                    .selected(false)
                    .checked(AccessibilityChecked::Checked),
            )
            .to_value();
        let WhiskerValue::Map(value) = value else {
            panic!("accessibility must encode as a map");
        };
        assert_eq!(
            value.get("label"),
            Some(&WhiskerValue::String("Play".into()))
        );
        assert_eq!(
            value.get("role"),
            Some(&WhiskerValue::String("button".into()))
        );
        let WhiskerValue::Map(state) = value.get("state").unwrap() else {
            panic!("accessibility state must encode as a map");
        };
        assert_eq!(state.get("disabled"), Some(&WhiskerValue::Bool(true)));
        assert_eq!(state.get("selected"), Some(&WhiskerValue::Bool(false)));
        assert_eq!(
            state.get("checked"),
            Some(&WhiskerValue::String("true".into()))
        );
    }
}
