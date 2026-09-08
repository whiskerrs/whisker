mod button;
mod chat;
mod connection;
mod navigation;
mod startup;
mod theme;

use crate::state::AppState;
use startup::Startup;
use whisker::prelude::*;
use whisker_router::Router;

pub fn root() -> Element {
    let app = AppState::new();
    provide_context(app.clone());
    let notice = app.notice();
    let insets = whisker_safe_area::safe_area_insets();
    let keyboard = whisker_keyboard::keyboard_height();
    render! {
        View(
            style: computed(move || {
                theme::screen()
                    .padding_top(px(insets.get().top as f32))
                    .padding_bottom(px(insets.get().bottom.max(keyboard.get()) as f32))
            }),
        ) {
            Show(when: move || !notice.with(String::is_empty)) {
                Text(value: notice, style: theme::muted().padding(px(12)))
            }
            Router(routes: navigation::routes()) {
                Startup()
            }
        }
    }
}
