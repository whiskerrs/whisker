use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};

use base64::Engine;
use whisker_protocol::{
    ResourceCommand, ResourceDimensions, ResourceEvent, ResourceFailureCode, ResourceId,
    ResourceKind, ResourceMessageError, ResourceRequest, ResourceSource,
};

use crate::gpu::RasterResource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DesktopResourceState {
    Loading,
    Ready { width: u32, height: u32 },
    Failed(ResourceFailureCode),
    Released,
}

#[derive(Debug)]
pub(crate) enum DesktopResourceUpdate {
    Ready {
        event: ResourceEvent,
        raster: RasterResource,
    },
    Failed(ResourceEvent),
    Released {
        resource: ResourceId,
        evict: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DesktopResourceError {
    InvalidMessage(ResourceMessageError),
    UnsupportedKind(ResourceKind),
    NonMonotonicGeneration {
        resource: ResourceId,
        current: u64,
        received: u64,
    },
}

impl fmt::Display for DesktopResourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Desktop resource error: {self:?}")
    }
}

impl Error for DesktopResourceError {}

struct Completion {
    resource: ResourceId,
    generation: u64,
    result: Result<RasterResource, (ResourceFailureCode, String)>,
}

pub(crate) struct DesktopResourceService {
    asset_root: PathBuf,
    current: HashMap<ResourceId, u64>,
    states: HashMap<(ResourceId, u64), DesktopResourceState>,
    sender: mpsc::Sender<Completion>,
    receiver: Receiver<Completion>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl DesktopResourceService {
    pub(crate) fn new(asset_root: PathBuf, wake: impl Fn() + Send + Sync + 'static) -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            asset_root,
            current: HashMap::new(),
            states: HashMap::new(),
            sender,
            receiver,
            wake: Arc::new(wake),
        }
    }

    pub(crate) fn command(
        &mut self,
        command: ResourceCommand,
    ) -> Result<Vec<DesktopResourceUpdate>, DesktopResourceError> {
        command
            .validate()
            .map_err(DesktopResourceError::InvalidMessage)?;
        match command {
            ResourceCommand::Load(request) => {
                self.load(request)?;
                Ok(Vec::new())
            }
            ResourceCommand::Release {
                resource,
                generation,
            } => Ok(vec![self.release(resource, generation)]),
        }
    }

    fn load(&mut self, request: ResourceRequest) -> Result<(), DesktopResourceError> {
        if request.kind != ResourceKind::RasterImage {
            return Err(DesktopResourceError::UnsupportedKind(request.kind));
        }
        if let Some(current) = self.current.get(&request.resource).copied()
            && request.generation <= current
        {
            return Err(DesktopResourceError::NonMonotonicGeneration {
                resource: request.resource,
                current,
                received: request.generation,
            });
        }
        self.current.insert(request.resource, request.generation);
        self.states.insert(
            (request.resource, request.generation),
            DesktopResourceState::Loading,
        );
        let sender = self.sender.clone();
        let wake = Arc::clone(&self.wake);
        let asset_root = self.asset_root.clone();
        std::thread::Builder::new()
            .name("whisker-desktop-resource".into())
            .spawn(move || {
                let completion = Completion {
                    resource: request.resource,
                    generation: request.generation,
                    result: acquire_raster(request.source, asset_root),
                };
                let _ = sender.send(completion);
                wake();
            })
            .expect("Desktop Host can spawn a resource worker");
        Ok(())
    }

    fn release(&mut self, resource: ResourceId, generation: u64) -> DesktopResourceUpdate {
        self.states
            .insert((resource, generation), DesktopResourceState::Released);
        let evict = self.current.get(&resource).copied() == Some(generation);
        if evict {
            self.current.remove(&resource);
        }
        DesktopResourceUpdate::Released { resource, evict }
    }

    pub(crate) fn drain(&mut self) -> Vec<DesktopResourceUpdate> {
        let mut updates = Vec::new();
        while let Ok(completion) = self.receiver.try_recv() {
            if self.current.get(&completion.resource).copied() != Some(completion.generation)
                || self
                    .states
                    .get(&(completion.resource, completion.generation))
                    .is_some_and(|state| *state == DesktopResourceState::Released)
            {
                continue;
            }
            match completion.result {
                Ok(raster) => {
                    let dimensions = ResourceDimensions {
                        width: raster.width as f32,
                        height: raster.height as f32,
                        scale: 1.0,
                    };
                    self.states.insert(
                        (completion.resource, completion.generation),
                        DesktopResourceState::Ready {
                            width: raster.width,
                            height: raster.height,
                        },
                    );
                    updates.push(DesktopResourceUpdate::Ready {
                        event: ResourceEvent::Ready {
                            resource: completion.resource,
                            generation: completion.generation,
                            dimensions: Some(dimensions),
                        },
                        raster,
                    });
                }
                Err((code, diagnostic)) => {
                    self.states.insert(
                        (completion.resource, completion.generation),
                        DesktopResourceState::Failed(code),
                    );
                    updates.push(DesktopResourceUpdate::Failed(ResourceEvent::Failed {
                        resource: completion.resource,
                        generation: completion.generation,
                        code,
                        diagnostic: Some(diagnostic),
                    }));
                }
            }
        }
        updates
    }

    #[cfg(all(test, feature = "host-conformance"))]
    pub(crate) fn state(
        &self,
        resource: ResourceId,
        generation: u64,
    ) -> Option<DesktopResourceState> {
        self.states.get(&(resource, generation)).copied()
    }
}

fn acquire_raster(
    source: ResourceSource,
    asset_root: PathBuf,
) -> Result<RasterResource, (ResourceFailureCode, String)> {
    let bytes = match source {
        ResourceSource::Bytes { data, .. } => data,
        ResourceSource::BundledAsset(path) => std::fs::read(asset_root.join(path))
            .map_err(|error| (ResourceFailureCode::NotFound, error.to_string()))?,
        ResourceSource::Url(url) if url.starts_with("data:") => decode_data_url(&url)?,
        ResourceSource::Url(url) if url.starts_with("https://") || url.starts_with("http://") => {
            let response = ureq::get(&url)
                .call()
                .map_err(|error| (ResourceFailureCode::Network, error.to_string()))?;
            let mut bytes = Vec::new();
            response
                .into_reader()
                .take(64 * 1024 * 1024)
                .read_to_end(&mut bytes)
                .map_err(|error| (ResourceFailureCode::Network, error.to_string()))?;
            bytes
        }
        ResourceSource::Url(url) => {
            return Err((
                ResourceFailureCode::Unsupported,
                format!("unsupported Desktop resource URL: {url}"),
            ));
        }
    };
    let image = image::load_from_memory(&bytes)
        .map_err(|error| (ResourceFailureCode::Decode, error.to_string()))?
        .into_rgba8();
    let (width, height) = image.dimensions();
    RasterResource::new(width, height, image.into_raw())
        .map_err(|error| (ResourceFailureCode::Decode, error.to_string()))
}

fn decode_data_url(url: &str) -> Result<Vec<u8>, (ResourceFailureCode, String)> {
    let (metadata, payload) = url.split_once(',').ok_or_else(|| {
        (
            ResourceFailureCode::Decode,
            "data URL has no payload delimiter".into(),
        )
    })?;
    if !metadata.starts_with("data:image/") || !metadata.ends_with(";base64") {
        return Err((
            ResourceFailureCode::Unsupported,
            "Desktop raster data URL must be base64 image data".into(),
        ));
    }
    base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|error| (ResourceFailureCode::Decode, error.to_string()))
}
