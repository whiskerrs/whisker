use super::theme::{self, radius, size, space};
use whisker::css::{Cursor, FontWeight, PointerEvents};
use whisker::prelude::*;
use whisker_icons::Icon;

#[component]
pub fn button(
    label: Signal<String>,
    on_press: Callback,
    #[prop(default = Signal::from(false))] primary: Signal<bool>,
    #[prop(default = false)] compact: bool,
    #[prop(default = false)] plain: bool,
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
                    .justify_content(if compact {
                        JustifyContent::FlexStart
                    } else {
                        JustifyContent::Center
                    })
                    .gap(px(space::SM))
                    .padding_left(px(if compact { space::MD } else { space::LG }))
                    .padding_right(px(if compact { space::MD } else { space::LG }))
                    .min_height(px(if compact { 32.0 } else { size::TOUCH }))
                    .flex_shrink(0.0)
                    .border_radius(px(radius::CONTROL))
                    .cursor(if disabled.get() {
                        Cursor::NotAllowed
                    } else {
                        Cursor::Pointer
                    })
                    .opacity(if disabled.get() { 0.45 } else { 1.0 })
                    .background_color(if primary.get() {
                        Color::hex(palette.accent)
                    } else if plain {
                        Color::Transparent
                    } else {
                        Color::hex(palette.tint)
                    })
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
                                if primary.get() {
                                    palette.on_accent
                                } else {
                                    palette.ink
                                }
                            )
                        }),
                        size: if compact { "16" } else { "18" },
                    )
                }
            }
            Show(when: move || !label.get().is_empty()) {
                Text(
                    value: label,
                    style: theme::style(move |palette| {
                        palette
                            .text(if compact { 13.0 } else { size::LABEL })
                            .pointer_events(PointerEvents::None)
                            .font_weight(FontWeight::Numeric(500))
                            .color(Color::hex(if primary.get() {
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
