use super::theme::{self, color, radius, size, space};
use whisker::css::FontWeight;
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
            style: computed(move || {
                theme::row()
                    .justify_content(JustifyContent::Center)
                    .gap(px(space::SM))
                    .padding_left(px(space::LG))
                    .padding_right(px(space::LG))
                    .min_height(px(size::TOUCH))
                    .flex_shrink(0.0)
                    .border_radius(px(radius::CONTROL))
                    .opacity(if disabled.get() { 0.45 } else { 1.0 })
                    .background_color(Color::hex(if primary {
                        color::ACCENT
                    } else {
                        color::TINT
                    }))
            }),
        ) {
            Show(when: move || !icon.is_empty()) {
                Icon(svg: icon, color: if primary { "#ffffff" } else { "#272722" }, size: "18")
            }
            Show(when: move || !label.get().is_empty()) {
                Text(
                    value: label,
                    style: theme::text(size::LABEL)
                        .font_weight(FontWeight::Numeric(600))
                        .color(Color::hex(if primary {
                            color::ON_ACCENT
                        } else {
                            color::INK
                        })),
                )
            }
        }
    }
}
