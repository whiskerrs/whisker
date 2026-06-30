//! Pure value types for the Bluesky example.
//!
//! No I/O, no UI, no whisker dependency — the data layer maps AT Protocol wire
//! types into these, and the UI renders them. Kept deliberately small: only the
//! fields the timeline's `PostCard` needs for milestone 1.

use serde::{Deserialize, Serialize};

/// The author of a post (atproto `ProfileViewBasic`, trimmed).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
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
    /// Content hash of the record — needed (with `uri`) to build the
    /// strong reference a like / repost record points at.
    pub cid: String,
    pub author: Author,
    /// The post body text.
    pub text: String,
    /// Engagement counts (default 0 when the API omits them).
    pub reply_count: u64,
    pub repost_count: u64,
    pub like_count: u64,
    /// Viewer state: the `at://` URI of *this viewer's* like / repost
    /// record on the post, if any. `Some` ⇒ liked / reposted (and the
    /// URI is what `deleteRecord` needs to undo it).
    pub like_uri: Option<String>,
    pub repost_uri: Option<String>,
    /// ISO-8601 timestamp the post was indexed.
    pub indexed_at: String,
}

/// A post thread (atproto `getPostThread`): the focused post plus its
/// direct replies. Parent context is omitted for now (see MEMO).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Thread {
    pub post: Option<FeedPost>,
    pub replies: Vec<FeedPost>,
}

/// A user profile (atproto `getProfile` → `ProfileViewDetailed`, trimmed).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Profile {
    pub did: String,
    pub handle: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub avatar: Option<String>,
    pub banner: Option<String>,
    pub followers_count: u64,
    pub follows_count: u64,
    pub posts_count: u64,
    /// Viewer state: the `at://` URI of *this viewer's* follow record on
    /// this account, if any. `Some` ⇒ following.
    pub following_uri: Option<String>,
}

impl Profile {
    /// Display name if set and non-empty, else `@handle`.
    pub fn name(&self) -> String {
        match &self.display_name {
            Some(n) if !n.trim().is_empty() => n.clone(),
            _ => format!("@{}", self.handle),
        }
    }
}

/// A search-result actor (atproto `ProfileView`, trimmed). Lighter than
/// [`Profile`] — `searchActors` returns no counts or banner.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ActorView {
    pub did: String,
    pub handle: String,
    pub display_name: Option<String>,
    pub avatar: Option<String>,
    pub description: Option<String>,
}

impl ActorView {
    /// Display name if set and non-empty, else `@handle`.
    pub fn name(&self) -> String {
        match &self.display_name {
            Some(n) if !n.trim().is_empty() => n.clone(),
            _ => format!("@{}", self.handle),
        }
    }
}

/// One notification (atproto `listNotifications` → `Notification`,
/// trimmed). `reason` is `like` / `repost` / `follow` / `mention` /
/// `reply` / `quote` / …; `text` carries the post body for the reasons
/// whose record is a post (reply / mention / quote).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Notification {
    /// `at://` URI of the notification record (a post for reply/mention/
    /// quote; a like/repost/follow record otherwise). Used as the list key.
    pub uri: String,
    pub reason: String,
    /// `at://` URI of the subject the notification is about (e.g. the post
    /// that was liked / reposted). Absent for follows.
    pub reason_subject: Option<String>,
    pub author: Author,
    /// Post body, when the notification's record is itself a post.
    pub text: Option<String>,
    pub is_read: bool,
    pub indexed_at: String,
}

/// A page of the home timeline.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Timeline {
    pub posts: Vec<FeedPost>,
    /// Opaque pagination cursor for the next page (None = end / first page).
    pub cursor: Option<String>,
}
