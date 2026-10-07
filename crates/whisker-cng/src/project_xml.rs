//! Shared structured XML serialization for native metadata.
use crate::render::escape_xml;
use anyhow::{Result, ensure};
use std::collections::BTreeMap;
use whisker_plugin::project::{XmlElement, XmlNode};
pub(crate) fn xml(node: &XmlElement, inherited: &BTreeMap<String, String>) -> Result<String> {
    let mut namespaces = inherited.clone();
    namespaces.insert("xml".into(), "http://www.w3.org/XML/1998/namespace".into());
    for (k, v) in &node.attributes {
        if let Some(prefix) = k.strip_prefix("xmlns:") {
            ensure!(
                prefix != "xml" || v == "http://www.w3.org/XML/1998/namespace",
                "invalid xml namespace binding"
            );
            namespaces.insert(prefix.into(), v.clone());
        }
    }
    let check = |s: &str| -> Result<()> {
        ensure!(
            s.split(':').count() <= 2
                && s.split(':').all(|part| !part.is_empty()
                    && part.chars().enumerate().all(|(i, c)| c == '_'
                        || c.is_alphabetic()
                        || i > 0 && (c.is_ascii_digit() || c == '-' || c == '.'))),
            "invalid XML name {s:?}"
        );
        if let Some((prefix, _)) = s.split_once(':') {
            ensure!(
                prefix == "xmlns" || namespaces.contains_key(prefix),
                "unbound XML prefix {prefix}"
            );
        }
        Ok(())
    };
    check(&node.name)?;
    let escape = |s: &str| -> Result<String> {
        ensure!(
            s.chars().all(|c| matches!(c, '\t' | '\n' | '\r')
                || c >= ' ' && c != '\u{fffe}' && c != '\u{ffff}'),
            "invalid XML character"
        );
        Ok(escape_xml(s)
            .replace('\n', "&#10;")
            .replace('\r', "&#13;")
            .replace('\t', "&#9;"))
    };
    let mut out = format!("<{}", node.name);
    for (k, v) in &node.attributes {
        check(k)?;
        out += &format!(" {k}=\"{}\"", escape(v)?);
    }
    if node.children.is_empty() {
        out += " />";
    } else {
        out.push('>');
        for child in &node.children {
            out += &match child {
                XmlNode::Text(t) => escape(t)?,
                XmlNode::Element(e) => xml(e, &namespaces)?,
            };
        }
        out += &format!("</{}>", node.name);
    }
    Ok(out)
}
