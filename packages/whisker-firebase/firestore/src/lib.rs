//! Cloud Firestore for Whisker on Android and iOS (default app, default database).
//!
//! ```ignore
//! use serde::{Deserialize, Serialize};
//! use whisker_firebase::firestore::{Direction, FieldValue, FilterOp, Firestore, Timestamp, data};
//!
//! #[derive(Serialize, Deserialize)]
//! struct City { name: String, population: i64, founded: Timestamp }
//!
//! let db = Firestore::instance()?;
//! let tokyo = db.collection("cities").doc("tokyo");
//! tokyo.set(&City { name: "Tokyo".into(), population: 14_000_000, founded: Timestamp::now() }).await?;
//! tokyo.update(&data! { "population" => FieldValue::increment(1) }).await?;
//! let city: Option<City> = tokyo.get().await?.data()?;
//!
//! let big = db.collection("cities")
//!     .where_field("population", FilterOp::GreaterThan, 1_000_000)
//!     .order_by("population", Direction::Descending)
//!     .limit(10)
//!     .get()
//!     .await?;
//! let _registration = tokyo.on_snapshot(|snapshot| { /* … */ })?;
//! ```
mod batch;
mod de;
mod native;
mod query;
mod reference;
mod ser;
mod snapshot;
mod value;

pub use batch::{SetOptions, WriteBatch};
pub use de::from_value;
pub use native::Source;
pub use query::{Direction, FilterOp, Query};
pub use reference::{CollectionReference, DocumentReference};
pub use ser::{Fields, to_value};
pub use snapshot::{
    DocumentChange, DocumentChangeType, DocumentSnapshot, QuerySnapshot, SnapshotMetadata,
    SnapshotOptions,
};
pub use value::{Bytes, FieldValue, GeoPoint, Increment, Map, Value};
pub use whisker_firebase_core::{FirebaseError, ListenerRegistration, Result, Timestamp};

use whisker_firebase_core::FirebaseApp;

/// The default Firestore database of the default Firebase app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Firestore {
    _private: (),
}

impl Firestore {
    /// Initialize the default Firebase app if needed and return its database.
    pub fn instance() -> Result<Self> {
        FirebaseApp::initialize()?;
        Ok(Self { _private: () })
    }

    /// Route this database to the Firestore emulator. Call it before any other
    /// Firestore operation; Android Emulator guests normally reach the host at `10.0.2.2`.
    pub fn use_emulator(&self, host: &str, port: u16) -> Result<()> {
        if host.is_empty() || host.contains(|c: char| c.is_whitespace() || c == '/') || port == 0 {
            return Err(FirebaseError::invalid_argument(
                "firestore",
                "expected an emulator hostname and a nonzero port",
            ));
        }
        native::use_emulator(host, port)
    }

    /// A collection by slash-separated path, e.g. `"users"` or `"users/alice/posts"`.
    pub fn collection(&self, path: &str) -> CollectionReference {
        CollectionReference::new(path.into())
    }

    /// A document by slash-separated path, e.g. `"users/alice"`.
    pub fn doc(&self, path: &str) -> DocumentReference {
        DocumentReference::new(path.into())
    }

    /// Every collection with this ID, at any depth.
    pub fn collection_group(&self, collection_id: &str) -> Query {
        Query::group(collection_id.into())
    }

    /// Start a set of writes committed atomically.
    pub fn batch(&self) -> WriteBatch {
        WriteBatch::new()
    }
}

#[cfg(test)]
mod tests;
