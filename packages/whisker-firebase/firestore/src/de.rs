//! Firestore values → `Deserialize`, mirroring `serde_json::from_value`.
use crate::value::{GEO_POINT, REFERENCE, TIMESTAMP};
use crate::{Map, Value};
use serde::de::{
    self, DeserializeOwned, DeserializeSeed, EnumAccess, IntoDeserializer, MapAccess, SeqAccess,
    VariantAccess, Visitor,
};
use whisker_firebase_core::FirebaseError;

/// Deserialize stored Firestore data into `T`.
pub fn from_value<T: DeserializeOwned>(value: Value) -> Result<T, FirebaseError> {
    T::deserialize(value).map_err(|e| {
        FirebaseError::new(
            "firestore",
            "invalid-data",
            format!("cannot decode document: {e}"),
        )
    })
}

#[derive(Debug)]
pub struct Error(String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
impl de::Error for Error {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        Error(msg.to_string())
    }
}

impl<'de> IntoDeserializer<'de, Error> for Value {
    type Deserializer = Value;
    fn into_deserializer(self) -> Value {
        self
    }
}

impl<'de> de::Deserializer<'de> for Value {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self {
            Value::Null => visitor.visit_unit(),
            Value::Bool(v) => visitor.visit_bool(v),
            Value::Integer(v) => visitor.visit_i64(v),
            Value::Double(v) => visitor.visit_f64(v),
            Value::String(v) => visitor.visit_string(v),
            Value::Bytes(v) => visitor.visit_byte_buf(v.0),
            // Single-key maps keep the type recoverable through self-describing buffers.
            Value::Timestamp(t) => visitor.visit_map(Tagged::new(
                TIMESTAMP,
                Value::Array(vec![
                    Value::Integer(t.seconds()),
                    Value::Integer(t.nanoseconds().into()),
                ]),
            )),
            Value::GeoPoint(p) => visitor.visit_map(Tagged::new(
                GEO_POINT,
                Value::Array(vec![Value::Double(p.latitude), Value::Double(p.longitude)]),
            )),
            Value::Reference(r) => {
                visitor.visit_map(Tagged::new(REFERENCE, Value::String(r.path().into())))
            }
            Value::Array(v) => visitor.visit_seq(Seq(v.into_iter())),
            Value::Map(v) => visitor.visit_map(Fields::new(v)),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self {
            Value::Null => visitor.visit_none(),
            other => visitor.visit_some(other),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        match (name, self) {
            (TIMESTAMP, Value::Timestamp(t)) => visitor.visit_seq(Seq(vec![
                Value::Integer(t.seconds()),
                Value::Integer(t.nanoseconds().into()),
            ]
            .into_iter())),
            (GEO_POINT, Value::GeoPoint(p)) => visitor.visit_seq(Seq(vec![
                Value::Double(p.latitude),
                Value::Double(p.longitude),
            ]
            .into_iter())),
            (REFERENCE, Value::Reference(r)) => visitor.visit_string(r.path().into()),
            (TIMESTAMP | GEO_POINT | REFERENCE, other) => Err(de::Error::custom(format!(
                "expected {}, found {}",
                describe_name(name),
                describe(&other)
            ))),
            (_, other) => visitor.visit_newtype_struct(other),
        }
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self {
            Value::String(variant) => visitor.visit_enum(variant.into_deserializer()),
            Value::Map(fields) if fields.len() == 1 => {
                let (variant, value) = fields.into_iter().next().unwrap();
                visitor.visit_enum(Enum(variant, value))
            }
            other => Err(de::Error::custom(format!(
                "expected an enum (string or single-key map), found {}",
                describe(&other)
            ))),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct seq tuple tuple_struct map struct identifier ignored_any
    }
}

fn describe_name(name: &str) -> &'static str {
    match name {
        TIMESTAMP => "a timestamp",
        GEO_POINT => "a geographic point",
        _ => "a document reference",
    }
}

fn describe(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Integer(_) => "an integer",
        Value::Double(_) => "a double",
        Value::String(_) => "a string",
        Value::Bytes(_) => "bytes",
        Value::Timestamp(_) => "a timestamp",
        Value::GeoPoint(_) => "a geographic point",
        Value::Reference(_) => "a document reference",
        Value::Array(_) => "an array",
        Value::Map(_) => "a map",
    }
}

struct Seq(std::vec::IntoIter<Value>);
impl<'de> SeqAccess<'de> for Seq {
    type Error = Error;
    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        self.0
            .next()
            .map(|value| seed.deserialize(value))
            .transpose()
    }
    fn size_hint(&self) -> Option<usize> {
        Some(self.0.len())
    }
}

struct Fields {
    entries: std::collections::btree_map::IntoIter<String, Value>,
    value: Option<Value>,
}
impl Fields {
    fn new(map: Map) -> Self {
        Self {
            entries: map.into_iter(),
            value: None,
        }
    }
}
impl<'de> MapAccess<'de> for Fields {
    type Error = Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        let Some((key, value)) = self.entries.next() else {
            return Ok(None);
        };
        self.value = Some(value);
        seed.deserialize(Value::String(key)).map(Some)
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        seed.deserialize(self.value.take().unwrap_or_default())
    }
    fn size_hint(&self) -> Option<usize> {
        Some(self.entries.len())
    }
}

struct Tagged(Option<(&'static str, Value)>, Option<Value>);
impl Tagged {
    fn new(key: &'static str, value: Value) -> Self {
        Self(Some((key, value)), None)
    }
}
impl<'de> MapAccess<'de> for Tagged {
    type Error = Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        let Some((key, value)) = self.0.take() else {
            return Ok(None);
        };
        self.1 = Some(value);
        seed.deserialize(Value::String(key.into())).map(Some)
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        seed.deserialize(self.1.take().unwrap_or_default())
    }
}

struct Enum(String, Value);
impl<'de> EnumAccess<'de> for Enum {
    type Error = Error;
    type Variant = Value;
    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Value), Error> {
        Ok((seed.deserialize(Value::String(self.0))?, self.1))
    }
}
impl<'de> VariantAccess<'de> for Value {
    type Error = Error;
    fn unit_variant(self) -> Result<(), Error> {
        match self {
            Value::Null => Ok(()),
            other => Err(de::Error::custom(format!(
                "expected a unit variant, found {}",
                describe(&other)
            ))),
        }
    }
    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Error> {
        seed.deserialize(self)
    }
    fn tuple_variant<V: Visitor<'de>>(self, _: usize, visitor: V) -> Result<V::Value, Error> {
        de::Deserializer::deserialize_any(self, visitor)
    }
    fn struct_variant<V: Visitor<'de>>(
        self,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        de::Deserializer::deserialize_any(self, visitor)
    }
}
