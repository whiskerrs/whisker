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
            Self::Unauthorized => "APIキーを確認してください。認証に失敗しました。".into(),
            Self::RateLimited => {
                "利用制限または残高を確認し、しばらくしてから再試行してください。".into()
            }
            Self::ModelsUnsupported => {
                "モデル一覧を取得できません。モデルIDを手入力して保存できます。".into()
            }
            Self::Http(status) => format!("接続先がエラーを返しました（HTTP {status}）。"),
            Self::Network => "接続できませんでした。通信状態と接続先を確認してください。".into(),
            Self::Timeout => "応答が途切れました。通信状態を確認して再試行してください。".into(),
            Self::InvalidResponse => "接続先からの応答を読み取れませんでした。".into(),
            Self::Interrupted => "回答の完了前に接続が切れました。".into(),
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
