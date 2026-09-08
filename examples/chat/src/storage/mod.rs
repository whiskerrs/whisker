use crate::state::{Connection, Conversation};
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
pub fn load_conversation() -> Result<Conversation, String> {
    Ok(load(CONVERSATION)?.unwrap_or_default())
}
pub fn save_connection(connection: &Connection) -> Result<(), String> {
    save(SETTINGS, connection)
}
pub fn save_conversation(conversation: &Conversation) -> Result<(), String> {
    save(CONVERSATION, conversation)
}

fn load<T: serde::de::DeserializeOwned>(key: &str) -> Result<Option<T>, String> {
    let Some(json) =
        WhiskerLocalStore::load(key.into()).map_err(|_| "保存データを読み込めませんでした。")?
    else {
        return Ok(None);
    };
    let stored: Stored<T> = serde_json::from_str(&json)
        .map_err(|_| "保存データを読み取れませんでした。データは削除されていません。")?;
    if stored.version != 1 {
        return Err("このバージョンでは保存データを読み込めません。".into());
    }
    Ok(Some(stored.data))
}

fn save<T: Serialize>(key: &str, value: &T) -> Result<(), String> {
    let json = serde_json::to_string(&Stored {
        version: 1,
        data: value,
    })
    .map_err(|_| "保存データを作成できませんでした。")?;
    match WhiskerLocalStore::save(key.into(), json) {
        Ok(true) => Ok(()),
        _ => Err("変更を保存できていません。空き容量を確認してください。".into()),
    }
}

pub fn persistent_keys_available() -> bool {
    cfg!(any(target_os = "android", target_os = "ios"))
}

pub fn load_key(connection: &Connection) -> Result<Option<String>, String> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    return whisker_secure_store::WhiskerSecureStore::load(format!(
        "chat.api-key.v1:{}",
        connection.base_url
    ))
    .map_err(|_| "APIキーを復元できませんでした。接続設定から再入力してください。".into());
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
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
            .map_err(|_| "APIキーを安全に保存できません。今回だけ使用する方法を選択できます。")?
            {
                return Ok(());
            }
            return Err("APIキーを安全に保存できませんでした。".into());
        }
        WhiskerSecureStore::remove(format!("chat.api-key.v1:{}", connection.base_url))
            .map_err(|_| "保存済みAPIキーを削除できませんでした。".into())
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = (connection, key, remember);
        Ok(())
    }
}
