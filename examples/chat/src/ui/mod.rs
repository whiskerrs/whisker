mod appearance;
mod button;
mod chat;
mod chat_layout;
mod composer;
mod connection;
mod history;
mod history_dialog;
mod history_row;
mod history_sidebar;
mod history_transition;
mod markdown;
mod messages;
pub(crate) mod navigation;
mod settings;
mod settings_shell;
mod startup;
mod theme;
mod welcome;

use crate::state::AppState;
use startup::Startup;
use whisker::prelude::*;
use whisker_router::Router;

pub fn root() -> Element {
    theme::provide_appearance();
    let app = AppState::new();
    provide_context(app.clone());
    let notice = app.notice();
    let insets = whisker_safe_area::safe_area_insets();
    let keyboard = whisker_keyboard::keyboard_height();
    render! {
        View(
            style: theme::style(move |palette| {
                palette
                    .screen()
                    .padding_top(px(insets.get().top as f32))
                    .padding_bottom(px(insets.get().bottom.max(keyboard.get()) as f32))
            }),
        ) {
            Show(when: move || !notice.with(String::is_empty)) {
                Text(
                    value: notice,
                    style: theme::style(move |palette| palette.muted().padding(px(12))),
                )
            }
            Router(routes: navigation::routes()) {
                Startup()
            }
        }
    }
}
