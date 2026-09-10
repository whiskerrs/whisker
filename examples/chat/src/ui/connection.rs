use super::{
    button::Button,
    key_storage_choice::KeyStorageChoice,
    navigation,
    settings_shell::{self, SettingsSection, SettingsShell},
    theme::{self, size, space},
};
use crate::{hooks::use_connection_form, state::AppState, storage};
use whisker::prelude::*;
use whisker_input::{AutoCapitalize, Input, KeyboardType};
use whisker_router::{use_navigator, use_pathname};

#[component]
pub fn connection_screen() -> Element {
    let app = use_context::<AppState>().expect("AppState context");
    let nav = use_navigator();
    let notice = app.notice();
    let setup = use_pathname().get_untracked() == navigation::SETUP;
    let form = use_connection_form(Callback::new(move |()| {
        if setup {
            navigation::complete_setup(&nav, notice);
        } else {
            navigation::return_to_settings(&nav, notice);
        }
    }));
    render! {
        SettingsShell(section: SettingsSection::Connection, setup: setup) {
            View(style: theme::column().gap(px(16))) {
                Text(
                    value: "API key & provider",
                    style: theme::style(settings_shell::heading),
                )
                Text(
                    value: if setup {
                        "Connect a provider to start chatting."
                    } else {
                        "Manage your provider, API key, and model."
                    },
                    style: theme::style(settings_shell::description),
                )
                View(style: theme::row().gap(px(space::SM))) {
                    Button(compact: true, label: "OpenAI", on_press: move |()| form.preset(false))
                    Button(compact: true, label: "DeepSeek", on_press: move |()| form.preset(true))
                }
                View(style: theme::column().gap(px(8))) {
                    Text(value: "Name", style: theme::style(settings_shell::description))
                    Input(text: form.name, style: theme::style(settings_shell::field))
                    Text(value: "API base URL", style: theme::style(settings_shell::description))
                    Input(
                        text: form.base_url,
                        keyboard_type: KeyboardType::Url,
                        auto_capitalize: AutoCapitalize::None,
                        autocorrect: false,
                        style: theme::style(settings_shell::field),
                        on_input: move |_: String| form.endpoint_changed(),
                    )
                    Text(value: "API key", style: theme::style(settings_shell::description))
                    Input(
                        text: form.key,
                        secure: true,
                        auto_capitalize: AutoCapitalize::None,
                        autocorrect: false,
                        placeholder: if setup {
                            "Enter your API key"
                        } else {
                            "Enter a key, or keep your saved key"
                        },
                        style: theme::style(settings_shell::field),
                    )
                    View(style: theme::row()) {
                        Button(
                            compact: true,
                            label: computed(move || {
                                if form.checking.get() {
                                    "Connecting…".into()
                                } else {
                                    "Test connection & find models".into()
                                }
                            }),
                            disabled: form.checking,
                            on_press: form.discover,
                        )
                    }
                    Text(value: "Model", style: theme::style(settings_shell::description))
                    Input(
                        text: form.model,
                        placeholder: "Model ID",
                        auto_capitalize: AutoCapitalize::None,
                        autocorrect: false,
                        style: theme::style(settings_shell::field),
                    )
                    Show(when: move || !form.models.with(Vec::is_empty)) {
                        List(
                            each: move || form.models.get(),
                            key: |id: &String| id.clone(),
                            children: move |id: ReadSignal<String>| render! {
                                Button(
                                    compact: true,
                                    label: id,
                                    on_press: move |()| form.model.set(id.get_untracked()),
                                )
                            },
                            content_style: theme::column().row_gap(px(space::SM)),
                            style: theme::column().height(px(132)).flex_shrink(0.0),
                        )
                    }
                    Show(when: storage::secure_keys_available) {
                        Button(
                            compact: true,
                            label: computed(move || {
                                if form.remember.get() {
                                    "✓ Remember key securely".into()
                                } else {
                                    "Session only".into()
                                }
                            }),
                            on_press: move |()| form.remember.update(|remember| *remember = !*remember),
                        )
                    }
                    Show(when: || cfg!(target_arch = "wasm32")) {
                        KeyStorageChoice(remember: form.remember)
                    }
                    Show(
                        when: || !storage::secure_keys_available() && !cfg!(target_arch = "wasm32"),
                    ) {
                        Text(
                            value: "Session-only key. Re-enter it after restarting.",
                            style: theme::style(settings_shell::description),
                        )
                    }
                    Show(when: move || !form.notice.with(String::is_empty)) {
                        Text(
                            value: form.notice,
                            style: theme::style(move |palette| palette.text(size::LABEL).color(Color::hex(palette.accent))),
                        )
                    }
                    View(style: theme::row().padding_top(px(8))) {
                        Button(
                            compact: true,
                            label: if setup { "Start chatting" } else { "Save connection" },
                            primary: true,
                            on_press: form.save,
                        )
                    }
                }
                Text(
                    value: "Usage is billed by your provider. Conversations are saved locally on this device.",
                    style: theme::style(settings_shell::description),
                )
            }
        }
    }
}
