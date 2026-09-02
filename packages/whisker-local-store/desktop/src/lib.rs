//! Desktop Host implementation for `whisker-local-store`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use whisker_desktop::{ModuleDefinition, WhiskerModule, WhiskerValue};

const MODULE_NAME: &str = "whisker-local-store:WhiskerLocalStore";

struct LocalStoreModule;

#[whisker_desktop::WhiskerModule]
impl WhiskerModule for LocalStoreModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new()
            .name(MODULE_NAME)
            .function("save", |args, _| {
                let [WhiskerValue::String(key), WhiskerValue::String(value)] = args else {
                    return argument_error("save", "key and value strings");
                };
                mutate_store(|values| {
                    values.insert(key.clone(), value.clone());
                    WhiskerValue::Bool(true)
                })
            })
            .function("load", |args, _| {
                let [WhiskerValue::String(key)] = args else {
                    return argument_error("load", "one key string");
                };
                read_store(|values| {
                    values
                        .get(key)
                        .cloned()
                        .map_or(WhiskerValue::Null, WhiskerValue::String)
                })
            })
            .function("remove", |args, _| {
                let [WhiskerValue::String(key)] = args else {
                    return argument_error("remove", "one key string");
                };
                mutate_store(|values| {
                    values.remove(key);
                    WhiskerValue::Null
                })
            })
    }
}

struct FileStore {
    path: PathBuf,
    values: BTreeMap<String, String>,
}

impl FileStore {
    fn open(path: PathBuf) -> Result<Self, String> {
        let values = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| format!("decode {}: {error}", path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(format!("read {}: {error}", path.display())),
        };
        Ok(Self { path, values })
    }

    fn persist(&self) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| format!("store path has no parent: {}", self.path.display()))?;
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
        let bytes = serde_json::to_vec(&self.values)
            .map_err(|error| format!("encode local store: {error}"))?;
        std::fs::write(&self.path, bytes)
            .map_err(|error| format!("write {}: {error}", self.path.display()))
    }
}

static STORE: OnceLock<Result<Mutex<FileStore>, String>> = OnceLock::new();

fn store() -> Result<&'static Mutex<FileStore>, String> {
    STORE
        .get_or_init(|| FileStore::open(default_store_path()).map(Mutex::new))
        .as_ref()
        .map_err(Clone::clone)
}

fn read_store(operation: impl FnOnce(&BTreeMap<String, String>) -> WhiskerValue) -> WhiskerValue {
    let result = store().and_then(|store| {
        store
            .lock()
            .map_err(|_| "local store lock is poisoned".to_owned())
    });
    match result {
        Ok(store) => operation(&store.values),
        Err(error) => WhiskerValue::Error(error),
    }
}

fn mutate_store(
    operation: impl FnOnce(&mut BTreeMap<String, String>) -> WhiskerValue,
) -> WhiskerValue {
    let result = store().and_then(|store| {
        store
            .lock()
            .map_err(|_| "local store lock is poisoned".to_owned())
    });
    let mut store = match result {
        Ok(store) => store,
        Err(error) => return WhiskerValue::Error(error),
    };
    let previous = store.values.clone();
    let value = operation(&mut store.values);
    if let Err(error) = store.persist() {
        store.values = previous;
        return WhiskerValue::Error(error);
    }
    value
}

fn default_store_path() -> PathBuf {
    let application = std::env::current_exe()
        .ok()
        .and_then(|path| path.file_stem().map(|name| name.to_owned()))
        .and_then(|name| name.to_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| "application".to_owned());
    user_data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("whisker")
        .join(application)
        .join("local-store.json")
}

#[cfg(target_os = "macos")]
fn user_data_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library/Application Support"))
}

#[cfg(target_os = "windows")]
fn user_data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn user_data_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".local/share"))
        })
}

fn argument_error(operation: &str, expected: &str) -> WhiskerValue {
    WhiskerValue::Error(format!("WhiskerLocalStore.{operation} requires {expected}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_store_persists_string_values() {
        let root =
            std::env::temp_dir().join(format!("whisker-local-store-test-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        let path = root.join("store.json");
        let mut store = FileStore::open(path.clone()).unwrap();
        store.values.insert("theme".into(), "dark".into());
        store.persist().unwrap();

        let reopened = FileStore::open(path).unwrap();
        assert_eq!(
            reopened.values.get("theme").map(String::as_str),
            Some("dark")
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn host_definition_contains_no_views() {
        assert!(__whisker_module_definition().factories().is_empty());
    }
}
