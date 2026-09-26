use crate::batch::{SetOptions, WriteBatch, WriteOp};
use crate::native::{self, Source};
use crate::query::Query;
use crate::snapshot::{DocumentSnapshot, SnapshotOptions};
use crate::{FirebaseError, ListenerRegistration, Result};
use serde::Serialize;
use whisker::ReadSignal;

/// A document location in the default database. Creating one never fails; an
/// invalid path is reported by the first operation that uses it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocumentReference {
    path: String,
}

/// A collection location in the default database.
#[derive(Debug, Clone, PartialEq)]
pub struct CollectionReference {
    path: String,
    query: Query,
}

impl DocumentReference {
    pub(crate) fn new(path: String) -> Self {
        Self { path }
    }

    /// The document ID (last path segment).
    pub fn id(&self) -> &str {
        last_segment(&self.path)
    }

    /// Slash-separated path from the database root, e.g. `users/alice`.
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn parent(&self) -> CollectionReference {
        CollectionReference::new(parent_path(&self.path).to_owned())
    }

    /// A subcollection of this document.
    pub fn collection(&self, path: &str) -> CollectionReference {
        CollectionReference::new(format!("{}/{path}", self.path))
    }

    /// Read with the SDK's default policy: the server when reachable, else the cache.
    pub async fn get(&self) -> Result<DocumentSnapshot> {
        self.get_with(Source::Default).await
    }

    pub async fn get_with(&self, source: Source) -> Result<DocumentSnapshot> {
        validate_document_path(&self.path)?;
        native::get_document(&self.path, source).await
    }

    /// Create or overwrite the document. Resolves once the server acknowledges the write.
    pub async fn set<T: Serialize + ?Sized>(&self, data: &T) -> Result<()> {
        self.set_with(data, SetOptions::overwrite()).await
    }

    /// Like [`set`](Self::set) with [`SetOptions::merge`] or [`SetOptions::merge_fields`].
    pub async fn set_with<T: Serialize + ?Sized>(
        &self,
        data: &T,
        options: SetOptions,
    ) -> Result<()> {
        self.commit(WriteOp::set(self, data, options)).await
    }

    /// Update fields of an existing document; fails with `not-found` if it is missing.
    /// Top-level keys are dotted field paths (`"address.city"`).
    pub async fn update<T: Serialize + ?Sized>(&self, fields: &T) -> Result<()> {
        self.commit(WriteOp::update(self, fields)).await
    }

    /// Delete the document. Deleting a missing document succeeds.
    /// Subcollections are not deleted.
    pub async fn delete(&self) -> Result<()> {
        self.commit(Ok(WriteOp::delete(self))).await
    }

    async fn commit(&self, op: Result<WriteOp>) -> Result<()> {
        let mut batch = WriteBatch::new();
        batch.push(op);
        batch.commit().await
    }

    /// Receive the current document and every later change until the registration drops.
    pub fn on_snapshot(
        &self,
        callback: impl Fn(Result<DocumentSnapshot>) + 'static,
    ) -> Result<ListenerRegistration> {
        self.on_snapshot_with(SnapshotOptions::default(), callback)
    }

    pub fn on_snapshot_with(
        &self,
        options: SnapshotOptions,
        callback: impl Fn(Result<DocumentSnapshot>) + 'static,
    ) -> Result<ListenerRegistration> {
        validate_document_path(&self.path)?;
        native::listen_document(&self.path, options, callback)
    }

    /// The latest snapshot as a signal; `None` until the first one arrives.
    /// Call inside a component: the listener stops when the component is disposed.
    pub fn snapshot_signal(&self) -> ReadSignal<Option<Result<DocumentSnapshot>>> {
        let this = self.clone();
        whisker_firebase_core::__private::signal_from_listener(
            None,
            move |set| this.on_snapshot(move |snapshot| set(Some(snapshot))),
            |error| Some(Err(error)),
        )
    }
}

impl CollectionReference {
    pub(crate) fn new(path: String) -> Self {
        Self {
            query: Query::collection(path.clone()),
            path,
        }
    }

    /// The collection ID (last path segment).
    pub fn id(&self) -> &str {
        last_segment(&self.path)
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// The document containing this subcollection, or `None` for a root collection.
    pub fn parent(&self) -> Option<DocumentReference> {
        self.path
            .contains('/')
            .then(|| DocumentReference::new(parent_path(&self.path).to_owned()))
    }

    /// A document in this collection. `id` may itself be a relative path.
    pub fn doc(&self, id: &str) -> DocumentReference {
        DocumentReference::new(format!("{}/{id}", self.path))
    }

    /// A reference with a new random ID, for writing later (e.g. inside a batch).
    pub fn new_doc(&self) -> Result<DocumentReference> {
        validate_collection_path(&self.path)?;
        Ok(self.doc(&native::new_document_id(&self.path)?))
    }

    /// Create a document with a random ID and return its reference.
    pub async fn add<T: Serialize + ?Sized>(&self, data: &T) -> Result<DocumentReference> {
        let doc = self.new_doc()?;
        doc.set(data).await?;
        Ok(doc)
    }
}

/// Every [`Query`] method is available on a collection.
impl std::ops::Deref for CollectionReference {
    type Target = Query;
    fn deref(&self) -> &Query {
        &self.query
    }
}

impl From<CollectionReference> for Query {
    fn from(collection: CollectionReference) -> Self {
        collection.query
    }
}

fn last_segment(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn parent_path(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

fn validate_path(path: &str, documents: bool) -> Result<()> {
    let segments: Vec<_> = path.split('/').collect();
    let valid_segments = segments.iter().all(|segment| {
        !segment.is_empty()
            && *segment != "."
            && *segment != ".."
            && segment.len() <= 1500
            && !segment.contains('\0')
            && !(segment.starts_with("__") && segment.ends_with("__"))
    });
    if !valid_segments || (segments.len() % 2 == 0) != documents {
        let kind = if documents { "document" } else { "collection" };
        return Err(FirebaseError::invalid_argument(
            "firestore",
            format!("`{path}` is not a valid {kind} path"),
        ));
    }
    Ok(())
}

pub(crate) fn validate_document_path(path: &str) -> Result<()> {
    validate_path(path, true)
}

pub(crate) fn validate_collection_path(path: &str) -> Result<()> {
    validate_path(path, false)
}

pub(crate) fn validate_collection_id(id: &str) -> Result<()> {
    if id.contains('/') {
        return Err(FirebaseError::invalid_argument(
            "firestore",
            "collection group IDs cannot contain '/'",
        ));
    }
    validate_collection_path(id)
}
