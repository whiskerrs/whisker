//! Raw C ABI shared by Whisker's Rust runtime and native Hosts.
//!
//! This crate is the source of truth for layout-compatible data, numeric tags,
//! and callback types. Runtime ownership, callback safety, and protocol
//! conversion live in `whisker-driver`.

use std::ffi::c_char;

mod mobile;

pub use mobile::*;

#[cfg(target_os = "android")]
unsafe extern "C" {
    /// Linker anchor for the Android JNI Host glue archive.
    pub fn whisker_mobile_bridge_anchor();
}

/// Discriminants for [`WhiskerValueRaw::type`].
pub const VALUE_NULL: u8 = 0;
pub const VALUE_BOOL: u8 = 1;
pub const VALUE_INT: u8 = 2;
pub const VALUE_FLOAT: u8 = 3;
pub const VALUE_STRING: u8 = 4;
pub const VALUE_BYTES: u8 = 5;
pub const VALUE_ARRAY: u8 = 6;
pub const VALUE_MAP: u8 = 7;
pub const VALUE_ERROR: u8 = 8;

/// Borrowed UTF-8 value.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct WhiskerStringRef {
    pub ptr: *const c_char,
    pub len: usize,
}

/// Borrowed byte value.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct WhiskerBytesRef {
    pub ptr: *const u8,
    pub len: usize,
}

/// Borrowed array value.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct WhiskerValueArray {
    pub items: *mut WhiskerValueRaw,
    pub count: usize,
}

/// Borrowed map value.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct WhiskerValueMap {
    pub entries: *mut WhiskerKeyValueRaw,
    pub count: usize,
}

/// Payload union for [`WhiskerValueRaw`].
#[repr(C)]
#[derive(Copy, Clone)]
pub union WhiskerValueUnion {
    pub b: bool,
    pub i: i64,
    pub f: f64,
    pub s: WhiskerStringRef,
    pub bytes: WhiskerBytesRef,
    pub array: WhiskerValueArray,
    pub map: WhiskerValueMap,
}

/// Raw FFI form of `WhiskerValue`.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct WhiskerValueRaw {
    pub r#type: u8,
    pub _pad: [u8; 7],
    pub v: WhiskerValueUnion,
}

/// String-keyed map entry.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct WhiskerKeyValueRaw {
    pub key: WhiskerStringRef,
    pub value: WhiskerValueRaw,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_layout_matches_the_c_abi() {
        if usize::BITS == 64 {
            assert_eq!(std::mem::size_of::<WhiskerValueRaw>(), 24);
            assert_eq!(std::mem::align_of::<WhiskerValueRaw>(), 8);
            assert_eq!(std::mem::size_of::<WhiskerKeyValueRaw>(), 40);
            assert_eq!(std::mem::offset_of!(WhiskerValueRaw, r#type), 0);
            assert_eq!(std::mem::offset_of!(WhiskerValueRaw, v), 8);
            assert_eq!(std::mem::offset_of!(WhiskerKeyValueRaw, key), 0);
            assert_eq!(std::mem::offset_of!(WhiskerKeyValueRaw, value), 16);
        }
    }
}
