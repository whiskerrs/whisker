use crate::de::from_value;
use crate::{DocumentReference, Map, Result, Value};
use serde::de::DeserializeOwned;

/// Options for `on_snapshot_with`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SnapshotOptions {
    /// Also deliver snapshots when only `metadata` changes (e.g. a pending write is acknowledged).
    pub include_metadata_changes: bool,
}

impl SnapshotOptions {
    pub fn include_metadata_changes(mut self, include: bool) -> Self {
        self.include_metadata_changes = include;
        self
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct SnapshotMetadata {
    /// The data came from the local cache and may be stale.
    pub from_cache: bool,
    /// The data includes local writes the server has not yet acknowledged.
    pub has_pending_writes: bool,
}

/// A document read at one point in time. [`exists`](Self::exists) is false for a
/// missing document.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentSnapshot {
    pub(crate) reference: DocumentReference,
    pub(crate) fields: Option<Map>,
    pub(crate) metadata: SnapshotMetadata,
}

impl DocumentSnapshot {
    pub fn id(&self) -> &str {
        self.reference.id()
    }

    pub fn reference(&self) -> &DocumentReference {
        &self.reference
    }

    pub fn exists(&self) -> bool {
        self.fields.is_some()
    }

    /// Decode the document into `T`; `Ok(None)` if the document does not exist.
    pub fn data<T: DeserializeOwned>(&self) -> Result<Option<T>> {
        self.fields
            .clone()
            .map(|fields| from_value(Value::Map(fields)))
            .transpose()
    }

    /// The raw fields, or `None` if the document does not exist.
    pub fn fields(&self) -> Option<&Map> {
        self.fields.as_ref()
    }

    /// Decode one field by dotted path; `Ok(None)` if the field or document is missing.
    pub fn get<T: DeserializeOwned>(&self, field: &str) -> Result<Option<T>> {
        let Some(fields) = &self.fields else {
            return Ok(None);
        };
        let mut parts = field.split('.');
        let first = parts.next().and_then(|key| fields.get(key));
        parts
            .try_fold(first, |value, key| Some(value?.as_map()?.get(key)))
            .flatten()
            .cloned()
            .map(from_value)
            .transpose()
    }

    pub fn metadata(&self) -> SnapshotMetadata {
        self.metadata
    }
}

/// How a document changed between two query snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DocumentChangeType {
    Added,
    Modified,
    Removed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentChange {
    pub kind: DocumentChangeType,
    pub doc: DocumentSnapshot,
    /// Position in the previous snapshot; `None` for added documents.
    pub old_index: Option<usize>,
    /// Position in this snapshot; `None` for removed documents.
    pub new_index: Option<usize>,
}

/// The results of a query. Iterate it for the documents.
#[derive(Debug, Clone, PartialEq)]
pub struct QuerySnapshot {
    pub(crate) docs: Vec<DocumentSnapshot>,
    pub(crate) changes: Vec<DocumentChange>,
    pub(crate) metadata: SnapshotMetadata,
}

impl QuerySnapshot {
    pub fn docs(&self) -> &[DocumentSnapshot] {
        &self.docs
    }

    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// Changes since the previous snapshot of the same listener (all documents are
    /// `Added` in the first snapshot and in one-time reads).
    pub fn doc_changes(&self) -> &[DocumentChange] {
        &self.changes
    }

    pub fn metadata(&self) -> SnapshotMetadata {
        self.metadata
    }

    /// Decode every document into `T`.
    pub fn data<T: DeserializeOwned>(&self) -> Result<Vec<T>> {
        self.docs
            .iter()
            .filter_map(|doc| doc.data().transpose())
            .collect()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, DocumentSnapshot> {
        self.docs.iter()
    }
}

impl IntoIterator for QuerySnapshot {
    type Item = DocumentSnapshot;
    type IntoIter = std::vec::IntoIter<DocumentSnapshot>;
    fn into_iter(self) -> Self::IntoIter {
        self.docs.into_iter()
    }
}

impl<'a> IntoIterator for &'a QuerySnapshot {
    type Item = &'a DocumentSnapshot;
    type IntoIter = std::slice::Iter<'a, DocumentSnapshot>;
    fn into_iter(self) -> Self::IntoIter {
        self.docs.iter()
    }
}
