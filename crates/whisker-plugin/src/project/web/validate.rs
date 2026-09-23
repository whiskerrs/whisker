use super::*;
use crate::project::validate::{files, ids, unique_paths};
use crate::project::{ResourceKind, RustArtifactKind};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeSet;

pub(in crate::project) fn validate_web(web: &WebProjectIr) -> Result<()> {
    ensure!(
        web.wasm.as_ref().context("missing Web wasm build")?.kind == RustArtifactKind::Cdylib,
        "Web wasm artifact must be a cdylib"
    );
    let document = web.document.as_ref().context("missing Web document")?;
    url_directory(&web.base_path)?;
    files(&web.files)?;
    ids(web.generated_artifacts.keys())?;
    ids(web.service_workers.keys())?;
    unique_paths(
        std::iter::once(document)
            .chain(web.generated_artifacts.values())
            .chain(web.resources.iter().map(|r| &r.destination))
            .chain(web.manifest.iter().map(|m| &m.path)),
        "Web distribution",
    )?;
    attributes(&web.html_attributes)?;
    ensure!(
        !web.html_attributes
            .keys()
            .any(|k| k.eq_ignore_ascii_case("lang")),
        "html lang is owned by WebProjectIr.lang"
    );
    let mut bases = 0;
    for list in [&web.head, &web.body_before, &web.body, &web.body_after] {
        let mut seen = BTreeSet::new();
        for item in list {
            ensure!(
                !item.id.trim().is_empty() && seen.insert(&item.id),
                "empty or duplicate HTML contribution ID"
            );
            html(&item.element, web.manifest.is_some(), &mut bases)?;
        }
    }
    ensure!(bases <= 1, "multiple HTML base elements");
    // A base element belongs in head and must be an immediate metadata child.
    for item in web
        .body_before
        .iter()
        .chain(&web.body)
        .chain(&web.body_after)
    {
        ensure!(!contains_base(&item.element), "HTML base must be in head");
    }
    for item in &web.head {
        for child in &item.element.children {
            if let HtmlNode::Element(child) = child {
                ensure!(
                    !contains_base(child),
                    "HTML base must be a direct head child"
                );
            }
        }
    }
    let mut scopes = BTreeSet::new();
    for worker in web.service_workers.values() {
        ensure!(
            distributed(web, &worker.script),
            "service worker script is not distributed: {}",
            worker.script.as_str()
        );
        let scope = if let Some(scope) = &worker.scope {
            url_directory(scope)?;
            scope.clone()
        } else {
            let url = web.output_url(&worker.script)?;
            url[..=url.rfind('/').expect("rooted URL")].to_owned()
        };
        ensure!(scopes.insert(scope), "duplicate service worker scope");
    }
    let mut rules = BTreeSet::new();
    for rule in &web.response_headers {
        ensure!(
            !rule.id.trim().is_empty() && rules.insert(&rule.id),
            "empty or duplicate response rule ID"
        );
        for (name, value) in &rule.headers {
            ensure!(
                !name.is_empty()
                    && name.bytes().all(|b| b.is_ascii_lowercase()
                        || b.is_ascii_digit()
                        || b"!#$%&'*+-.^_`|~".contains(&b)),
                "HTTP header names must be lowercase tokens"
            );
            ensure!(
                !value.chars().any(|c| c.is_control() && c != '\t'),
                "control character in HTTP header value"
            );
        }
    }
    Ok(())
}
pub(super) fn url_directory(value: &str) -> Result<()> {
    ensure!(
        value.starts_with('/')
            && !value.starts_with("//")
            && value.ends_with('/')
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/-._~".contains(&b))
            && (value == "/"
                || value[1..value.len() - 1]
                    .split('/')
                    .all(|s| !matches!(s, "" | "." | ".."))),
        "expected an origin-relative URL directory using unreserved ASCII characters: {value:?}"
    );
    Ok(())
}
fn distributed(web: &WebProjectIr, path: &ProjectPath) -> bool {
    web.generated_artifacts.values().any(|p| p == path)
        || web.resources.iter().any(|r| {
            r.destination == *path && r.kind == ResourceKind::File
                || r.kind == ResourceKind::Directory
                    && path
                        .as_str()
                        .strip_prefix(r.destination.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
        })
}
fn attributes(attrs: &BTreeMap<String, Option<String>>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for name in attrs.keys() {
        ensure!(
            !name.is_empty()
                && !name
                    .chars()
                    .any(|c| c.is_control() || c.is_whitespace() || "\"'>/=<".contains(c)),
            "invalid HTML attribute name"
        );
        ensure!(
            seen.insert(name.to_ascii_lowercase()),
            "case-colliding HTML attributes"
        );
    }
    Ok(())
}
fn contains_base(e: &HtmlElement) -> bool {
    e.name.eq_ignore_ascii_case("base")
        || e.children
            .iter()
            .any(|c| matches!(c,HtmlNode::Element(c) if contains_base(c)))
}
fn html(e: &HtmlElement, manifest: bool, bases: &mut usize) -> Result<()> {
    let name = e.name.to_ascii_lowercase();
    ensure!(
        !name.is_empty()
            && name.as_bytes()[0].is_ascii_alphabetic()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-:".contains(&b)),
        "invalid HTML tag name"
    );
    attributes(&e.attributes)?;
    ensure!(name != "title", "HTML title is owned by WebProjectIr.title");
    if name == "base" {
        *bases += 1;
    }
    if manifest && name == "link" {
        for (key, value) in &e.attributes {
            if key.eq_ignore_ascii_case("rel") {
                ensure!(
                    !value
                        .as_deref()
                        .unwrap_or("")
                        .split_ascii_whitespace()
                        .any(|v| v.eq_ignore_ascii_case("manifest")),
                    "manifest link is owned by WebAppManifest"
                );
            }
        }
    }
    if [
        "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
        "source", "track", "wbr",
    ]
    .contains(&name.as_str())
    {
        ensure!(
            e.children.is_empty(),
            "HTML void element {name} cannot have children"
        );
    }
    if matches!(name.as_str(), "script" | "style") {
        let mut text = String::new();
        for node in &e.children {
            let HtmlNode::Text(value) = node else {
                anyhow::bail!("raw-text HTML element cannot have element children");
            };
            text.push_str(value);
        }
        // Reject closing tags even when a dangerous sequence crosses text nodes.
        ensure!(
            !text.to_ascii_lowercase().contains(&format!("</{name}")),
            "raw-text HTML contains a closing-tag sequence"
        );
    }
    for node in &e.children {
        if let HtmlNode::Element(child) = node {
            html(child, manifest, bases)?;
        }
    }
    Ok(())
}
