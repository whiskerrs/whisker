use super::{
    appearance::AppearanceToggle,
    button::Button,
    navigation,
    theme::{self, size, space},
};
use crate::{hooks::use_connection_form, state::AppState, storage};
use whisker::css::AlignSelf;
use whisker::prelude::*;
use whisker_input::{AutoCapitalize, Input, KeyboardType};
use whisker_router::use_navigator;

#[component]
pub fn connection_screen() -> Element {
    let app = use_context::<AppState>().expect("AppState context");
    let nav = use_navigator();
    let back_nav = nav.clone();
    let notice = app.notice();
    let form = use_connection_form(Callback::new(move |()| {
        navigation::return_to_chat(&nav, notice)
    }));
    let can_back = app.connection().get_untracked().is_some();
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
                View(style: theme::row().justify_content(JustifyContent::SpaceBetween)) {
                    Text(
                        value: "WHISKER CHAT / SETTINGS",
                        style: theme::style(move |palette| palette.muted()),
                    )
                    AppearanceToggle()
                }
                Text(
                    value: "Connect your model",
                    style: theme::style(move |palette| palette.display()),
                )
                Text(
                    value: "Connect a provider to start a conversation.\nYour key stays on this device.",
                    style: theme::style(move |palette| palette.muted()),
                )
                View(style: theme::row().gap(px(space::SM))) {
                    Button(label: "OpenAI", on_press: move |()| form.preset(false))
                    Button(label: "DeepSeek", on_press: move |()| form.preset(true))
                }
                View(style: theme::style(move |palette| palette.card())) {
                    Text(value: "Connection", style: theme::style(move |palette| palette.title()))
                    Text(value: "Name", style: theme::style(move |palette| palette.muted()))
                    Input(text: form.name, style: theme::style(move |palette| palette.field()))
                    Text(value: "API base URL", style: theme::style(move |palette| palette.muted()))
                    Input(
                        text: form.base_url,
                        keyboard_type: KeyboardType::Url,
                        auto_capitalize: AutoCapitalize::None,
                        autocorrect: false,
                        style: theme::style(move |palette| palette.field()),
                        on_input: move |_: String| form.endpoint_changed(),
                    )
                    Text(value: "API key", style: theme::style(move |palette| palette.muted()))
                    Input(
                        text: form.key,
                        secure: true,
                        auto_capitalize: AutoCapitalize::None,
                        autocorrect: false,
                        placeholder: "Enter a key, or keep your saved key",
                        style: theme::style(move |palette| palette.field()),
                    )
                    Button(
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
                    Text(value: "Model", style: theme::style(move |palette| palette.muted()))
                    Input(
                        text: form.model,
                        placeholder: "Model ID",
                        auto_capitalize: AutoCapitalize::None,
                        autocorrect: false,
                        style: theme::style(move |palette| palette.field()),
                    )
                    Show(when: move || !form.models.with(Vec::is_empty)) {
                        List(
                            each: move || form.models.get(),
                            key: |id: &String| id.clone(),
                            children: move |id: ReadSignal<String>| render! {
                                Button(
                                    label: id,
                                    on_press: move |()| form.model.set(id.get_untracked()),
                                )
                            },
                            style: theme::column().height(px(132)).flex_shrink(0.0),
                        )
                    }
                    Show(when: storage::persistent_keys_available) {
                        Button(
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
                    Show(when: || !storage::persistent_keys_available()) {
                        Text(
                            value: "Session-only key. Re-enter it after restarting.",
                            style: theme::style(move |palette| palette.muted()),
                        )
                    }
                    Show(when: move || !form.notice.with(String::is_empty)) {
                        Text(
                            value: form.notice,
                            style: theme::style(move |palette| palette.text(size::LABEL).color(Color::hex(palette.accent))),
                        )
                    }
                    Button(
                        label: if can_back { "Save connection" } else { "Start chatting" },
                        primary: true,
                        on_press: form.save,
                    )
                }
                Show(when: move || can_back) {
                    Button(
                        label: "Back to chat",
                        on_press: {
                            let nav = back_nav.clone();
                            move |()| navigation::return_to_chat(&nav, notice)
                        },
                    )
                }
                Text(
                    value: "Usage is billed by your provider. Conversations are saved locally on this device.",
                    style: theme::style(move |palette| palette.muted()),
                )
            }
        }
    }
}
