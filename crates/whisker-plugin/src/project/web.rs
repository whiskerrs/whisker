//! Static Web document, resources, and progressive Web app declarations

use super::{ProjectFiles, ProjectPath, Resource, RustBuild};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod compose;
#[cfg(test)]
mod tests;
mod validate;
pub(super) use validate::validate_web;

/// A Web Host's document and distribution inputs
///
/// Static metadata is generated before launch; runtime DOM changes are outside
/// this model. Resources must be carried into the final distribution, not merely
/// written to the intermediate generation directory.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebProjectIr {
    /// Cargo cdylib compiled to WebAssembly. Absent during composition only.
    pub wasm: Option<RustBuild>,
    /// Entry HTML output path. Absent during composition; required at validation.
    pub document: Option<ProjectPath>,
    /// Backend-defined generated artifacts keyed by stable output role (e.g. wasm/js).
    /// All output paths must be declared; the backend rejects unsupported roles.
    #[serde(default)]
    pub generated_artifacts: BTreeMap<String, ProjectPath>,
    /// Root html attributes excluding model-owned lang.
    #[serde(default)]
    pub html_attributes: BTreeMap<String, Option<String>>,
    /// Document title.
    pub title: String,
    /// Document language, such as en or ja.
    pub lang: String,
    /// Origin-relative directory prefix, such as `/` or `/my-app/`.
    /// Uses unreserved ASCII segments; no dot segments, percent escapes, query,
    /// fragment, or authority. Distinct from the document's HTML base element.
    pub base_path: String,
    /// Ordered additions to the document head, excluding the title above.
    #[serde(default)]
    pub head: Vec<WebHtmlContribution>,
    /// Application body, including the runtime mount and bootstrap declarations.
    #[serde(default)]
    pub body: Vec<WebHtmlContribution>,
    /// Static body content before the application body, such as fallback markup.
    #[serde(default)]
    pub body_before: Vec<WebHtmlContribution>,
    /// Static body content after the application body.
    #[serde(default)]
    pub body_after: Vec<WebHtmlContribution>,
    /// Resources copied relative to the Web distribution root.
    #[serde(default)]
    pub resources: Vec<Resource>,
    /// Optional Web App Manifest and its output path.
    pub manifest: Option<WebAppManifest>,
    /// Registrations keyed by stable ID; each resolved scope must be distinct.
    #[serde(default)]
    pub service_workers: BTreeMap<String, ServiceWorker>,
    /// Ordered server response rules. Later matching rules override earlier values.
    /// Hosting backends must report unsupported rules rather than discard them.
    #[serde(default)]
    pub response_headers: Vec<WebResponseHeaders>,
    /// Files staged at the generated project root.
    #[serde(default)]
    pub files: ProjectFiles,
}

/// An HTML element retaining boolean attributes and child order
///
/// `None` attribute values denote boolean attributes. Text is logical content;
/// the HTML renderer owns escaping and script/style raw-text rules. This is a
/// document declaration, not a runtime Whisker element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HtmlElement {
    /// HTML tag name.
    pub name: String,
    /// Attributes; None represents presence without a value.
    #[serde(default)]
    pub attributes: BTreeMap<String, Option<String>>,
    /// Ordered child elements and text.
    #[serde(default)]
    pub children: Vec<HtmlNode>,
}

/// A node in static HTML
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HtmlNode {
    /// Nested HTML element.
    Element(HtmlElement),
    /// Text interpreted according to the containing element's HTML rules.
    Text(String),
}

/// Web App Manifest properties and their distribution path
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebAppManifest {
    /// Output path relative to the Web distribution root.
    pub path: ProjectPath,
    /// Cross-origin request credentials for the generated rel=manifest link.
    pub crossorigin: Option<WebCrossOrigin>,
    /// Native manifest JSON, retaining nested icons, shortcuts, and share_target.
    pub properties: BTreeMap<String, serde_json::Value>,
}

/// Service worker registration settings
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceWorker {
    /// Script path in the final distribution, resolved under WebProjectIr.base_path.
    pub script: ProjectPath,
    /// Origin-relative directory URL, e.g. /app/. None uses the script directory.
    /// Wider scopes may require a Service-Worker-Allowed response header.
    pub scope: Option<String>,
    /// Cache policy for worker update requests; None preserves the browser default.
    pub update_via_cache: Option<ServiceWorkerUpdateViaCache>,
    /// JavaScript loading semantics.
    pub kind: ServiceWorkerKind,
}

/// JavaScript service worker loading mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceWorkerKind {
    /// Classic script.
    Classic,
    /// JavaScript module.
    Module,
}

/// A named ordered static HTML contribution
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebHtmlContribution {
    /// Stable identity within its head/body list, independent of tag name.
    pub id: String,
    /// Element and its ordered contents.
    pub element: HtmlElement,
}

/// Manifest-link CORS setting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WebCrossOrigin {
    /// Anonymous CORS request.
    Anonymous,
    /// Include credentials in the CORS request.
    UseCredentials,
}

/// Browser cache policy when updating a service worker
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceWorkerUpdateViaCache {
    /// Consult cache only for imported scripts.
    Imports,
    /// Consult cache for the main script and imports.
    All,
    /// Bypass cache for the main script and imports.
    None,
}

/// A server-neutral response-header rule
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebResponseHeaders {
    /// Stable rule identity; declaration order defines override precedence.
    pub id: String,
    /// Distribution paths to which this rule applies.
    pub scope: WebResponseScope,
    /// Lowercase HTTP header name to value. Multiple values use native field syntax.
    pub headers: BTreeMap<String, String>,
}

/// Response rule matching in the final distribution
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WebResponseScope {
    /// Every output in the deployment.
    All,
    /// One exact file output.
    File {
        /// Distribution-relative output path.
        path: ProjectPath,
    },
    /// All descendants of a distribution directory.
    Directory {
        /// Distribution-relative directory.
        path: ProjectPath,
    },
}

impl WebProjectIr {
    /// Resolve a distribution path to an origin-relative URL beneath base_path
    ///
    /// Base paths use a restricted URL directory spelling checked by structural
    /// validation. File path segments are percent-encoded as UTF-8 bytes so that
    /// spaces, %, #, and ? remain filename characters, not URL delimiters.
    /// HTML and manifest URL-valued attributes retain their own native bases.
    pub fn output_url(&self, path: &ProjectPath) -> anyhow::Result<String> {
        validate::url_directory(&self.base_path)?;
        let mut url = self.base_path.clone();
        for byte in path.as_str().bytes() {
            if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
                url.push(char::from(byte));
            } else {
                use std::fmt::Write;
                write!(url, "%{byte:02X}").expect("writing a String");
            }
        }
        Ok(url)
    }
}
