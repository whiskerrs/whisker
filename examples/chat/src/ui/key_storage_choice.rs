use super::{settings_shell, theme};
use whisker::css::{Cursor, PointerEvents};
use whisker::prelude::*;
use whisker_icons::{Icon, lucide};

const LABEL: &str = "I understand the risks. Save my API key in this browser.";

#[component]
pub fn key_storage_choice(remember: RwSignal<bool>) -> Element {
    let appearance = theme::use_appearance();
    render! {
        View(style: theme::column().gap(px(8)).padding_top(px(8))) {
            Text(
                value: "Browser storage is not encrypted by this app. Malicious scripts, extensions with access to this site, or someone using your browser profile could read the key.",
                style: theme::style(settings_shell::description),
            )
            View(
                accessibility: computed(move || {
                    Accessibility::new()
                        .label(LABEL)
                        .role(AccessibilityRole::Checkbox)
                        .state(AccessibilityState::new().checked(if remember.get() {
                            AccessibilityChecked::Checked
                        } else {
                            AccessibilityChecked::Unchecked
                        }))
                }),
                on_tap: move |_| remember.update(|checked| *checked = !*checked),
                style: theme::row()
                    .align_items(AlignItems::FlexStart)
                    .gap(px(8))
                    .padding_top(px(8))
                    .padding_bottom(px(8))
                    .cursor(Cursor::Pointer),
            ) {
                View(
                    style: theme::column()
                        .flex_shrink(0.0)
                        .pointer_events(PointerEvents::None),
                ) {
                    Icon(
                        svg: computed(move || {
                            if remember.get() {
                                lucide::SquareCheck.to_owned()
                            } else {
                                lucide::Square.to_owned()
                            }
                        }),
                        size: "20",
                        color: computed(move || format!("#{:06x}", appearance.get().palette().ink)),
                    )
                }
                Text(
                    value: LABEL,
                    style: theme::style(move |palette| {
                        settings_shell::label(palette)
                            .flex_grow(1.0)
                            .flex_basis(px(0))
                            .pointer_events(PointerEvents::None)
                    }),
                )
            }
            Text(
                value: "Leave unchecked for session-only use. To remove a saved key, uncheck this option and save the connection.",
                style: theme::style(settings_shell::description),
            )
        }
    }
}
