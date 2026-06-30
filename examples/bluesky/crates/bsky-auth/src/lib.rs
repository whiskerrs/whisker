//! AT Protocol OAuth client for the Bluesky example.
//!
//! Wraps `atrium-oauth` behind a tiny, app-shaped API:
//! [`begin_login`] → authorization URL (shown in a WebView), then
//! [`complete_login`] with the captured redirect URL. The OAuth client is a
//! process-global singleton because its `state_store` must persist the PKCE
//! verifier / state between `authorize` and `callback`.
//!
//! TLS is rustls (not native-tls) and DNS resolution is DNS-over-HTTPS, so the
//! whole stack — including atrium's RustCrypto-based DPoP — cross-compiles to
//! iOS/Android with no OpenSSL and no reliance on system DNS config.

use std::sync::{Mutex, OnceLock};

use atrium_api::agent::{Agent, SessionManager};
use atrium_api::app::bsky::actor::{defs as actor_defs, get_profile, search_actors};
use atrium_api::app::bsky::feed::{
    get_author_feed, get_post_thread, get_timeline, like, post, repost, search_posts,
};
use atrium_api::app::bsky::graph::follow;
use atrium_api::app::bsky::notification::list_notifications;
use atrium_api::app::bsky::richtext::facet;
use atrium_api::com::atproto::repo::{create_record, delete_record, strong_ref};
use atrium_api::types::string::{AtIdentifier, Cid, Datetime, Did, Nsid, RecordKey};
use atrium_api::types::{TryFromUnknown, TryIntoUnknown, Union, Unknown};
use atrium_common::store::Store;
use atrium_identity::did::{CommonDidResolver, CommonDidResolverConfig, DEFAULT_PLC_DIRECTORY_URL};
use atrium_identity::handle::{
    AtprotoHandleResolver, AtprotoHandleResolverConfig, DohDnsTxtResolver, DohDnsTxtResolverConfig,
};
use atrium_oauth::store::session::{Session, SessionStore};
use atrium_oauth::store::state::MemoryStateStore;
use atrium_oauth::{
    AtprotoLocalhostClientMetadata, AuthorizeOptions, CallbackParams, KnownScope, OAuthClient,
    OAuthClientConfig, OAuthResolverConfig, OAuthSession, Scope,
};
use atrium_xrpc::HttpClient;
use atrium_xrpc::http::{Request, Response, Uri};
use std::sync::Arc;
use whisker_local_store::WhiskerLocalStore;
use whisker_secure_store::WhiskerSecureStore;

/// Loopback redirect URI declared for the localhost development client. The
/// authorization server redirects here; we never actually serve it — the
/// WebView's navigation callback reads the URL before it loads.
const REDIRECT_URI: &str = "http://127.0.0.1/callback";
/// DNS-over-HTTPS endpoint for `_atproto.<handle>` TXT lookups.
const DOH_SERVICE_URL: &str = "https://dns.google/dns-query";

/// Minimal [`HttpClient`] over a rustls `reqwest::Client` (mirrors
/// atrium-oauth's `DefaultHttpClient`, but with our TLS backend so no OpenSSL
/// is pulled in). atrium uses this for PAR/token/DPoP and identity resolution.
pub struct RustlsHttpClient {
    client: reqwest::Client,
}

impl Default for RustlsHttpClient {
    fn default() -> Self {
        Self {
            client: reqwest::Client::builder()
                .build()
                .expect("build rustls reqwest client"),
        }
    }
}

impl HttpClient for RustlsHttpClient {
    async fn send_http(
        &self,
        request: Request<Vec<u8>>,
    ) -> Result<Response<Vec<u8>>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let response = self.client.execute(request.try_into()?).await?;
        let mut builder = Response::builder().status(response.status());
        for (k, v) in response.headers() {
            builder = builder.header(k, v);
        }
        builder
            .body(response.bytes().await?.to_vec())
            .map_err(Into::into)
    }
}

type Http = RustlsHttpClient;
type DidR = CommonDidResolver<Http>;
type HandleR = AtprotoHandleResolver<DohDnsTxtResolver<Http>, Http>;
type Client = OAuthClient<MemoryStateStore, SecureSessionStore, DidR, HandleR, Http>;
type OAuthSess = OAuthSession<Http, DidR, HandleR, SecureSessionStore>;
type BskyAgent = Agent<OAuthSess>;

/// Key under which the active account DID (not a secret) is kept in the
/// plaintext local store, so a fresh launch knows which session to
/// restore from the secure store.
const ACTIVE_DID_KEY: &str = "bsky.active_did";

/// Secure-store key for a given account's serialized OAuth `Session`
/// (DPoP key + token set). Namespaced per-DID so multiple accounts
/// wouldn't collide.
fn session_key(did: &Did) -> String {
    format!("bsky.session.{}", did.as_str())
}

/// Error surfaced from the secure session store. `atrium_common::Store`
/// requires `type Error: std::error::Error`.
#[derive(Debug)]
pub struct SessionStoreError(String);

impl std::fmt::Display for SessionStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SessionStoreError {}

/// atrium-oauth session store backed by `whisker-secure-store`. atrium
/// calls `set` after the token exchange and on every token refresh, and
/// `get` on `restore` — so the DPoP-bound session is persisted to the
/// platform secure store (iOS Keychain / Android Tink+Keystore) and
/// survives app restarts, never touching plaintext.
///
/// The `Session` value is `serde` JSON; the store is keyed by `Did`.
/// Our app is single-account, so we also mirror the active DID into the
/// (non-secret) local store for `restore_session` to read on launch.
#[derive(Default)]
struct SecureSessionStore;

impl Store<Did, Session> for SecureSessionStore {
    type Error = SessionStoreError;

    async fn get(&self, key: &Did) -> Result<Option<Session>, Self::Error> {
        let raw = WhiskerSecureStore::load(session_key(key))
            .map_err(|e| SessionStoreError(e.to_string()))?;
        match raw {
            Some(json) => serde_json::from_str(&json)
                .map(Some)
                .map_err(|e| SessionStoreError(e.to_string())),
            None => Ok(None),
        }
    }

    async fn set(&self, key: Did, value: Session) -> Result<(), Self::Error> {
        let json = serde_json::to_string(&value).map_err(|e| SessionStoreError(e.to_string()))?;
        WhiskerSecureStore::save(session_key(&key), json)
            .map_err(|e| SessionStoreError(e.to_string()))?;
        // Remember which account is active so `restore_session` can find
        // it on the next launch (the DID is not a secret).
        let _ = WhiskerLocalStore::save(ACTIVE_DID_KEY.to_string(), key.as_str().to_string());
        Ok(())
    }

    async fn del(&self, key: &Did) -> Result<(), Self::Error> {
        WhiskerSecureStore::remove(session_key(key))
            .map_err(|e| SessionStoreError(e.to_string()))?;
        let _ = WhiskerLocalStore::remove(ACTIVE_DID_KEY.to_string());
        Ok(())
    }

    async fn clear(&self) -> Result<(), Self::Error> {
        // No key enumeration in the secure store; we only track the one
        // active account, so clear that entry (best-effort).
        if let Ok(Some(did)) = WhiskerLocalStore::load(ACTIVE_DID_KEY.to_string())
            && let Ok(did) = Did::new(did)
        {
            let _ = WhiskerSecureStore::remove(session_key(&did));
        }
        let _ = WhiskerLocalStore::remove(ACTIVE_DID_KEY.to_string());
        Ok(())
    }
}

impl SessionStore for SecureSessionStore {}

static CLIENT: OnceLock<Client> = OnceLock::new();
/// The authenticated agent, set once [`complete_login`] succeeds. Presence ==
/// logged in. Cloned out (Arc) before any await so we never hold the lock
/// across a suspension point.
static AGENT: Mutex<Option<Arc<BskyAgent>>> = Mutex::new(None);

fn build_client() -> Client {
    let http_client = Arc::new(RustlsHttpClient::default());
    let config = OAuthClientConfig {
        client_metadata: AtprotoLocalhostClientMetadata {
            redirect_uris: Some(vec![REDIRECT_URI.to_string()]),
            scopes: Some(vec![
                Scope::Known(KnownScope::Atproto),
                Scope::Known(KnownScope::TransitionGeneric),
            ]),
        },
        keys: None,
        resolver: OAuthResolverConfig {
            did_resolver: CommonDidResolver::new(CommonDidResolverConfig {
                plc_directory_url: DEFAULT_PLC_DIRECTORY_URL.to_string(),
                http_client: Arc::clone(&http_client),
            }),
            handle_resolver: AtprotoHandleResolver::new(AtprotoHandleResolverConfig {
                dns_txt_resolver: DohDnsTxtResolver::new(DohDnsTxtResolverConfig {
                    service_url: DOH_SERVICE_URL.to_string(),
                    http_client: Arc::clone(&http_client),
                }),
                http_client: Arc::clone(&http_client),
            }),
            authorization_server_metadata: Default::default(),
            protected_resource_metadata: Default::default(),
        },
        state_store: MemoryStateStore::default(),
        session_store: SecureSessionStore,
        http_client: RustlsHttpClient::default(),
    };
    OAuthClient::new(config).expect("construct atproto OAuth client")
}

fn client() -> &'static Client {
    CLIENT.get_or_init(build_client)
}

fn scopes() -> Vec<Scope> {
    vec![
        Scope::Known(KnownScope::Atproto),
        Scope::Known(KnownScope::TransitionGeneric),
    ]
}

/// Resolve `handle`'s identity, push the authorization request (PAR), and
/// return the authorization URL to load in a WebView.
pub async fn begin_login(handle: &str) -> Result<String, String> {
    client()
        .authorize(
            handle.to_string(),
            AuthorizeOptions {
                scopes: scopes(),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| e.to_string())
}

/// Process the captured redirect URL: parse `code`/`state`/`iss`, exchange the
/// code for a DPoP-bound session, and build the authenticated agent.
pub async fn complete_login(callback_url: &str) -> Result<(), String> {
    let uri = callback_url.parse::<Uri>().map_err(|e| e.to_string())?;
    let query = uri.query().ok_or("callback URL has no query string")?;
    let params: CallbackParams = serde_html_form::from_str(query).map_err(|e| e.to_string())?;
    let (session, _state) = client().callback(params).await.map_err(|e| e.to_string())?;
    // atrium has already persisted the `Session` (DPoP key + tokens) via
    // our `SecureSessionStore`; record the active DID so a future launch
    // can restore it. `did()` is async (it reads the session's `sub`).
    if let Some(did) = session.did().await {
        let _ = WhiskerLocalStore::save(ACTIVE_DID_KEY.to_string(), did.as_str().to_string());
    }
    let agent = Arc::new(Agent::new(session));
    *AGENT.lock().unwrap() = Some(agent);
    Ok(())
}

/// Restore a persisted session on launch, rebuilding the authenticated
/// agent without a fresh login. Returns `true` when a usable session was
/// restored. A stale / invalid persisted session is cleared so we don't
/// retry it on every launch.
///
/// Reads the active DID from the local store, then asks atrium to
/// `restore` it — which loads the `Session` from our secure store and
/// refreshes the access token if needed (DPoP-bound).
pub async fn restore_session() -> bool {
    let Ok(Some(did_str)) = WhiskerLocalStore::load(ACTIVE_DID_KEY.to_string()) else {
        return false;
    };
    let Ok(did) = Did::new(did_str) else {
        let _ = WhiskerLocalStore::remove(ACTIVE_DID_KEY.to_string());
        return false;
    };
    match client().restore(&did).await {
        Ok(session) => {
            *AGENT.lock().unwrap() = Some(Arc::new(Agent::new(session)));
            true
        }
        Err(_) => {
            // Session gone / unrefreshable (revoked, expired refresh,
            // keyset reset) — drop the pointer so login starts clean.
            let _ = WhiskerLocalStore::remove(ACTIVE_DID_KEY.to_string());
            false
        }
    }
}

/// Forget the current session: drop the in-memory agent and erase the
/// persisted session + active-DID pointer from the secure / local stores.
pub async fn logout() {
    if let Ok(Some(did_str)) = WhiskerLocalStore::load(ACTIVE_DID_KEY.to_string())
        && let Ok(did) = Did::new(did_str)
    {
        let _ = client().revoke(&did).await;
        let _ = WhiskerSecureStore::remove(session_key(&did));
    }
    let _ = WhiskerLocalStore::remove(ACTIVE_DID_KEY.to_string());
    *AGENT.lock().unwrap() = None;
}

/// True once a login has completed in this process.
pub fn is_authenticated() -> bool {
    AGENT.lock().unwrap().is_some()
}

/// Fetch the home timeline as domain types. Errors if not logged in.
pub async fn fetch_timeline(limit: u8) -> Result<bsky_domain::Timeline, String> {
    let agent = AGENT.lock().unwrap().clone().ok_or("not authenticated")?;
    let output = agent
        .api
        .app
        .bsky
        .feed
        .get_timeline(
            get_timeline::ParametersData {
                algorithm: None,
                cursor: None,
                limit: limit.try_into().ok(),
            }
            .into(),
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(bsky_domain::Timeline {
        cursor: output.cursor.clone(),
        posts: output.feed.iter().map(map_feed_post).collect(),
    })
}

/// Map atrium's `FeedViewPost` into our trimmed [`bsky_domain::FeedPost`].
fn map_feed_post(fv: &atrium_api::app::bsky::feed::defs::FeedViewPost) -> bsky_domain::FeedPost {
    map_post_view(&fv.post)
}

/// Map a `PostView` (the shape `getTimeline` / `getPostThread` / author
/// feeds all carry) into our [`bsky_domain::FeedPost`], including the
/// content hash and the viewer's like / repost record URIs.
fn map_post_view(p: &atrium_api::app::bsky::feed::defs::PostView) -> bsky_domain::FeedPost {
    // The post body lives in an `Unknown` record; deserialize the app.bsky.feed.post
    // shape out of it, falling back to empty text if it isn't a normal post.
    let text = post::RecordData::try_from_unknown(p.record.clone())
        .map(|r| r.text)
        .unwrap_or_default();
    let a = &p.author;
    let (like_uri, repost_uri) = match &p.viewer {
        Some(v) => (v.like.clone(), v.repost.clone()),
        None => (None, None),
    };
    bsky_domain::FeedPost {
        uri: p.uri.clone(),
        cid: p.cid.as_ref().to_string(),
        author: bsky_domain::Author {
            did: a.did.as_str().to_string(),
            handle: a.handle.as_str().to_string(),
            display_name: a.display_name.clone(),
            avatar: a.avatar.clone(),
        },
        text,
        reply_count: p.reply_count.unwrap_or(0).max(0) as u64,
        repost_count: p.repost_count.unwrap_or(0).max(0) as u64,
        like_count: p.like_count.unwrap_or(0).max(0) as u64,
        like_uri,
        repost_uri,
        indexed_at: p.indexed_at.as_str().to_string(),
    }
}

/// Fetch a post's thread (the focused post + its direct replies).
pub async fn get_post_thread(uri: &str) -> Result<bsky_domain::Thread, String> {
    use atrium_api::app::bsky::feed::defs::ThreadViewPostRepliesItem as ReplyItem;
    use atrium_api::app::bsky::feed::get_post_thread::OutputThreadRefs as ThreadRef;

    let agent = AGENT.lock().unwrap().clone().ok_or("not authenticated")?;
    let out = agent
        .api
        .app
        .bsky
        .feed
        .get_post_thread(
            get_post_thread::ParametersData {
                uri: uri.to_string(),
                depth: None,
                parent_height: None,
            }
            .into(),
        )
        .await
        .map_err(|e| e.to_string())?;

    match &out.thread {
        Union::Refs(ThreadRef::AppBskyFeedDefsThreadViewPost(tv)) => {
            let post = map_post_view(&tv.post);
            let replies = tv
                .replies
                .iter()
                .flatten()
                .filter_map(|item| match item {
                    Union::Refs(ReplyItem::ThreadViewPost(child)) => {
                        Some(map_post_view(&child.post))
                    }
                    _ => None,
                })
                .collect();
            Ok(bsky_domain::Thread {
                post: Some(post),
                replies,
            })
        }
        _ => Err("post not found or blocked".to_string()),
    }
}

/// Build a strong reference (`{uri, cid}`) to a record subject.
fn strong_ref(uri: &str, cid: &str) -> Result<strong_ref::Main, String> {
    let cid = cid.parse::<Cid>().map_err(|e| e.to_string())?;
    Ok(strong_ref::MainData {
        uri: uri.to_string(),
        cid,
    }
    .into())
}

/// `createRecord` in the signed-in user's repo; returns the new record URI.
async fn create_record(
    agent: &BskyAgent,
    collection: &str,
    record: Unknown,
) -> Result<String, String> {
    let did = agent.did().await.ok_or("no session DID")?;
    let collection = collection.parse::<Nsid>().map_err(|e| e.to_string())?;
    let out = agent
        .api
        .com
        .atproto
        .repo
        .create_record(
            create_record::InputData {
                collection,
                record,
                repo: did.into(),
                rkey: None,
                swap_commit: None,
                validate: None,
            }
            .into(),
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(out.uri.clone())
}

/// `deleteRecord` for a record the viewer authored, identified by its
/// `at://…/<collection>/<rkey>` URI.
async fn delete_record(
    agent: &BskyAgent,
    collection: &str,
    record_uri: &str,
) -> Result<(), String> {
    let did = agent.did().await.ok_or("no session DID")?;
    let rkey = record_uri
        .rsplit('/')
        .next()
        .ok_or("malformed record URI")?
        .parse::<RecordKey>()
        .map_err(|e| e.to_string())?;
    let collection = collection.parse::<Nsid>().map_err(|e| e.to_string())?;
    agent
        .api
        .com
        .atproto
        .repo
        .delete_record(
            delete_record::InputData {
                collection,
                repo: did.into(),
                rkey,
                swap_commit: None,
                swap_record: None,
            }
            .into(),
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Like a post; returns the new like record's URI (pass it to [`unlike`]).
pub async fn like(subject_uri: &str, subject_cid: &str) -> Result<String, String> {
    let agent = AGENT.lock().unwrap().clone().ok_or("not authenticated")?;
    let record = like::RecordData {
        created_at: Datetime::now(),
        subject: strong_ref(subject_uri, subject_cid)?,
        via: None,
    }
    .try_into_unknown()
    .map_err(|e| e.to_string())?;
    create_record(&agent, "app.bsky.feed.like", record).await
}

/// Undo a like, given the like record URI returned by [`like`].
pub async fn unlike(like_uri: &str) -> Result<(), String> {
    let agent = AGENT.lock().unwrap().clone().ok_or("not authenticated")?;
    delete_record(&agent, "app.bsky.feed.like", like_uri).await
}

/// Repost a post; returns the new repost record's URI (pass it to [`unrepost`]).
pub async fn repost(subject_uri: &str, subject_cid: &str) -> Result<String, String> {
    let agent = AGENT.lock().unwrap().clone().ok_or("not authenticated")?;
    let record = repost::RecordData {
        created_at: Datetime::now(),
        subject: strong_ref(subject_uri, subject_cid)?,
        via: None,
    }
    .try_into_unknown()
    .map_err(|e| e.to_string())?;
    create_record(&agent, "app.bsky.feed.repost", record).await
}

/// Undo a repost, given the repost record URI returned by [`repost`].
pub async fn unrepost(repost_uri: &str) -> Result<(), String> {
    let agent = AGENT.lock().unwrap().clone().ok_or("not authenticated")?;
    delete_record(&agent, "app.bsky.feed.repost", repost_uri).await
}

/// The signed-in user's own DID, if a session is active.
pub async fn my_did() -> Option<String> {
    let agent = AGENT.lock().unwrap().clone()?;
    agent.did().await.map(|d| d.as_str().to_string())
}

/// Fetch a profile by DID or handle.
pub async fn get_profile(actor: &str) -> Result<bsky_domain::Profile, String> {
    let agent = AGENT.lock().unwrap().clone().ok_or("not authenticated")?;
    let id = actor.parse::<AtIdentifier>().map_err(|e| e.to_string())?;
    let out = agent
        .api
        .app
        .bsky
        .actor
        .get_profile(get_profile::ParametersData { actor: id }.into())
        .await
        .map_err(|e| e.to_string())?;
    let following_uri = out.viewer.as_ref().and_then(|v| v.following.clone());
    Ok(bsky_domain::Profile {
        did: out.did.as_str().to_string(),
        handle: out.handle.as_str().to_string(),
        display_name: out.display_name.clone(),
        description: out.description.clone(),
        avatar: out.avatar.clone(),
        banner: out.banner.clone(),
        followers_count: out.followers_count.unwrap_or(0).max(0) as u64,
        follows_count: out.follows_count.unwrap_or(0).max(0) as u64,
        posts_count: out.posts_count.unwrap_or(0).max(0) as u64,
        following_uri,
    })
}

/// Fetch an account's authored posts (its profile feed).
pub async fn get_author_feed(actor: &str, limit: u8) -> Result<Vec<bsky_domain::FeedPost>, String> {
    let agent = AGENT.lock().unwrap().clone().ok_or("not authenticated")?;
    let id = actor.parse::<AtIdentifier>().map_err(|e| e.to_string())?;
    let out = agent
        .api
        .app
        .bsky
        .feed
        .get_author_feed(
            get_author_feed::ParametersData {
                actor: id,
                cursor: None,
                filter: None,
                include_pins: Some(true),
                limit: limit.try_into().ok(),
            }
            .into(),
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(out.feed.iter().map(map_feed_post).collect())
}

/// Map atrium's `ProfileView` (the shape `searchActors` returns) into our
/// trimmed [`bsky_domain::ActorView`].
fn map_actor(p: &actor_defs::ProfileView) -> bsky_domain::ActorView {
    bsky_domain::ActorView {
        did: p.did.as_str().to_string(),
        handle: p.handle.as_str().to_string(),
        display_name: p.display_name.clone(),
        avatar: p.avatar.clone(),
        description: p.description.clone(),
    }
}

/// Search for accounts by query string (`searchActors`).
pub async fn search_actors(query: &str, limit: u8) -> Result<Vec<bsky_domain::ActorView>, String> {
    let agent = AGENT.lock().unwrap().clone().ok_or("not authenticated")?;
    let out = agent
        .api
        .app
        .bsky
        .actor
        .search_actors(
            search_actors::ParametersData {
                cursor: None,
                limit: limit.try_into().ok(),
                q: Some(query.to_string()),
                term: None,
            }
            .into(),
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(out.actors.iter().map(map_actor).collect())
}

/// Search for posts by query string (`searchPosts`).
pub async fn search_posts(query: &str, limit: u8) -> Result<Vec<bsky_domain::FeedPost>, String> {
    let agent = AGENT.lock().unwrap().clone().ok_or("not authenticated")?;
    let out = agent
        .api
        .app
        .bsky
        .feed
        .search_posts(
            search_posts::ParametersData {
                author: None,
                cursor: None,
                domain: None,
                lang: None,
                limit: limit.try_into().ok(),
                mentions: None,
                q: query.to_string(),
                since: None,
                sort: None,
                tag: None,
                until: None,
                url: None,
            }
            .into(),
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(out.posts.iter().map(map_post_view).collect())
}

/// Pull the `text` field out of an arbitrary record without assuming it's a
/// post. We deliberately do NOT use `post::RecordData::try_from_unknown`
/// here: atrium's `TryFromUnknown` impl `.unwrap()`s the inner
/// `serde_json::from_slice` (atrium-api types.rs:279), so probing a non-post
/// record (a follow / like / repost) panics with "missing field `text`"
/// instead of returning `Err`. Serialising to JSON and reading `text`
/// defensively can't panic.
fn record_text(record: &Unknown) -> Option<String> {
    let json = serde_json::to_vec(record).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&json).ok()?;
    value
        .get("text")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
}

/// Map atrium's `Notification` into our trimmed [`bsky_domain::Notification`].
/// `text` is the post body when the record is itself a post (reply / mention
/// / quote); for like / repost / follow the record isn't a post, so it's None.
fn map_notification(n: &list_notifications::Notification) -> bsky_domain::Notification {
    let text = record_text(&n.record);
    let a = &n.author;
    bsky_domain::Notification {
        uri: n.uri.clone(),
        reason: n.reason.clone(),
        reason_subject: n.reason_subject.clone(),
        author: bsky_domain::Author {
            did: a.did.as_str().to_string(),
            handle: a.handle.as_str().to_string(),
            display_name: a.display_name.clone(),
            avatar: a.avatar.clone(),
        },
        text,
        is_read: n.is_read,
        indexed_at: n.indexed_at.as_str().to_string(),
    }
}

/// Fetch the signed-in user's notifications (`listNotifications`).
pub async fn list_notifications(limit: u8) -> Result<Vec<bsky_domain::Notification>, String> {
    let agent = AGENT.lock().unwrap().clone().ok_or("not authenticated")?;
    let out = agent
        .api
        .app
        .bsky
        .notification
        .list_notifications(
            list_notifications::ParametersData {
                cursor: None,
                limit: limit.try_into().ok(),
                priority: None,
                reasons: None,
                seen_at: None,
            }
            .into(),
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(out.notifications.iter().map(map_notification).collect())
}

/// Follow an account; returns the new follow record URI (for [`unfollow`]).
pub async fn follow(did: &str) -> Result<String, String> {
    let agent = AGENT.lock().unwrap().clone().ok_or("not authenticated")?;
    let subject = did.parse::<Did>().map_err(|e| e.to_string())?;
    let record = follow::RecordData {
        created_at: Datetime::now(),
        subject,
    }
    .try_into_unknown()
    .map_err(|e| e.to_string())?;
    create_record(&agent, "app.bsky.graph.follow", record).await
}

/// Undo a follow, given the follow record URI returned by [`follow`].
pub async fn unfollow(follow_uri: &str) -> Result<(), String> {
    let agent = AGENT.lock().unwrap().clone().ok_or("not authenticated")?;
    delete_record(&agent, "app.bsky.graph.follow", follow_uri).await
}

/// Publish a new text post. Link / hashtag facets are detected from the
/// text (mentions are deferred — see MEMO). Returns the new post's URI.
pub async fn create_post(text: &str) -> Result<String, String> {
    let agent = AGENT.lock().unwrap().clone().ok_or("not authenticated")?;
    let facets = detect_facets(text);
    let record = post::RecordData {
        created_at: Datetime::now(),
        text: text.to_string(),
        facets: if facets.is_empty() {
            None
        } else {
            Some(facets)
        },
        embed: None,
        entities: None,
        labels: None,
        langs: None,
        reply: None,
        tags: None,
    }
    .try_into_unknown()
    .map_err(|e| e.to_string())?;
    create_record(&agent, "app.bsky.feed.post", record).await
}

/// Detect link (`http(s)://…`) and hashtag (`#tag`) facets and their
/// UTF-8 byte ranges. A deliberately small scanner — good enough for the
/// example; mention facets (which need handle→DID resolution) are left
/// out for now.
fn detect_facets(text: &str) -> Vec<facet::Main> {
    let mut out = Vec::new();

    // Links.
    for proto in ["https://", "http://"] {
        let mut from = 0;
        while let Some(rel) = text[from..].find(proto) {
            let bs = from + rel;
            let mut be = bs + proto.len();
            for (i, c) in text[bs + proto.len()..].char_indices() {
                if c.is_whitespace() {
                    break;
                }
                be = bs + proto.len() + i + c.len_utf8();
            }
            // Trim trailing punctuation commonly adjacent to URLs.
            while be > bs + proto.len()
                && matches!(
                    text.as_bytes()[be - 1],
                    b'.' | b',' | b'!' | b'?' | b')' | b';' | b':'
                )
            {
                be -= 1;
            }
            out.push(link_facet(bs, be, text[bs..be].to_string()));
            from = be.max(bs + 1);
        }
    }

    // Hashtags: `#` at a word boundary followed by alphanumerics / `_`.
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (bs, c) = chars[i];
        let boundary = i == 0 || chars[i - 1].1.is_whitespace();
        if c == '#' && boundary {
            let tag_start = bs + 1;
            let mut be = tag_start;
            let mut j = i + 1;
            while let Some(&(idx, cc)) = chars.get(j) {
                if cc.is_alphanumeric() || cc == '_' {
                    be = idx + cc.len_utf8();
                    j += 1;
                } else {
                    break;
                }
            }
            if be > tag_start {
                out.push(tag_facet(bs, be, text[tag_start..be].to_string()));
                i = j;
                continue;
            }
        }
        i += 1;
    }

    out
}

fn link_facet(byte_start: usize, byte_end: usize, uri: String) -> facet::Main {
    facet::MainData {
        index: facet::ByteSliceData {
            byte_start,
            byte_end,
        }
        .into(),
        features: vec![Union::Refs(facet::MainFeaturesItem::Link(Box::new(
            facet::LinkData { uri }.into(),
        )))],
    }
    .into()
}

fn tag_facet(byte_start: usize, byte_end: usize, tag: String) -> facet::Main {
    facet::MainData {
        index: facet::ByteSliceData {
            byte_start,
            byte_end,
        }
        .into(),
        features: vec![Union::Refs(facet::MainFeaturesItem::Tag(Box::new(
            facet::TagData { tag }.into(),
        )))],
    }
    .into()
}

/// Does `url` look like our loopback redirect (the signal to call
/// [`complete_login`])?
pub fn is_redirect(url: &str) -> bool {
    url.starts_with(REDIRECT_URI)
}
