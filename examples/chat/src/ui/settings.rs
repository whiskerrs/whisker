use super::{
    appearance::AppearancePicker,
    button::Button,
    navigation,
    settings_shell::{self, SettingsSection, SettingsShell},
    theme,
};
use crate::state::AppState;
use whisker::css::FlexWrap;
use whisker::prelude::*;
use whisker_router::use_navigator;

#[component]
pub fn settings_screen() -> Element {
    let app = use_context::<AppState>().expect("AppState context");
    let connection = app.connection();
    let notice = app.notice();
    let nav = use_navigator();
    let edit_connection = Callback::new(move |()| navigation::open_connection(&nav, notice));
    render! {
        SettingsShell(section: SettingsSection::Appearance) {
            View(style: theme::column().gap(px(6))) {
                Text(value: "Appearance", style: theme::style(settings_shell::heading))
                Text(
                    value: "Personalize your workspace.",
                    style: theme::style(settings_shell::description),
                )
            }
            View(style: theme::style(setting_row)) {
                View(
                    style: theme::column()
                        .flex_grow(1.0)
                        .flex_basis(px(240))
                        .gap(px(4)),
                ) {
                    Text(value: "Color theme", style: theme::style(settings_shell::label))
                    Text(
                        value: "Saved on this device.",
                        style: theme::style(settings_shell::description),
                    )
                }
                AppearancePicker()
            }
            View(style: theme::column().gap(px(16))) {
                Text(value: "Connection", style: theme::style(settings_shell::label))
                View(style: theme::style(setting_row)) {
                    View(
                        style: theme::column()
                            .flex_grow(1.0)
                            .flex_basis(px(240))
                            .min_width(px(0))
                            .gap(px(4)),
                    ) {
                        Text(
                            value: computed(move || {
                                connection
                                    .get()
                                    .map(|c| c.name)
                                    .unwrap_or_else(|| "No provider connected".into())
                            }),
                            style: theme::style(settings_shell::label),
                        )
                        Text(
                            value: computed(move || {
                                connection
                                    .get()
                                    .map(|c| c.model)
                                    .unwrap_or_else(|| "Add an API key to start chatting.".into())
                            }),
                            style: theme::style(settings_shell::description),
                        )
                    }
                    Button(
                        label: "API key & provider",
                        compact: true,
                        on_press: edit_connection,
                    )
                }
            }
        }
    }
}

fn setting_row(palette: theme::Palette) -> Css {
    theme::row()
        .flex_wrap(FlexWrap::Wrap)
        .gap(px(16))
        .padding_bottom(px(24))
        .border_bottom_style(whisker::css::BorderStyle::Solid)
        .border_bottom_width(px(1))
        .border_bottom_color(Color::hex(palette.border))
}
