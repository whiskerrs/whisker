//! Cloud Storage for Firebase for Whisker on Android and iOS (default app and bucket).
//!
//! ```ignore
//! use whisker_firebase::storage::{SettableMetadata, Storage};
//!
//! let storage = Storage::instance()?;
//! let avatar = storage.reference("users/alice/avatar.png");
//! avatar.put_bytes_with(png, &SettableMetadata::new().content_type("image/png")).await?;
//! let url = avatar.download_url().await?;
//! let bytes = avatar.get_bytes(1024 * 1024).await?;
//! ```
use std::collections::BTreeMap;
use std::path::Path;
use whisker::platform_module::WhiskerValue as Wire;
use whisker_firebase_core::__private::unwrap_response;
use whisker_firebase_core::FirebaseApp;
pub use whisker_firebase_core::{FirebaseError, Result, Timestamp};

const SERVICE: &str = "storage";

fn module() -> whisker::PlatformModule {
    whisker::module!("FirebaseStorage")
}

/// Both SDKs report the same numeric codes; map them to the JavaScript SDK's names.
fn normalize(mut error: FirebaseError) -> FirebaseError {
    let code = match error.code.as_str() {
        "-13000" => "unknown",
        "-13010" => "object-not-found",
        "-13011" => "bucket-not-found",
        "-13012" => "project-not-found",
        "-13013" => "quota-exceeded",
        "-13020" => "unauthenticated",
        "-13021" => "unauthorized",
        "-13030" => "retry-limit-exceeded",
        "-13031" => "invalid-checksum",
        "-13032" => "download-size-exceeded",
        "-13040" => "canceled",
        "-13050" => "invalid-argument",
        _ => return error,
    };
    error.code = code.into();
    error
}

async fn call(method: &str, args: Vec<Wire>) -> Result<Wire> {
    unwrap_response(SERVICE, module().invoke_async(method, args).await).map_err(normalize)
}

fn string(value: &str) -> Wire {
    Wire::String(value.into())
}

fn unit(value: Wire) -> Result<()> {
    match value {
        Wire::Null => Ok(()),
        _ => Err(FirebaseError::invalid_response(
            SERVICE,
            "expected an empty result",
        )),
    }
}

/// Cloud Storage for the default app's default bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Storage {
    _private: (),
}

impl Storage {
    /// Initialize the default Firebase app if needed and return its Storage instance.
    /// Fails with `no-default-bucket` when the configuration file has no storage bucket.
    pub fn instance() -> Result<Self> {
        if FirebaseApp::initialize()?.storage_bucket.is_none() {
            return Err(FirebaseError::new(
                SERVICE,
                "no-default-bucket",
                "the Firebase configuration has no storage bucket",
            ));
        }
        Ok(Self { _private: () })
    }

    /// Route Storage to the Storage emulator. Call it before any other Storage operation.
    pub fn use_emulator(&self, host: &str, port: u16) -> Result<()> {
        unwrap_response(
            SERVICE,
            module().invoke("useEmulator", vec![string(host), Wire::Int(port.into())]),
        )
        .and_then(unit)
    }

    /// The bucket root.
    pub fn root(&self) -> StorageReference {
        StorageReference::new("")
    }

    /// An object or folder by slash-separated path, e.g. `"images/cat.png"`.
    pub fn reference(&self, path: &str) -> StorageReference {
        StorageReference::new(path)
    }
}

/// A location in the bucket. Creating one never contacts the server.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StorageReference {
    path: String,
}

impl StorageReference {
    fn new(path: &str) -> Self {
        Self {
            path: path
                .split('/')
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("/"),
        }
    }

    /// The path from the bucket root, without a leading slash; empty for the root.
    pub fn full_path(&self) -> &str {
        &self.path
    }

    /// The last path segment; empty for the root.
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or("")
    }

    /// `None` for the root.
    pub fn parent(&self) -> Option<StorageReference> {
        if self.path.is_empty() {
            return None;
        }
        Some(StorageReference::new(
            self.path.rsplit_once('/').map_or("", |(parent, _)| parent),
        ))
    }

    pub fn root(&self) -> StorageReference {
        StorageReference::new("")
    }

    pub fn child(&self, path: &str) -> StorageReference {
        StorageReference::new(&format!("{}/{path}", self.path))
    }

    fn object(&self) -> Result<Wire> {
        if self.path.is_empty() {
            return Err(FirebaseError::invalid_argument(
                SERVICE,
                "the bucket root is not an object",
            ));
        }
        Ok(string(&self.path))
    }

    /// Upload data, replacing any existing object. Resolves when the upload completes.
    pub async fn put_bytes(&self, data: &[u8]) -> Result<FullMetadata> {
        self.put_bytes_with(data, &SettableMetadata::new()).await
    }

    pub async fn put_bytes_with(
        &self,
        data: &[u8],
        metadata: &SettableMetadata,
    ) -> Result<FullMetadata> {
        FullMetadata::decode(
            call(
                "putBytes",
                vec![
                    self.object()?,
                    Wire::Bytes(data.to_vec()),
                    metadata.encode(),
                ],
            )
            .await?,
        )
    }

    /// Upload a local file without loading it into memory.
    pub async fn put_file(&self, file: impl AsRef<Path>) -> Result<FullMetadata> {
        self.put_file_with(file, &SettableMetadata::new()).await
    }

    pub async fn put_file_with(
        &self,
        file: impl AsRef<Path>,
        metadata: &SettableMetadata,
    ) -> Result<FullMetadata> {
        FullMetadata::decode(
            call(
                "putFile",
                vec![self.object()?, path(file.as_ref())?, metadata.encode()],
            )
            .await?,
        )
    }

    /// Download the object into memory; fails if it is larger than `max_size` bytes.
    pub async fn get_bytes(&self, max_size: u64) -> Result<Vec<u8>> {
        let max = i64::try_from(max_size).unwrap_or(i64::MAX);
        match call("getBytes", vec![self.object()?, Wire::Int(max)]).await? {
            Wire::Bytes(bytes) => Ok(bytes),
            _ => Err(FirebaseError::invalid_response(SERVICE, "expected bytes")),
        }
    }

    /// Download the object to a local file, replacing it if it exists.
    pub async fn write_to_file(&self, file: impl AsRef<Path>) -> Result<()> {
        unit(call("writeToFile", vec![self.object()?, path(file.as_ref())?]).await?)
    }

    /// A long-lived HTTPS URL that anyone holding it can use to download the object.
    pub async fn download_url(&self) -> Result<String> {
        match call("downloadUrl", vec![self.object()?]).await? {
            Wire::String(url) => Ok(url),
            _ => Err(FirebaseError::invalid_response(SERVICE, "expected a URL")),
        }
    }

    pub async fn metadata(&self) -> Result<FullMetadata> {
        FullMetadata::decode(call("getMetadata", vec![self.object()?]).await?)
    }

    /// Change the given fields; unset fields are left unchanged.
    pub async fn update_metadata(&self, metadata: &SettableMetadata) -> Result<FullMetadata> {
        FullMetadata::decode(call("updateMetadata", vec![self.object()?, metadata.encode()]).await?)
    }

    pub async fn delete(&self) -> Result<()> {
        unit(call("delete", vec![self.object()?]).await?)
    }

    /// Every object and folder directly under this reference.
    pub async fn list_all(&self) -> Result<ListResult> {
        ListResult::decode(call("list", vec![string(&self.path), Wire::Null, Wire::Null]).await?)
    }

    /// One page of results; pass the returned `next_page_token` to continue.
    pub async fn list(&self, max_results: u32, page_token: Option<&str>) -> Result<ListResult> {
        if !(1..=1000).contains(&max_results) {
            return Err(FirebaseError::invalid_argument(
                SERVICE,
                "max_results must be between 1 and 1000",
            ));
        }
        ListResult::decode(
            call(
                "list",
                vec![
                    string(&self.path),
                    Wire::Int(max_results.into()),
                    page_token.map_or(Wire::Null, string),
                ],
            )
            .await?,
        )
    }
}

fn path(file: &Path) -> Result<Wire> {
    file.to_str()
        .filter(|path| file.is_absolute() && !path.is_empty())
        .map(string)
        .ok_or_else(|| {
            FirebaseError::invalid_argument(SERVICE, "expected an absolute UTF-8 file path")
        })
}

/// Object metadata you can set on upload or with [`StorageReference::update_metadata`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SettableMetadata {
    pub content_type: Option<String>,
    pub cache_control: Option<String>,
    pub content_disposition: Option<String>,
    pub content_encoding: Option<String>,
    pub content_language: Option<String>,
    pub custom_metadata: BTreeMap<String, String>,
}

impl SettableMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn content_type(mut self, value: &str) -> Self {
        self.content_type = Some(value.into());
        self
    }

    pub fn cache_control(mut self, value: &str) -> Self {
        self.cache_control = Some(value.into());
        self
    }

    pub fn content_disposition(mut self, value: &str) -> Self {
        self.content_disposition = Some(value.into());
        self
    }

    pub fn content_encoding(mut self, value: &str) -> Self {
        self.content_encoding = Some(value.into());
        self
    }

    pub fn content_language(mut self, value: &str) -> Self {
        self.content_language = Some(value.into());
        self
    }

    pub fn custom(mut self, key: &str, value: &str) -> Self {
        self.custom_metadata.insert(key.into(), value.into());
        self
    }

    fn encode(&self) -> Wire {
        let mut fields = BTreeMap::new();
        for (key, value) in [
            ("content_type", &self.content_type),
            ("cache_control", &self.cache_control),
            ("content_disposition", &self.content_disposition),
            ("content_encoding", &self.content_encoding),
            ("content_language", &self.content_language),
        ] {
            if let Some(value) = value {
                fields.insert(key.to_owned(), string(value));
            }
        }
        if !self.custom_metadata.is_empty() {
            fields.insert(
                "custom_metadata".into(),
                Wire::Map(
                    self.custom_metadata
                        .iter()
                        .map(|(k, v)| (k.clone(), string(v)))
                        .collect(),
                ),
            );
        }
        Wire::Map(fields)
    }
}

/// Metadata of a stored object.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FullMetadata {
    pub bucket: String,
    pub full_path: String,
    pub name: String,
    pub size: u64,
    pub generation: Option<String>,
    pub md5_hash: Option<String>,
    pub time_created: Option<Timestamp>,
    pub updated: Option<Timestamp>,
    pub settable: SettableMetadata,
}

impl FullMetadata {
    fn decode(wire: Wire) -> Result<Self> {
        let fields = map(wire)?;
        let text = |key: &str| match fields.get(key) {
            Some(Wire::String(value)) => Some(value.clone()),
            _ => None,
        };
        let time = |key: &str| match fields.get(key) {
            Some(Wire::Int(millis)) => Timestamp::from_millis(*millis).ok(),
            _ => None,
        };
        let custom_metadata = match fields.get("custom_metadata") {
            Some(Wire::Map(custom)) => custom
                .iter()
                .filter_map(|(k, v)| match v {
                    Wire::String(v) => Some((k.clone(), v.clone())),
                    _ => None,
                })
                .collect(),
            _ => BTreeMap::new(),
        };
        Ok(Self {
            bucket: text("bucket").unwrap_or_default(),
            full_path: text("full_path").unwrap_or_default(),
            name: text("name").unwrap_or_default(),
            size: match fields.get("size") {
                Some(Wire::Int(size)) if *size >= 0 => *size as u64,
                _ => 0,
            },
            generation: text("generation"),
            md5_hash: text("md5_hash"),
            time_created: time("time_created"),
            updated: time("updated"),
            settable: SettableMetadata {
                content_type: text("content_type"),
                cache_control: text("cache_control"),
                content_disposition: text("content_disposition"),
                content_encoding: text("content_encoding"),
                content_language: text("content_language"),
                custom_metadata,
            },
        })
    }
}

/// Objects (`items`) and folders (`prefixes`) under a reference.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ListResult {
    pub items: Vec<StorageReference>,
    pub prefixes: Vec<StorageReference>,
    pub next_page_token: Option<String>,
}

impl ListResult {
    fn decode(wire: Wire) -> Result<Self> {
        let fields = map(wire)?;
        let refs = |key: &str| match fields.get(key) {
            Some(Wire::Array(paths)) => paths
                .iter()
                .filter_map(|path| match path {
                    Wire::String(path) => Some(StorageReference::new(path)),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        Ok(Self {
            items: refs("items"),
            prefixes: refs("prefixes"),
            next_page_token: match fields.get("next_page_token") {
                Some(Wire::String(token)) => Some(token.clone()),
                _ => None,
            },
        })
    }
}

fn map(wire: Wire) -> Result<BTreeMap<String, Wire>> {
    match wire {
        Wire::Map(fields) => Ok(fields),
        _ => Err(FirebaseError::invalid_response(SERVICE, "expected a map")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_normalize_paths() {
        let storage = Storage { _private: () };
        let photo = storage.reference("/users//alice/photo.png/");
        assert_eq!(photo.full_path(), "users/alice/photo.png");
        assert_eq!(photo.name(), "photo.png");
        assert_eq!(photo.parent().unwrap().full_path(), "users/alice");
        assert_eq!(storage.root().child("a").child("b/c").full_path(), "a/b/c");
        assert!(storage.root().parent().is_none());
        assert!(storage.root().object().is_err());
        assert_eq!(
            normalize(FirebaseError::new(SERVICE, "-13010", "")).code,
            "object-not-found"
        );
    }
}
