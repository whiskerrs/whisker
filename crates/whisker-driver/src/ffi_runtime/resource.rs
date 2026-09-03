use super::*;

pub(super) struct MobileResourceHost {
    pub(super) callback: ResourceCommandCallback,
    pub(super) data: *mut c_void,
}

impl MobileResourceHost {
    pub(super) fn send(&self, command: &ResourceCommand) -> bool {
        let empty_string = WhiskerStringRef {
            ptr: std::ptr::null(),
            len: 0,
        };
        let empty_bytes = WhiskerBytesRef {
            ptr: std::ptr::null(),
            len: 0,
        };
        let value = match command {
            ResourceCommand::Load(request) => {
                let (source, identifier, data) = match &request.source {
                    ResourceSource::Url(value) => {
                        (RESOURCE_SOURCE_URL, string_ref(value), empty_bytes)
                    }
                    ResourceSource::BundledAsset(value) => (
                        RESOURCE_SOURCE_BUNDLED_ASSET,
                        string_ref(value),
                        empty_bytes,
                    ),
                    ResourceSource::Bytes { media_type, data } => (
                        RESOURCE_SOURCE_BYTES,
                        string_ref(media_type),
                        WhiskerBytesRef {
                            ptr: data.as_ptr(),
                            len: data.len(),
                        },
                    ),
                };
                MobileResourceCommand {
                    command: RESOURCE_COMMAND_LOAD,
                    kind: encode_resource_kind(request.kind),
                    source,
                    _reserved: 0,
                    resource: request.resource.get(),
                    generation: request.generation,
                    identifier,
                    data,
                }
            }
            ResourceCommand::Release {
                resource,
                generation,
            } => MobileResourceCommand {
                command: RESOURCE_COMMAND_RELEASE,
                kind: 0,
                source: RESOURCE_SOURCE_NONE,
                _reserved: 0,
                resource: resource.get(),
                generation: *generation,
                identifier: empty_string,
                data: empty_bytes,
            },
        };
        (self.callback)(self.data, &value)
    }
}

pub(super) fn string_ref(value: &str) -> WhiskerStringRef {
    WhiskerStringRef {
        ptr: value.as_ptr().cast(),
        len: value.len(),
    }
}

pub(super) fn encode_resource_kind(kind: ResourceKind) -> u32 {
    match kind {
        ResourceKind::RasterImage => RESOURCE_RASTER_IMAGE,
        ResourceKind::VectorImage => RESOURCE_VECTOR_IMAGE,
        ResourceKind::Font => RESOURCE_FONT,
        ResourceKind::Cursor => RESOURCE_CURSOR,
        ResourceKind::PaintServer => RESOURCE_PAINT_SERVER,
    }
}

pub(super) fn decode_resource_event(event: &MobileResourceEvent) -> Option<ResourceEvent> {
    let resource = ResourceId::new(event.resource)?;
    match event.status {
        RESOURCE_EVENT_READY => {
            let dimensions = if event.dimensions_mask & RESOURCE_DIMENSIONS_PRESENT != 0 {
                Some(ResourceDimensions {
                    width: event.width,
                    height: event.height,
                    scale: event.scale,
                })
            } else {
                None
            };
            let event = ResourceEvent::Ready {
                resource,
                generation: event.generation,
                dimensions,
            };
            event.validate().ok()?;
            Some(event)
        }
        RESOURCE_EVENT_FAILED => {
            let diagnostic = decode_optional_string(event.diagnostic)?;
            let event = ResourceEvent::Failed {
                resource,
                generation: event.generation,
                code: decode_resource_failure(event.failure_code)?,
                diagnostic,
            };
            event.validate().ok()?;
            Some(event)
        }
        _ => None,
    }
}

pub(super) fn decode_optional_string(value: WhiskerStringRef) -> Option<Option<String>> {
    if value.len == 0 {
        return Some(None);
    }
    if value.ptr.is_null() {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(value.ptr.cast::<u8>(), value.len) };
    Some(Some(std::str::from_utf8(bytes).ok()?.to_owned()))
}

pub(super) fn decode_resource_failure(value: u32) -> Option<ResourceFailureCode> {
    Some(match value {
        RESOURCE_FAILURE_NOT_FOUND => ResourceFailureCode::NotFound,
        RESOURCE_FAILURE_DENIED => ResourceFailureCode::Denied,
        RESOURCE_FAILURE_NETWORK => ResourceFailureCode::Network,
        RESOURCE_FAILURE_DECODE => ResourceFailureCode::Decode,
        RESOURCE_FAILURE_CANCELLED => ResourceFailureCode::Cancelled,
        RESOURCE_FAILURE_UNSUPPORTED => ResourceFailureCode::Unsupported,
        _ => return None,
    })
}

pub(super) struct MobileBootstrapOwned {
    pub(super) value: MobileBootstrap,
    _strings: Vec<Box<[u8]>>,
    _members: Vec<Vec<MobileMemberRegistration>>,
    _registrations: Vec<MobileElementRegistration>,
}

impl MobileBootstrapOwned {
    pub(super) fn new(source: &[ElementRegistration]) -> Self {
        let mut strings = Vec::new();
        let mut members = Vec::<Vec<MobileMemberRegistration>>::new();
        let mut registrations = Vec::with_capacity(source.len());
        for registration in source {
            let name = push_string(&mut strings, &registration.name);
            let properties = registration
                .properties
                .iter()
                .map(|item| {
                    member_registration(
                        item.property.get(),
                        &item.name,
                        item.value,
                        false,
                        &mut strings,
                    )
                })
                .collect::<Vec<_>>();
            let events = registration
                .events
                .iter()
                .map(|item| {
                    member_registration(
                        item.event.get(),
                        &item.name,
                        item.detail.unwrap_or(ElementValueKind::Null),
                        item.detail.is_some(),
                        &mut strings,
                    )
                })
                .collect::<Vec<_>>();
            let commands = registration
                .commands
                .iter()
                .map(|item| {
                    member_registration(
                        item.command.get(),
                        &item.name,
                        item.arguments,
                        false,
                        &mut strings,
                    )
                })
                .collect::<Vec<_>>();
            members.push(properties);
            let (property_ptr, property_count) = (
                members.last().unwrap().as_ptr(),
                members.last().unwrap().len(),
            );
            members.push(events);
            let (event_ptr, event_count) = (
                members.last().unwrap().as_ptr(),
                members.last().unwrap().len(),
            );
            members.push(commands);
            let commands = members.last().unwrap();
            registrations.push(MobileElementRegistration {
                element_type: registration.element_type.get(),
                child_policy: match registration.child_policy {
                    ChildPolicy::None => 0,
                    ChildPolicy::Elements => 1,
                    ChildPolicy::PlainText => 2,
                },
                measurement: match registration.measurement {
                    ElementMeasurement::None => 0,
                    ElementMeasurement::Text => 1,
                    ElementMeasurement::ReplacedContent => 2,
                    ElementMeasurement::Custom => 3,
                },
                text_style: u8::from(registration.text_style),
                _pad: 0,
                name,
                properties: property_ptr,
                property_count,
                events: event_ptr,
                event_count,
                commands: commands.as_ptr(),
                command_count: commands.len(),
            });
        }
        let value = MobileBootstrap {
            abi_major: MOBILE_ABI_MAJOR,
            abi_minor: MOBILE_ABI_MINOR,
            protocol_major: whisker_engine::whisker_protocol::PROTOCOL_MAJOR,
            protocol_minor: whisker_engine::whisker_protocol::PROTOCOL_MINOR,
            registrations: registrations.as_ptr(),
            registration_count: registrations.len(),
        };
        Self {
            value,
            _strings: strings,
            _members: members,
            _registrations: registrations,
        }
    }
}

pub(super) fn push_string(strings: &mut Vec<Box<[u8]>>, value: &str) -> WhiskerStringRef {
    if value.is_empty() {
        return empty_string();
    }
    let value = value.as_bytes().to_vec().into_boxed_slice();
    let result = WhiskerStringRef {
        ptr: value.as_ptr().cast(),
        len: value.len(),
    };
    strings.push(value);
    result
}

pub(super) fn empty_string() -> WhiskerStringRef {
    WhiskerStringRef {
        ptr: std::ptr::null(),
        len: 0,
    }
}

pub(super) fn member_registration(
    id: u32,
    name: &str,
    kind: ElementValueKind,
    optional: bool,
    strings: &mut Vec<Box<[u8]>>,
) -> MobileMemberRegistration {
    MobileMemberRegistration {
        id,
        value_kind: match kind {
            ElementValueKind::Null => 0,
            ElementValueKind::Bool => 1,
            ElementValueKind::Int => 2,
            ElementValueKind::Float => 3,
            ElementValueKind::String => 4,
            ElementValueKind::Bytes => 5,
            ElementValueKind::Array => 6,
            ElementValueKind::Map => 7,
        },
        optional_kind: u8::from(optional),
        _pad: [0; 2],
        name: push_string(strings, name),
    }
}
