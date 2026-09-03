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
    fn accessibility_roles_have_stable_protocol_names() {
        assert_eq!(AccessibilityRole::Group.as_str(), "group");
        assert_eq!(AccessibilityRole::Text.as_str(), "text");
        assert_eq!(AccessibilityRole::Button.as_str(), "button");
        assert_eq!(AccessibilityRole::Link.as_str(), "link");
        assert_eq!(AccessibilityRole::Image.as_str(), "image");
        assert_eq!(AccessibilityRole::Header.as_str(), "header");
        assert_eq!(AccessibilityRole::Checkbox.as_str(), "checkbox");
        assert_eq!(AccessibilityRole::Radio.as_str(), "radio");
        assert_eq!(AccessibilityRole::Switch.as_str(), "switch");
        assert_eq!(AccessibilityRole::Adjustable.as_str(), "adjustable");
        assert_eq!(AccessibilityRole::SearchBox.as_str(), "searchbox");
        assert_eq!(AccessibilityRole::Tab.as_str(), "tab");
    }

    #[test]
    fn accessibility_checked_states_have_stable_protocol_names() {
        assert_eq!(AccessibilityChecked::Unchecked.as_str(), "false");
        assert_eq!(AccessibilityChecked::Checked.as_str(), "true");
        assert_eq!(AccessibilityChecked::Mixed.as_str(), "mixed");
    }

    #[test]
    fn accessibility_has_a_stable_value_shape() {
        let actual = Accessibility::new()
            .label("Play")
            .hint("Starts playback")
            .role(AccessibilityRole::Button)
            .identifier("play-button")
            .hidden(true)
            .modal(true)
            .state(
                AccessibilityState::new()
                    .disabled(true)
                    .selected(false)
                    .checked(AccessibilityChecked::Checked)
                    .expanded(false),
            )
            .to_value();
        let expected = WhiskerValue::map([
            ("label", WhiskerValue::String("Play".into())),
            ("hint", WhiskerValue::String("Starts playback".into())),
            ("role", WhiskerValue::String("button".into())),
            ("identifier", WhiskerValue::String("play-button".into())),
            ("hidden", WhiskerValue::Bool(true)),
            ("modal", WhiskerValue::Bool(true)),
            (
                "state",
                WhiskerValue::map([
                    ("disabled", WhiskerValue::Bool(true)),
                    ("selected", WhiskerValue::Bool(false)),
                    ("checked", WhiskerValue::String("true".into())),
                    ("expanded", WhiskerValue::Bool(false)),
                ]),
            ),
        ]);

        assert_eq!(actual, expected);
    }

    #[test]
    fn empty_accessibility_encodes_absent_values_as_null() {
        let actual = Accessibility::new().to_value();
        let expected = WhiskerValue::map([
            ("label", WhiskerValue::Null),
            ("hint", WhiskerValue::Null),
            ("role", WhiskerValue::Null),
            ("identifier", WhiskerValue::Null),
            ("hidden", WhiskerValue::Bool(false)),
            ("modal", WhiskerValue::Bool(false)),
            (
                "state",
                WhiskerValue::map([
                    ("disabled", WhiskerValue::Null),
                    ("selected", WhiskerValue::Null),
                    ("checked", WhiskerValue::Null),
                    ("expanded", WhiskerValue::Null),
                ]),
            ),
        ]);

        assert_eq!(actual, expected);
    }
}
