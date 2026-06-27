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

use atrium_api::agent::Agent;
use atrium_api::app::bsky::feed::{get_timeline, post};
use atrium_api::types::TryFromUnknown;
use atrium_identity::did::{CommonDidResolver, CommonDidResolverConfig, DEFAULT_PLC_DIRECTORY_URL};
use atrium_identity::handle::{
    AtprotoHandleResolver, AtprotoHandleResolverConfig, DohDnsTxtResolver, DohDnsTxtResolverConfig,
};
use atrium_oauth::store::session::MemorySessionStore;
use atrium_oauth::store::state::MemoryStateStore;
use atrium_oauth::{
    AtprotoLocalhostClientMetadata, AuthorizeOptions, CallbackParams, KnownScope, OAuthClient,
    OAuthClientConfig, OAuthResolverConfig, OAuthSession, Scope,
};
use atrium_xrpc::HttpClient;
use atrium_xrpc::http::{Request, Response, Uri};
use std::sync::Arc;

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
type Client = OAuthClient<MemoryStateStore, MemorySessionStore, DidR, HandleR, Http>;
type Session = OAuthSession<Http, DidR, HandleR, MemorySessionStore>;
type BskyAgent = Agent<Session>;

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
        session_store: MemorySessionStore::default(),
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
    let agent = Arc::new(Agent::new(session));
    *AGENT.lock().unwrap() = Some(agent);
    Ok(())
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
    let p = &fv.post;
    // The post body lives in an `Unknown` record; deserialize the app.bsky.feed.post
    // shape out of it, falling back to empty text if it isn't a normal post.
    let text = post::RecordData::try_from_unknown(p.record.clone())
        .map(|r| r.text)
        .unwrap_or_default();
    let a = &p.author;
    bsky_domain::FeedPost {
        uri: p.uri.clone(),
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
        indexed_at: p.indexed_at.as_str().to_string(),
    }
}

/// Does `url` look like our loopback redirect (the signal to call
/// [`complete_login`])?
pub fn is_redirect(url: &str) -> bool {
    url.starts_with(REDIRECT_URI)
}
