use super::{
    chat::ChatScreen, connection::ConnectionScreen, history::HistoryScreen,
    settings::SettingsScreen,
};
use whisker::RwSignal;
use whisker_router::{NavError, RouteSet, RouterHandle, routes};

const CHAT: &str = "/";
const SETTINGS: &str = "/settings";
const CONNECTION: &str = "/settings/connection";

pub fn routes() -> RouteSet {
    routes! {
        Stack {
            Route(path: "", component: ChatScreen)
            Route(path: "settings", component: SettingsScreen)
            Route(path: "settings/connection", component: ConnectionScreen)
            Route(path: "history", component: HistoryScreen)
        }
    }
}

pub fn start_setup(nav: &RouterHandle) -> Result<(), String> {
    nav.replace(SETTINGS)
        .map_err(|_| "Could not open settings.".into())
}

pub fn open_settings(nav: &RouterHandle, notice: RwSignal<String>) {
    if nav.navigate(SETTINGS).is_err() {
        notice.set("Could not open settings.".into());
    }
}

pub fn open_connection(nav: &RouterHandle, notice: RwSignal<String>) {
    if nav.navigate(CONNECTION).is_err() {
        notice.set("Could not open API connection settings.".into());
    }
}

pub fn return_to_settings(nav: &RouterHandle, notice: RwSignal<String>) {
    let result = match nav.back() {
        Err(NavError::NothingToPop) => nav.replace(SETTINGS),
        result => result,
    };
    if result.is_err() {
        notice.set("Could not return to settings.".into());
    }
}

pub fn return_to_chat(nav: &RouterHandle, notice: RwSignal<String>) {
    let result = match nav.back() {
        Err(NavError::NothingToPop) => nav.replace(CHAT),
        result => result,
    };
    if result.is_err() {
        notice.set("Could not return to the conversation.".into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker::{Owner, signal};
    use whisker_router::{provide_router, use_pathname};

    fn with_navigation(test: impl FnOnce(RouterHandle, RwSignal<String>)) {
        let runtime =
            whisker::runtime::RuntimeContext::new(whisker::runtime::RuntimeWakeHandle::new(|| {}));
        runtime.enter(|| {
            let owner = Owner::new(None);
            owner.with(|| {
                let nav = RouterHandle::new(routes());
                provide_router(nav.clone());
                test(nav, signal(String::new()));
            });
            owner.dispose();
        });
    }

    #[test]
    fn completing_setup_replaces_the_initial_settings_entry() {
        with_navigation(|nav, notice| {
            start_setup(&nav).unwrap();
            assert_eq!(use_pathname().get_untracked(), SETTINGS);
            return_to_chat(&nav, notice);
            assert_eq!(use_pathname().get_untracked(), CHAT);
            assert_eq!(nav.back(), Err(NavError::NothingToPop));
            assert!(notice.get_untracked().is_empty());
        });
    }

    #[test]
    fn returning_from_settings_unwinds_without_an_extra_chat_entry() {
        with_navigation(|nav, notice| {
            open_settings(&nav, notice);
            open_settings(&nav, notice);
            assert_eq!(use_pathname().get_untracked(), SETTINGS);
            return_to_chat(&nav, notice);
            assert_eq!(use_pathname().get_untracked(), CHAT);
            assert_eq!(nav.back(), Err(NavError::NothingToPop));
            open_settings(&nav, notice);
            nav.back().unwrap();
            assert_eq!(use_pathname().get_untracked(), CHAT);
            assert!(notice.get_untracked().is_empty());
        });
    }

    #[test]
    fn connection_editing_returns_to_settings_before_chat() {
        with_navigation(|nav, notice| {
            open_settings(&nav, notice);
            open_connection(&nav, notice);
            assert_eq!(use_pathname().get_untracked(), CONNECTION);
            return_to_settings(&nav, notice);
            assert_eq!(use_pathname().get_untracked(), SETTINGS);
            return_to_chat(&nav, notice);
            assert_eq!(use_pathname().get_untracked(), CHAT);
            assert_eq!(nav.back(), Err(NavError::NothingToPop));
            assert!(notice.get_untracked().is_empty());
        });
    }

    #[test]
    fn direct_connection_entry_has_a_settings_fallback() {
        with_navigation(|nav, notice| {
            nav.replace(CONNECTION).unwrap();
            return_to_settings(&nav, notice);
            assert_eq!(use_pathname().get_untracked(), SETTINGS);
            assert_eq!(nav.back(), Err(NavError::NothingToPop));
            assert!(notice.get_untracked().is_empty());
        });
    }
}
