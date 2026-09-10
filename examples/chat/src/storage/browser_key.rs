use super::{load as load_value, save as save_value};
use crate::state::Connection;
use serde::{Deserialize, Serialize};
use whisker_local_store::WhiskerLocalStore;

const KEY: &str = "chat.browser-api-key.v1";

#[derive(Deserialize, Serialize)]
struct SavedKey {
    base_url: String,
    key: String,
}

pub fn load(connection: &Connection) -> Result<Option<String>, String> {
    let saved: Option<SavedKey> = load_value(KEY)
        .map_err(|_| "Could not read your saved API key. Enter it again to continue.".to_owned())?;
    Ok(saved
        .filter(|saved| saved.base_url == connection.base_url && !saved.key.is_empty())
        .map(|saved| saved.key))
}

pub fn save(connection: &Connection, key: &str, remember: bool) -> Result<(), String> {
    if remember {
        save_value(
            KEY,
            &SavedKey {
                base_url: connection.base_url.clone(),
                key: key.to_owned(),
            },
        )
        .map_err(|_| {
            "Could not save your API key. Uncheck browser storage to use it for this session only."
                .into()
        })
    } else {
        WhiskerLocalStore::remove(KEY.into()).map_err(|_| {
            "Could not remove the saved API key. Check your browser storage settings.".into()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    use whisker::runtime::{
        module::{ModuleHost, with_module_host},
        value::WhiskerValue,
    };

    #[test]
    fn opt_in_restores_only_the_matching_endpoint_and_opt_out_removes_the_key() {
        let stored = Rc::new(RefCell::new(None::<String>));
        let host_store = stored.clone();
        let host = ModuleHost::new(
            move |module, method, args, _, result| {
                assert_eq!(module, "whisker-local-store:WhiskerLocalStore");
                assert_eq!(args[0], WhiskerValue::String(KEY.into()));
                result(match method {
                    "load" => host_store
                        .borrow()
                        .clone()
                        .map_or(WhiskerValue::Null, WhiskerValue::String),
                    "save" => {
                        let WhiskerValue::String(value) = &args[1] else {
                            panic!("expected string")
                        };
                        *host_store.borrow_mut() = Some(value.clone());
                        WhiskerValue::Bool(true)
                    }
                    "remove" => {
                        host_store.borrow_mut().take();
                        WhiskerValue::Null
                    }
                    _ => panic!("unexpected operation"),
                });
                true
            },
            |_, _, _| {},
        );
        with_module_host(&host, || {
            let first = Connection::default();
            let second = Connection {
                base_url: "https://example.com/v1".into(),
                ..first.clone()
            };
            assert!(load(&first).unwrap().is_none());
            save(&first, "test-key", false).unwrap();
            assert!(stored.borrow().is_none());
            save(&first, "test-key", true).unwrap();
            assert_eq!(load(&first).unwrap().as_deref(), Some("test-key"));
            assert!(load(&second).unwrap().is_none());
            save(&second, "replacement-key", true).unwrap();
            assert!(load(&first).unwrap().is_none());
            assert_eq!(load(&second).unwrap().as_deref(), Some("replacement-key"));
            save(&second, "replacement-key", false).unwrap();
            assert!(stored.borrow().is_none());
        });
    }

    #[test]
    fn storage_failures_are_reported_without_echoing_keys() {
        let host = ModuleHost::new(
            |_, _, _, _, result| {
                result(WhiskerValue::Error("sensitive-test-data".into()));
                true
            },
            |_, _, _| {},
        );
        with_module_host(&host, || {
            let connection = Connection::default();
            for error in [
                load(&connection).unwrap_err(),
                save(&connection, "test-key", true).unwrap_err(),
                save(&connection, "test-key", false).unwrap_err(),
            ] {
                assert!(!error.contains("sensitive-test-data"));
                assert!(!error.contains("test-key"));
            }
        });
    }
}
