//! Heavy work off the UI thread, or on the web, in short slices between which the page can draw.

use std::sync::Arc;

use crate::geo::TileId;
use crate::style::{GeoJsonOptions, Layer, SourceKind};
use crate::tiles::geojson::{ClusterQuery, GeoJsonIndex};
use crate::tiles::{TileContent, process, vector_content};

/// Makes a tile from its source's bytes: on a worker thread, or on the web in a Web Worker,
/// falling back to slices of the page's own thread where no Worker can run.
pub(crate) async fn tile(
    id: TileId,
    kind: SourceKind,
    bytes: Vec<u8>,
    layers: Arc<Vec<Arc<Layer>>>,
    indices: Vec<usize>,
    max_zoom: u8,
) -> Option<TileContent> {
    #[cfg(target_arch = "wasm32")]
    let bytes = match crate::worker::tile(id, &kind, bytes, &layers, &indices, max_zoom).await {
        Ok(content) => return content,
        Err(bytes) => bytes,
    };
    run(move || process(id, &kind, &bytes, &layers, &indices, max_zoom)).await
}

/// A GeoJSON source's index: on the page, or in the Web Worker that built it.
#[derive(Clone)]
pub(crate) enum GeoJson {
    Local(Arc<GeoJsonIndex>),
    #[cfg(target_arch = "wasm32")]
    Remote(std::rc::Rc<crate::worker::RemoteIndex>),
}

/// Indexes the GeoJSON `text` for cutting into tiles; `None` when it does not parse.
pub(crate) async fn geojson_index(text: Vec<u8>, options: GeoJsonOptions) -> Option<GeoJson> {
    #[cfg(target_arch = "wasm32")]
    let (text, options) = match crate::worker::geojson_index(text, options) {
        Ok(remote) => return Some(GeoJson::Remote(std::rc::Rc::new(remote))),
        Err(back) => back,
    };
    let index = run(move || {
        let data = serde_json::from_slice(&text).ok()?;
        Some(Arc::new(GeoJsonIndex::new(&data, options)))
    })
    .await?;
    Some(GeoJson::Local(index))
}

/// Cuts tile `id` from `index` and makes it with the style's layers.
#[cfg_attr(
    not(target_arch = "wasm32"),
    allow(clippy::infallible_destructuring_match)
)]
pub(crate) async fn geojson_tile(
    index: GeoJson,
    id: TileId,
    layers: Arc<Vec<Arc<Layer>>>,
    indices: Vec<usize>,
    max_zoom: u8,
) -> Option<TileContent> {
    let local = match index {
        GeoJson::Local(index) => index,
        #[cfg(target_arch = "wasm32")]
        GeoJson::Remote(remote) => {
            match crate::worker::geojson_tile(&remote, id, &layers, &indices, max_zoom).await {
                Ok(content) => return content,
                Err(()) => remote.local().await?,
            }
        }
    };
    run(move || {
        let layer = local.tile(id);
        Some(vector_content(
            id,
            Arc::new(vec![layer]),
            &layers,
            &indices,
            max_zoom,
        ))
    })
    .await
}

/// Answers a cluster question about a GeoJSON source's index.
#[cfg_attr(
    not(target_arch = "wasm32"),
    allow(clippy::infallible_destructuring_match)
)]
pub(crate) async fn geojson_cluster(index: GeoJson, query: ClusterQuery) -> serde_json::Value {
    let local = match index {
        GeoJson::Local(index) => index,
        #[cfg(target_arch = "wasm32")]
        GeoJson::Remote(remote) => match crate::worker::geojson_cluster(&remote, query).await {
            Ok(answer) => return answer,
            Err(()) => match remote.local().await {
                Some(local) => local,
                None => return serde_json::Value::Null,
            },
        },
    };
    run(move || local.cluster(query)).await
}

/// Tells the Workers about a style's layers, which tile jobs then name.
pub(crate) fn register_layers(layers: &Arc<Vec<Arc<Layer>>>, json: &serde_json::Value) {
    #[cfg(target_arch = "wasm32")]
    crate::worker::register_layers(layers, json);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (layers, json);
}

/// Runs `job` on a worker thread, or on the web, once the current slice of work has yielded.
pub(crate) async fn run<T, F>(job: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        whisker::run_blocking(job).await
    }
    #[cfg(target_arch = "wasm32")]
    {
        web::yield_if_due().await;
        job()
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::Cell;

    use wasm_bindgen::{JsCast, JsValue};
    use web_time::{Duration, Instant};

    /// How long queued work may hold the main thread before letting it draw.
    const SLICE: Duration = Duration::from_millis(6);

    thread_local! {
        static SLICE_START: Cell<Option<Instant>> = const { Cell::new(None) };
    }

    pub(super) async fn yield_if_due() {
        let due = SLICE_START.with(|start| match start.get() {
            Some(start) => start.elapsed() >= SLICE,
            None => true,
        });
        if due {
            next_task().await;
            SLICE_START.with(|start| start.set(Some(Instant::now())));
        }
    }

    /// Resolves in a later task of the event loop, after any pending rendering.
    async fn next_task() {
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            let global = js_sys::global();
            if let Ok(set_timeout) = js_sys::Reflect::get(&global, &JsValue::from_str("setTimeout"))
                && let Some(set_timeout) = set_timeout.dyn_ref::<js_sys::Function>()
            {
                let _ = set_timeout.call2(&global, &resolve, &JsValue::from_f64(0.0));
            } else {
                let _ = resolve.call0(&JsValue::NULL);
            }
        });
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    }
}
