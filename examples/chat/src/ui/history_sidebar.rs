use super::{history::HistoryPanel, theme};
use crate::hooks::SidebarState;
use whisker::css::{Overflow, Transform, TransformFn};
use whisker::prelude::*;

#[component]
pub fn history_sidebar(state: SidebarState) -> Element {
    render! {
        View(
            style: computed(move || {
                theme::column()
                    .width(px(theme::size::SIDEBAR * state.progress.get()))
                    .height(percent(100))
                    .flex_shrink(0.0)
                    .overflow(Overflow::Hidden)
            }),
        ) {
            Show(when: move || state.visible.get()) {
                View(
                    style: computed(move || {
                        theme::column()
                            .width(px(theme::size::SIDEBAR))
                            .height(percent(100))
                            .flex_shrink(0.0)
                            .transform(Transform::new().push(TransformFn::TranslateX(
                                px(-theme::size::SIDEBAR * (1.0 - state.progress.get())).into(),
                            )))
                    }),
                ) {
                    HistoryPanel(sidebar: true, closed: move |()| state.open.set(false))
                }
            }
        }
    }
}
