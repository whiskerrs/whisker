//! Fetching tiles, styles, glyphs, and sprites.

use std::io::Read;

#[derive(Debug, Clone)]
pub(crate) struct FetchError(pub String);

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A successful response: its body, or `None` when the server has no such resource.
pub(crate) type Fetched = Result<Option<Vec<u8>>, FetchError>;

pub(crate) fn gunzip(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::MultiGzDecoder::new(data).read_to_end(&mut out)?;
    Ok(out)
}

/// Fetches a URL, or `range` bytes of it, off the UI thread where there is one.
pub(crate) async fn fetch(url: String, range: Option<(u64, u64)>) -> Fetched {
    let key = crate::cache::key(&url, range);
    if let Some(bytes) = crate::cache::memory_get(&key) {
        return Ok(Some(bytes));
    }
    let fetched = fetch_uncached(url, range, key.clone()).await;
    if let Ok(Some(bytes)) = &fetched {
        crate::cache::memory_put(key, bytes);
    }
    fetched
}

async fn fetch_uncached(url: String, range: Option<(u64, u64)>, key: String) -> Fetched {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(path) = url.strip_prefix("file://") {
            let path = path.to_string();
            return whisker::run_blocking(move || read_file(&path, range)).await;
        }
        whisker::run_blocking(move || {
            use crate::cache::disk;
            let cached = disk::read(&key);
            if let Some((bytes, true)) = cached {
                return Ok(Some(bytes));
            }
            match fetch_blocking(&url, range) {
                Ok(Some(bytes)) => {
                    disk::write(&key, &bytes);
                    Ok(Some(bytes))
                }
                // Offline, a stale copy is better than nothing.
                Err(error) => cached
                    .map(|(bytes, _)| Ok(Some(bytes)))
                    .unwrap_or(Err(error)),
                Ok(None) => Ok(None),
            }
        })
        .await
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = key;
        web::fetch(&url, range).await
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_file(path: &str, range: Option<(u64, u64)>) -> Fetched {
    use std::io::{Seek, SeekFrom};
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(FetchError(e.to_string())),
    };
    let mut out = Vec::new();
    match range {
        Some((offset, length)) => {
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| FetchError(e.to_string()))?;
            file.take(length)
                .read_to_end(&mut out)
                .map_err(|e| FetchError(e.to_string()))?;
        }
        None => {
            file.read_to_end(&mut out)
                .map_err(|e| FetchError(e.to_string()))?;
        }
    }
    Ok(Some(out))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn fetch_blocking(url: &str, range: Option<(u64, u64)>) -> Fetched {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    // A stalled connection would otherwise hold one of the source's few request slots forever.
    let agent = AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(15))
            .timeout_read(std::time::Duration::from_secs(30))
            .user_agent("whisker-map")
            .build()
    });
    let mut request = agent.get(url);
    if let Some((offset, length)) = range {
        request = request.set(
            "Range",
            &format!("bytes={}-{}", offset, offset + length.saturating_sub(1)),
        );
    }
    match request.call() {
        Ok(response) => {
            let mut body = Vec::new();
            response
                .into_reader()
                .take(256 * 1024 * 1024)
                .read_to_end(&mut body)
                .map_err(|e| FetchError(format!("{url}: {e}")))?;
            Ok(Some(body))
        }
        Err(ureq::Error::Status(404 | 204, _)) => Ok(None),
        Err(e) => Err(FetchError(format!("{url}: {e}"))),
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use super::{FetchError, Fetched};
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    pub(super) async fn fetch(url: &str, range: Option<(u64, u64)>) -> Fetched {
        let error = |e: wasm_bindgen::JsValue| FetchError(format!("{url}: {e:?}"));
        let init = web_sys::RequestInit::new();
        let request = web_sys::Request::new_with_str_and_init(url, &init).map_err(error)?;
        if let Some((offset, length)) = range {
            request
                .headers()
                .set(
                    "Range",
                    &format!("bytes={}-{}", offset, offset + length.saturating_sub(1)),
                )
                .map_err(error)?;
        }
        let global = js_sys::global();
        let promise = if let Some(window) = global.dyn_ref::<web_sys::Window>() {
            window.fetch_with_request(&request)
        } else {
            global
                .unchecked_ref::<web_sys::WorkerGlobalScope>()
                .fetch_with_request(&request)
        };
        let response: web_sys::Response = JsFuture::from(promise)
            .await
            .map_err(error)?
            .unchecked_into();
        if response.status() == 404 || response.status() == 204 {
            return Ok(None);
        }
        if !response.ok() {
            return Err(FetchError(format!("{url}: HTTP {}", response.status())));
        }
        let buffer = JsFuture::from(response.array_buffer().map_err(error)?)
            .await
            .map_err(error)?;
        Ok(Some(js_sys::Uint8Array::new(&buffer).to_vec()))
    }
}
