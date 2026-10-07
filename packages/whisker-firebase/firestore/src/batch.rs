use crate::query::validate_field_path;
use crate::reference::validate_document_path;
use crate::ser::{WriteValue, to_fields};
use crate::value::Transform;
use crate::{DocumentReference, FirebaseError, Result, native};
use serde::Serialize;
use std::collections::BTreeMap;

/// How `set` treats existing data.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SetOptions {
    pub(crate) merge: Merge,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum Merge {
    #[default]
    Overwrite,
    All,
    Fields(Vec<String>),
}

impl SetOptions {
    /// Replace the whole document (the default for `set`).
    pub fn overwrite() -> Self {
        Self::default()
    }

    /// Merge the given data into the existing document, creating it if missing.
    pub fn merge() -> Self {
        Self { merge: Merge::All }
    }

    /// Only write the listed dotted field paths from the given data.
    pub fn merge_fields<S: Into<String>>(fields: impl IntoIterator<Item = S>) -> Self {
        Self {
            merge: Merge::Fields(fields.into_iter().map(Into::into).collect()),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum WriteOp {
    Set {
        path: String,
        data: BTreeMap<String, WriteValue>,
        options: SetOptions,
    },
    Update {
        path: String,
        data: BTreeMap<String, WriteValue>,
    },
    Delete {
        path: String,
    },
}

impl WriteOp {
    pub(crate) fn set<T: Serialize + ?Sized>(
        doc: &DocumentReference,
        data: &T,
        options: SetOptions,
    ) -> Result<Self> {
        validate_document_path(doc.path())?;
        let data = to_fields(data)?;
        match &options.merge {
            Merge::Overwrite => {
                if contains_delete(data.values()) {
                    return Err(invalid(
                        "FieldValue::delete() requires `update` or `set` with merge",
                    ));
                }
            }
            Merge::All => {}
            Merge::Fields(fields) => {
                for field in fields {
                    validate_field_path(field)?;
                }
            }
        }
        Ok(WriteOp::Set {
            path: doc.path().into(),
            data,
            options,
        })
    }

    pub(crate) fn update<T: Serialize + ?Sized>(
        doc: &DocumentReference,
        fields: &T,
    ) -> Result<Self> {
        validate_document_path(doc.path())?;
        let data = to_fields(fields)?;
        if data.is_empty() {
            return Err(invalid("`update` needs at least one field"));
        }
        for (path, value) in &data {
            validate_field_path(path)?;
            if let WriteValue::Map(nested) = value
                && contains_delete(nested.values())
            {
                return Err(invalid(
                    "FieldValue::delete() must be at the top level of update data; use a dotted path",
                ));
            }
        }
        Ok(WriteOp::Update {
            path: doc.path().into(),
            data,
        })
    }

    pub(crate) fn delete(doc: &DocumentReference) -> Self {
        WriteOp::Delete {
            path: doc.path().into(),
        }
    }

    pub(crate) fn validate_path(&self) -> Result<()> {
        match self {
            WriteOp::Delete { path } => validate_document_path(path),
            _ => Ok(()),
        }
    }
}

fn contains_delete<'a>(values: impl IntoIterator<Item = &'a WriteValue>) -> bool {
    values.into_iter().any(|value| match value {
        WriteValue::Transform(Transform::Delete) => true,
        WriteValue::Map(nested) => contains_delete(nested.values()),
        _ => false,
    })
}

fn invalid(message: &str) -> FirebaseError {
    FirebaseError::invalid_argument("firestore", message)
}

/// Writes applied atomically: all succeed or none do (at most 500 operations).
///
/// ```ignore
/// let mut batch = db.batch();
/// batch.set(&db.doc("cities/tokyo"), &tokyo)
///      .update(&db.doc("stats/cities"), &data! { "count" => FieldValue::increment(1) })
///      .delete(&db.doc("cities/old"));
/// batch.commit().await?;
/// ```
#[derive(Debug, Clone, Default)]
#[must_use = "a batch does nothing until `commit` is awaited"]
pub struct WriteBatch {
    ops: Vec<WriteOp>,
    error: Option<FirebaseError>,
}

impl WriteBatch {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push(&mut self, op: Result<WriteOp>) -> &mut Self {
        match op {
            Ok(op) => self.ops.push(op),
            Err(error) => {
                self.error.get_or_insert(error);
            }
        }
        self
    }

    pub fn set<T: Serialize + ?Sized>(&mut self, doc: &DocumentReference, data: &T) -> &mut Self {
        self.set_with(doc, data, SetOptions::overwrite())
    }

    pub fn set_with<T: Serialize + ?Sized>(
        &mut self,
        doc: &DocumentReference,
        data: &T,
        options: SetOptions,
    ) -> &mut Self {
        self.push(WriteOp::set(doc, data, options))
    }

    pub fn update<T: Serialize + ?Sized>(
        &mut self,
        doc: &DocumentReference,
        fields: &T,
    ) -> &mut Self {
        self.push(WriteOp::update(doc, fields))
    }

    pub fn delete(&mut self, doc: &DocumentReference) -> &mut Self {
        self.push(Ok(WriteOp::delete(doc)))
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Apply every write. Resolves once the server acknowledges the batch.
    pub async fn commit(self) -> Result<()> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if self.ops.len() > 500 {
            return Err(invalid("a batch can contain at most 500 writes"));
        }
        for op in &self.ops {
            op.validate_path()?;
        }
        native::commit(&self.ops).await
    }
}
