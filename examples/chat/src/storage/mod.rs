#[cfg(any(target_arch = "wasm32", test))]
mod browser_key;

use crate::state::{Connection, Conversation, Library};
use serde::{Deserialize, Serialize};
use whisker_local_store::WhiskerLocalStore;

const SETTINGS: &str = "chat.connection.v1";
const CONVERSATION: &str = "chat.conversation.v1";

#[derive(Serialize, Deserialize)]
struct Stored<T> {
    version: u32,
    data: T,
}

pub fn load_connection() -> Result<Option<Connection>, String> {
    load(SETTINGS)
}
pub fn load_library() -> Result<Library, String> {
    if let Some(library) = load("chat.library.v2")? {
        return Ok(library);
    }
    let legacy: Conversation = load(CONVERSATION)?.unwrap_or_default();
    Ok(Library {
        active: 0,
        conversations: vec![legacy],
    })
}
pub fn save_connection(connection: &Connection) -> Result<(), String> {
    save(SETTINGS, connection)
}
pub fn save_library(library: &Library) -> Result<(), String> {
    save("chat.library.v2", library)
}

fn load<T: serde::de::DeserializeOwned>(key: &str) -> Result<Option<T>, String> {
    let Some(json) =
        WhiskerLocalStore::load(key.into()).map_err(|_| "Could not load your saved data.")?
    else {
        return Ok(None);
    };
    let stored: Stored<T> = serde_json::from_str(&json)
        .map_err(|_| "Could not read your saved data. It has not been deleted.")?;
    if stored.version != 1 {
        return Err("This app version cannot read your saved data.".into());
    }
    Ok(Some(stored.data))
}

fn save<T: Serialize>(key: &str, value: &T) -> Result<(), String> {
    let json = serde_json::to_string(&Stored {
        version: 1,
        data: value,
    })
    .map_err(|_| "Could not prepare your data for saving.")?;
    match WhiskerLocalStore::save(key.into(), json) {
        Ok(true) => Ok(()),
        _ => Err("Changes have not been saved. Check your available storage.".into()),
    }
}

pub fn secure_keys_available() -> bool {
    cfg!(any(target_os = "android", target_os = "ios"))
}

pub fn load_key(connection: &Connection) -> Result<Option<String>, String> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    return whisker_secure_store::WhiskerSecureStore::load(format!(
        "chat.api-key.v1:{}",
        connection.base_url
    ))
    .map_err(|_| "Could not restore your API key. Enter it again in Settings.".into());
    #[cfg(target_arch = "wasm32")]
    return browser_key::load(connection);
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    {
        let _ = connection;
        Ok(None)
    }
}

pub fn save_key(connection: &Connection, key: &str, remember: bool) -> Result<(), String> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        use whisker_secure_store::WhiskerSecureStore;
        if remember {
            if WhiskerSecureStore::save(
                format!("chat.api-key.v1:{}", connection.base_url),
                key.into(),
            )
            .map_err(
                |_| "Could not store your API key securely. You can use it for this session only.",
            )? {
                return Ok(());
            }
            return Err("Could not store your API key securely.".into());
        }
        WhiskerSecureStore::remove(format!("chat.api-key.v1:{}", connection.base_url))
            .map_err(|_| "Could not remove the saved API key.".into())
    }
    #[cfg(target_arch = "wasm32")]
    return browser_key::save(connection, key, remember);
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    {
        let _ = (connection, key, remember);
        Ok(())
    }
}

pub fn load_appearance() -> Result<Option<crate::design::Appearance>, String> {
    load("chat.appearance.v1")
}

pub fn save_appearance(appearance: crate::design::Appearance) -> Result<(), String> {
    save("chat.appearance.v1", &appearance)
}
