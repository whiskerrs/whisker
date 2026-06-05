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

use podcast_domain::{Episode, Podcast};
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

/// One iTunes `lookup` response item — either the podcast
/// metadata (when `wrapperType == "track"` and `kind ==
/// "podcast"`) or an episode (when `wrapperType ==
/// "podcastEpisode"`). The lookup endpoint mixes both in one
/// `results` array.
///
/// We only care about the discriminator + the episode payload;
/// the `Episode` variant deserialises straight into the domain
/// type. The non-episode entry is dropped silently.
#[derive(Debug, Deserialize)]
struct LookupItem {
    #[serde(rename = "wrapperType", default)]
    wrapper_type: String,
    #[serde(flatten)]
    episode: Option<Episode>,
}

#[derive(Debug, Deserialize)]
struct LookupResponse {
    #[serde(default)]
    results: Vec<LookupItem>,
}

/// Blocking lookup of every episode for `collection_id`. iTunes
/// caps `limit` at 200; the call returns the show row as the
/// first result (we drop it) followed by episodes in reverse
/// chronological order.
///
/// Episodes that don't carry an `episodeUrl` are filtered out —
/// they can't be played, and the rest of the pipeline assumes
/// every rendered row has a URL to hand to the audio module.
pub fn fetch_episodes_blocking(collection_id: u64, limit: u32) -> Result<Vec<Episode>, FetchError> {
    let url = format!(
        "https://itunes.apple.com/lookup?id={}&entity=podcastEpisode&limit={}",
        collection_id, limit,
    );

    let response = ureq::get(&url).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => FetchError::Status(code),
        ureq::Error::Transport(t) => FetchError::Network(t.to_string()),
    })?;

    let body = response
        .into_string()
        .map_err(|e| FetchError::Network(format!("read: {e}")))?;

    let parsed: LookupResponse =
        serde_json::from_str(&body).map_err(|e| FetchError::Parse(e.to_string()))?;

    Ok(parsed
        .results
        .into_iter()
        .filter(|i| i.wrapper_type == "podcastEpisode")
        .filter_map(|i| i.episode)
        .filter(|ep| {
            ep.episode_url
                .as_deref()
                .map(|u| !u.is_empty())
                .unwrap_or(false)
        })
        .collect())
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

    /// Sample mirroring the iTunes `entity=podcastEpisode` wire
    /// shape — first row is the show, second is an episode. The
    /// pipeline must drop the show row, accept the episode, and
    /// surface the wire fields onto the domain type.
    #[test]
    fn lookup_response_parses_episodes_and_skips_show() {
        let body = r#"{
            "resultCount": 2,
            "results": [
                {
                    "wrapperType": "track",
                    "kind": "podcast",
                    "collectionId": 1528594034,
                    "collectionName": "The Interview"
                },
                {
                    "wrapperType": "podcastEpisode",
                    "trackId": 1000700000000,
                    "trackName": "Episode A",
                    "episodeUrl": "https://cdn.example.com/a.mp3",
                    "trackTimeMillis": 1920000,
                    "releaseDate": "2026-05-30T10:00:00Z",
                    "description": "notes"
                }
            ]
        }"#;
        let parsed: LookupResponse = serde_json::from_str(body).expect("parse");
        let episodes: Vec<Episode> = parsed
            .results
            .into_iter()
            .filter(|i| i.wrapper_type == "podcastEpisode")
            .filter_map(|i| i.episode)
            .collect();
        assert_eq!(episodes.len(), 1);
        let ep = &episodes[0];
        assert_eq!(ep.id, 1000700000000);
        assert_eq!(ep.track_name, "Episode A");
        assert_eq!(
            ep.episode_url.as_deref(),
            Some("https://cdn.example.com/a.mp3")
        );
        assert_eq!(ep.track_time_millis, Some(1920000));
        assert_eq!(ep.release_date.as_deref(), Some("2026-05-30T10:00:00Z"));
    }

    /// Episodes with an empty / missing `episodeUrl` are unplayable;
    /// the production filter drops them. Spot-check the filter
    /// runs end-to-end on the parsed shape.
    #[test]
    fn lookup_filters_episodes_without_url() {
        let body = r#"{
            "resultCount": 2,
            "results": [
                {"wrapperType": "podcastEpisode", "trackId": 1, "trackName": "A"},
                {"wrapperType": "podcastEpisode", "trackId": 2, "trackName": "B",
                 "episodeUrl": "https://x/b.mp3"}
            ]
        }"#;
        let parsed: LookupResponse = serde_json::from_str(body).expect("parse");
        let playable: Vec<Episode> = parsed
            .results
            .into_iter()
            .filter(|i| i.wrapper_type == "podcastEpisode")
            .filter_map(|i| i.episode)
            .filter(|ep| {
                ep.episode_url
                    .as_deref()
                    .map(|u| !u.is_empty())
                    .unwrap_or(false)
            })
            .collect();
        assert_eq!(playable.len(), 1);
        assert_eq!(playable[0].id, 2);
    }
}
