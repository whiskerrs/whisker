use super::theme;
use whisker::prelude::*;

#[component]
pub fn button(
    label: Signal<String>,
    on_press: Callback,
    #[prop(default = false)] primary: bool,
) -> Element {
    render! {
        View(
            on_tap: move |_| on_press.call(),
            style: theme::row()
                .justify_content(JustifyContent::Center)
                .padding_left(px(16))
                .padding_right(px(16))
                .min_height(px(44))
                .flex_shrink(0.0)
                .border_radius(px(12))
                .background_color(Color::hex(if primary { theme::ACCENT } else { 0xe9eeea })),
        ) {
            Text(
                value: label,
                style: theme::text(14.0).color(Color::hex(if primary { 0xffffff } else { theme::INK })),
            )
        }
    }
}
