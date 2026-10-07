//! Explicit edits to the CNG-owned Manifest tree, separate from native merging

use super::AndroidSourceSet;
use crate::project::{XmlElement, XmlNode};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// An element identified by its qualified name and selected attribute values
///
/// A path starts below the manifest root. For example, application followed by
/// activity with android:name identifies one Activity. More than one match is
/// an error. Names/prefixes are compared literally; namespace declarations and
/// class-name normalization are the caller's responsibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AndroidManifestSelector {
    /// Qualified tag name.
    pub name: String,
    /// Identity attributes, e.g. android:name; these cannot be renamed by an edit.
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

/// An intentional change to one attribute
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AndroidManifestAttributeEdit {
    /// Add or repeat the same value; a differing existing value is a conflict.
    Set(String),
    /// Explicitly replace the value, including a conflicting existing value.
    Override(String),
    /// Explicitly remove the attribute; absence is a no-op.
    Remove,
}

/// Selector-based Manifest changes with explicit override/removal semantics
///
/// These edit the single CNG tree. They do not execute tools:node/tools:replace
/// or reproduce Android's library/variant Manifest merger. Use Upsert for named
/// elements. Append preserves distinct intent filters without guessing identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AndroidManifestEdit {
    /// Find or create a unique element path and apply the supplied contribution.
    Upsert {
        /// Selectors below the manifest root; empty selects the root itself.
        path: Vec<AndroidManifestSelector>,
        /// Changes to attributes of the selected element.
        #[serde(default)]
        attributes: BTreeMap<String, AndroidManifestAttributeEdit>,
        /// Additional children; exact duplicate nodes are idempotent.
        /// Native element legality/order beyond root application placement is
        /// the caller's responsibility. This does not merge named descendants.
        #[serde(default)]
        append: Vec<XmlNode>,
    },
    /// Remove a uniquely selected element; absence is a no-op.
    Remove {
        /// Nonempty path below the manifest root.
        path: Vec<AndroidManifestSelector>,
    },
}

impl AndroidSourceSet {
    /// Apply a batch of Manifest edits atomically
    ///
    /// Creates an empty manifest root when no tree exists. New root children are
    /// inserted before application; other new nodes append. A conflict, ambiguous
    /// selector, or attempted root removal leaves the source set unchanged.
    /// Required namespace declarations must be supplied explicitly by the caller.
    ///
    /// ```
    /// use std::collections::BTreeMap;
    /// use whisker_plugin::project::{AndroidSourceSet, AndroidManifestEdit,
    ///     AndroidManifestSelector, AndroidManifestAttributeEdit};
    /// let mut source = AndroidSourceSet::default();
    /// source.manifest = Some(whisker_plugin::project::XmlElement::new("manifest"));
    /// source.manifest.as_mut().unwrap().attributes.insert(
    ///     "xmlns:android".into(), "http://schemas.android.com/apk/res/android".into());
    /// source.edit_manifest(&[AndroidManifestEdit::Upsert {
    ///     path: vec![AndroidManifestSelector {
    ///         name: "application".into(), attributes: BTreeMap::new(),
    ///     }],
    ///     attributes: BTreeMap::from([("android:label".into(),
    ///         AndroidManifestAttributeEdit::Set("@string/app_name".into()))]),
    ///     append: vec![],
    /// }])?;
    /// assert!(source.manifest.is_some());
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn edit_manifest(&mut self, edits: &[AndroidManifestEdit]) -> Result<()> {
        for edit in edits {
            let path = match edit {
                AndroidManifestEdit::Remove { path } => {
                    ensure!(!path.is_empty(), "cannot remove the Manifest root");
                    path
                }
                AndroidManifestEdit::Upsert { path, .. } => path,
            };
            for selector in path {
                ensure!(
                    !selector.name.trim().is_empty(),
                    "empty Manifest selector name"
                );
            }
        }
        if edits.is_empty()
            || (self.manifest.is_none()
                && edits
                    .iter()
                    .all(|edit| matches!(edit, AndroidManifestEdit::Remove { .. })))
        {
            return Ok(());
        }
        let mut root = self
            .manifest
            .clone()
            .unwrap_or_else(|| XmlElement::new("manifest"));
        ensure!(root.name == "manifest", "Manifest root must be <manifest>");
        for (index, edit) in edits.iter().enumerate() {
            apply(&mut root, edit).with_context(|| format!("Manifest edit {index}"))?;
        }
        self.manifest = Some(root);
        Ok(())
    }
}

fn apply(root: &mut XmlElement, edit: &AndroidManifestEdit) -> Result<()> {
    match edit {
        AndroidManifestEdit::Upsert {
            path,
            attributes,
            append,
        } => {
            if let Some(selector) = path.last() {
                for (key, value) in &selector.attributes {
                    if let Some(edit) = attributes.get(key) {
                        ensure!(
                            matches!(edit, AndroidManifestAttributeEdit::Set(v)
                            | AndroidManifestAttributeEdit::Override(v) if v == value),
                            "cannot change selector identity attribute {key}; remove and add the element instead"
                        );
                    }
                }
            }
            let mut element = root;
            for selector in path {
                let index = match find(element, selector)? {
                    Some(index) => index,
                    None => {
                        let child = XmlNode::Element(XmlElement {
                            name: selector.name.clone(),
                            attributes: selector.attributes.clone(),
                            children: Vec::new(),
                        });
                        insert(element, child)
                    }
                };
                let XmlNode::Element(child) = &mut element.children[index] else {
                    unreachable!()
                };
                element = child;
            }
            for (key, edit) in attributes {
                match edit {
                    AndroidManifestAttributeEdit::Set(value) => {
                        ensure!(
                            element.attributes.get(key).is_none_or(|v| v == value),
                            "conflicting Manifest attribute {}.{key}",
                            element.name
                        );
                        element.attributes.insert(key.clone(), value.clone());
                    }
                    AndroidManifestAttributeEdit::Override(value) => {
                        element.attributes.insert(key.clone(), value.clone());
                    }
                    AndroidManifestAttributeEdit::Remove => {
                        element.attributes.remove(key);
                    }
                }
            }
            for node in append {
                if !element.children.contains(node) {
                    insert(element, node.clone());
                }
            }
        }
        AndroidManifestEdit::Remove { path } => {
            ensure!(!path.is_empty(), "cannot remove the Manifest root");
            let mut parent = root;
            for selector in &path[..path.len() - 1] {
                let Some(index) = find(parent, selector)? else {
                    return Ok(());
                };
                let XmlNode::Element(child) = &mut parent.children[index] else {
                    unreachable!()
                };
                parent = child;
            }
            if let Some(index) = find(parent, path.last().expect("nonempty path"))? {
                parent.children.remove(index);
            }
        }
    }
    Ok(())
}

fn find(element: &XmlElement, selector: &AndroidManifestSelector) -> Result<Option<usize>> {
    ensure!(
        !selector.name.trim().is_empty(),
        "empty Manifest selector name"
    );
    let matches: Vec<_> = element
        .children
        .iter()
        .enumerate()
        .filter_map(|(index, node)| match node {
            XmlNode::Element(child)
                if child.name == selector.name
                    && selector
                        .attributes
                        .iter()
                        .all(|(k, v)| child.attributes.get(k) == Some(v)) =>
            {
                Some(index)
            }
            _ => None,
        })
        .collect();
    ensure!(
        matches.len() <= 1,
        "ambiguous Manifest selector {} below {}",
        selector.name,
        element.name
    );
    Ok(matches.first().copied())
}

fn insert(parent: &mut XmlElement, child: XmlNode) -> usize {
    let before_application = parent.name == "manifest"
        && matches!(&child, XmlNode::Element(element) if element.name != "application");
    let index = if before_application {
        parent
            .children
            .iter()
            .position(|node| matches!(node, XmlNode::Element(e) if e.name == "application"))
            .unwrap_or(parent.children.len())
    } else {
        parent.children.len()
    };
    parent.children.insert(index, child);
    index
}
