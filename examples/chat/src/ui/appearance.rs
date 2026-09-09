use super::{button::Button, theme};
use whisker::prelude::*;
use whisker_icons::lucide;

#[component]
pub fn appearance_picker() -> Element {
    render! {
        View(style: theme::row().gap(px(theme::space::SM))) {
            AppearanceOption(value: theme::Appearance::Light, label: "Light", icon: lucide::Sun)
            AppearanceOption(value: theme::Appearance::Dark, label: "Dark", icon: lucide::Moon)
        }
    }
}

#[component]
fn appearance_option(value: theme::Appearance, label: &'static str, icon: &'static str) -> Element {
    let appearance = theme::use_appearance();
    let app = use_context::<crate::state::AppState>().expect("AppState context");
    render! {
        Button(
            label: computed(move || {
                if appearance.get() == value {
                    format!("✓ {label}")
                } else {
                    label.into()
                }
            }),
            primary: computed(move || appearance.get() == value),
            icon: icon,
            on_press: move |()| {
                appearance.set(value);
                if let Err(error) = crate::storage::save_appearance(value) {
                    app.notice().set(error);
                }
            },
        )
    }
}
