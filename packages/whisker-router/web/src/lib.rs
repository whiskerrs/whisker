//! Web History API Host adapter for `whisker-router`.

use std::cell::RefCell;

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use whisker_web::{ModuleDefinition, WhiskerModule, WhiskerValue};

const MODULE_NAME: &str = "whisker-router:History";
const CHANGED_EVENT: &str = "changed";
const STATE_PREFIX: &str = "whisker-router:";
type PopStateListener = Closure<dyn FnMut(web_sys::PopStateEvent)>;

thread_local! {
    static POPSTATE_LISTENER: RefCell<Option<PopStateListener>> = const { RefCell::new(None) };
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HistoryState {
    index: i64,
    target: String,
}

impl HistoryState {
    fn encode(&self) -> String {
        format!("{STATE_PREFIX}{}\n{}", self.index, self.target)
    }

    fn decode(value: &wasm_bindgen::JsValue) -> Option<Self> {
        let encoded = value.as_string()?;
        Self::parse(&encoded)
    }

    fn parse(encoded: &str) -> Option<Self> {
        let rest = encoded.strip_prefix(STATE_PREFIX)?;
        let (index, target) = rest.split_once('\n')?;
        Some(Self {
            index: index.parse().ok()?,
            target: target.to_owned(),
        })
    }
}

struct HistoryModule;

#[whisker_web::WhiskerModule]
impl WhiskerModule for HistoryModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new()
            .name(MODULE_NAME)
            .function("initialize", |args, _| {
                if !args.is_empty() {
                    return WhiskerValue::Error(
                        "History.initialize does not accept arguments".into(),
                    );
                }
                result_value(initialize())
            })
            .function("push", |args, _| result_value(write(args, false)))
            .function("replace", |args, _| result_value(write(args, true)))
            .function("back", |args, _| {
                if !args.is_empty() {
                    return WhiskerValue::Error("History.back does not accept arguments".into());
                }
                match back() {
                    Ok(handled) => WhiskerValue::Bool(handled),
                    Err(error) => WhiskerValue::Error(error),
                }
            })
            .event(CHANGED_EVENT)
            .on_start_observing(CHANGED_EVENT, install_popstate_listener)
            .on_stop_observing(CHANGED_EVENT, |_| remove_popstate_listener())
    }
}

fn window() -> Result<web_sys::Window, String> {
    web_sys::window().ok_or_else(|| "browser Window is unavailable".into())
}

fn current_url(window: &web_sys::Window) -> Result<String, String> {
    let location = window.location();
    Ok(format!(
        "{}{}{}",
        location.pathname().map_err(js_error)?,
        location.search().map_err(js_error)?,
        location.hash().map_err(js_error)?,
    ))
}

fn initialize() -> Result<WhiskerValue, String> {
    let window = window()?;
    let history = window.history().map_err(js_error)?;
    let url = current_url(&window)?;
    let state = history
        .state()
        .ok()
        .as_ref()
        .and_then(HistoryState::decode)
        .unwrap_or_else(|| HistoryState {
            index: 0,
            target: url.clone(),
        });
    history
        .replace_state_with_url(
            &wasm_bindgen::JsValue::from_str(&state.encode()),
            "",
            Some(&url),
        )
        .map_err(js_error)?;
    Ok(location_payload(url, state.target))
}

fn write(args: &[WhiskerValue], replace: bool) -> Result<WhiskerValue, String> {
    let [WhiskerValue::String(url), WhiskerValue::String(target)] = args else {
        return Err("History push/replace requires public URL and internal target strings".into());
    };
    let history = window()?.history().map_err(js_error)?;
    let current_index = history
        .state()
        .ok()
        .as_ref()
        .and_then(HistoryState::decode)
        .map_or(0, |state| state.index);
    let state = HistoryState {
        index: if replace {
            current_index
        } else {
            current_index.saturating_add(1)
        },
        target: target.clone(),
    };
    let encoded = wasm_bindgen::JsValue::from_str(&state.encode());
    if replace {
        history
            .replace_state_with_url(&encoded, "", Some(url))
            .map_err(js_error)?;
    } else {
        history
            .push_state_with_url(&encoded, "", Some(url))
            .map_err(js_error)?;
    }
    Ok(WhiskerValue::Null)
}

fn back() -> Result<bool, String> {
    let history = window()?.history().map_err(js_error)?;
    let can_go_back = history
        .state()
        .ok()
        .as_ref()
        .and_then(HistoryState::decode)
        .is_some_and(|state| state.index > 0);
    if can_go_back {
        history.back().map_err(js_error)?;
    }
    Ok(can_go_back)
}

fn install_popstate_listener(emitter: &whisker::runtime::module::ModuleEventEmitter) {
    POPSTATE_LISTENER.with(|slot| {
        if slot.borrow().is_some() {
            return;
        }
        let Ok(window) = window() else {
            return;
        };
        let event_emitter = emitter.clone();
        let event_window = window.clone();
        let listener = Closure::<dyn FnMut(web_sys::PopStateEvent)>::new(
            move |event: web_sys::PopStateEvent| {
                let Ok(url) = current_url(&event_window) else {
                    return;
                };
                let target = HistoryState::decode(&event.state())
                    .map(|state| state.target)
                    .unwrap_or_else(|| url.clone());
                event_emitter.emit(CHANGED_EVENT, location_payload(url, target));
            },
        );
        if window
            .add_event_listener_with_callback("popstate", listener.as_ref().unchecked_ref())
            .is_ok()
        {
            *slot.borrow_mut() = Some(listener);
        }
    });
}

fn remove_popstate_listener() {
    POPSTATE_LISTENER.with(|slot| {
        let Some(listener) = slot.borrow_mut().take() else {
            return;
        };
        if let Ok(window) = window() {
            let _ = window
                .remove_event_listener_with_callback("popstate", listener.as_ref().unchecked_ref());
        }
    });
}

fn location_payload(url: String, target: String) -> WhiskerValue {
    WhiskerValue::map([
        ("url", WhiskerValue::String(url)),
        ("target", WhiskerValue::String(target)),
    ])
}

fn result_value(result: Result<WhiskerValue, String>) -> WhiskerValue {
    result.unwrap_or_else(WhiskerValue::Error)
}

fn js_error(value: wasm_bindgen::JsValue) -> String {
    value
        .as_string()
        .unwrap_or_else(|| format!("browser History API failed: {value:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_state_round_trips_qualified_targets() {
        let state = HistoryState {
            index: 42,
            target: "/(search)/detail/7?q=rust#reply".into(),
        };
        assert_eq!(HistoryState::parse(&state.encode()), Some(state),);
    }
}
