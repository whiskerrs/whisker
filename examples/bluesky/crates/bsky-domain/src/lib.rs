//! Pure value types for the Bluesky example.
//!
//! No I/O, no UI, no whisker dependency — the data layer maps AT Protocol wire
//! types into these, and the UI renders them. Kept deliberately small: only the
//! fields the timeline's `PostCard` needs for milestone 1.

use serde::{Deserialize, Serialize};

/// The author of a post (atproto `ProfileViewBasic`, trimmed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Author {
    pub did: String,
    pub handle: String,
    /// Optional friendly name; fall back to `handle` when absent.
    pub display_name: Option<String>,
    /// Avatar image URL (CDN thumbnail), if the account has one.
    pub avatar: Option<String>,
}

impl Author {
    /// Name to show: display name if set and non-empty, else `@handle`.
    pub fn name(&self) -> String {
        match &self.display_name {
            Some(n) if !n.trim().is_empty() => n.clone(),
            _ => format!("@{}", self.handle),
        }
    }
}

/// A single timeline entry (atproto `FeedViewPost` → `PostView`, trimmed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedPost {
    /// `at://` URI — stable identity, used as the list key.
    pub uri: String,
    pub author: Author,
    /// The post body text.
    pub text: String,
    /// Engagement counts (default 0 when the API omits them).
    pub reply_count: u64,
    pub repost_count: u64,
    pub like_count: u64,
    /// ISO-8601 timestamp the post was indexed.
    pub indexed_at: String,
}

/// A page of the home timeline.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Timeline {
    pub posts: Vec<FeedPost>,
    /// Opaque pagination cursor for the next page (None = end / first page).
    pub cursor: Option<String>,
}
