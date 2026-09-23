//! Structured XML for native manifests and metadata

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// An XML element retaining attributes and ordered mixed content
///
/// Names use qualified spelling, such as `android:name`; namespace declarations
/// are explicit `xmlns` attributes. A renderer escapes values and validates
/// namespace bindings. This model does not parse XML or apply Android's manifest
/// merger rules. Keeping child order allows repeated tags and mixed text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct XmlElement {
    /// Qualified element name, such as `manifest` or `uap:Extension`.
    pub name: String,
    /// Attributes, including namespace declarations when required.
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    /// Child nodes in document order.
    #[serde(default)]
    pub children: Vec<XmlNode>,
}

impl XmlElement {
    /// Create an empty element with the supplied qualified name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            attributes: BTreeMap::new(),
            children: Vec::new(),
        }
    }
}

/// An element or text node within structured XML
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum XmlNode {
    /// A nested element.
    Element(XmlElement),
    /// Text content, escaped by the renderer.
    Text(String),
}

mod edit;
pub use edit::*;
