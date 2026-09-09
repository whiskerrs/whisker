use super::theme::{self, radius, size, space};
use whisker::css::{Cursor, FontWeight, PointerEvents};
use whisker::prelude::*;
use whisker_icons::Icon;

#[component]
pub fn button(
    label: Signal<String>,
    on_press: Callback,
    #[prop(default = false)] primary: bool,
    #[prop(default = "")] icon: &'static str,
    #[prop(default = Signal::from(false))] disabled: Signal<bool>,
    #[prop(default = "")] accessible_label: &'static str,
) -> Element {
    let appearance = theme::use_appearance();
    render! {
        View(
            accessibility: computed(move || {
                Accessibility::new()
                    .label(if accessible_label.is_empty() {
                        label.get()
                    } else {
                        accessible_label.into()
                    })
                    .role(AccessibilityRole::Button)
                    .state(AccessibilityState::new().disabled(disabled.get()))
            }),
            on_tap: move |_| {
                if !disabled.get() {
                    on_press.call();
                }
            },
            style: theme::style(move |palette| {
                theme::row()
                    .justify_content(JustifyContent::Center)
                    .gap(px(space::SM))
                    .padding_left(px(space::LG))
                    .padding_right(px(space::LG))
                    .min_height(px(size::TOUCH))
                    .flex_shrink(0.0)
                    .border_radius(px(radius::CONTROL))
                    .cursor(if disabled.get() {
                        Cursor::NotAllowed
                    } else {
                        Cursor::Pointer
                    })
                    .opacity(if disabled.get() { 0.45 } else { 1.0 })
                    .background_color(Color::hex(if primary {
                        palette.accent
                    } else {
                        palette.tint
                    }))
            }),
        ) {
            Show(when: move || !icon.is_empty()) {
                View(style: Css::new().pointer_events(PointerEvents::None)) {
                    Icon(
                        svg: icon,
                        color: computed(move || {
                            let palette = appearance.get().palette();
                            format!(
                                "#{:06x}",
                                if primary {
                                    palette.on_accent
                                } else {
                                    palette.ink
                                }
                            )
                        }),
                        size: "18",
                    )
                }
            }
            Show(when: move || !label.get().is_empty()) {
                Text(
                    value: label,
                    style: theme::style(move |palette| {
                        palette
                            .text(size::LABEL)
                            .pointer_events(PointerEvents::None)
                            .font_weight(FontWeight::Numeric(500))
                            .color(Color::hex(if primary {
                                palette.on_accent
                            } else {
                                palette.ink
                            }))
                    }),
                )
            }
        }
    }
}
