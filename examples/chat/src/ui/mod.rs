mod button;
mod chat;
mod connection;
mod theme;

use crate::state::{AppState, Page};
use button::Button;
use chat::ChatScreen;
use connection::ConnectionScreen;
use whisker::prelude::*;

pub fn root() -> Element {
    let app = AppState::new();
    provide_context(app.clone());
    let page = app.page();
    let notice = app.notice();
    let restore = app.clone();
    on_mount(move || restore.restore());
    let insets = whisker_safe_area::safe_area_insets();
    let keyboard = whisker_keyboard::keyboard_height();
    render! {
        View(
            style: computed(move || {
                theme::fill()
                    .background_color(Color::hex(theme::SURFACE))
                    .padding_top(px(insets.get().top as f32))
                    .padding_bottom(px(insets.get().bottom.max(keyboard.get()) as f32))
            }),
        ) {
            Show(when: move || !notice.with(String::is_empty)) {
                Text(value: notice, style: theme::muted().padding(px(12)))
            }
            Show(when: move || page.get() == Page::Loading) {
                Text(
                    value: "Whisker Chat — Loading…",
                    style: theme::text(20.0).padding(px(24)),
                )
            }
            Show(when: move || page.get() == Page::RestoreError) {
                Button(
                    label: "Try loading again",
                    on_press: {
                        let app = app.clone();
                        move |()| app.restore()
                    },
                )
            }
            Show(when: move || page.get() == Page::Connection) {
                ConnectionScreen()
            }
            Show(when: move || page.get() == Page::Chat) {
                ChatScreen()
            }
        }
    }
}
