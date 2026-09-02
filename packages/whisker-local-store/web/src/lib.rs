//! Web Host implementation for `whisker-local-store`.

use whisker_web::{ModuleDefinition, WhiskerModule, WhiskerValue, wasm_bindgen};

const MODULE_NAME: &str = "whisker-local-store:WhiskerLocalStore";
const KEY_PREFIX: &str = "whisker-local-store:";

struct LocalStoreModule;

#[whisker_web::WhiskerModule]
impl WhiskerModule for LocalStoreModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new()
            .name(MODULE_NAME)
            .function("save", |args, _| {
                let [WhiskerValue::String(key), WhiskerValue::String(value)] = args else {
                    return argument_error("save", "key and value strings");
                };
                with_storage(|storage| {
                    storage
                        .set_item(&storage_key(key), value)
                        .map(|()| WhiskerValue::Bool(true))
                })
            })
            .function("load", |args, _| {
                let [WhiskerValue::String(key)] = args else {
                    return argument_error("load", "one key string");
                };
                with_storage(|storage| {
                    storage
                        .get_item(&storage_key(key))
                        .map(|value| value.map_or(WhiskerValue::Null, WhiskerValue::String))
                })
            })
            .function("remove", |args, _| {
                let [WhiskerValue::String(key)] = args else {
                    return argument_error("remove", "one key string");
                };
                with_storage(|storage| {
                    storage
                        .remove_item(&storage_key(key))
                        .map(|()| WhiskerValue::Null)
                })
            })
    }
}

fn storage_key(key: &str) -> String {
    format!("{KEY_PREFIX}{key}")
}

fn with_storage(
    operation: impl FnOnce(web_sys::Storage) -> Result<WhiskerValue, wasm_bindgen::JsValue>,
) -> WhiskerValue {
    let result = web_sys::window()
        .ok_or_else(|| wasm_bindgen::JsValue::from_str("browser Window is unavailable"))
        .and_then(|window| {
            window.local_storage().and_then(|storage| {
                storage.ok_or_else(|| {
                    wasm_bindgen::JsValue::from_str("browser localStorage is unavailable")
                })
            })
        })
        .and_then(operation);
    result.unwrap_or_else(|error| {
        WhiskerValue::Error(format!("localStorage operation failed: {error:?}"))
    })
}

fn argument_error(operation: &str, expected: &str) -> WhiskerValue {
    WhiskerValue::Error(format!("WhiskerLocalStore.{operation} requires {expected}"))
}
