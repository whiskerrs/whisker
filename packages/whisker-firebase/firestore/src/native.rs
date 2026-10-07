//! Native module calls and the tagged wire format shared with Swift/Kotlin.
//!
//! Every value crosses as `{type, value}` so maps stay distinct from typed values.
use crate::batch::{Merge, WriteOp};
use crate::query::{Cursor, Query, Target};
use crate::ser::WriteValue;
use crate::snapshot::{
    DocumentChange, DocumentChangeType, DocumentSnapshot, QuerySnapshot, SnapshotMetadata,
    SnapshotOptions,
};
use crate::value::Transform;
use crate::{
    Bytes, DocumentReference, FirebaseError, GeoPoint, ListenerRegistration, Map, Result, Value,
};
use std::collections::BTreeMap;
use std::rc::Rc;
use whisker::platform_module::WhiskerValue as Wire;
use whisker_firebase_core::__private::{Listeners, listen, unwrap_response};
use whisker_firebase_core::Timestamp;

const SERVICE: &str = "firestore";

/// Where a one-time read gets its data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Source {
    /// The server when reachable, otherwise the local cache.
    #[default]
    Default,
    /// The server only; fails with `unavailable` when offline.
    Server,
    /// The local cache only; fails with `unavailable` if the document is not cached.
    Cache,
}

impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Source::Default => "default",
            Source::Server => "server",
            Source::Cache => "cache",
        }
    }
}

fn module() -> whisker::PlatformModule {
    whisker::module!("Firestore")
}

whisker::runtime_local! {
    static LISTENERS: Rc<Listeners> = Listeners::new(module(), SERVICE, "snapshot", "unlistenAll");
}

pub(crate) fn invoke(method: &str, args: Vec<Wire>) -> Result<Wire> {
    unwrap_response(SERVICE, module().invoke(method, args))
}

async fn invoke_async(method: &str, args: Vec<Wire>) -> Result<Wire> {
    unwrap_response(SERVICE, module().invoke_async(method, args).await)
}

pub(crate) fn use_emulator(host: &str, port: u16) -> Result<()> {
    unit(invoke(
        "useEmulator",
        vec![Wire::String(host.into()), Wire::Int(port.into())],
    )?)
}

pub(crate) fn new_document_id(collection: &str) -> Result<String> {
    match invoke("documentId", vec![Wire::String(collection.into())])? {
        Wire::String(id) => Ok(id),
        _ => Err(response("expected a document ID")),
    }
}

pub(crate) async fn get_document(path: &str, source: Source) -> Result<DocumentSnapshot> {
    let wire = invoke_async(
        "getDocument",
        vec![
            Wire::String(path.into()),
            Wire::String(source.as_str().into()),
        ],
    )
    .await?;
    decode_document(wire)
}

pub(crate) async fn get_query(query: &Query, source: Source) -> Result<QuerySnapshot> {
    let wire = invoke_async(
        "getQuery",
        vec![encode_query(query)?, Wire::String(source.as_str().into())],
    )
    .await?;
    decode_query(wire)
}

pub(crate) async fn count(query: &Query) -> Result<u64> {
    match invoke_async("count", vec![encode_query(query)?]).await? {
        Wire::Int(count) if count >= 0 => Ok(count as u64),
        _ => Err(response("expected a count")),
    }
}

pub(crate) async fn commit(ops: &[WriteOp]) -> Result<()> {
    let ops = ops.iter().map(encode_op).collect::<Result<_>>()?;
    unit(invoke_async("commit", vec![Wire::Array(ops)]).await?)
}

pub(crate) fn listen_document(
    path: &str,
    options: SnapshotOptions,
    callback: impl Fn(Result<DocumentSnapshot>) + 'static,
) -> Result<ListenerRegistration> {
    let target = map([
        ("kind", Wire::String("document".into())),
        ("path", Wire::String(path.into())),
    ]);
    start_listening(target, options, move |wire| {
        callback(wire.and_then(decode_document))
    })
}

pub(crate) fn listen_query(
    query: &Query,
    options: SnapshotOptions,
    callback: impl Fn(Result<QuerySnapshot>) + 'static,
) -> Result<ListenerRegistration> {
    let target = map([
        ("kind", Wire::String("query".into())),
        ("query", encode_query(query)?),
    ]);
    start_listening(target, options, move |wire| {
        callback(wire.and_then(decode_query))
    })
}

fn start_listening(
    target: Wire,
    options: SnapshotOptions,
    callback: impl Fn(Result<Wire>) + 'static,
) -> Result<ListenerRegistration> {
    let listeners = LISTENERS.with(Rc::clone);
    listen(&listeners, "unlisten", callback, |id| {
        unit(invoke(
            "listen",
            vec![
                Wire::Int(id),
                target,
                Wire::Bool(options.include_metadata_changes),
            ],
        )?)
    })
}

// ---------------------------------------------------------------------------
// Encoding.

fn map<const N: usize>(entries: [(&str, Wire); N]) -> Wire {
    Wire::Map(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
    )
}

fn tag(kind: &str, value: Wire) -> Wire {
    map([("type", Wire::String(kind.into())), ("value", value)])
}

fn encode_op(op: &WriteOp) -> Result<Wire> {
    Ok(match op {
        WriteOp::Set {
            path,
            data,
            options,
        } => {
            let merge_fields = match &options.merge {
                Merge::Fields(fields) => {
                    Wire::Array(fields.iter().map(|f| Wire::String(f.clone())).collect())
                }
                _ => Wire::Null,
            };
            map([
                ("op", Wire::String("set".into())),
                ("path", Wire::String(path.clone())),
                ("data", encode_fields(data, true)?),
                ("merge", Wire::Bool(options.merge == Merge::All)),
                ("merge_fields", merge_fields),
            ])
        }
        WriteOp::Update { path, data } => map([
            ("op", Wire::String("update".into())),
            ("path", Wire::String(path.clone())),
            // Top-level keys are field paths, validated separately.
            ("data", encode_fields(data, false)?),
        ]),
        WriteOp::Delete { path } => map([
            ("op", Wire::String("delete".into())),
            ("path", Wire::String(path.clone())),
        ]),
    })
}

fn encode_fields(fields: &BTreeMap<String, WriteValue>, check_keys: bool) -> Result<Wire> {
    fields
        .iter()
        .map(|(key, value)| {
            if check_keys {
                validate_field_name(key)?;
            }
            Ok((key.clone(), encode(value)?))
        })
        .collect::<Result<_>>()
        .map(Wire::Map)
}

fn validate_field_name(key: &str) -> Result<()> {
    if key.is_empty()
        || key.len() > 1500
        || (key.len() >= 4 && key.starts_with("__") && key.ends_with("__"))
    {
        return Err(FirebaseError::invalid_argument(
            SERVICE,
            format!("`{key}` is not a valid field name"),
        ));
    }
    Ok(())
}

fn encode(value: &WriteValue) -> Result<Wire> {
    Ok(match value {
        WriteValue::Null => tag("null", Wire::Null),
        WriteValue::Bool(v) => tag("bool", Wire::Bool(*v)),
        WriteValue::Integer(v) => tag("integer", Wire::Int(*v)),
        WriteValue::Double(v) => tag("double", Wire::Float(*v)),
        WriteValue::String(v) => tag("string", Wire::String(v.clone())),
        WriteValue::Bytes(v) => tag("bytes", Wire::Bytes(v.clone())),
        WriteValue::Timestamp(t) => tag(
            "timestamp",
            Wire::Array(vec![
                Wire::Int(t.seconds()),
                Wire::Int(t.nanoseconds().into()),
            ]),
        ),
        WriteValue::GeoPoint(p) => tag(
            "geo_point",
            Wire::Array(vec![Wire::Float(p.latitude), Wire::Float(p.longitude)]),
        ),
        WriteValue::Reference(path) => {
            crate::reference::validate_document_path(path)?;
            tag("reference", Wire::String(path.clone()))
        }
        WriteValue::Array(values) => tag(
            "array",
            Wire::Array(values.iter().map(encode).collect::<Result<_>>()?),
        ),
        WriteValue::Map(fields) => tag("map", encode_fields(fields, true)?),
        WriteValue::Transform(transform) => match transform {
            Transform::ServerTimestamp => tag("server_timestamp", Wire::Null),
            Transform::Delete => tag("delete", Wire::Null),
            Transform::Increment(by) => tag("increment", encode_value(by)?),
            Transform::ArrayUnion(values) => tag("array_union", encode_values(values)?),
            Transform::ArrayRemove(values) => tag("array_remove", encode_values(values)?),
        },
    })
}

fn encode_value(value: &Value) -> Result<Wire> {
    encode(&WriteValue::from(value.clone()))
}

fn encode_values(values: &[Value]) -> Result<Wire> {
    Ok(Wire::Array(
        values.iter().map(encode_value).collect::<Result<_>>()?,
    ))
}

impl From<Value> for WriteValue {
    fn from(value: Value) -> Self {
        match value {
            Value::Null => WriteValue::Null,
            Value::Bool(v) => WriteValue::Bool(v),
            Value::Integer(v) => WriteValue::Integer(v),
            Value::Double(v) => WriteValue::Double(v),
            Value::String(v) => WriteValue::String(v),
            Value::Bytes(v) => WriteValue::Bytes(v.0),
            Value::Timestamp(v) => WriteValue::Timestamp(v),
            Value::GeoPoint(v) => WriteValue::GeoPoint(v),
            Value::Reference(v) => WriteValue::Reference(v.path().into()),
            Value::Array(v) => WriteValue::Array(v.into_iter().map(Into::into).collect()),
            Value::Map(v) => WriteValue::Map(v.into_iter().map(|(k, v)| (k, v.into())).collect()),
        }
    }
}

fn encode_query(query: &Query) -> Result<Wire> {
    let (path, group) = match &query.target {
        Target::Collection(path) => (Wire::String(path.clone()), Wire::Null),
        Target::Group(id) => (Wire::Null, Wire::String(id.clone())),
    };
    let filters = query
        .filters
        .iter()
        .map(|filter| {
            Ok(map([
                ("field", Wire::String(filter.field.clone())),
                ("op", Wire::String(filter.op.as_str().into())),
                ("value", encode_value(&filter.value)?),
            ]))
        })
        .collect::<Result<_>>()?;
    let order_by = query
        .order_by
        .iter()
        .map(|(field, direction)| {
            map([
                ("field", Wire::String(field.clone())),
                (
                    "descending",
                    Wire::Bool(*direction == crate::Direction::Descending),
                ),
            ])
        })
        .collect();
    let cursor = |cursor: &Option<Cursor>| -> Result<Wire> {
        Ok(match cursor {
            Some(cursor) => map([
                ("values", encode_values(&cursor.values)?),
                ("inclusive", Wire::Bool(cursor.inclusive)),
            ]),
            None => Wire::Null,
        })
    };
    Ok(map([
        ("path", path),
        ("group", group),
        ("filters", Wire::Array(filters)),
        ("order_by", Wire::Array(order_by)),
        (
            "limit",
            query.limit.map_or(Wire::Null, |(n, _)| Wire::Int(n.into())),
        ),
        (
            "limit_to_last",
            Wire::Bool(matches!(query.limit, Some((_, true)))),
        ),
        ("start", cursor(&query.start)?),
        ("end", cursor(&query.end)?),
    ]))
}

// ---------------------------------------------------------------------------
// Decoding.

fn response(message: &str) -> FirebaseError {
    FirebaseError::invalid_response(SERVICE, message)
}

fn unit(value: Wire) -> Result<()> {
    match value {
        Wire::Null => Ok(()),
        _ => Err(response("expected an empty result")),
    }
}

fn fields_of(wire: Wire) -> Result<BTreeMap<String, Wire>> {
    match wire {
        Wire::Map(fields) => Ok(fields),
        _ => Err(response("expected a map")),
    }
}

fn bool_field(fields: &BTreeMap<String, Wire>, key: &str) -> Result<bool> {
    match fields.get(key) {
        Some(Wire::Bool(value)) => Ok(*value),
        _ => Err(response("invalid snapshot metadata")),
    }
}

fn metadata(fields: &BTreeMap<String, Wire>) -> Result<SnapshotMetadata> {
    Ok(SnapshotMetadata {
        from_cache: bool_field(fields, "from_cache")?,
        has_pending_writes: bool_field(fields, "has_pending_writes")?,
    })
}

pub(crate) fn decode_document(wire: Wire) -> Result<DocumentSnapshot> {
    let mut fields = fields_of(wire)?;
    let Some(Wire::String(path)) = fields.remove("path") else {
        return Err(response("snapshot has no path"));
    };
    let data = match fields.remove("data") {
        Some(Wire::Null) => None,
        Some(Wire::Map(data)) => Some(decode_fields(data)?),
        _ => return Err(response("invalid snapshot data")),
    };
    Ok(DocumentSnapshot {
        reference: DocumentReference::new(path),
        fields: data,
        metadata: metadata(&fields)?,
    })
}

fn decode_query(wire: Wire) -> Result<QuerySnapshot> {
    let mut fields = fields_of(wire)?;
    let Some(Wire::Array(docs)) = fields.remove("docs") else {
        return Err(response("query snapshot has no documents"));
    };
    let Some(Wire::Array(changes)) = fields.remove("changes") else {
        return Err(response("query snapshot has no changes"));
    };
    let index = |value: Option<&Wire>| match value {
        Some(Wire::Int(i)) if *i >= 0 => Some(*i as usize),
        _ => None,
    };
    let changes = changes
        .into_iter()
        .map(|change| {
            let mut change = fields_of(change)?;
            let kind = match change.get("type") {
                Some(Wire::String(kind)) if kind == "added" => DocumentChangeType::Added,
                Some(Wire::String(kind)) if kind == "modified" => DocumentChangeType::Modified,
                Some(Wire::String(kind)) if kind == "removed" => DocumentChangeType::Removed,
                _ => return Err(response("unknown document change type")),
            };
            Ok(DocumentChange {
                kind,
                old_index: index(change.get("old_index")),
                new_index: index(change.get("new_index")),
                doc: decode_document(
                    change
                        .remove("doc")
                        .ok_or_else(|| response("change has no document"))?,
                )?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(QuerySnapshot {
        docs: docs
            .into_iter()
            .map(decode_document)
            .collect::<Result<_>>()?,
        changes,
        metadata: metadata(&fields)?,
    })
}

fn decode_fields(data: BTreeMap<String, Wire>) -> Result<Map> {
    data.into_iter()
        .map(|(key, value)| Ok((key, decode(value)?)))
        .collect()
}

pub(crate) fn decode(wire: Wire) -> Result<Value> {
    let mut fields = fields_of(wire)?;
    let Some(Wire::String(kind)) = fields.remove("type") else {
        return Err(response("missing Firestore type"));
    };
    let value = fields
        .remove("value")
        .ok_or_else(|| response("missing Firestore value"))?;
    Ok(match (kind.as_str(), value) {
        ("null", Wire::Null) => Value::Null,
        ("bool", Wire::Bool(v)) => Value::Bool(v),
        ("integer", Wire::Int(v)) => Value::Integer(v),
        ("double", Wire::Float(v)) => Value::Double(v),
        ("string", Wire::String(v)) => Value::String(v),
        ("bytes", Wire::Bytes(v)) => Value::Bytes(Bytes(v)),
        ("reference", Wire::String(v)) => Value::Reference(DocumentReference::new(v)),
        ("array", Wire::Array(v)) => {
            Value::Array(v.into_iter().map(decode).collect::<Result<_>>()?)
        }
        ("map", Wire::Map(v)) => Value::Map(decode_fields(v)?),
        ("timestamp", Wire::Array(v)) => match v.as_slice() {
            [Wire::Int(seconds), Wire::Int(nanos)] => Value::Timestamp(
                u32::try_from(*nanos)
                    .ok()
                    .and_then(|nanos| Timestamp::new(*seconds, nanos).ok())
                    .ok_or_else(|| response("invalid timestamp"))?,
            ),
            _ => return Err(response("invalid timestamp")),
        },
        ("geo_point", Wire::Array(v)) => match v.as_slice() {
            [Wire::Float(latitude), Wire::Float(longitude)] => {
                Value::GeoPoint(GeoPoint::new(*latitude, *longitude))
            }
            _ => return Err(response("invalid geographic point")),
        },
        _ => return Err(response("unknown or malformed Firestore value")),
    })
}

#[cfg(test)]
pub(crate) fn encode_for_test(value: &WriteValue) -> Result<Wire> {
    encode(value)
}
