use super::{button::Button, theme};
use whisker::prelude::*;
use whisker_icons::lucide;

#[component]
pub fn appearance_toggle() -> Element {
    let appearance = theme::use_appearance();
    let app = use_context::<crate::state::AppState>().expect("AppState context");
    render! {
        Button(
            label: "",
            accessible_label: "Toggle color theme",
            icon: lucide::SunMoon,
            on_press: move |()| {
                let next = match appearance.get_untracked() {
                    theme::Appearance::Dark => theme::Appearance::Light,
                    theme::Appearance::Light => theme::Appearance::Dark,
                };
                appearance.set(next);
                if let Err(error) = crate::storage::save_appearance(next) {
                    app.notice().set(error);
                }
            },
        )
    }
}
