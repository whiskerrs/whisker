//! Resource acquisition messages kept outside per-frame scene transactions.

use crate::ResourceId;

/// Kind of platform resource requested by semantic paint or measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResourceKind {
    /// A decoded raster image.
    RasterImage,
    /// A scalable vector image.
    VectorImage,
    /// A font file or registered font face.
    Font,
    /// A pointing-device cursor image.
    Cursor,
    /// An external mask, filter, or paint server.
    PaintServer,
}

/// Host-independent location of resource bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceSource {
    /// An application or network URL resolved under Host policy.
    Url(String),
    /// A path in the application bundle produced by Whisker CNG.
    BundledAsset(String),
    /// Owned bytes transferred once through the resource channel, not per frame.
    Bytes {
        /// MIME media type.
        media_type: String,
        /// Encoded resource contents.
        data: Vec<u8>,
    },
}

/// A versioned request to acquire one resource.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceRequest {
    /// Stable resource identity referenced by frames.
    pub resource: ResourceId,
    /// Monotonic generation used to reject stale completions after replacement.
    pub generation: u64,
    /// Expected resource kind.
    pub kind: ResourceKind,
    /// Source resolved by the Host resource service.
    pub source: ResourceSource,
}

/// Command sent over the resource channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceCommand {
    /// Starts or replaces a resource acquisition.
    Load(ResourceRequest),
    /// Releases Host caches once no accepted frame references this generation.
    Release {
        /// Stable resource identity.
        resource: ResourceId,
        /// Generation being released.
        generation: u64,
    },
}

/// Intrinsic pixel dimensions reported for a ready resource.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResourceDimensions {
    /// Intrinsic width in logical pixels.
    pub width: f32,
    /// Intrinsic height in logical pixels.
    pub height: f32,
    /// Encoded pixels per logical pixel.
    pub scale: f32,
}

/// Stable failure classification; platform error strings remain diagnostic only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResourceFailureCode {
    /// Source could not be found.
    NotFound,
    /// Host policy denied acquisition.
    Denied,
    /// Network acquisition failed.
    Network,
    /// Bytes could not be decoded as the declared resource kind.
    Decode,
    /// Operation was cancelled because the generation was replaced or released.
    Cancelled,
    /// Host does not support this resource kind or source.
    Unsupported,
}

/// Completion sent by the Host resource service back to Rust.
#[derive(Clone, Debug, PartialEq)]
pub enum ResourceEvent {
    /// Resource is ready for painting or intrinsic measurement.
    Ready {
        /// Stable resource identity.
        resource: ResourceId,
        /// Generation completed by this event.
        generation: u64,
        /// Optional intrinsic dimensions for replaced content.
        dimensions: Option<ResourceDimensions>,
    },
    /// Resource acquisition failed deterministically.
    Failed {
        /// Stable resource identity.
        resource: ResourceId,
        /// Generation completed by this event.
        generation: u64,
        /// Portable failure classification.
        code: ResourceFailureCode,
        /// Optional Host diagnostic; application logic must not parse it.
        diagnostic: Option<String>,
    },
}

/// Malformed resource-channel message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceMessageError {
    /// Generations start at one so zero can represent absence in packed ABIs.
    ZeroGeneration,
    /// URL, asset path, or MIME type is empty.
    EmptyIdentifier,
    /// An inline byte source has no encoded data.
    EmptyData,
    /// Intrinsic dimensions are non-finite or negative, or scale is not positive.
    InvalidDimensions,
}

impl ResourceRequest {
    /// Validates a load request without performing Host I/O.
    pub fn validate(&self) -> Result<(), ResourceMessageError> {
        if self.generation == 0 {
            return Err(ResourceMessageError::ZeroGeneration);
        }
        match &self.source {
            ResourceSource::Url(value) | ResourceSource::BundledAsset(value)
                if value.trim().is_empty() =>
            {
                Err(ResourceMessageError::EmptyIdentifier)
            }
            ResourceSource::Bytes { media_type, .. } if media_type.trim().is_empty() => {
                Err(ResourceMessageError::EmptyIdentifier)
            }
            ResourceSource::Bytes { data, .. } if data.is_empty() => {
                Err(ResourceMessageError::EmptyData)
            }
            _ => Ok(()),
        }
    }
}

impl ResourceCommand {
    /// Validates a resource command without changing lifecycle state.
    pub fn validate(&self) -> Result<(), ResourceMessageError> {
        match self {
            Self::Load(request) => request.validate(),
            Self::Release { generation: 0, .. } => Err(ResourceMessageError::ZeroGeneration),
            Self::Release { .. } => Ok(()),
        }
    }
}

impl ResourceEvent {
    /// Validates a Host completion before it reaches runtime state.
    pub fn validate(&self) -> Result<(), ResourceMessageError> {
        let (generation, dimensions) = match self {
            Self::Ready {
                generation,
                dimensions,
                ..
            } => (*generation, *dimensions),
            Self::Failed { generation, .. } => (*generation, None),
        };
        if generation == 0 {
            return Err(ResourceMessageError::ZeroGeneration);
        }
        if let Some(dimensions) = dimensions
            && (!dimensions.width.is_finite()
                || dimensions.width < 0.0
                || !dimensions.height.is_finite()
                || dimensions.height < 0.0
                || !dimensions.scale.is_finite()
                || dimensions.scale <= 0.0)
        {
            return Err(ResourceMessageError::InvalidDimensions);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_messages_validate_generation_source_and_dimensions() {
        let request = ResourceRequest {
            resource: ResourceId::new(1).unwrap(),
            generation: 1,
            kind: ResourceKind::RasterImage,
            source: ResourceSource::BundledAsset("images/logo.png".into()),
        };
        assert_eq!(request.validate(), Ok(()));
        assert_eq!(ResourceCommand::Load(request).validate(), Ok(()));
        assert_eq!(
            ResourceEvent::Ready {
                resource: ResourceId::new(1).unwrap(),
                generation: 1,
                dimensions: Some(ResourceDimensions {
                    width: 100.0,
                    height: 50.0,
                    scale: 2.0,
                }),
            }
            .validate(),
            Ok(())
        );
    }

    #[test]
    fn zero_generation_and_invalid_inline_data_are_rejected() {
        let mut request = ResourceRequest {
            resource: ResourceId::new(1).unwrap(),
            generation: 0,
            kind: ResourceKind::VectorImage,
            source: ResourceSource::Bytes {
                media_type: "image/svg+xml".into(),
                data: Vec::new(),
            },
        };
        assert_eq!(
            request.validate(),
            Err(ResourceMessageError::ZeroGeneration)
        );
        request.generation = 1;
        assert_eq!(request.validate(), Err(ResourceMessageError::EmptyData));
        assert_eq!(
            ResourceCommand::Release {
                resource: ResourceId::new(1).unwrap(),
                generation: 0,
            }
            .validate(),
            Err(ResourceMessageError::ZeroGeneration)
        );
    }
}
