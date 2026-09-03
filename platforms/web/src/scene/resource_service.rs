use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use js_sys::{Array, Uint8Array};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use whisker_protocol::{
    ResourceCommand, ResourceDimensions, ResourceEvent, ResourceFailureCode, ResourceId,
    ResourceKind, ResourceRequest, ResourceSource,
};

use crate::scene::resource_store::WebResourceStore;
use crate::{WebError, js_error};

/// Observable state retained for one resource generation.
#[derive(Clone, Debug, PartialEq)]
pub enum WebResourceState {
    /// Acquisition or browser decoding has not completed yet.
    Loading,
    /// The generation decoded successfully and is available to paint.
    Ready {
        /// Intrinsic dimensions reported to the runtime.
        dimensions: ResourceDimensions,
    },
    /// Acquisition or decoding failed.
    Failed {
        /// Portable protocol failure classification.
        code: ResourceFailureCode,
        /// Optional browser diagnostic.
        diagnostic: Option<String>,
    },
    /// The exact generation was released.
    Released,
}

#[derive(Clone)]
struct GenerationRecord {
    state: WebResourceState,
    owned_url: Option<String>,
}

#[derive(Default)]
struct ServiceState {
    current: HashMap<ResourceId, u64>,
    published: HashMap<ResourceId, u64>,
    generations: HashMap<(ResourceId, u64), GenerationRecord>,
    events: HashMap<(ResourceId, u64), ResourceEvent>,
    pending_events: VecDeque<ResourceEvent>,
}

/// Browser implementation of Whisker's out-of-frame resource channel.
///
/// The service consumes protocol commands directly. Successful decodes publish
/// a URL into the frame sink's [`WebResourceStore`] only after the requested
/// generation is still current.
#[derive(Clone, Default)]
pub struct WebResourceService {
    store: WebResourceStore,
    state: Rc<RefCell<ServiceState>>,
}

impl WebResourceService {
    /// Creates a service backed by the URL store read by the DOM frame sink.
    pub fn new(store: WebResourceStore) -> Self {
        Self {
            store,
            state: Rc::new(RefCell::new(ServiceState::default())),
        }
    }

    /// Applies one protocol command, awaiting browser acquisition and decode
    /// for `Load` and returning its non-stale completion event.
    pub async fn handle(
        &self,
        command: ResourceCommand,
    ) -> Result<Option<ResourceEvent>, WebError> {
        command
            .validate()
            .map_err(|error| WebError(format!("invalid Web resource command: {error:?}")))?;
        match command {
            ResourceCommand::Load(request) => self.load(request).await,
            ResourceCommand::Release {
                resource,
                generation,
            } => {
                self.release(resource, generation)?;
                Ok(None)
            }
        }
    }

    /// Returns retained lifecycle state for one exact generation.
    pub fn state(&self, resource: ResourceId, generation: u64) -> Option<WebResourceState> {
        self.state
            .borrow()
            .generations
            .get(&(resource, generation))
            .map(|record| record.state.clone())
    }

    /// Returns the retained protocol completion for one exact generation.
    pub fn event(&self, resource: ResourceId, generation: u64) -> Option<ResourceEvent> {
        self.state
            .borrow()
            .events
            .get(&(resource, generation))
            .cloned()
    }

    /// Drains completions not yet delivered to the runtime resource channel.
    pub fn take_events(&self) -> Vec<ResourceEvent> {
        self.state.borrow_mut().pending_events.drain(..).collect()
    }

    async fn load(&self, request: ResourceRequest) -> Result<Option<ResourceEvent>, WebError> {
        {
            let mut state = self.state.borrow_mut();
            if state
                .current
                .get(&request.resource)
                .is_some_and(|generation| *generation >= request.generation)
            {
                return Err(WebError(format!(
                    "Web resource {} generation {} does not advance the current generation",
                    request.resource.get(),
                    request.generation
                )));
            }
            state.current.insert(request.resource, request.generation);
            state.generations.insert(
                (request.resource, request.generation),
                GenerationRecord {
                    state: WebResourceState::Loading,
                    owned_url: None,
                },
            );
        }

        if request.kind != ResourceKind::RasterImage {
            return Ok(self.complete_failure(
                request.resource,
                request.generation,
                ResourceFailureCode::Unsupported,
                Some("Web resource service currently decodes raster images only".into()),
            ));
        }

        let (url, owned) = match source_url(&request.source) {
            Ok(value) => value,
            Err((code, diagnostic)) => {
                return Ok(self.complete_failure(
                    request.resource,
                    request.generation,
                    code,
                    Some(diagnostic),
                ));
            }
        };
        let decoded = decode_raster(&url).await;
        match decoded {
            Ok(dimensions) => {
                self.complete_ready(request.resource, request.generation, url, owned, dimensions)
            }
            Err(diagnostic) => {
                if owned {
                    let _ = web_sys::Url::revoke_object_url(&url);
                }
                Ok(self.complete_failure(
                    request.resource,
                    request.generation,
                    match request.source {
                        ResourceSource::Url(_) => ResourceFailureCode::Network,
                        _ => ResourceFailureCode::Decode,
                    },
                    Some(diagnostic),
                ))
            }
        }
    }

    fn complete_ready(
        &self,
        resource: ResourceId,
        generation: u64,
        url: String,
        owned: bool,
        dimensions: ResourceDimensions,
    ) -> Result<Option<ResourceEvent>, WebError> {
        let mut state = self.state.borrow_mut();
        if state.current.get(&resource) != Some(&generation) {
            if owned {
                let _ = web_sys::Url::revoke_object_url(&url);
            }
            return Ok(None);
        }

        self.store.register_url(resource, url.clone())?;
        let event = ResourceEvent::Ready {
            resource,
            generation,
            dimensions: Some(dimensions),
        };
        state.published.insert(resource, generation);
        state.generations.insert(
            (resource, generation),
            GenerationRecord {
                state: WebResourceState::Ready { dimensions },
                owned_url: owned.then_some(url),
            },
        );
        retain_event(&mut state, event.clone());
        Ok(Some(event))
    }

    fn complete_failure(
        &self,
        resource: ResourceId,
        generation: u64,
        code: ResourceFailureCode,
        diagnostic: Option<String>,
    ) -> Option<ResourceEvent> {
        let mut state = self.state.borrow_mut();
        if state.current.get(&resource) != Some(&generation) {
            return None;
        }
        let event = ResourceEvent::Failed {
            resource,
            generation,
            code,
            diagnostic: diagnostic.clone(),
        };
        state.generations.insert(
            (resource, generation),
            GenerationRecord {
                state: WebResourceState::Failed { code, diagnostic },
                owned_url: None,
            },
        );
        retain_event(&mut state, event.clone());
        Some(event)
    }

    fn release(&self, resource: ResourceId, generation: u64) -> Result<(), WebError> {
        let (owned_url, unpublish) = {
            let mut state = self.state.borrow_mut();
            let owned_url = state
                .generations
                .get(&(resource, generation))
                .and_then(|record| record.owned_url.clone());
            state.generations.insert(
                (resource, generation),
                GenerationRecord {
                    state: WebResourceState::Released,
                    owned_url: None,
                },
            );
            let unpublish = state.published.get(&resource) == Some(&generation);
            if unpublish {
                state.published.remove(&resource);
            }
            if state.current.get(&resource) == Some(&generation) {
                state.current.remove(&resource);
            }
            (owned_url, unpublish)
        };
        if unpublish {
            self.store.unregister(resource);
        }
        if let Some(url) = owned_url {
            web_sys::Url::revoke_object_url(&url)
                .map_err(|error| js_error("revoke Web resource object URL", error))?;
        }
        Ok(())
    }
}

fn retain_event(state: &mut ServiceState, event: ResourceEvent) {
    let key = match &event {
        ResourceEvent::Ready {
            resource,
            generation,
            ..
        }
        | ResourceEvent::Failed {
            resource,
            generation,
            ..
        } => (*resource, *generation),
    };
    state.events.insert(key, event.clone());
    state.pending_events.push_back(event);
}

fn source_url(source: &ResourceSource) -> Result<(String, bool), (ResourceFailureCode, String)> {
    match source {
        ResourceSource::Url(url) => Ok((url.clone(), false)),
        ResourceSource::Bytes { media_type, data } => {
            let bytes = Uint8Array::from(data.as_slice());
            let parts = Array::new();
            parts.push(&bytes);
            let options = web_sys::BlobPropertyBag::new();
            options.set_type(media_type);
            let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options)
                .map_err(|error| {
                    (
                        ResourceFailureCode::Decode,
                        js_diagnostic("create Web resource Blob", error),
                    )
                })?;
            web_sys::Url::create_object_url_with_blob(&blob)
                .map(|url| (url, true))
                .map_err(|error| {
                    (
                        ResourceFailureCode::Decode,
                        js_diagnostic("create Web resource object URL", error),
                    )
                })
        }
        ResourceSource::BundledAsset(_) => Err((
            ResourceFailureCode::Unsupported,
            "Web bundled assets require a generated asset URL".into(),
        )),
    }
}

async fn decode_raster(url: &str) -> Result<ResourceDimensions, String> {
    let image = web_sys::HtmlImageElement::new()
        .map_err(|error| js_diagnostic("create Web resource image", error))?;
    image.set_src(url);
    JsFuture::from(image.decode())
        .await
        .map_err(|error| js_diagnostic("decode Web raster resource", error))?;
    Ok(ResourceDimensions {
        width: image.natural_width() as f32,
        height: image.natural_height() as f32,
        scale: 1.0,
    })
}

fn js_diagnostic(context: &str, error: JsValue) -> String {
    format!(
        "{context}: {}",
        error.as_string().unwrap_or_else(|| format!("{error:?}"))
    )
}
