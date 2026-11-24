//! Heavy work off the UI thread, or on the web, in short slices between which the page can draw.

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
