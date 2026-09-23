//! Versioned subprocess protocol for declarative project plugins.
//!
//! Engines first send Describe with no project data. Only after checking the
//! protocol, IR schema, plugin identity, and ordering do they send Contribute
//! in a fresh process. Both directions check versions before decoding payloads.
//! There is no fallback to the unversioned legacy mobile protocol.
//!
//! Bump the IR schema version whenever the context, update, or nested project
//! wire schema changes, even for fields with serde defaults. Exact agreement
//! prevents an older plugin from silently omitting declarations in a replacement.
//! Incompatible composition semantics also require a schema bump, such as the
//! empty Android/Apple application identity used by initializer contributions.
//! Bump the protocol version for changes to message/handshake semantics.

use super::{ProjectContext, ProjectPlugin, ProjectUpdate};
use crate::PluginConfig;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::{Read, Write};

/// Required wire and project schema identities, independent of crate versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Protocol {
    /// Envelope and handshake version.
    pub version: u32,
    /// Exact context, project model, and update schema version.
    pub ir_schema: u32,
}

impl Protocol {
    /// The only protocol/schema pair supported by this implementation.
    pub const CURRENT: Self = Self {
        version: 1,
        ir_schema: 6,
    };

    /// Reject unsupported versions; never silently downgrade or supply defaults.
    pub fn check(self) -> Result<()> {
        ensure!(
            self == Self::CURRENT,
            "unsupported project plugin protocol/schema {self:?}; expected {:?}",
            Self::CURRENT
        );
        Ok(())
    }
}

/// A plugin's identity and deterministic execution constraints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    /// Stable PluginConfig::NAME.
    pub name: String,
    /// Plugins that must run first.
    pub after: Vec<String>,
    /// Plugins that must run later.
    pub before: Vec<String>,
}

impl Descriptor {
    /// Obtain the same metadata for in-process and subprocess implementations.
    pub fn of<P: ProjectPlugin>(plugin: &P) -> Self {
        Self {
            name: P::Config::NAME.into(),
            after: plugin.after().iter().map(|s| (*s).into()).collect(),
            before: plugin.before().iter().map(|s| (*s).into()).collect(),
        }
    }
}

/// Engine-to-plugin payload, enclosed in a versioned envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    /// Query compatibility and execution constraints without supplying an IR.
    Describe,
    /// Evaluate a contribution against the current project.
    Contribute {
        /// Descriptor accepted during preflight; must still match this binary.
        descriptor: Descriptor,
        /// Null means the plugin's default configuration.
        config: serde_json::Value,
        /// Read-only project state and input root.
        context: Box<ProjectContext>,
    },
}

/// Plugin-to-engine payload, enclosed in a versioned envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum Response {
    /// Compatibility query result.
    Describe {
        /// Plugin identity and ordering constraints.
        descriptor: Descriptor,
    },
    /// One explicit operation; the engine retains ownership of its context.
    Contribute {
        /// Must match the preflight descriptor.
        descriptor: Descriptor,
        /// Operation checked and applied by the engine.
        update: ProjectUpdate,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<T> {
    protocol: Protocol,
    payload: T,
}

/// Encode a payload with the current mandatory protocol/schema header.
pub fn encode<T: Serialize>(payload: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(&Envelope {
        protocol: Protocol::CURRENT,
        payload,
    })
    .context("encode project plugin message")
}

/// Validate the header before decoding a typed payload. Missing versions,
/// unknown envelope fields, unsupported versions, and malformed data are errors.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let envelope: Envelope<serde_json::Value> =
        serde_json::from_slice(bytes).context("decode versioned project plugin envelope")?;
    envelope.protocol.check()?;
    serde_json::from_value(envelope.payload).context("decode project plugin payload")
}

/// Validate configuration and invoke an in-process plugin with wire-equivalent
/// default handling. Errors include the plugin identity; no update is applied.
pub fn contribute<P: ProjectPlugin>(
    plugin: &P,
    context: &ProjectContext,
    config: &serde_json::Value,
) -> Result<ProjectUpdate> {
    let config = if config.is_null() {
        P::Config::default()
    } else {
        serde_json::from_value(config.clone())
            .with_context(|| format!("decode configuration for `{}`", P::Config::NAME))?
    };
    plugin
        .validate(&config)
        .with_context(|| format!("validate `{}`", P::Config::NAME))?;
    plugin
        .contribute(context, &config)
        .with_context(|| format!("contribute `{}`", P::Config::NAME))
}

/// Serve exactly one Describe or Contribute request over supplied streams.
/// Describe does not validate configuration or execute the plugin. Version and
/// descriptor mismatches are rejected before contribution code runs.
pub fn serve<P: ProjectPlugin>(
    plugin: P,
    mut input: impl Read,
    mut output: impl Write,
) -> Result<()> {
    let mut bytes = Vec::new();
    input
        .read_to_end(&mut bytes)
        .context("read project plugin request")?;
    let descriptor = Descriptor::of(&plugin);
    let response = match decode(&bytes)? {
        Request::Describe => Response::Describe { descriptor },
        Request::Contribute {
            descriptor: expected,
            config,
            context,
        } => {
            ensure!(
                descriptor == expected,
                "project plugin descriptor changed or identity mismatch"
            );
            let update = contribute(&plugin, &context, &config)?;
            Response::Contribute { descriptor, update }
        }
    };
    output
        .write_all(&encode(&response)?)
        .context("write project plugin response")?;
    output.write_all(b"\n")?;
    Ok(())
}

/// Entry point for a project plugin executable. Keep diagnostics on stderr.
/// Each invocation handles one request and exits; an engine uses separate
/// invocations for preflight and contribution.
pub fn run_as_subprocess<P: ProjectPlugin>(plugin: P) -> Result<()> {
    serve(plugin, std::io::stdin().lock(), std::io::stdout().lock())
}

#[cfg(test)]
mod tests;
