use crate::DocumentReference;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeTupleStruct, Serializer};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use whisker_firebase_core::Timestamp;

pub(crate) use whisker_firebase_core::__private::TIMESTAMP_NAME as TIMESTAMP;
pub(crate) const GEO_POINT: &str = "$__whisker_firestore_geo_point";
pub(crate) const REFERENCE: &str = "$__whisker_firestore_reference";
pub(crate) const BYTES: &str = "$__whisker_firestore_bytes";
pub(crate) const FIELD_VALUE: &str = "$__whisker_firestore_field_value";

/// Document fields keyed by literal field name.
pub type Map = BTreeMap<String, Value>;

/// A value stored in a Firestore document.
///
/// Most code reads and writes its own `Serialize`/`Deserialize` types instead; use
/// `Value` for dynamic data. Integers are signed 64-bit, as in Firestore.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Integer(i64),
    Double(f64),
    String(String),
    Bytes(Bytes),
    Timestamp(Timestamp),
    GeoPoint(GeoPoint),
    /// A reference to a document in the same database.
    Reference(DocumentReference),
    /// Arrays cannot directly contain arrays.
    Array(Vec<Value>),
    Map(Map),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Integer(value) => Some(*value),
            _ => None,
        }
    }

    /// Integers are widened, since Firestore compares numbers across both types.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Double(value) => Some(*value),
            Value::Integer(value) => Some(*value as f64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&Map> {
        match self {
            Value::Map(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(value) => Some(value),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Look up a nested field by dotted path, e.g. `"address.city"`.
    pub fn get(&self, path: &str) -> Option<&Value> {
        path.split('.')
            .try_fold(self, |value, key| value.as_map()?.get(key))
    }
}

/// A latitude/longitude pair.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoPoint {
    pub latitude: f64,
    pub longitude: f64,
}

impl GeoPoint {
    /// Coordinates are validated when written (latitude ±90, longitude ±180).
    pub fn new(latitude: f64, longitude: f64) -> Self {
        Self {
            latitude,
            longitude,
        }
    }

    pub(crate) fn is_valid(&self) -> bool {
        (-90.0..=90.0).contains(&self.latitude) && (-180.0..=180.0).contains(&self.longitude)
    }
}

/// Binary data (Firestore `Bytes`/`Blob`). A plain `Vec<u8>` serializes as an array
/// of integers; wrap it in `Bytes` (or use `serde_bytes`) to store a blob.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Bytes(pub Vec<u8>);

impl From<Vec<u8>> for Bytes {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl From<&[u8]> for Bytes {
    fn from(value: &[u8]) -> Self {
        Self(value.to_vec())
    }
}

impl std::ops::Deref for Bytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.0
    }
}

/// Write-time sentinels: server timestamps, field deletion, increments, and array
/// unions/removals. Use them anywhere a written value is accepted, including inside
/// your own `Serialize` types; they are rejected in queries and inside arrays.
///
/// ```ignore
/// doc.update(&data! {
///     "visits" => FieldValue::increment(1),
///     "updatedAt" => FieldValue::server_timestamp(),
///     "draft" => FieldValue::delete(),
/// }).await?;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct FieldValue(pub(crate) Transform);

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Transform {
    ServerTimestamp,
    Delete,
    Increment(Value),
    ArrayUnion(Vec<Value>),
    ArrayRemove(Vec<Value>),
}

impl FieldValue {
    /// Replaced by the server's commit time.
    pub fn server_timestamp() -> Self {
        Self(Transform::ServerTimestamp)
    }

    /// Removes the field. Valid in `update` and in `set` with merge.
    pub fn delete() -> Self {
        Self(Transform::Delete)
    }

    /// Adds to the stored number (treating a missing or non-numeric field as 0).
    pub fn increment(by: impl Into<Increment>) -> Self {
        Self(Transform::Increment(match by.into() {
            Increment::Integer(value) => Value::Integer(value),
            Increment::Double(value) => Value::Double(value),
        }))
    }

    /// Adds each element not already present.
    pub fn array_union<V: Into<Value>>(elements: impl IntoIterator<Item = V>) -> Self {
        Self(Transform::ArrayUnion(
            elements.into_iter().map(Into::into).collect(),
        ))
    }

    /// Removes every instance of each element.
    pub fn array_remove<V: Into<Value>>(elements: impl IntoIterator<Item = V>) -> Self {
        Self(Transform::ArrayRemove(
            elements.into_iter().map(Into::into).collect(),
        ))
    }
}

/// The operand of [`FieldValue::increment`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Increment {
    Integer(i64),
    Double(f64),
}

macro_rules! increment_from {
    ($variant:ident: $($ty:ty),*) => {$(
        impl From<$ty> for Increment {
            fn from(value: $ty) -> Self {
                Increment::$variant(value.into())
            }
        }
    )*};
}
increment_from!(Integer: i8, i16, i32, i64, u8, u16, u32);
increment_from!(Double: f32, f64);

// ---------------------------------------------------------------------------
// Conversions into `Value`.

macro_rules! value_from {
    ($variant:ident: $($ty:ty),*) => {$(
        impl From<$ty> for Value {
            fn from(value: $ty) -> Self {
                Value::$variant(value.into())
            }
        }
    )*};
}
value_from!(Bool: bool);
value_from!(Integer: i8, i16, i32, i64, u8, u16, u32);
value_from!(Double: f32, f64);
value_from!(String: String, &str, &String, Box<str>);
value_from!(Bytes: Bytes);
value_from!(Timestamp: Timestamp);
value_from!(GeoPoint: GeoPoint);
value_from!(Reference: DocumentReference);
value_from!(Map: Map);

impl From<&DocumentReference> for Value {
    fn from(value: &DocumentReference) -> Self {
        Value::Reference(value.clone())
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(value: Option<T>) -> Self {
        value.map_or(Value::Null, Into::into)
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(value: Vec<T>) -> Self {
        Value::Array(value.into_iter().map(Into::into).collect())
    }
}

impl<K: Into<String>, V: Into<Value>> FromIterator<(K, V)> for Value {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        Value::Map(
            iter.into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        )
    }
}

impl<K: Into<String>, V: Into<Value>> From<HashMap<K, V>> for Value {
    fn from(value: HashMap<K, V>) -> Self {
        value.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// Serde for the special types. Each uses a reserved newtype name that the
// Firestore serializer/deserializer intercepts; other formats see plain data.

fn serialize_pair<S: Serializer, A: Serialize, B: Serialize>(
    serializer: S,
    name: &'static str,
    a: A,
    b: B,
) -> Result<S::Ok, S::Error> {
    struct Pair<A, B>(A, B);
    impl<A: Serialize, B: Serialize> Serialize for Pair<A, B> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut tuple = serializer.serialize_tuple_struct("Pair", 2)?;
            tuple.serialize_field(&self.0)?;
            tuple.serialize_field(&self.1)?;
            tuple.end()
        }
    }
    serializer.serialize_newtype_struct(name, &Pair(a, b))
}

impl Serialize for GeoPoint {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_pair(serializer, GEO_POINT, self.latitude, self.longitude)
    }
}

impl<'de> Deserialize<'de> for GeoPoint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct GeoPointVisitor;
        impl<'de> Visitor<'de> for GeoPointVisitor {
            type Value = GeoPoint;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a geographic point")
            }
            fn visit_newtype_struct<D: Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<GeoPoint, D::Error> {
                deserializer.deserialize_any(self)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<GeoPoint, A::Error> {
                let latitude = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let longitude = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                Ok(GeoPoint::new(latitude, longitude))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<GeoPoint, A::Error> {
                let (mut latitude, mut longitude) = (None, None);
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        GEO_POINT => {
                            let (lat, lng) = map.next_value()?;
                            return Ok(GeoPoint::new(lat, lng));
                        }
                        "latitude" => latitude = Some(map.next_value()?),
                        "longitude" => longitude = Some(map.next_value()?),
                        _ => {
                            map.next_value::<de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(GeoPoint::new(
                    latitude.ok_or_else(|| de::Error::missing_field("latitude"))?,
                    longitude.ok_or_else(|| de::Error::missing_field("longitude"))?,
                ))
            }
        }
        deserializer.deserialize_newtype_struct(GEO_POINT, GeoPointVisitor)
    }
}

impl Serialize for Bytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        struct Raw<'a>(&'a [u8]);
        impl Serialize for Raw<'_> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_bytes(self.0)
            }
        }
        serializer.serialize_newtype_struct(BYTES, &Raw(&self.0))
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BytesVisitor;
        impl<'de> Visitor<'de> for BytesVisitor {
            type Value = Bytes;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("bytes")
            }
            fn visit_newtype_struct<D: Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<Bytes, D::Error> {
                deserializer.deserialize_byte_buf(self)
            }
            fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Bytes, E> {
                Ok(Bytes(v.to_vec()))
            }
            fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Bytes, E> {
                Ok(Bytes(v))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Bytes, A::Error> {
                let mut bytes = Vec::new();
                while let Some(byte) = seq.next_element()? {
                    bytes.push(byte);
                }
                Ok(Bytes(bytes))
            }
        }
        deserializer.deserialize_newtype_struct(BYTES, BytesVisitor)
    }
}

impl Serialize for DocumentReference {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_newtype_struct(REFERENCE, self.path())
    }
}

impl<'de> Deserialize<'de> for DocumentReference {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ReferenceVisitor;
        impl<'de> Visitor<'de> for ReferenceVisitor {
            type Value = DocumentReference;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a document path")
            }
            fn visit_newtype_struct<D: Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<DocumentReference, D::Error> {
                deserializer.deserialize_any(self)
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<DocumentReference, E> {
                Ok(DocumentReference::new(v.into()))
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<DocumentReference, A::Error> {
                match map.next_key::<String>()?.as_deref() {
                    Some(REFERENCE) => Ok(DocumentReference::new(map.next_value()?)),
                    _ => Err(de::Error::custom("expected a document reference")),
                }
            }
        }
        deserializer.deserialize_newtype_struct(REFERENCE, ReferenceVisitor)
    }
}

impl Serialize for FieldValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (kind, operand) = match &self.0 {
            Transform::ServerTimestamp => ("server_timestamp", Value::Null),
            Transform::Delete => ("delete", Value::Null),
            Transform::Increment(by) => ("increment", by.clone()),
            Transform::ArrayUnion(values) => ("array_union", Value::Array(values.clone())),
            Transform::ArrayRemove(values) => ("array_remove", Value::Array(values.clone())),
        };
        serialize_pair(serializer, FIELD_VALUE, kind, operand)
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Null => serializer.serialize_unit(),
            Value::Bool(v) => serializer.serialize_bool(*v),
            Value::Integer(v) => serializer.serialize_i64(*v),
            Value::Double(v) => serializer.serialize_f64(*v),
            Value::String(v) => serializer.serialize_str(v),
            Value::Bytes(v) => v.serialize(serializer),
            Value::Timestamp(v) => v.serialize(serializer),
            Value::GeoPoint(v) => v.serialize(serializer),
            Value::Reference(v) => v.serialize(serializer),
            Value::Array(v) => v.serialize(serializer),
            Value::Map(v) => v.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;
impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a Firestore value")
    }
    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        d.deserialize_any(self)
    }
    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Integer(v))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        i64::try_from(v)
            .map(Value::Integer)
            .map_err(|_| E::custom("integer exceeds the signed 64-bit range"))
    }
    fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
        Ok(Value::Double(v))
    }
    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.into()))
    }
    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }
    fn visit_bytes<E>(self, v: &[u8]) -> Result<Value, E> {
        Ok(Value::Bytes(Bytes(v.to_vec())))
    }
    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Value, E> {
        Ok(Value::Bytes(Bytes(v)))
    }
    fn visit_newtype_struct<D: Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        d.deserialize_any(self)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = seq.next_element()? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let Some(first) = map.next_key::<String>()? else {
            return Ok(Value::Map(Map::new()));
        };
        match first.as_str() {
            TIMESTAMP => {
                let (s, n): (i64, u32) = map.next_value()?;
                return Timestamp::new(s, n)
                    .map(Value::Timestamp)
                    .map_err(de::Error::custom);
            }
            GEO_POINT => {
                let (lat, lng) = map.next_value()?;
                return Ok(Value::GeoPoint(GeoPoint::new(lat, lng)));
            }
            REFERENCE => {
                return Ok(Value::Reference(DocumentReference::new(map.next_value()?)));
            }
            _ => {}
        }
        let mut fields = Map::new();
        fields.insert(first, map.next_value()?);
        while let Some((key, value)) = map.next_entry()? {
            fields.insert(key, value);
        }
        Ok(Value::Map(fields))
    }
}
