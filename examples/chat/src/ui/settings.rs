use super::{
    appearance::AppearancePicker,
    button::Button,
    navigation,
    theme::{self, size, space},
};
use crate::state::AppState;
use whisker::css::AlignSelf;
use whisker::prelude::*;
use whisker_icons::lucide;
use whisker_router::use_navigator;

#[component]
pub fn settings_screen() -> Element {
    let app = use_context::<AppState>().expect("AppState context");
    let connection = app.connection();
    let notice = app.notice();
    let nav = use_navigator();
    let back_nav = nav.clone();
    render! {
        ScrollView(style: theme::style(move |palette| palette.screen())) {
            View(
                style: theme::column()
                    .width(percent(100))
                    .max_width(px(size::FORM))
                    .align_self(AlignSelf::Center)
                    .padding(px(space::XL))
                    .gap(px(space::XL))
                    .flex_shrink(0.0),
            ) {
                View(style: theme::row()) {
                    Button(
                        label: "Back to chat",
                        icon: lucide::ArrowLeft,
                        on_press: move |()| navigation::return_to_chat(&back_nav, notice),
                    )
                }
                Text(value: "Settings", style: theme::style(move |palette| palette.display()))
                View(style: theme::style(move |palette| palette.card())) {
                    Text(value: "Color theme", style: theme::style(move |palette| palette.title()))
                    Text(
                        value: "Choose how Whisker Chat looks on this device.",
                        style: theme::style(move |palette| palette.muted()),
                    )
                    AppearancePicker()
                }
                View(style: theme::style(move |palette| palette.card())) {
                    Text(
                        value: "API connection",
                        style: theme::style(move |palette| palette.title()),
                    )
                    Text(
                        value: computed(move || {
                            connection
                                .get()
                                .map(|c| format!("{} · {}", c.name, c.model))
                                .unwrap_or_else(|| "No provider connected".into())
                        }),
                        style: theme::style(move |palette| palette.muted()),
                    )
                    Button(
                        label: "API key & provider",
                        icon: lucide::KeyRound,
                        on_press: move |()| navigation::open_connection(&nav, notice),
                    )
                }
            }
        }
    }
}
