use super::*;

/// Minimal [`HttpClient`] over a rustls `reqwest::Client`.
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
pub(super) type BskyAgent = Agent<OAuthSess>;

/// Key under which the active account DID (not a secret) is kept in the
/// plaintext local store, so a fresh launch knows which session to
/// restore from the secure store.
const ACTIVE_DID_KEY: &str = "bsky.active_did";

fn stored_active_did() -> Option<Did> {
    WhiskerLocalStore::load(ACTIVE_DID_KEY.to_string())
        .ok()
        .flatten()
        .and_then(|did| Did::new(did).ok())
}

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
pub(super) struct SecureSessionStore;

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
        if let Some(did) = stored_active_did() {
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
pub(super) static AGENT: Mutex<Option<Arc<BskyAgent>>> = Mutex::new(None);

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

pub(super) fn client() -> &'static Client {
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
    if let Some(did) = stored_active_did() {
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
