//! Stable, borrowed C-ABI views used by the retained Android and iOS Hosts.
//!
//! Every pointer is valid only for the duration of the callback receiving the
//! enclosing value. Hosts must copy anything they retain after that callback.

use std::ffi::c_void;

use crate::{WhiskerBytesRef, WhiskerStringRef, WhiskerValueRaw};

pub const MOBILE_ABI_MAJOR: u16 = 2;
pub const MOBILE_ABI_MINOR: u16 = 30;
pub const FRAME_PROTOCOL_MAJOR: u16 = 1;
pub const FRAME_PROTOCOL_MINOR: u16 = 4;

pub const CAPABILITY_ELLIPTICAL_BORDER_RADIUS: u64 = 0x0001;
pub const CAPABILITY_BACKGROUND_LAYERS: u64 = 0x0002;
pub const CAPABILITY_VISUAL_EFFECTS: u64 = 0x0004;
pub const CAPABILITY_TEXT_EFFECTS: u64 = 0x0008;
pub const CAPABILITY_TEXT_TYPOGRAPHY: u64 = 0x0010;
pub const CAPABILITY_CURSOR: u64 = 0x0040;
pub const CAPABILITY_RESOURCE_LIFECYCLE: u64 = 0x0080;
pub const CAPABILITY_LINEAR_GRADIENTS: u64 = 0x0100;
pub const CAPABILITY_RADIAL_GRADIENTS: u64 = 0x0200;
pub const CAPABILITY_CONIC_GRADIENTS: u64 = 0x0400;
pub const CAPABILITY_BACKGROUND_GEOMETRY: u64 = 0x0800;
pub const CAPABILITY_BACKGROUND_LAYER_STACKING: u64 = 0x1000;
pub const CAPABILITY_BACKGROUND_IMAGE_RESOURCES: u64 = 0x2000;
pub const CAPABILITY_BACKDROP_BLUR: u64 = 0x4000;
pub const CAPABILITY_RADIAL_GRADIENT_VARIANTS: u64 = 0x8000;

pub const POINTER_DOWN: u32 = 0;
pub const POINTER_MOVE: u32 = 1;
pub const POINTER_UP: u32 = 2;
pub const POINTER_CANCEL: u32 = 3;
pub const POINTER_MOUSE: u32 = 0;
pub const POINTER_TOUCH: u32 = 1;
pub const POINTER_PEN: u32 = 2;
pub const POINTER_UNKNOWN: u32 = 3;

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
pub const OP_CURSOR: u32 = 26;
pub const OP_TEXT_STYLE: u32 = 27;
pub const OP_ACCESSIBILITY: u32 = 28;

pub const IMAGE_RENDERING_AUTO: i32 = 0;
pub const IMAGE_RENDERING_PIXELATED: i32 = 1;
pub const IMAGE_RENDERING_CRISP_EDGES: i32 = 2;

pub const BACKGROUND_LINEAR: u32 = 0;
pub const BACKGROUND_RADIAL: u32 = 1;
pub const BACKGROUND_CONIC: u32 = 2;
pub const BACKGROUND_RESOURCE: u32 = 3;

pub const RADIAL_SHAPE_CIRCLE: u32 = 0;
pub const RADIAL_SHAPE_ELLIPSE: u32 = 1;
pub const RADIAL_EXTENT_CLOSEST_SIDE: u32 = 0;
pub const RADIAL_EXTENT_FARTHEST_SIDE: u32 = 1;
pub const RADIAL_EXTENT_CLOSEST_CORNER: u32 = 2;
pub const RADIAL_EXTENT_FARTHEST_CORNER: u32 = 3;
pub const RADIAL_EXTENT_EXPLICIT: u32 = 4;

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

/// Host-advertised renderer profile supplied once when a surface is created.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MobileHostCapabilities {
    pub abi_major: u16,
    pub abi_minor: u16,
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub native: u64,
    pub emulated: u64,
}

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

/// One resolved, non-repeating radial gradient.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileRadialGradient {
    pub shape: u32,
    pub extent: u32,
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
pub struct MobileFontFeature {
    pub tag: [u8; 4],
    pub value: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileFontVariation {
    pub tag: [u8; 4],
    pub value: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MobileText {
    pub text: WhiskerStringRef,
    pub font_families: *const WhiskerStringRef,
    pub font_family_count: usize,
    pub font_size: f32,
    pub font_weight: u16,
    pub font_style: u8,
    pub wrap: u8,
    pub word_break: u8,
    pub overflow: u8,
    pub max_lines: u32,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub font_features: *const MobileFontFeature,
    pub font_feature_count: usize,
    pub font_variations: *const MobileFontVariation,
    pub font_variation_count: usize,
    pub font_optical_sizing: u8,
    pub _font_pad: [u8; 7],
    pub color: MobileColor,
    pub shadow_offset_x: f32,
    pub shadow_offset_y: f32,
    pub shadow_blur_radius: f32,
    pub shadow_flags: u32,
    pub shadow_color: MobileColor,
    pub decoration_flags: u32,
    pub decoration_style: u32,
    pub decoration_color: MobileColor,
    pub alignment: u32,
    pub indent_logical_pixels: f32,
    pub indent_percentage: f32,
    pub prepared_content: u64,
    pub direction: u32,
    pub _direction_pad: u32,
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
    pub text_style: u8,
    pub _pad: u8,
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
    pub word_break: u8,
    pub overflow: u8,
    pub text: WhiskerStringRef,
    pub locale: WhiskerStringRef,
    pub font_families: *const WhiskerStringRef,
    pub font_family_count: usize,
    pub font_size: f32,
    pub font_weight: u16,
    pub payload_version: u16,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub font_features: *const MobileFontFeature,
    pub font_feature_count: usize,
    pub font_variations: *const MobileFontVariation,
    pub font_variation_count: usize,
    pub font_optical_sizing: u8,
    pub _font_pad: [u8; 7],
    pub indent_logical_pixels: f32,
    pub indent_percentage: f32,
    pub max_lines: u32,
    pub payload: WhiskerBytesRef,
    pub intrinsic_width: f32,
    pub intrinsic_height: f32,
    pub intrinsic_mask: u32,
    pub direction: u8,
    pub alignment: u8,
    pub _flow_pad: [u8; 6],
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

pub type RequestFrameCallback = extern "C" fn(*mut c_void);
pub type ModuleResultCallback = extern "C" fn(*mut c_void, *const WhiskerValueRaw);
pub type InvokeModuleCallback = extern "C" fn(
    *mut c_void,
    *const u8,
    usize,
    *const u8,
    usize,
    *const WhiskerValueRaw,
    usize,
    bool,
    ModuleResultCallback,
    *mut c_void,
) -> bool;
pub type ObserveModuleCallback =
    extern "C" fn(*mut c_void, *const u8, usize, *const u8, usize, bool);

// These symbols are implemented in each user application by `#[whisker::main]`.
// Declaring them here keeps the native Host header and the generated Rust
// exports on one shared type-level Interface.
unsafe extern "C" {
    pub fn whisker_view_create(
        width: f32,
        height: f32,
        scale: f32,
        capabilities: *const MobileHostCapabilities,
        request_frame: RequestFrameCallback,
        request_frame_data: *mut c_void,
        bootstrap: BootstrapCallback,
        bootstrap_data: *mut c_void,
        measure: MeasureCallback,
        measure_data: *mut c_void,
        present_frame: PresentFrameCallback,
        present_frame_data: *mut c_void,
        resource_command: ResourceCommandCallback,
        resource_data: *mut c_void,
        invoke_module: InvokeModuleCallback,
        observe_module: ObserveModuleCallback,
        module_data: *mut c_void,
    ) -> *mut c_void;
    pub fn whisker_view_tick(
        handle: *mut c_void,
        timestamp_ms: f64,
        width: f32,
        height: f32,
        scale: f32,
    ) -> bool;
    pub fn whisker_view_destroy(handle: *mut c_void);
    pub fn whisker_view_dispatch_event(
        handle: *mut c_void,
        timestamp_ms: f64,
        node: u64,
        name: *const u8,
        name_len: usize,
        detail: *const WhiskerValueRaw,
    ) -> bool;
    pub fn whisker_view_dispatch_pointer(
        handle: *mut c_void,
        timestamp_ms: f64,
        event: u32,
        pointer_id: u64,
        pointer_kind: u32,
        x: f32,
        y: f32,
        buttons: u32,
        changed_button: i16,
    ) -> bool;
    pub fn whisker_view_dispatch_module_event(
        handle: *mut c_void,
        module: *const u8,
        module_len: usize,
        event: *const u8,
        event_len: usize,
        payload: *const WhiskerValueRaw,
    ) -> bool;
    pub fn whisker_view_dispatch_resource_event(
        handle: *mut c_void,
        event: *const MobileResourceEvent,
    ) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert_eq!(std::mem::size_of::<MobileMeasureRequest>(), 224);
            assert_eq!(std::mem::size_of::<MobileMeasureResponse>(), 64);
            assert_eq!(std::mem::size_of::<MobileText>(), 248);
            assert_eq!(std::mem::size_of::<MobileBoxPaint>(), 272);
            assert_eq!(std::mem::size_of::<MobileBoxShadow>(), 56);
            assert_eq!(std::mem::size_of::<MobileClipInset>(), 96);
            assert_eq!(std::mem::size_of::<MobileClipCircle>(), 24);
            assert_eq!(std::mem::size_of::<MobileClipEllipse>(), 32);
            assert_eq!(std::mem::size_of::<MobilePathCommand>(), 56);
            assert_eq!(std::mem::size_of::<MobileClipPathCommands>(), 24);
            assert_eq!(std::mem::size_of::<MobileClipPath>(), 24);
            assert_eq!(std::mem::size_of::<MobileGradientStop>(), 40);
            assert_eq!(std::mem::size_of::<MobileRadialGradient>(), 56);
            assert_eq!(std::mem::size_of::<MobileConicGradient>(), 32);
            assert_eq!(std::mem::size_of::<MobileBackgroundImage>(), 24);
            assert_eq!(std::mem::size_of::<MobileBackgroundLayer>(), 88);
            assert_eq!(std::mem::size_of::<MobileResourceCommand>(), 64);
            assert_eq!(std::mem::size_of::<MobileResourceEvent>(), 56);
            assert_eq!(std::mem::align_of::<MobileFrame>(), 8);
            assert_eq!(std::mem::align_of::<MobileMeasureRequest>(), 8);
            assert_eq!(std::mem::offset_of!(MobileOperation, tag), 0);
            assert_eq!(std::mem::offset_of!(MobileOperation, node), 8);
            assert_eq!(std::mem::offset_of!(MobileOperation, integer), 40);
            assert_eq!(std::mem::offset_of!(MobileOperation, wide), 48);
            assert_eq!(std::mem::offset_of!(MobileOperation, payload), 56);
            assert_eq!(std::mem::offset_of!(MobileOperation, payload_count), 64);
            assert_eq!(std::mem::offset_of!(MobileFrame, mode), 8);
            assert_eq!(std::mem::offset_of!(MobileFrame, surface), 16);
            assert_eq!(std::mem::offset_of!(MobileFrame, operations), 56);
            assert_eq!(std::mem::offset_of!(MobileFrame, operation_count), 64);
        }
    }
}
