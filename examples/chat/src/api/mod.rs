mod client;
mod sse;

pub use client::ApiClient;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiError {
    Unauthorized,
    RateLimited,
    ModelsUnsupported,
    Http(u16),
    Network,
    Timeout,
    InvalidResponse,
    Interrupted,
}

impl ApiError {
    pub fn message(&self) -> String {
        match self {
            Self::Unauthorized => "Authentication failed. Check your API key.".into(),
            Self::RateLimited => "Check your usage limits or balance, then try again later.".into(),
            Self::ModelsUnsupported => {
                "Model discovery is unavailable. You can enter a model ID manually.".into()
            }
            Self::Http(status) => format!("The provider returned an error (HTTP {status})."),
            Self::Network => "Could not connect. Check your network and API base URL.".into(),
            Self::Timeout => "The response timed out. Check your connection and try again.".into(),
            Self::InvalidResponse => "Could not read the response from the provider.".into(),
            Self::Interrupted => "The connection closed before the response was complete.".into(),
        }
    }
}

#[derive(Clone, Serialize)]
pub struct Message {
    pub role: &'static str,
    pub content: String,
}

pub async fn delay(duration: std::time::Duration) {
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(duration).await;
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(duration.as_millis().min(u32::MAX as u128) as u32)
        .await;
}

async fn deadline<T>(
    future: impl std::future::Future<Output = T>,
    seconds: u64,
) -> Result<T, ApiError> {
    use futures_util::future::{Either, select};
    let work = Box::pin(future);
    let timer = Box::pin(delay(std::time::Duration::from_secs(seconds)));
    match select(work, timer).await {
        Either::Left((value, _)) => Ok(value),
        Either::Right(_) => Err(ApiError::Timeout),
    }
}
