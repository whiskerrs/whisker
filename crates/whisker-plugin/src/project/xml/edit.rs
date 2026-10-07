//! Explicit edits for native XML documents; no schema-dependent merge guesses.
use super::*;
use anyhow::{Result, ensure};

/// Select a child by qualified element name and exact identity attributes
///
/// Paths begin below the existing root. Names, prefixes and attribute values are
/// compared literally; namespace normalization remains the native backend's job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct XmlSelector {
    /// Qualified tag name.
    pub name: String,
    /// Required identity attributes.
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

/// Explicit attribute contribution or replacement
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum XmlAttributeEdit {
    /// Add or repeat the same value; conflicting existing values are errors.
    Set(String),
    /// Intentionally replace any existing value.
    Override(String),
    /// Remove the attribute; missing values are a no-op.
    Remove,
}

/// Ordered edits to one native XML document
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum XmlEdit {
    /// Select or create a unique path; new elements append without reordering.
    Upsert {
        /// Empty path selects the existing root.
        path: Vec<XmlSelector>,
        /// Attribute changes on the selected element.
        #[serde(default)]
        attributes: BTreeMap<String, XmlAttributeEdit>,
        /// Exact duplicate nodes are ignored; distinct nodes retain their order.
        #[serde(default)]
        append: Vec<XmlNode>,
    },
    /// Remove a uniquely selected descendant; absence is a no-op.
    Remove {
        /// Nonempty path below the root.
        path: Vec<XmlSelector>,
    },
    /// Deliberately replace text/mixed content of an existing element.
    ReplaceChildren {
        /// Empty path selects the existing root; missing elements are errors.
        path: Vec<XmlSelector>,
        /// Complete ordered replacement, without interpretation or deduplication.
        children: Vec<XmlNode>,
    },
}
impl XmlElement {
    /// Apply explicit XML edits atomically, preserving the original on any error
    ///
    /// Ambiguous selectors and identity-attribute changes are errors. Renaming
    /// requires removing and adding an element. Qualified names and namespace
    /// declarations are literal; schema, child order and native merge rules are
    /// not inferred. Use AndroidSourceSet::edit_manifest for Manifest placement.
    pub fn edit(&mut self, edits: &[XmlEdit]) -> Result<()> {
        let mut next = self.clone();
        for edit in edits {
            let path = match edit {
                XmlEdit::Upsert { path, .. }
                | XmlEdit::Remove { path }
                | XmlEdit::ReplaceChildren { path, .. } => path,
            };
            for selector in path {
                ensure!(!selector.name.trim().is_empty(), "empty XML selector name");
            }
            apply(&mut next, edit)?;
        }
        *self = next;
        Ok(())
    }
}
fn index(root: &XmlElement, selector: &XmlSelector) -> Result<Option<usize>> {
    ensure!(!selector.name.trim().is_empty(), "empty XML selector name");
    let indices: Vec<_> = root
        .children
        .iter()
        .enumerate()
        .filter_map(|(i, node)| match node {
            XmlNode::Element(e)
                if e.name == selector.name
                    && selector
                        .attributes
                        .iter()
                        .all(|(k, v)| e.attributes.get(k) == Some(v)) =>
            {
                Some(i)
            }
            _ => None,
        })
        .collect();
    ensure!(
        indices.len() <= 1,
        "ambiguous XML selector {}",
        selector.name
    );
    Ok(indices.first().copied())
}
fn select<'a>(
    root: &'a mut XmlElement,
    path: &[XmlSelector],
    create: bool,
) -> Result<Option<&'a mut XmlElement>> {
    let mut current = root;
    for selector in path {
        let i = if let Some(i) = index(current, selector)? {
            i
        } else if create {
            current.children.push(XmlNode::Element(XmlElement {
                name: selector.name.clone(),
                attributes: selector.attributes.clone(),
                children: vec![],
            }));
            current.children.len() - 1
        } else {
            return Ok(None);
        };
        let XmlNode::Element(child) = &mut current.children[i] else {
            unreachable!()
        };
        current = child;
    }
    Ok(Some(current))
}
fn apply(root: &mut XmlElement, edit: &XmlEdit) -> Result<()> {
    match edit {
        XmlEdit::Upsert {
            path,
            attributes,
            append,
        } => {
            if let Some(selector) = path.last() {
                for (key, value) in &selector.attributes {
                    if let Some(edit) = attributes.get(key) {
                        ensure!(
                            matches!(edit,XmlAttributeEdit::Set(v)|XmlAttributeEdit::Override(v) if v==value),
                            "cannot change XML selector identity {key}"
                        );
                    }
                }
            }
            let target = select(root, path, true)?.expect("created path");
            for (key, edit) in attributes {
                match edit {
                    XmlAttributeEdit::Set(value) => {
                        ensure!(
                            target.attributes.get(key).is_none_or(|v| v == value),
                            "conflicting XML attribute {key}"
                        );
                        target.attributes.insert(key.clone(), value.clone());
                    }
                    XmlAttributeEdit::Override(value) => {
                        target.attributes.insert(key.clone(), value.clone());
                    }
                    XmlAttributeEdit::Remove => {
                        target.attributes.remove(key);
                    }
                }
            }
            crate::project::merge::append(&mut target.children, append);
        }
        XmlEdit::Remove { path } => {
            let Some((last, parent)) = path.split_last() else {
                anyhow::bail!("cannot remove XML root");
            };
            if let Some(parent) = select(root, parent, false)?
                && let Some(i) = index(parent, last)?
            {
                parent.children.remove(i);
            }
        }
        XmlEdit::ReplaceChildren { path, children } => {
            let Some(target) = select(root, path, false)? else {
                anyhow::bail!("XML replacement target not found");
            };
            target.children = children.clone();
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_xml_edits_preserve_order_and_rollback_conflicts() {
        let mut root = XmlElement::new("Package");
        let selector = XmlSelector {
            name: "Identity".into(),
            attributes: BTreeMap::new(),
        };
        let add = XmlEdit::Upsert {
            path: vec![selector.clone()],
            attributes: BTreeMap::from([("Name".into(), XmlAttributeEdit::Set("Example".into()))]),
            append: vec![],
        };
        root.edit(std::slice::from_ref(&add)).unwrap();
        root.edit(std::slice::from_ref(&add)).unwrap();
        assert_eq!(root.children.len(), 1);
        let before = root.clone();
        let bad = XmlEdit::Upsert {
            path: vec![selector.clone()],
            attributes: BTreeMap::from([(
                "Name".into(),
                XmlAttributeEdit::Set("Different".into()),
            )]),
            append: vec![],
        };
        assert!(
            root.edit(&[
                XmlEdit::ReplaceChildren {
                    path: vec![selector.clone()],
                    children: vec![XmlNode::Text("temporary".into())]
                },
                bad
            ])
            .is_err()
        );
        assert_eq!(root, before);
        root.edit(&[XmlEdit::Remove {
            path: vec![selector],
        }])
        .unwrap();
        assert!(root.children.is_empty());
        assert!(root.edit(&[XmlEdit::Remove { path: vec![] }]).is_err());
    }
}
