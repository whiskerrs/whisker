use super::theme::{self, size};
use whisker::css::{Cursor, PointerEvents};
use whisker::prelude::*;
use whisker_icons::{Icon, lucide};

#[component]
pub fn composer_action(
    busy: ReadSignal<bool>,
    draft: RwSignal<String>,
    on_press: Callback,
) -> Element {
    let disabled = computed(move || !busy.get() && draft.with(|text| text.trim().is_empty()));
    let appearance = theme::use_appearance();
    render! {
        View(
            accessibility: computed(move || {
                Accessibility::new()
                    .label(if busy.get() {
                        "Stop generation"
                    } else {
                        "Send message"
                    })
                    .role(AccessibilityRole::Button)
                    .state(AccessibilityState::new().disabled(disabled.get()))
            }),
            on_tap: move |_| {
                if !disabled.get_untracked() {
                    on_press.call();
                }
            },
            style: theme::style(move |palette| {
                theme::row()
                    .justify_content(JustifyContent::Center)
                    .width(px(size::TOUCH))
                    .height(px(size::TOUCH))
                    .flex_shrink(0.0)
                    .border_radius(px(size::TOUCH / 2.0))
                    .background_color(Color::hex(palette.accent))
                    .opacity(if disabled.get() { 0.4 } else { 1.0 })
                    .cursor(if disabled.get() {
                        Cursor::NotAllowed
                    } else {
                        Cursor::Pointer
                    })
            }),
        ) {
            View(style: Css::new().pointer_events(PointerEvents::None)) {
                Icon(
                    svg: computed(move || {
                        if busy.get() {
                            lucide::Square.into()
                        } else {
                            lucide::ArrowUp.into()
                        }
                    }),
                    color: computed(move || format!("#{:06x}", appearance.get().palette().on_accent)),
                    size: "20",
                )
            }
        }
    }
}
