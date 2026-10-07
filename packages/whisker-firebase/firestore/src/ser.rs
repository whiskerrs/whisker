//! `Serialize` → Firestore write values, mirroring `serde_json::to_value`.
use crate::value::{BYTES, FIELD_VALUE, GEO_POINT, REFERENCE, TIMESTAMP, Transform};
use crate::{FieldValue, GeoPoint, Map, Value};
use serde::Serialize;
use serde::ser::{self, Impossible};
use std::collections::BTreeMap;
use whisker_firebase_core::{FirebaseError, Timestamp};

/// A value on its way to Firestore: stored data plus write-only transforms.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum WriteValue {
    Null,
    Bool(bool),
    Integer(i64),
    Double(f64),
    String(String),
    Bytes(Vec<u8>),
    Timestamp(Timestamp),
    GeoPoint(GeoPoint),
    Reference(String),
    Array(Vec<WriteValue>),
    Map(BTreeMap<String, WriteValue>),
    Transform(Transform),
}

impl WriteValue {
    /// Stored data only; `None` if a transform is present.
    pub(crate) fn into_value(self) -> Option<Value> {
        Some(match self {
            WriteValue::Null => Value::Null,
            WriteValue::Bool(v) => Value::Bool(v),
            WriteValue::Integer(v) => Value::Integer(v),
            WriteValue::Double(v) => Value::Double(v),
            WriteValue::String(v) => Value::String(v),
            WriteValue::Bytes(v) => Value::Bytes(v.into()),
            WriteValue::Timestamp(v) => Value::Timestamp(v),
            WriteValue::GeoPoint(v) => Value::GeoPoint(v),
            WriteValue::Reference(v) => Value::Reference(crate::DocumentReference::new(v)),
            WriteValue::Array(v) => Value::Array(
                v.into_iter()
                    .map(WriteValue::into_value)
                    .collect::<Option<_>>()?,
            ),
            WriteValue::Map(v) => Value::Map(
                v.into_iter()
                    .map(|(k, v)| Some((k, v.into_value()?)))
                    .collect::<Option<Map>>()?,
            ),
            WriteValue::Transform(_) => return None,
        })
    }
}

/// Serialize a value into stored Firestore data (no [`FieldValue`] sentinels).
pub fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<Value, FirebaseError> {
    to_write_value(value)?
        .into_value()
        .ok_or_else(|| invalid("FieldValue sentinels are only valid in writes"))
}

pub(crate) fn to_write_value<T: Serialize + ?Sized>(
    value: &T,
) -> Result<WriteValue, FirebaseError> {
    value.serialize(ValueSerializer).map_err(|e| invalid(&e.0))
}

/// Serialize document data: the top level must be a map (a struct, map, or `data!`).
pub(crate) fn to_fields<T: Serialize + ?Sized>(
    value: &T,
) -> Result<BTreeMap<String, WriteValue>, FirebaseError> {
    match to_write_value(value)? {
        WriteValue::Map(fields) => Ok(fields),
        _ => Err(invalid(
            "document data must serialize to a map (e.g. a struct)",
        )),
    }
}

fn invalid(message: &str) -> FirebaseError {
    FirebaseError::invalid_argument("firestore", message)
}

#[derive(Debug)]
pub(crate) struct Error(pub(crate) String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
impl ser::Error for Error {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        Error(msg.to_string())
    }
}
fn err<T>(message: impl Into<String>) -> Result<T, Error> {
    Err(Error(message.into()))
}

struct ValueSerializer;

impl ser::Serializer for ValueSerializer {
    type Ok = WriteValue;
    type Error = Error;
    type SerializeSeq = SeqSerializer;
    type SerializeTuple = SeqSerializer;
    type SerializeTupleStruct = SeqSerializer;
    type SerializeTupleVariant = VariantSerializer<SeqSerializer>;
    type SerializeMap = MapSerializer;
    type SerializeStruct = MapSerializer;
    type SerializeStructVariant = VariantSerializer<MapSerializer>;

    fn serialize_bool(self, v: bool) -> Result<WriteValue, Error> {
        Ok(WriteValue::Bool(v))
    }
    fn serialize_i8(self, v: i8) -> Result<WriteValue, Error> {
        Ok(WriteValue::Integer(v.into()))
    }
    fn serialize_i16(self, v: i16) -> Result<WriteValue, Error> {
        Ok(WriteValue::Integer(v.into()))
    }
    fn serialize_i32(self, v: i32) -> Result<WriteValue, Error> {
        Ok(WriteValue::Integer(v.into()))
    }
    fn serialize_i64(self, v: i64) -> Result<WriteValue, Error> {
        Ok(WriteValue::Integer(v))
    }
    fn serialize_i128(self, v: i128) -> Result<WriteValue, Error> {
        i64::try_from(v)
            .map(WriteValue::Integer)
            .or_else(|_| err("integer exceeds Firestore's signed 64-bit range"))
    }
    fn serialize_u8(self, v: u8) -> Result<WriteValue, Error> {
        Ok(WriteValue::Integer(v.into()))
    }
    fn serialize_u16(self, v: u16) -> Result<WriteValue, Error> {
        Ok(WriteValue::Integer(v.into()))
    }
    fn serialize_u32(self, v: u32) -> Result<WriteValue, Error> {
        Ok(WriteValue::Integer(v.into()))
    }
    fn serialize_u64(self, v: u64) -> Result<WriteValue, Error> {
        i64::try_from(v)
            .map(WriteValue::Integer)
            .or_else(|_| err("integer exceeds Firestore's signed 64-bit range"))
    }
    fn serialize_u128(self, v: u128) -> Result<WriteValue, Error> {
        i64::try_from(v)
            .map(WriteValue::Integer)
            .or_else(|_| err("integer exceeds Firestore's signed 64-bit range"))
    }
    fn serialize_f32(self, v: f32) -> Result<WriteValue, Error> {
        Ok(WriteValue::Double(v.into()))
    }
    fn serialize_f64(self, v: f64) -> Result<WriteValue, Error> {
        Ok(WriteValue::Double(v))
    }
    fn serialize_char(self, v: char) -> Result<WriteValue, Error> {
        Ok(WriteValue::String(v.into()))
    }
    fn serialize_str(self, v: &str) -> Result<WriteValue, Error> {
        Ok(WriteValue::String(v.into()))
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<WriteValue, Error> {
        Ok(WriteValue::Bytes(v.into()))
    }
    fn serialize_none(self) -> Result<WriteValue, Error> {
        Ok(WriteValue::Null)
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<WriteValue, Error> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<WriteValue, Error> {
        Ok(WriteValue::Null)
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<WriteValue, Error> {
        Ok(WriteValue::Null)
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<WriteValue, Error> {
        Ok(WriteValue::String(variant.into()))
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<WriteValue, Error> {
        let inner = value.serialize(ValueSerializer)?;
        let pair = |inner: WriteValue| match inner {
            WriteValue::Array(mut items) if items.len() == 2 => {
                let second = items.pop().unwrap();
                Ok((items.pop().unwrap(), second))
            }
            _ => err(format!("malformed {name}")),
        };
        Ok(match name {
            TIMESTAMP => match pair(inner)? {
                (WriteValue::Integer(s), WriteValue::Integer(n)) => WriteValue::Timestamp(
                    Timestamp::new(
                        s,
                        u32::try_from(n).map_err(|_| Error("invalid timestamp".into()))?,
                    )
                    .map_err(|e| Error(e.message))?,
                ),
                _ => return err("malformed timestamp"),
            },
            GEO_POINT => match pair(inner)? {
                (WriteValue::Double(latitude), WriteValue::Double(longitude)) => {
                    let point = GeoPoint::new(latitude, longitude);
                    if !point.is_valid() {
                        return err("latitude must be within ±90 and longitude within ±180");
                    }
                    WriteValue::GeoPoint(point)
                }
                _ => return err("malformed geographic point"),
            },
            REFERENCE => match inner {
                WriteValue::String(path) => {
                    crate::reference::validate_document_path(&path)
                        .map_err(|e| Error(e.message))?;
                    WriteValue::Reference(path)
                }
                _ => return err("malformed document reference"),
            },
            BYTES => inner,
            FIELD_VALUE => match pair(inner)? {
                (WriteValue::String(kind), operand) => {
                    let operand = operand
                        .into_value()
                        .ok_or_else(|| Error("FieldValue sentinels cannot be nested".into()))?;
                    let arrays = |operand: Value| match operand {
                        Value::Array(values) => Ok(values),
                        _ => err("malformed array transform"),
                    };
                    WriteValue::Transform(match kind.as_str() {
                        "server_timestamp" => Transform::ServerTimestamp,
                        "delete" => Transform::Delete,
                        "increment" => Transform::Increment(operand),
                        "array_union" => Transform::ArrayUnion(arrays(operand)?),
                        "array_remove" => Transform::ArrayRemove(arrays(operand)?),
                        _ => return err("unknown FieldValue"),
                    })
                }
                _ => return err("malformed FieldValue"),
            },
            _ => inner,
        })
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<WriteValue, Error> {
        Ok(WriteValue::Map(BTreeMap::from([(
            variant.into(),
            value.serialize(ValueSerializer)?,
        )])))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<SeqSerializer, Error> {
        Ok(SeqSerializer(Vec::with_capacity(len.unwrap_or(0))))
    }
    fn serialize_tuple(self, len: usize) -> Result<SeqSerializer, Error> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(self, _: &'static str, len: usize) -> Result<SeqSerializer, Error> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<VariantSerializer<SeqSerializer>, Error> {
        Ok(VariantSerializer(variant, self.serialize_seq(Some(len))?))
    }
    fn serialize_map(self, _: Option<usize>) -> Result<MapSerializer, Error> {
        Ok(MapSerializer::default())
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<MapSerializer, Error> {
        Ok(MapSerializer::default())
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<VariantSerializer<MapSerializer>, Error> {
        Ok(VariantSerializer(variant, MapSerializer::default()))
    }
}

pub(crate) struct SeqSerializer(Vec<WriteValue>);

impl SeqSerializer {
    fn finish(self) -> Result<WriteValue, Error> {
        for value in &self.0 {
            match value {
                WriteValue::Array(_) => {
                    return err("Firestore arrays cannot directly contain arrays");
                }
                WriteValue::Transform(_) => {
                    return err("FieldValue sentinels are not allowed inside arrays");
                }
                _ => {}
            }
        }
        Ok(WriteValue::Array(self.0))
    }
}

impl ser::SerializeSeq for SeqSerializer {
    type Ok = WriteValue;
    type Error = Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.0.push(value.serialize(ValueSerializer)?);
        Ok(())
    }
    fn end(self) -> Result<WriteValue, Error> {
        self.finish()
    }
}
impl ser::SerializeTuple for SeqSerializer {
    type Ok = WriteValue;
    type Error = Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<WriteValue, Error> {
        // Tuples also carry the special types' parts; nesting is validated by the caller.
        Ok(WriteValue::Array(self.0))
    }
}
impl ser::SerializeTupleStruct for SeqSerializer {
    type Ok = WriteValue;
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<WriteValue, Error> {
        Ok(WriteValue::Array(self.0))
    }
}

#[derive(Default)]
pub(crate) struct MapSerializer {
    fields: BTreeMap<String, WriteValue>,
    key: Option<String>,
}

impl ser::SerializeMap for MapSerializer {
    type Ok = WriteValue;
    type Error = Error;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Error> {
        self.key = Some(key.serialize(KeySerializer)?);
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let key = self
            .key
            .take()
            .ok_or_else(|| Error("missing map key".into()))?;
        self.fields.insert(key, value.serialize(ValueSerializer)?);
        Ok(())
    }
    fn end(self) -> Result<WriteValue, Error> {
        Ok(WriteValue::Map(self.fields))
    }
}
impl ser::SerializeStruct for MapSerializer {
    type Ok = WriteValue;
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.fields
            .insert(key.into(), value.serialize(ValueSerializer)?);
        Ok(())
    }
    fn end(self) -> Result<WriteValue, Error> {
        Ok(WriteValue::Map(self.fields))
    }
}

pub(crate) struct VariantSerializer<S>(&'static str, S);

impl ser::SerializeTupleVariant for VariantSerializer<SeqSerializer> {
    type Ok = WriteValue;
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(&mut self.1, value)
    }
    fn end(self) -> Result<WriteValue, Error> {
        Ok(WriteValue::Map(BTreeMap::from([(
            self.0.into(),
            self.1.finish()?,
        )])))
    }
}
impl ser::SerializeStructVariant for VariantSerializer<MapSerializer> {
    type Ok = WriteValue;
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        ser::SerializeStruct::serialize_field(&mut self.1, key, value)
    }
    fn end(self) -> Result<WriteValue, Error> {
        Ok(WriteValue::Map(BTreeMap::from([(
            self.0.into(),
            WriteValue::Map(self.1.fields),
        )])))
    }
}

/// Map keys: strings, chars, integers, and unit variants, as in `serde_json`.
struct KeySerializer;

macro_rules! integer_keys {
    ($($method:ident: $ty:ty),*) => {$(
        fn $method(self, v: $ty) -> Result<String, Error> {
            Ok(v.to_string())
        }
    )*};
}

impl ser::Serializer for KeySerializer {
    type Ok = String;
    type Error = Error;
    type SerializeSeq = Impossible<String, Error>;
    type SerializeTuple = Impossible<String, Error>;
    type SerializeTupleStruct = Impossible<String, Error>;
    type SerializeTupleVariant = Impossible<String, Error>;
    type SerializeMap = Impossible<String, Error>;
    type SerializeStruct = Impossible<String, Error>;
    type SerializeStructVariant = Impossible<String, Error>;

    integer_keys!(serialize_i8: i8, serialize_i16: i16, serialize_i32: i32, serialize_i64: i64,
        serialize_u8: u8, serialize_u16: u16, serialize_u32: u32, serialize_u64: u64);

    fn serialize_str(self, v: &str) -> Result<String, Error> {
        Ok(v.into())
    }
    fn serialize_char(self, v: char) -> Result<String, Error> {
        Ok(v.into())
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<String, Error> {
        Ok(variant.into())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<String, Error> {
        value.serialize(self)
    }
    fn serialize_bool(self, _: bool) -> Result<String, Error> {
        key_error()
    }
    fn serialize_f32(self, _: f32) -> Result<String, Error> {
        key_error()
    }
    fn serialize_f64(self, _: f64) -> Result<String, Error> {
        key_error()
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<String, Error> {
        key_error()
    }
    fn serialize_none(self) -> Result<String, Error> {
        key_error()
    }
    fn serialize_some<T: Serialize + ?Sized>(self, _: &T) -> Result<String, Error> {
        key_error()
    }
    fn serialize_unit(self) -> Result<String, Error> {
        key_error()
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<String, Error> {
        key_error()
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<String, Error> {
        key_error()
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        key_error()
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Error> {
        key_error()
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        key_error()
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        key_error()
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Error> {
        key_error()
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, Error> {
        key_error()
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        key_error()
    }
}

fn key_error<T>() -> Result<T, Error> {
    err("map keys must be strings or integers")
}

/// Document fields built with [`data!`](crate::data). Conversion errors are reported
/// when the data is written.
#[derive(Debug, Clone, Default)]
pub struct Fields {
    fields: BTreeMap<String, WriteValue>,
    error: Option<String>,
}

impl Fields {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a field. The key is a literal name in `set`/`add`, a dotted path in `update`.
    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: &(impl Serialize + ?Sized),
    ) -> &mut Self {
        match value.serialize(ValueSerializer) {
            Ok(value) => {
                self.fields.insert(key.into(), value);
            }
            Err(error) => {
                self.error.get_or_insert(error.0);
            }
        }
        self
    }
}

impl Serialize for Fields {
    fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use ser::SerializeMap;
        if let Some(error) = &self.error {
            return Err(ser::Error::custom(error));
        }
        let mut map = serializer.serialize_map(Some(self.fields.len()))?;
        for (key, value) in &self.fields {
            map.serialize_entry(key, &WriteValueRef(value))?;
        }
        map.end()
    }
}

/// Re-serializes an already converted value (used by [`Fields`]).
struct WriteValueRef<'a>(&'a WriteValue);
impl Serialize for WriteValueRef<'_> {
    fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            WriteValue::Null => serializer.serialize_unit(),
            WriteValue::Bool(v) => serializer.serialize_bool(*v),
            WriteValue::Integer(v) => serializer.serialize_i64(*v),
            WriteValue::Double(v) => serializer.serialize_f64(*v),
            WriteValue::String(v) => serializer.serialize_str(v),
            WriteValue::Bytes(v) => crate::Bytes(v.clone()).serialize(serializer),
            WriteValue::Timestamp(v) => v.serialize(serializer),
            WriteValue::GeoPoint(v) => v.serialize(serializer),
            WriteValue::Reference(v) => {
                crate::DocumentReference::new(v.clone()).serialize(serializer)
            }
            WriteValue::Array(v) => serializer.collect_seq(v.iter().map(WriteValueRef)),
            WriteValue::Map(v) => {
                serializer.collect_map(v.iter().map(|(k, v)| (k, WriteValueRef(v))))
            }
            WriteValue::Transform(v) => FieldValue(v.clone()).serialize(serializer),
        }
    }
}

/// Build document data from heterogeneous values, including [`FieldValue`] sentinels.
///
/// ```ignore
/// doc.update(&data! {
///     "name" => "Alice",
///     "stats.visits" => FieldValue::increment(1),
///     "updatedAt" => FieldValue::server_timestamp(),
/// }).await?;
/// ```
#[macro_export]
macro_rules! data {
    () => { $crate::Fields::new() };
    ($($key:expr => $value:expr),+ $(,)?) => {{
        let mut fields = $crate::Fields::new();
        $( fields.insert($key, &$value); )+
        fields
    }};
}
