//! Deserializes a borrowed [`WhiskerValue`] with the same shape rules as
//! `serde_json::from_value(value.to_json())`, without building the JSON tree.

use std::collections::btree_map;
use std::fmt;
use std::slice;

use serde::de::{
    self, DeserializeSeed, Deserializer, EnumAccess, Error as _, IntoDeserializer, MapAccess,
    SeqAccess, Unexpected, VariantAccess, Visitor,
};
use serde::forward_to_deserialize_any;

use crate::WhiskerValue;

#[derive(Debug)]
pub(crate) struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl de::Error for Error {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self(message.to_string())
    }
}

pub(crate) struct ValueDeserializer<'a>(pub(crate) &'a WhiskerValue);

impl ValueDeserializer<'_> {
    fn unexpected(&self) -> Unexpected<'_> {
        match self.0 {
            WhiskerValue::Null => Unexpected::Unit,
            WhiskerValue::Bool(value) => Unexpected::Bool(*value),
            WhiskerValue::Int(value) if *value >= 0 => Unexpected::Unsigned(*value as u64),
            WhiskerValue::Int(value) => Unexpected::Signed(*value),
            WhiskerValue::Float(value) if value.is_finite() => Unexpected::Float(*value),
            WhiskerValue::Float(_) => Unexpected::Unit,
            WhiskerValue::String(value) | WhiskerValue::Error(value) => Unexpected::Str(value),
            WhiskerValue::Bytes(_) | WhiskerValue::Array(_) => Unexpected::Seq,
            WhiskerValue::Map(_) => Unexpected::Map,
        }
    }
}

/// Fails when a sequence visitor stops early, as `serde_json` does for arrays.
fn visit_all<'de, V, A>(
    visitor: V,
    len: usize,
    mut access: A,
    remaining: impl Fn(&A) -> usize,
) -> Result<V::Value, Error>
where
    V: Visitor<'de>,
    A: SeqAccess<'de, Error = Error>,
{
    let value = visitor.visit_seq(&mut access)?;
    if remaining(&access) == 0 {
        Ok(value)
    } else {
        Err(Error::invalid_length(len, &"fewer elements in array"))
    }
}

impl<'de> Deserializer<'de> for ValueDeserializer<'_> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            WhiskerValue::Null => visitor.visit_unit(),
            WhiskerValue::Bool(value) => visitor.visit_bool(*value),
            WhiskerValue::Int(value) if *value >= 0 => visitor.visit_u64(*value as u64),
            WhiskerValue::Int(value) => visitor.visit_i64(*value),
            WhiskerValue::Float(value) if value.is_finite() => visitor.visit_f64(*value),
            WhiskerValue::Float(_) => visitor.visit_unit(),
            WhiskerValue::String(value) | WhiskerValue::Error(value) => visitor.visit_str(value),
            WhiskerValue::Bytes(bytes) => visit_all(
                visitor,
                bytes.len(),
                BytesDeserializer(bytes.iter()),
                |access| access.0.len(),
            ),
            WhiskerValue::Array(items) => visit_all(
                visitor,
                items.len(),
                SeqDeserializer(items.iter()),
                |access| access.0.len(),
            ),
            WhiskerValue::Map(entries) => {
                let len = entries.len();
                let mut access = MapDeserializer {
                    entries: entries.iter(),
                    value: None,
                };
                let value = visitor.visit_map(&mut access)?;
                if access.entries.len() == 0 {
                    Ok(value)
                } else {
                    Err(Error::invalid_length(len, &"fewer elements in map"))
                }
            }
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            WhiskerValue::Null => visitor.visit_none(),
            WhiskerValue::Float(value) if !value.is_finite() => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        let (variant, value) = match self.0 {
            WhiskerValue::Map(entries) => {
                let mut entries = entries.iter();
                let Some((variant, value)) = entries.next() else {
                    return Err(Error::invalid_value(
                        Unexpected::Map,
                        &"map with a single key",
                    ));
                };
                if entries.next().is_some() {
                    return Err(Error::invalid_value(
                        Unexpected::Map,
                        &"map with a single key",
                    ));
                }
                (variant.as_str(), Some(value))
            }
            WhiskerValue::String(variant) | WhiskerValue::Error(variant) => {
                (variant.as_str(), None)
            }
            _ => return Err(Error::invalid_type(self.unexpected(), &"string or map")),
        };
        visitor.visit_enum(EnumDeserializer { variant, value })
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_unit()
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct seq tuple tuple_struct map struct identifier
    }
}

struct SeqDeserializer<'a>(slice::Iter<'a, WhiskerValue>);

impl<'de> SeqAccess<'de> for SeqDeserializer<'_> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        self.0
            .next()
            .map(|value| seed.deserialize(ValueDeserializer(value)))
            .transpose()
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.0.len())
    }
}

struct BytesDeserializer<'a>(slice::Iter<'a, u8>);

impl<'de> SeqAccess<'de> for BytesDeserializer<'_> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        self.0
            .next()
            .map(|byte| {
                seed.deserialize(IntoDeserializer::<Error>::into_deserializer(u64::from(
                    *byte,
                )))
            })
            .transpose()
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.0.len())
    }
}

struct MapDeserializer<'a> {
    entries: btree_map::Iter<'a, String, WhiskerValue>,
    value: Option<&'a WhiskerValue>,
}

impl<'de> MapAccess<'de> for MapDeserializer<'_> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        let Some((key, value)) = self.entries.next() else {
            return Ok(None);
        };
        self.value = Some(value);
        seed.deserialize(KeyDeserializer(key)).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        let value = self
            .value
            .take()
            .ok_or_else(|| Error::custom("value is missing"))?;
        seed.deserialize(ValueDeserializer(value))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.entries.len())
    }
}

/// Map keys are strings on the wire; numeric and boolean key types parse them.
struct KeyDeserializer<'a>(&'a str);

macro_rules! parsed_key {
    ($($method:ident => $visit:ident),* $(,)?) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
            match self.0.parse() {
                Ok(value) => visitor.$visit(value),
                Err(_) => visitor.visit_str(self.0),
            }
        }
    )*};
}

impl<'de> Deserializer<'de> for KeyDeserializer<'_> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_str(self.0)
    }

    parsed_key! {
        deserialize_i8 => visit_i8, deserialize_i16 => visit_i16, deserialize_i32 => visit_i32,
        deserialize_i64 => visit_i64, deserialize_i128 => visit_i128,
        deserialize_u8 => visit_u8, deserialize_u16 => visit_u16, deserialize_u32 => visit_u32,
        deserialize_u64 => visit_u64, deserialize_u128 => visit_u128,
        deserialize_f32 => visit_f32, deserialize_f64 => visit_f64,
        deserialize_bool => visit_bool,
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_some(self)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_enum(IntoDeserializer::<Error>::into_deserializer(self.0))
    }

    forward_to_deserialize_any! {
        char str string bytes byte_buf unit unit_struct seq tuple tuple_struct map
        struct identifier ignored_any
    }
}

struct EnumDeserializer<'a> {
    variant: &'a str,
    value: Option<&'a WhiskerValue>,
}

impl<'de, 'a> EnumAccess<'de> for EnumDeserializer<'a> {
    type Error = Error;
    type Variant = VariantDeserializer<'a>;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), Error> {
        let variant =
            seed.deserialize(IntoDeserializer::<Error>::into_deserializer(self.variant))?;
        Ok((variant, VariantDeserializer(self.value)))
    }
}

struct VariantDeserializer<'a>(Option<&'a WhiskerValue>);

impl<'de> VariantAccess<'de> for VariantDeserializer<'_> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        match self.0 {
            Some(value) => serde::Deserialize::deserialize(ValueDeserializer(value)),
            None => Ok(()),
        }
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Error> {
        match self.0 {
            Some(value) => seed.deserialize(ValueDeserializer(value)),
            None => Err(Error::invalid_type(
                Unexpected::UnitVariant,
                &"newtype variant",
            )),
        }
    }

    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Some(value @ WhiskerValue::Array(items)) if !items.is_empty() => {
                ValueDeserializer(value).deserialize_any(visitor)
            }
            Some(WhiskerValue::Array(_)) => visitor.visit_unit(),
            Some(value) => Err(Error::invalid_type(
                ValueDeserializer(value).unexpected(),
                &"tuple variant",
            )),
            None => Err(Error::invalid_type(
                Unexpected::UnitVariant,
                &"tuple variant",
            )),
        }
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self.0 {
            Some(value @ WhiskerValue::Map(_)) => ValueDeserializer(value).deserialize_any(visitor),
            Some(value) => Err(Error::invalid_type(
                ValueDeserializer(value).unexpected(),
                &"struct variant",
            )),
            None => Err(Error::invalid_type(
                Unexpected::UnitVariant,
                &"struct variant",
            )),
        }
    }
}
