//! Thin HTTPS client for the iTunes Search API.
//!
//! Endpoint: `https://itunes.apple.com/search` — public, no auth.
//! Docs: <https://developer.apple.com/library/archive/documentation/AudioVideo/Conceptual/iTuneSearchAPI/Searching.html>
//!
//! Only the fields the UI actually uses are deserialised. Unknown
//! fields are dropped by serde's default (no `deny_unknown_fields`)
//! so an upstream addition (e.g. a new metadata field) doesn't
//! break parsing.
//!
//! Responses are mapped directly into [`podcast_domain::Podcast`]
//! — the wire shape and the domain shape are close enough that a
//! handful of `#[serde(rename = ...)]` in the domain crate covers
//! the gap, with no intermediate DTO layer. If the shapes diverge
//! later, introduce a `dto` submodule here and map explicitly.

use podcast_domain::Podcast;
use serde::Deserialize;

/// Failure modes a `fetch_*` call can hit. `Network` covers DNS,
/// TLS, connect, and read errors uniformly — callers don't need to
/// distinguish between "connection refused" and "TLS handshake
/// failed" to render a "you're offline" message. `Parse` is the
/// JSON-decode path: surfaced separately because it indicates an
/// upstream change (or a corrupted response), not a user-network
/// issue.
#[derive(Debug, Clone)]
pub enum FetchError {
    /// Network-layer failure (DNS / TCP / TLS / I/O).
    Network(String),
    /// HTTP status outside 2xx.
    Status(u16),
    /// Response body didn't parse as the expected JSON shape.
    Parse(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(m) => write!(f, "network: {m}"),
            Self::Status(c) => write!(f, "http {c}"),
            Self::Parse(m) => write!(f, "parse: {m}"),
        }
    }
}

impl std::error::Error for FetchError {}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: Vec<Podcast>,
}

/// One iTunes search query. `term` is required by the API even
/// when filtering by `genre_id`. `country` defaults to "US" — the
/// browse screen design tracks the US store; localising later is a
/// follow-up.
pub struct SearchQuery<'a> {
    pub term: &'a str,
    pub limit: u32,
}

/// Blocking GET against the iTunes search endpoint. Must be called
/// from a worker thread (Whisker `run_blocking`) — never directly
/// from the main TASM thread.
///
/// The iTunes API returns 200 with an empty `results` array for
/// unknown terms; that's not modelled as an error here. Callers
/// that want to render an "empty state" can check `Vec::is_empty`.
pub fn search_blocking(q: SearchQuery<'_>) -> Result<Vec<Podcast>, FetchError> {
    let url = format!(
        "https://itunes.apple.com/search?media=podcast&entity=podcast&country=US&limit={}&term={}",
        q.limit,
        urlencode(q.term),
    );

    let response = ureq::get(&url).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => FetchError::Status(code),
        ureq::Error::Transport(t) => FetchError::Network(t.to_string()),
    })?;

    let body = response
        .into_string()
        .map_err(|e| FetchError::Network(format!("read: {e}")))?;

    let parsed: SearchResponse =
        serde_json::from_str(&body).map_err(|e| FetchError::Parse(e.to_string()))?;

    Ok(parsed.results)
}

/// Minimal URL-encoder for the `term` parameter. iTunes accepts
/// `+` for spaces and percent-encodes everything else; we only
/// need the small alphabet our hard-coded section seeds use, so
/// the table doesn't need to be exhaustive.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencode_basics() {
        assert_eq!(urlencode("news"), "news");
        assert_eq!(urlencode("true crime"), "true+crime");
        assert_eq!(urlencode("a&b"), "a%26b");
    }
}
