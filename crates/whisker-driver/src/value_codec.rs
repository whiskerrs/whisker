//! Owned conversions for the borrowed [`whisker_driver_sys`] value ABI.

use std::collections::BTreeMap;
use std::ffi::CString;

use whisker_driver_sys::{
    VALUE_ARRAY, VALUE_BOOL, VALUE_BYTES, VALUE_ERROR, VALUE_FLOAT, VALUE_INT, VALUE_MAP,
    VALUE_NULL, VALUE_STRING, WhiskerBytesRef, WhiskerKeyValueRaw, WhiskerStringRef,
    WhiskerValueArray, WhiskerValueMap, WhiskerValueRaw, WhiskerValueUnion,
};
use whisker_runtime::value::WhiskerValue;

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
            r#type: VALUE_NULL,
            _pad: [0; 7],
            v: WhiskerValueUnion { i: 0 },
        };
        match value {
            WhiskerValue::Null => {}
            WhiskerValue::Bool(value) => {
                raw.r#type = VALUE_BOOL;
                raw.v.b = *value;
            }
            WhiskerValue::Int(value) => {
                raw.r#type = VALUE_INT;
                raw.v.i = *value;
            }
            WhiskerValue::Float(value) => {
                raw.r#type = VALUE_FLOAT;
                raw.v.f = *value;
            }
            WhiskerValue::String(value) => {
                raw.r#type = VALUE_STRING;
                raw.v.s = self.string(value);
            }
            WhiskerValue::Bytes(value) => {
                let owned = value.clone();
                raw.r#type = VALUE_BYTES;
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
                raw.r#type = VALUE_ARRAY;
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
                raw.r#type = VALUE_MAP;
                raw.v.map = WhiskerValueMap {
                    entries: entries.as_mut_ptr(),
                    count: entries.len(),
                };
                self.maps.push(entries);
            }
            WhiskerValue::Error(value) => {
                raw.r#type = VALUE_ERROR;
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
        match raw.r#type {
            VALUE_NULL => WhiskerValue::Null,
            VALUE_BOOL => WhiskerValue::Bool(raw.v.b),
            VALUE_INT => WhiskerValue::Int(raw.v.i),
            VALUE_FLOAT => WhiskerValue::Float(raw.v.f),
            VALUE_STRING => WhiskerValue::String(read_string(raw.v.s)),
            VALUE_BYTES => WhiskerValue::Bytes(read_bytes(raw.v.bytes)),
            VALUE_ARRAY => {
                let values = raw.v.array;
                WhiskerValue::Array(
                    (0..values.count)
                        .map(|index| decode_value(values.items.add(index)))
                        .collect(),
                )
            }
            VALUE_MAP => {
                let values = raw.v.map;
                let mut map = BTreeMap::new();
                for index in 0..values.count {
                    let entry = &*values.entries.add(index);
                    map.insert(read_string(entry.key), decode_value(&entry.value));
                }
                WhiskerValue::Map(map)
            }
            VALUE_ERROR => WhiskerValue::Error(read_string(raw.v.s)),
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
}
