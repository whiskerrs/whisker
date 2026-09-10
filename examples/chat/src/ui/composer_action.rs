use super::{
    composer_surface::reveal,
    theme::{self, size},
};
use whisker::css::{Cursor, PointerEvents, Transform, TransformFn, Visibility};
use whisker::prelude::*;
use whisker_icons::{Icon, lucide};

#[component]
pub fn composer_action(
    busy: ReadSignal<bool>,
    progress: ReadSignal<f32>,
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
                if progress.get_untracked() > 0.0 && !disabled.get_untracked() {
                    on_press.call();
                }
            },
            style: computed(move || {
                theme::row()
                    .justify_content(JustifyContent::Center)
                    .width(px(64))
                    .height(px(64))
                    .flex_shrink(0.0)
                    .opacity(reveal(progress.get()) * if disabled.get() { 0.4 } else { 1.0 })
                    .visibility(if progress.get() > 0.0 {
                        Visibility::Visible
                    } else {
                        Visibility::Hidden
                    })
                    .pointer_events(if progress.get() > 0.0 {
                        PointerEvents::Auto
                    } else {
                        PointerEvents::None
                    })
                    .cursor(if disabled.get() {
                        Cursor::NotAllowed
                    } else {
                        Cursor::Pointer
                    })
            }),
        ) {
            View(
                style: theme::row()
                    .justify_content(JustifyContent::Center)
                    .width(px(size::TOUCH))
                    .height(px(size::TOUCH))
                    .transform(Transform::new().push(TransformFn::TranslateX(px(5.0).into())))
                    .pointer_events(PointerEvents::None),
            ) {
                Icon(
                    svg: computed(move || {
                        if busy.get() {
                            lucide::Square.into()
                        } else {
                            lucide::ArrowUp.into()
                        }
                    }),
                    color: computed(move || format!("#{:06x}", appearance.get().palette().ink)),
                    size: "20",
                )
            }
        }
    }
}
