use super::{button::Button, theme};
use crate::hooks::SidebarState;
use whisker::css::Overflow;
use whisker::prelude::*;
use whisker_icons::lucide;

#[component]
pub fn history_trigger(sidebar: SidebarState, pressed: Callback) -> Element {
    render! {
        View(
            style: computed(move || {
                let visibility = 1.0 - sidebar.progress.get();
                theme::row()
                    .width(px(
                        (theme::size::HISTORY_TRIGGER + theme::space::SM) * visibility
                    ))
                    .flex_shrink(0.0)
                    .overflow(Overflow::Hidden)
                    .opacity(visibility)
            }),
        ) {
            Show(when: move || sidebar.progress.get() < 1.0) {
                View(
                    style: theme::column()
                        .width(px(theme::size::HISTORY_TRIGGER))
                        .flex_shrink(0.0),
                ) {
                    Button(label: "Chats", icon: lucide::PanelLeft, on_press: pressed)
                }
            }
        }
    }
}
