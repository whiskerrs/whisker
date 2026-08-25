//! Stable, borrowed C-ABI views used by the retained Android and iOS Hosts.
//!
//! Every pointer is valid only for the duration of the callback receiving the
//! enclosing value. Hosts must copy anything they retain after that callback.

use std::collections::BTreeMap;
use std::ffi::{CString, c_void};

use whisker_driver_sys as ffi;
use whisker_runtime::value::WhiskerValue;

pub use ffi::{
    WhiskerBytesRef, WhiskerKeyValueRaw, WhiskerStringRef, WhiskerValueArray, WhiskerValueMap,
    WhiskerValueRaw, WhiskerValueType, WhiskerValueUnion,
};

pub const MOBILE_ABI_MAJOR: u16 = 2;
pub const MOBILE_ABI_MINOR: u16 = 16;

pub const APPLY_ACCEPTED: u8 = 0;
pub const APPLY_NEED_SNAPSHOT: u8 = 1;
pub const APPLY_REJECTED: u8 = 2;

pub const FRAME_SNAPSHOT: u8 = 0;
pub const FRAME_DELTA: u8 = 1;

pub const OP_CREATE: u32 = 1;
pub const OP_DELETE: u32 = 2;
pub const OP_INSERT: u32 = 3;
pub const OP_REMOVE: u32 = 4;
pub const OP_MOVE: u32 = 5;
pub const OP_LAYOUT: u32 = 6;
pub const OP_PAINT: u32 = 7;
pub const OP_CLIP: u32 = 8;
pub const OP_TRANSFORM: u32 = 9;
pub const OP_OPACITY: u32 = 10;
pub const OP_VISIBILITY: u32 = 11;
pub const OP_Z_ORDER: u32 = 12;
pub const OP_TEXT: u32 = 13;
pub const OP_PROPERTY: u32 = 14;
pub const OP_CLEAR_PROPERTY: u32 = 15;
pub const OP_EVENT_MASK: u32 = 16;
pub const OP_HIT_TEST: u32 = 17;
pub const OP_CAPTURE: u32 = 18;
pub const OP_RELEASE_CAPTURE: u32 = 19;
pub const OP_COMMAND: u32 = 20;
pub const OP_BACKGROUND_LAYERS: u32 = 21;
pub const OP_BOX_SHADOWS: u32 = 22;
pub const OP_CLIP_PATH: u32 = 23;
pub const OP_BACKDROP_BLUR: u32 = 24;
pub const OP_IMAGE_RENDERING: u32 = 25;

pub const IMAGE_RENDERING_AUTO: i32 = 0;
pub const IMAGE_RENDERING_PIXELATED: i32 = 1;
pub const IMAGE_RENDERING_CRISP_EDGES: i32 = 2;

pub const BACKGROUND_LINEAR: u32 = 0;
pub const BACKGROUND_RADIAL: u32 = 1;
pub const BACKGROUND_CONIC: u32 = 2;
pub const BACKGROUND_RESOURCE: u32 = 3;

pub const BACKGROUND_SIZE_AUTO: u32 = 0;
pub const BACKGROUND_SIZE_EXPLICIT: u32 = 1;
pub const BACKGROUND_SIZE_COVER: u32 = 2;
pub const BACKGROUND_SIZE_CONTAIN: u32 = 3;
pub const BACKGROUND_SIZE_WIDTH: u32 = 4;
pub const BACKGROUND_SIZE_HEIGHT: u32 = 5;

pub const BACKGROUND_REPEAT_REPEAT: u32 = 0;
pub const BACKGROUND_REPEAT_NO_REPEAT: u32 = 1;
pub const BACKGROUND_REPEAT_SPACE: u32 = 2;
pub const BACKGROUND_REPEAT_ROUND: u32 = 3;

pub const BACKGROUND_BOX_BORDER: u32 = 0;
pub const BACKGROUND_BOX_PADDING: u32 = 1;
pub const BACKGROUND_BOX_CONTENT: u32 = 2;
pub const BACKGROUND_BOX_BORDER_AREA: u32 = 3;

pub const BACKGROUND_ATTACHMENT_SCROLL: u32 = 0;
pub const BACKGROUND_BLEND_NORMAL: u32 = 0;

pub const CLIP_SHAPE_INSET: u32 = 0;
pub const CLIP_SHAPE_CIRCLE: u32 = 1;
pub const CLIP_SHAPE_ELLIPSE: u32 = 2;
pub const CLIP_SHAPE_PATH: u32 = 3;
pub const FILL_RULE_NON_ZERO: u32 = 0;
pub const FILL_RULE_EVEN_ODD: u32 = 1;
pub const PATH_MOVE_TO: u32 = 0;
pub const PATH_LINE_TO: u32 = 1;
pub const PATH_QUADRATIC_TO: u32 = 2;
pub const PATH_CUBIC_TO: u32 = 3;
pub const PATH_CLOSE: u32 = 4;

pub const MEASURE_TEXT: u32 = 1;
pub const MEASURE_REPLACED_CONTENT: u32 = 2;
pub const MEASURE_NATIVE_CONTROL: u32 = 3;
pub const MEASURE_EMBEDDED_SURFACE: u32 = 4;
pub const MEASURE_CUSTOM: u32 = 5;

pub const MEASURE_READY: u32 = 1;
pub const MEASURE_PENDING: u32 = 2;
pub const MEASURE_UNSUPPORTED: u32 = 3;

pub const RESOURCE_COMMAND_LOAD: u32 = 1;
pub const RESOURCE_COMMAND_RELEASE: u32 = 2;
pub const RESOURCE_RASTER_IMAGE: u32 = 1;
pub const RESOURCE_VECTOR_IMAGE: u32 = 2;
pub const RESOURCE_FONT: u32 = 3;
pub const RESOURCE_CURSOR: u32 = 4;
pub const RESOURCE_PAINT_SERVER: u32 = 5;
pub const RESOURCE_SOURCE_NONE: u32 = 0;
pub const RESOURCE_SOURCE_URL: u32 = 1;
pub const RESOURCE_SOURCE_BUNDLED_ASSET: u32 = 2;
pub const RESOURCE_SOURCE_BYTES: u32 = 3;
pub const RESOURCE_EVENT_READY: u32 = 1;
pub const RESOURCE_EVENT_FAILED: u32 = 2;
pub const RESOURCE_FAILURE_NONE: u32 = 0;
pub const RESOURCE_FAILURE_NOT_FOUND: u32 = 1;
pub const RESOURCE_FAILURE_DENIED: u32 = 2;
pub const RESOURCE_FAILURE_NETWORK: u32 = 3;
pub const RESOURCE_FAILURE_DECODE: u32 = 4;
pub const RESOURCE_FAILURE_CANCELLED: u32 = 5;
pub const RESOURCE_FAILURE_UNSUPPORTED: u32 = 6;
pub const RESOURCE_DIMENSIONS_PRESENT: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MobileRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MobileLayoutGeometry {
    pub border: MobileRect,
    pub content: MobileRect,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileColor {
    /// 0 = named, 1 = sRGBA. Named colors borrow `name`.
    pub kind: u32,
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub _pad: u8,
    pub alpha: f32,
    pub name: WhiskerStringRef,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MobileLengthPercentage {
    pub length: f32,
    pub fraction: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileBoxPaint {
    pub background: MobileColor,
    pub widths: [MobileLengthPercentage; 4],
    pub colors: [MobileColor; 4],
    pub styles: [u32; 4],
    pub radii_horizontal: [MobileLengthPercentage; 4],
    pub radii_vertical: [MobileLengthPercentage; 4],
}

/// One resolved box shadow. Arrays are ordered front to back.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileBoxShadow {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur_radius: f32,
    pub spread_radius: f32,
    pub color: MobileColor,
    pub inset: u8,
    pub _pad: [u8; 7],
}

/// One rounded inset basic shape. Arrays use CSS top/right/bottom/left and
/// top-left/top-right/bottom-right/bottom-left ordering respectively.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileClipInset {
    pub edges: [MobileLengthPercentage; 4],
    pub radii_horizontal: [MobileLengthPercentage; 4],
    pub radii_vertical: [MobileLengthPercentage; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileClipCircle {
    pub radius: MobileLengthPercentage,
    pub center_x: MobileLengthPercentage,
    pub center_y: MobileLengthPercentage,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileClipEllipse {
    pub radius_x: MobileLengthPercentage,
    pub radius_y: MobileLengthPercentage,
    pub center_x: MobileLengthPercentage,
    pub center_y: MobileLengthPercentage,
}

/// One fixed-width absolute path command. Point slots are x/y pairs: one for
/// move/line, two for quadratic, and three for cubic commands.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MobilePathCommand {
    pub kind: u32,
    pub _reserved: u32,
    pub points: [MobileLengthPercentage; 6],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileClipPathCommands {
    pub fill_rule: u32,
    pub _reserved: u32,
    pub commands: *const MobilePathCommand,
    pub command_count: usize,
}

/// One typed clip path. `payload` points to the shape selected by `shape_kind`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileClipPath {
    pub reference_box: u32,
    pub shape_kind: u32,
    pub payload: *const c_void,
    pub payload_count: usize,
}

/// One explicit color stop shared by the additive gradient ABI subsets.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileGradientStop {
    pub color: MobileColor,
    pub position: MobileLengthPercentage,
}

/// One explicit, non-repeating elliptical radial gradient.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileRadialGradient {
    pub center_x: MobileLengthPercentage,
    pub center_y: MobileLengthPercentage,
    pub radius_x: MobileLengthPercentage,
    pub radius_y: MobileLengthPercentage,
    pub stops: *const MobileGradientStop,
    pub stop_count: usize,
}

/// One non-repeating conic gradient with explicit fractional stops.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileConicGradient {
    pub center_x: MobileLengthPercentage,
    pub center_y: MobileLengthPercentage,
    pub stops: *const MobileGradientStop,
    pub stop_count: usize,
}

/// One typed gradient image nested in [`MobileBackgroundLayer`].
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileBackgroundImage {
    pub kind: u32,
    pub scalar: f32,
    pub payload: *const c_void,
    pub payload_count: usize,
}

/// One retained background layer with explicit geometry and a typed image.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileBackgroundLayer {
    pub image: MobileBackgroundImage,
    pub position_x: MobileLengthPercentage,
    pub position_y: MobileLengthPercentage,
    pub size_width: MobileLengthPercentage,
    pub size_height: MobileLengthPercentage,
    pub size_kind: u32,
    pub repeat_x: u32,
    pub repeat_y: u32,
    pub origin: u32,
    pub clip: u32,
    pub attachment: u32,
    pub blend_mode: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileText {
    pub text: WhiskerStringRef,
    pub font_size: f32,
    pub font_weight: u16,
    pub font_style: u8,
    pub wrap: u8,
    pub max_lines: u32,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub color: MobileColor,
    pub shadow_offset_x: f32,
    pub shadow_offset_y: f32,
    pub shadow_blur_radius: f32,
    pub shadow_flags: u32,
    pub shadow_color: MobileColor,
    pub prepared_content: u64,
}

/// Flat operation envelope. `payload` points to the type selected by `tag`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileOperation {
    pub tag: u32,
    pub flags: u32,
    pub node: u64,
    pub parent: u64,
    pub child: u64,
    pub index: u32,
    pub member: u32,
    pub integer: i32,
    pub scalar: f32,
    pub wide: u64,
    pub payload: *const c_void,
    pub payload_count: usize,
}

#[repr(C)]
pub struct MobileFrame {
    pub abi_major: u16,
    pub abi_minor: u16,
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub mode: u8,
    pub _pad: [u8; 7],
    pub surface: u64,
    pub scene_epoch: u32,
    pub viewport_epoch: u32,
    pub frame_id: u64,
    pub base_revision: u64,
    pub target_revision: u64,
    pub operations: *const MobileOperation,
    pub operation_count: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MobileApplyResponse {
    pub status: u8,
    pub _pad: [u8; 7],
    pub revision: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileMemberRegistration {
    pub id: u32,
    pub value_kind: u8,
    pub optional_kind: u8,
    pub _pad: [u8; 2],
    pub name: WhiskerStringRef,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileElementRegistration {
    pub element_type: u32,
    pub child_policy: u8,
    pub measurement: u8,
    pub _pad: [u8; 2],
    pub name: WhiskerStringRef,
    pub properties: *const MobileMemberRegistration,
    pub property_count: usize,
    pub events: *const MobileMemberRegistration,
    pub event_count: usize,
    pub commands: *const MobileMemberRegistration,
    pub command_count: usize,
}

#[repr(C)]
pub struct MobileBootstrap {
    pub abi_major: u16,
    pub abi_minor: u16,
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub registrations: *const MobileElementRegistration,
    pub registration_count: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileMeasureRequest {
    pub key: u64,
    pub node: u64,
    pub element_type: u32,
    pub kind: u32,
    pub environment_epoch: u64,
    pub known_width: f32,
    pub known_height: f32,
    pub known_mask: u32,
    pub available_width: f32,
    pub available_height: f32,
    pub available_width_kind: u8,
    pub available_height_kind: u8,
    pub font_style: u8,
    pub wrap: u8,
    pub text: WhiskerStringRef,
    pub locale: WhiskerStringRef,
    pub font_family: WhiskerStringRef,
    pub font_size: f32,
    pub font_weight: u16,
    pub payload_version: u16,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub max_lines: u32,
    pub payload: WhiskerBytesRef,
    pub intrinsic_width: f32,
    pub intrinsic_height: f32,
    pub intrinsic_mask: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MobileMeasureResponse {
    pub key: u64,
    pub environment_epoch: u64,
    pub status: u32,
    pub reason: u32,
    pub width: f32,
    pub height: f32,
    pub first_baseline: f32,
    pub last_baseline: f32,
    pub metrics_mask: u32,
    pub request_id: u64,
    pub prepared_content: u64,
}

/// One borrowed Rust-to-Host resource command. String and byte pointers are
/// valid only during the callback.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileResourceCommand {
    pub command: u32,
    pub kind: u32,
    pub source: u32,
    pub _reserved: u32,
    pub resource: u64,
    pub generation: u64,
    pub identifier: WhiskerStringRef,
    pub data: WhiskerBytesRef,
}

/// One borrowed Host-to-Rust resource completion.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileResourceEvent {
    pub status: u32,
    pub failure_code: u32,
    pub resource: u64,
    pub generation: u64,
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    pub dimensions_mask: u32,
    pub diagnostic: WhiskerStringRef,
}

pub type BootstrapCallback = extern "C" fn(*mut c_void, *const MobileBootstrap) -> bool;
pub type PresentFrameCallback =
    extern "C" fn(*mut c_void, *const MobileFrame, *mut MobileApplyResponse) -> bool;
pub type MeasureCallback = extern "C" fn(
    *mut c_void,
    *const MobileMeasureRequest,
    usize,
    *mut MobileMeasureResponse,
) -> bool;
pub type ResourceCommandCallback = extern "C" fn(*mut c_void, *const MobileResourceCommand) -> bool;

/// Pinned allocations referenced by borrowed `WhiskerValueRaw` trees.
#[derive(Default)]
pub struct RawValueArena {
    strings: Vec<CString>,
    bytes: Vec<Vec<u8>>,
    arrays: Vec<Vec<WhiskerValueRaw>>,
    maps: Vec<Vec<WhiskerKeyValueRaw>>,
}

impl RawValueArena {
    fn string(&mut self, value: &str) -> WhiskerStringRef {
        let string = CString::new(value).unwrap_or_default();
        let result = WhiskerStringRef {
            ptr: string.as_ptr(),
            len: string.as_bytes().len(),
        };
        self.strings.push(string);
        result
    }

    pub fn encode(&mut self, value: &WhiskerValue) -> WhiskerValueRaw {
        let mut raw = WhiskerValueRaw {
            type_: WhiskerValueType::Null as u8,
            _pad: [0; 7],
            v: WhiskerValueUnion { i: 0 },
        };
        match value {
            WhiskerValue::Null => {}
            WhiskerValue::Bool(value) => {
                raw.type_ = WhiskerValueType::Bool as u8;
                raw.v.b = *value;
            }
            WhiskerValue::Int(value) => {
                raw.type_ = WhiskerValueType::Int as u8;
                raw.v.i = *value;
            }
            WhiskerValue::Float(value) => {
                raw.type_ = WhiskerValueType::Float as u8;
                raw.v.f = *value;
            }
            WhiskerValue::String(value) => {
                raw.type_ = WhiskerValueType::String as u8;
                raw.v.s = self.string(value);
            }
            WhiskerValue::Bytes(value) => {
                let owned = value.clone();
                raw.type_ = WhiskerValueType::Bytes as u8;
                raw.v.bytes = WhiskerBytesRef {
                    ptr: owned.as_ptr(),
                    len: owned.len(),
                };
                self.bytes.push(owned);
            }
            WhiskerValue::Array(values) => {
                let mut items = values
                    .iter()
                    .map(|value| self.encode(value))
                    .collect::<Vec<_>>();
                raw.type_ = WhiskerValueType::Array as u8;
                raw.v.array = WhiskerValueArray {
                    items: items.as_mut_ptr(),
                    count: items.len(),
                };
                self.arrays.push(items);
            }
            WhiskerValue::Map(values) => {
                let mut entries = Vec::with_capacity(values.len());
                for (key, value) in values {
                    let key = self.string(key);
                    let value = self.encode(value);
                    entries.push(WhiskerKeyValueRaw { key, value });
                }
                raw.type_ = WhiskerValueType::Map as u8;
                raw.v.map = WhiskerValueMap {
                    entries: entries.as_mut_ptr(),
                    count: entries.len(),
                };
                self.maps.push(entries);
            }
            WhiskerValue::Error(value) => {
                raw.type_ = WhiskerValueType::Error as u8;
                raw.v.s = self.string(value);
            }
        }
        raw
    }
}

/// Copies one well-formed borrowed raw value into Rust-owned storage.
pub unsafe fn decode_value(raw: *const WhiskerValueRaw) -> WhiskerValue {
    if raw.is_null() {
        return WhiskerValue::Null;
    }
    let raw = unsafe { &*raw };
    unsafe {
        match raw.type_ {
            x if x == WhiskerValueType::Null as u8 => WhiskerValue::Null,
            x if x == WhiskerValueType::Bool as u8 => WhiskerValue::Bool(raw.v.b),
            x if x == WhiskerValueType::Int as u8 => WhiskerValue::Int(raw.v.i),
            x if x == WhiskerValueType::Float as u8 => WhiskerValue::Float(raw.v.f),
            x if x == WhiskerValueType::String as u8 => WhiskerValue::String(read_string(raw.v.s)),
            x if x == WhiskerValueType::Bytes as u8 => WhiskerValue::Bytes(read_bytes(raw.v.bytes)),
            x if x == WhiskerValueType::Array as u8 => {
                let values = raw.v.array;
                WhiskerValue::Array(
                    (0..values.count)
                        .map(|index| decode_value(values.items.add(index)))
                        .collect(),
                )
            }
            x if x == WhiskerValueType::Map as u8 => {
                let values = raw.v.map;
                let mut map = BTreeMap::new();
                for index in 0..values.count {
                    let entry = &*values.entries.add(index);
                    map.insert(read_string(entry.key), decode_value(&entry.value));
                }
                WhiskerValue::Map(map)
            }
            x if x == WhiskerValueType::Error as u8 => WhiskerValue::Error(read_string(raw.v.s)),
            other => WhiskerValue::Error(format!("unknown WhiskerValue ABI tag {other}")),
        }
    }
}

unsafe fn read_string(value: WhiskerStringRef) -> String {
    if value.ptr.is_null() || value.len == 0 {
        return String::new();
    }
    let bytes = unsafe { std::slice::from_raw_parts(value.ptr.cast::<u8>(), value.len) };
    String::from_utf8_lossy(bytes).into_owned()
}

unsafe fn read_bytes(value: WhiskerBytesRef) -> Vec<u8> {
    if value.ptr.is_null() || value.len == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(value.ptr, value.len) }.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_abi_round_trips_nested_values_without_json() {
        let value = WhiskerValue::Map(BTreeMap::from([
            ("bytes".into(), WhiskerValue::Bytes(vec![1, 2, 3])),
            (
                "values".into(),
                WhiskerValue::Array(vec![WhiskerValue::Bool(true), WhiskerValue::Int(7)]),
            ),
        ]));
        let mut arena = RawValueArena::default();
        let raw = arena.encode(&value);
        assert_eq!(unsafe { decode_value(&raw) }, value);
    }

    #[test]
    fn mobile_abi_layouts_are_stable_on_64_bit_hosts() {
        if usize::BITS == 64 {
            assert_eq!(std::mem::size_of::<MobileRect>(), 16);
            assert_eq!(std::mem::size_of::<MobileOperation>(), 72);
            assert_eq!(std::mem::size_of::<MobileFrame>(), 72);
            assert_eq!(std::mem::size_of::<MobileApplyResponse>(), 16);
            assert_eq!(std::mem::size_of::<MobileMemberRegistration>(), 24);
            assert_eq!(std::mem::size_of::<MobileElementRegistration>(), 72);
            assert_eq!(std::mem::size_of::<MobileBootstrap>(), 24);
            assert_eq!(std::mem::size_of::<MobileMeasureRequest>(), 160);
            assert_eq!(std::mem::size_of::<MobileMeasureResponse>(), 64);
            assert_eq!(std::mem::size_of::<MobileText>(), 128);
            assert_eq!(std::mem::size_of::<MobileBoxPaint>(), 272);
            assert_eq!(std::mem::size_of::<MobileBoxShadow>(), 56);
            assert_eq!(std::mem::size_of::<MobileClipInset>(), 96);
            assert_eq!(std::mem::size_of::<MobileClipCircle>(), 24);
            assert_eq!(std::mem::size_of::<MobileClipEllipse>(), 32);
            assert_eq!(std::mem::size_of::<MobilePathCommand>(), 56);
            assert_eq!(std::mem::size_of::<MobileClipPathCommands>(), 24);
            assert_eq!(std::mem::size_of::<MobileClipPath>(), 24);
            assert_eq!(std::mem::size_of::<MobileGradientStop>(), 40);
            assert_eq!(std::mem::size_of::<MobileRadialGradient>(), 48);
            assert_eq!(std::mem::size_of::<MobileConicGradient>(), 32);
            assert_eq!(std::mem::size_of::<MobileBackgroundImage>(), 24);
            assert_eq!(std::mem::size_of::<MobileBackgroundLayer>(), 88);
            assert_eq!(std::mem::size_of::<MobileResourceCommand>(), 64);
            assert_eq!(std::mem::size_of::<MobileResourceEvent>(), 56);
            assert_eq!(std::mem::align_of::<MobileFrame>(), 8);
            assert_eq!(std::mem::align_of::<MobileMeasureRequest>(), 8);
        }
    }
}
