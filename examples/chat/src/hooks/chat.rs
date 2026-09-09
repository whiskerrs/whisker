use super::chat_scroll::{ChatScroll, use_chat_scroll};
use crate::state::{AppState, SendError, Session};
use whisker::prelude::*;
use whisker_router::use_navigator;

#[derive(Clone)]
pub struct ChatActions {
    pub scroll: ChatScroll,
    pub list: ListHandle<u64>,
    pub send: Callback,
    pub retry: Callback,
    pub save: Callback,
    pub settings: Callback,
    pub latest: Callback,
}

pub fn use_chat(session: Session) -> ChatActions {
    let app = use_context::<AppState>().expect("AppState context");
    let nav = use_navigator();
    resource({
        let app = app.clone();
        move || {
            session.draft.get();
            let app = app.clone();
            async move {
                crate::api::delay(std::time::Duration::from_millis(400)).await;
                app.persist();
                Ok(())
            }
        }
    });
    let settings = Callback::new({
        let app = app.clone();
        move |()| {
            app.persist();
            crate::ui::navigation::open_settings(&nav, app.notice());
        }
    });
    let scroll = use_chat_scroll(session.turns.with_untracked(Vec::is_empty));
    let following = scroll.following;
    let list = ListHandle::<u64>::new();
    let revision = app.revision();
    effect({
        let list = list.clone();
        move || {
            revision.get();
            session.turns.with(Vec::len);
            if following.get_untracked() {
                let _ = list.scroll_to(ListScrollTarget::End, ScrollBehavior::Instant);
            }
        }
    });
    let send = Callback::new({
        let app = app.clone();
        move |()| {
            if app.busy().get_untracked() {
                app.stop();
            } else {
                following.set(true);
                if let Err(SendError::MissingConnection) = app.send(session) {
                    settings.call();
                }
            }
        }
    });
    let retry = Callback::new({
        let app = app.clone();
        move |()| {
            following.set(true);
            if let Err(SendError::MissingConnection) = app.retry(session) {
                settings.call();
            }
        }
    });
    let latest = Callback::new({
        let list = list.clone();
        move |()| {
            following.set(true);
            let _ = list.scroll_to(ListScrollTarget::End, ScrollBehavior::Smooth);
        }
    });
    ChatActions {
        scroll,
        list,
        send,
        retry,
        settings,
        latest,
        save: Callback::new(move |()| app.persist()),
    }
}
