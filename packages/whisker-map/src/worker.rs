//! Web Workers that run this same wasm module beside the page and make tiles there.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Weak};
use std::task::{Context, Poll, Waker};

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{
    Blob, BlobPropertyBag, DedicatedWorkerGlobalScope, MessageEvent, Url, Worker, WorkerOptions,
    WorkerType,
};

use crate::geo::TileId;
use crate::style::{GeoJsonOptions, Layer, SourceKind, parse_layers};
use crate::tiles::geojson::GeoJsonIndex;
use crate::tiles::{Features, TileContent, process, vector_content};
use crate::wire::{
    TileJob, decode_content, decode_geojson, decode_job, encode_content, encode_geojson, encode_job,
};

/// The wasm-bindgen glue `whisker build` and `whisker run` serve beside the page.
const GLUE: &str = "whisker_app.js";

const MAX_WORKERS: u32 = 4;

type MessageHandler = Closure<dyn FnMut(MessageEvent)>;

/// Imports the glue, instantiates the module the page sent, and hands messages that arrived
/// meanwhile to [`__whisker_map_worker`].
const BOOTSTRAP: &str = r#"
self.onmessage = async (first) => {
  const pending = [];
  self.onmessage = (event) => pending.push(event);
  try {
    const { glue, module } = first.data;
    const wasm = await import(glue);
    await wasm.default({ module_or_path: module });
    wasm.__whisker_map_worker(pending);
  } catch (error) {
    self.postMessage({ failed: String(error) });
  }
};
"#;

/// Makes the tile in a Worker, or hands `bytes` back when no Worker can.
pub(crate) async fn tile(
    id: TileId,
    kind: &SourceKind,
    bytes: Vec<u8>,
    layers: &Arc<Vec<Arc<Layer>>>,
    indices: &[usize],
    max_zoom: u8,
) -> Result<Option<TileContent>, Vec<u8>> {
    let Some(style) = style_id(layers) else {
        return Err(bytes);
    };
    let job = TileJob {
        style,
        id,
        kind: kind.clone(),
        indices: indices.to_vec(),
        max_zoom,
        bytes,
    };
    let data = encode_job(&job);
    let Some(reply) = send(&data, None, None) else {
        return Err(job.bytes);
    };
    match Wait(reply).await {
        Outcome::Done(Some(out)) => Ok(decode_content(&out, Features::encoded(job.bytes))),
        Outcome::Done(None) => Ok(None),
        Outcome::Retry => Err(job.bytes),
    }
}

/// Sends a style's layers to every Worker under a new id, and forgets styles no longer in use.
pub(crate) fn register_layers(layers: &Arc<Vec<Arc<Layer>>>, json: &serde_json::Value) {
    let json = json.to_string();
    let (id, dropped) = STYLES.with(|styles| {
        let mut styles = styles.borrow_mut();
        let mut dropped = Vec::new();
        styles.entries.retain(|entry| {
            let live = entry.layers.strong_count() > 0;
            if !live {
                dropped.push(entry.id);
            }
            live
        });
        styles.next += 1;
        let id = styles.next;
        styles.entries.push(StyleEntry {
            layers: Arc::downgrade(layers),
            id,
            json: json.clone(),
        });
        (id, dropped)
    });
    with_workers(|workers| {
        for worker in workers {
            for &drop in &dropped {
                post(&worker.worker, &[("drop", JsValue::from(drop))], None);
            }
            post_style(&worker.worker, id, &json);
        }
    });
}

struct StyleEntry {
    layers: Weak<Vec<Arc<Layer>>>,
    id: u32,
    json: String,
}

#[derive(Default)]
struct Styles {
    entries: Vec<StyleEntry>,
    next: u32,
}

enum Pool {
    Unstarted,
    Running {
        workers: Vec<WorkerSlot>,
        jobs: HashMap<u32, (usize, Rc<Reply>)>,
        next_job: u32,
    },
    /// No Worker could start, or one failed: every job runs on the page.
    Off,
}

struct WorkerSlot {
    worker: Worker,
    pending: usize,
}

thread_local! {
    static STYLES: RefCell<Styles> = RefCell::new(Styles::default());
    static POOL: RefCell<Pool> = const { RefCell::new(Pool::Unstarted) };
}

fn style_id(layers: &Arc<Vec<Arc<Layer>>>) -> Option<u32> {
    let target = Arc::downgrade(layers);
    STYLES.with(|styles| {
        styles
            .borrow()
            .entries
            .iter()
            .find(|entry| entry.layers.ptr_eq(&target))
            .map(|entry| entry.id)
    })
}

fn with_workers(f: impl FnOnce(&[WorkerSlot])) {
    POOL.with(|pool| {
        if let Pool::Running { workers, .. } = &*pool.borrow() {
            f(workers);
        }
    });
}

/// Runs `f` on the running pool, starting it first if no job has yet.
fn running<R>(
    f: impl FnOnce(&mut Vec<WorkerSlot>, &mut HashMap<u32, (usize, Rc<Reply>)>, &mut u32) -> Option<R>,
) -> Option<R> {
    POOL.with(|pool| {
        if matches!(*pool.borrow(), Pool::Unstarted) {
            let started = start();
            *pool.borrow_mut() = started;
        }
        match &mut *pool.borrow_mut() {
            Pool::Running {
                workers,
                jobs,
                next_job,
            } => f(workers, jobs, next_job),
            _ => None,
        }
    })
}

/// Posts a job to `target`, or to the Worker with the fewest jobs waiting.
fn send(data: &[u8], target: Option<usize>, geojson: Option<u32>) -> Option<Rc<Reply>> {
    running(|workers, jobs, next_job| {
        let index = match target {
            Some(index) => index,
            None => {
                workers
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, slot)| slot.pending)?
                    .0
            }
        };
        let slot = workers.get_mut(index)?;
        *next_job += 1;
        let job = *next_job;
        let array = js_sys::Uint8Array::from(data);
        let transfer = js_sys::Array::of1(&array.buffer());
        let mut fields = vec![("job", JsValue::from(job)), ("data", array.into())];
        if let Some(geojson) = geojson {
            fields.push(("geojson", JsValue::from(geojson)));
        }
        post(&slot.worker, &fields, Some(&transfer));
        slot.pending += 1;
        let reply = Rc::new(Reply::default());
        jobs.insert(job, (index, reply.clone()));
        Some(reply)
    })
}

/// A GeoJSON source's index, kept in one Worker; the page builds its own copy should that Worker
/// stop.
pub(crate) struct RemoteIndex {
    id: u32,
    worker: usize,
    text: Vec<u8>,
    options: GeoJsonOptions,
    local: RefCell<Option<Arc<GeoJsonIndex>>>,
}

impl RemoteIndex {
    /// The index on the page, built on first use.
    pub async fn local(&self) -> Option<Arc<GeoJsonIndex>> {
        if let Some(index) = self.local.borrow().clone() {
            return Some(index);
        }
        let text = self.text.clone();
        let options = self.options.clone();
        let index = crate::work::run(move || {
            let data = serde_json::from_slice(&text).ok()?;
            Some(Arc::new(GeoJsonIndex::new(&data, options)))
        })
        .await?;
        *self.local.borrow_mut() = Some(index.clone());
        Some(index)
    }
}

impl Drop for RemoteIndex {
    fn drop(&mut self) {
        let (id, worker) = (self.id, self.worker);
        with_workers(|workers| {
            if let Some(slot) = workers.get(worker) {
                post(&slot.worker, &[("dropgeojson", JsValue::from(id))], None);
            }
        });
    }
}

/// Indexes `text` in the Worker with the fewest jobs waiting, or `None` where no Worker can.
pub(crate) fn geojson_index(
    text: Vec<u8>,
    options: GeoJsonOptions,
) -> Result<RemoteIndex, (Vec<u8>, GeoJsonOptions)> {
    thread_local! {
        static NEXT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }
    let data = encode_geojson(&options, &text);
    let placed = running(|workers, _, _| {
        let (index, slot) = workers
            .iter()
            .enumerate()
            .min_by_key(|(_, slot)| slot.pending)?;
        let id = NEXT.with(|next| {
            next.set(next.get() + 1);
            next.get()
        });
        let array = js_sys::Uint8Array::from(&data[..]);
        let transfer = js_sys::Array::of1(&array.buffer());
        post(
            &slot.worker,
            &[("index", JsValue::from(id)), ("data", array.into())],
            Some(&transfer),
        );
        Some((id, index))
    });
    match placed {
        Some((id, worker)) => Ok(RemoteIndex {
            id,
            worker,
            text,
            options,
            local: RefCell::new(None),
        }),
        None => Err((text, options)),
    }
}

/// Cuts a tile from `index` in its Worker, or `Err` once that Worker has stopped.
pub(crate) async fn geojson_tile(
    index: &RemoteIndex,
    id: TileId,
    layers: &Arc<Vec<Arc<Layer>>>,
    indices: &[usize],
    max_zoom: u8,
) -> Result<Option<TileContent>, ()> {
    let style = style_id(layers).ok_or(())?;
    let job = TileJob {
        style,
        id,
        kind: SourceKind::Vector,
        indices: indices.to_vec(),
        max_zoom,
        bytes: Vec::new(),
    };
    let reply = send(&encode_job(&job), Some(index.worker), Some(index.id)).ok_or(())?;
    match Wait(reply).await {
        Outcome::Done(Some(out)) => Ok(decode_content(&out, Features::encoded(Vec::new()))),
        Outcome::Done(None) => Ok(None),
        Outcome::Retry => Err(()),
    }
}

fn start() -> Pool {
    let cores = js_sys::Reflect::get(&js_sys::global(), &"navigator".into())
        .and_then(|navigator| js_sys::Reflect::get(&navigator, &"hardwareConcurrency".into()))
        .ok()
        .and_then(|cores| cores.as_f64())
        .unwrap_or(2.0) as u32;
    let count = (cores / 2).clamp(1, MAX_WORKERS) as usize;
    let mut workers = Vec::with_capacity(count);
    for index in 0..count {
        let Some(worker) = spawn(index) else { break };
        STYLES.with(|styles| {
            for entry in &styles.borrow().entries {
                post_style(&worker, entry.id, &entry.json);
            }
        });
        workers.push(WorkerSlot { worker, pending: 0 });
    }
    if workers.is_empty() {
        return Pool::Off;
    }
    Pool::Running {
        workers,
        jobs: HashMap::new(),
        next_job: 0,
    }
}

/// Starts a Worker running this module, or `None` where the page cannot start one.
fn spawn(index: usize) -> Option<Worker> {
    let glue = glue_url()?;
    let parts = js_sys::Array::of1(&JsValue::from_str(BOOTSTRAP));
    let options = BlobPropertyBag::new();
    options.set_type("text/javascript");
    let blob = Blob::new_with_str_sequence_and_options(&parts, &options).ok()?;
    let url = Url::create_object_url_with_blob(&blob).ok()?;
    let worker_options = WorkerOptions::new();
    worker_options.set_type(WorkerType::Module);
    let worker = Worker::new_with_options(&url, &worker_options);
    let _ = Url::revoke_object_url(&url);
    let worker = worker.ok()?;
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        receive(index, event.data());
    });
    worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    on_message.forget();
    let on_error = Closure::<dyn FnMut(JsValue)>::new(|_| stop());
    worker.set_onerror(Some(on_error.as_ref().unchecked_ref()));
    on_error.forget();
    post(
        &worker,
        &[
            ("glue", JsValue::from_str(&glue)),
            ("module", wasm_bindgen::module()),
        ],
        None,
    );
    Some(worker)
}

/// The glue's absolute URL; a Blob Worker resolves relative imports against `blob:`.
fn glue_url() -> Option<String> {
    let document = web_sys::window()?.document()?;
    let base = document
        .query_selector("meta[name=whisker-base-path]")
        .ok()
        .flatten()
        .and_then(|meta| meta.get_attribute("content"))
        .unwrap_or_else(|| "/".into());
    let page = document.base_uri().ok().flatten()?;
    let url = Url::new_with_base(&format!("{}/{GLUE}", base.trim_end_matches('/')), &page).ok()?;
    Some(url.href())
}

fn receive(index: usize, data: JsValue) {
    if field(&data, "failed").is_some_and(|failed| !failed.is_undefined()) {
        stop();
        return;
    }
    let Some(job) = field(&data, "job").and_then(|job| job.as_f64()) else {
        return;
    };
    let reply = POOL.with(|pool| {
        let mut pool = pool.borrow_mut();
        let Pool::Running { workers, jobs, .. } = &mut *pool else {
            return None;
        };
        let (_, reply) = jobs.remove(&(job as u32))?;
        if let Some(slot) = workers.get_mut(index) {
            slot.pending = slot.pending.saturating_sub(1);
        }
        Some(reply)
    });
    let Some(reply) = reply else { return };
    let out = field(&data, "data")
        .and_then(|out| out.dyn_into::<js_sys::Uint8Array>().ok())
        .map(|out| out.to_vec());
    reply.finish(Outcome::Done(out));
}

/// Turns the pool off and hands every waiting job back to the page.
fn stop() {
    let previous = POOL.with(|pool| std::mem::replace(&mut *pool.borrow_mut(), Pool::Off));
    if let Pool::Running { workers, jobs, .. } = previous {
        for slot in workers {
            slot.worker.terminate();
        }
        for (_, (_, reply)) in jobs {
            reply.finish(Outcome::Retry);
        }
    }
}

fn post_style(worker: &Worker, id: u32, json: &str) {
    post(
        worker,
        &[
            ("style", JsValue::from(id)),
            ("layers", JsValue::from_str(json)),
        ],
        None,
    );
}

fn post(worker: &Worker, fields: &[(&str, JsValue)], transfer: Option<&js_sys::Array>) {
    let message = js_sys::Object::new();
    for (key, value) in fields {
        let _ = js_sys::Reflect::set(&message, &JsValue::from_str(key), value);
    }
    let _ = match transfer {
        Some(transfer) => worker.post_message_with_transfer(&message, transfer),
        None => worker.post_message(&message),
    };
}

fn field(data: &JsValue, key: &str) -> Option<JsValue> {
    js_sys::Reflect::get(data, &JsValue::from_str(key))
        .ok()
        .filter(|value| !value.is_undefined() && !value.is_null())
}

enum Outcome {
    Done(Option<Vec<u8>>),
    Retry,
}

#[derive(Default)]
struct Reply {
    outcome: RefCell<Option<Outcome>>,
    waker: RefCell<Option<Waker>>,
}

impl Reply {
    fn finish(&self, outcome: Outcome) {
        *self.outcome.borrow_mut() = Some(outcome);
        if let Some(waker) = self.waker.borrow_mut().take() {
            waker.wake();
        }
    }
}

struct Wait(Rc<Reply>);

impl Future for Wait {
    type Output = Outcome;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Outcome> {
        match self.0.outcome.borrow_mut().take() {
            Some(outcome) => Poll::Ready(outcome),
            None => {
                *self.0.waker.borrow_mut() = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

thread_local! {
    static HANDLER: RefCell<Option<MessageHandler>> = const { RefCell::new(None) };
    static WORKER_STYLES: RefCell<HashMap<u32, Arc<Vec<Arc<Layer>>>>> = RefCell::new(HashMap::new());
    static WORKER_GEOJSON: RefCell<HashMap<u32, GeoJsonIndex>> = RefCell::new(HashMap::new());
}

/// The Worker's entry point, called by [`BOOTSTRAP`] once the module is instantiated.
#[doc(hidden)]
#[wasm_bindgen]
pub fn __whisker_map_worker(pending: js_sys::Array) {
    let scope: DedicatedWorkerGlobalScope = js_sys::global().unchecked_into();
    let reply = scope.clone();
    let handler = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        handle(&reply, event.data());
    });
    scope.set_onmessage(Some(handler.as_ref().unchecked_ref()));
    HANDLER.with(|slot| *slot.borrow_mut() = Some(handler));
    for event in pending.iter() {
        handle(&scope, event.unchecked_into::<MessageEvent>().data());
    }
}

fn handle(scope: &DedicatedWorkerGlobalScope, data: JsValue) {
    if let Some(id) = field(&data, "style").and_then(|id| id.as_f64()) {
        let json = field(&data, "layers")
            .and_then(|json| json.as_string())
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or(serde_json::Value::Null);
        let layers = Arc::new(parse_layers(&json));
        WORKER_STYLES.with(|styles| styles.borrow_mut().insert(id as u32, layers));
        return;
    }
    if let Some(id) = field(&data, "drop").and_then(|id| id.as_f64()) {
        WORKER_STYLES.with(|styles| styles.borrow_mut().remove(&(id as u32)));
        return;
    }
    if let Some(id) = field(&data, "index").and_then(|id| id.as_f64()) {
        let index = field(&data, "data")
            .and_then(|data| data.dyn_into::<js_sys::Uint8Array>().ok())
            .and_then(|data| {
                let data = data.to_vec();
                let (options, text) = decode_geojson(&data)?;
                let json = serde_json::from_slice(text).ok()?;
                Some(GeoJsonIndex::new(&json, options))
            });
        if let Some(index) = index {
            WORKER_GEOJSON.with(|indexes| indexes.borrow_mut().insert(id as u32, index));
        }
        return;
    }
    if let Some(id) = field(&data, "dropgeojson").and_then(|id| id.as_f64()) {
        WORKER_GEOJSON.with(|indexes| indexes.borrow_mut().remove(&(id as u32)));
        return;
    }
    let Some(job_id) = field(&data, "job") else {
        return;
    };
    let geojson = field(&data, "geojson")
        .and_then(|id| id.as_f64())
        .map(|id| id as u32);
    let out = field(&data, "data")
        .and_then(|data| data.dyn_into::<js_sys::Uint8Array>().ok())
        .and_then(|data| decode_job(&data.to_vec()))
        .and_then(|job| {
            let layers = WORKER_STYLES.with(|styles| styles.borrow().get(&job.style).cloned())?;
            match geojson {
                Some(index) => WORKER_GEOJSON.with(|indexes| {
                    let layer = indexes.borrow().get(&index)?.tile(job.id);
                    Some(vector_content(
                        job.id,
                        Arc::new(vec![layer]),
                        &layers,
                        &job.indices,
                        job.max_zoom,
                    ))
                }),
                None => process(
                    job.id,
                    &job.kind,
                    &job.bytes,
                    &layers,
                    &job.indices,
                    job.max_zoom,
                ),
            }
        })
        .map(|content| js_sys::Uint8Array::from(&encode_content(&content, geojson.is_some())[..]));
    let message = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&message, &"job".into(), &job_id);
    let _ = js_sys::Reflect::set(
        &message,
        &"data".into(),
        out.as_ref().map_or(&JsValue::NULL, |out| out.as_ref()),
    );
    let _ = match &out {
        Some(out) => scope.post_message_with_transfer(&message, &js_sys::Array::of1(&out.buffer())),
        None => scope.post_message(&message),
    };
}
