use super::{button::Button, theme};
use crate::{
    state::{AppState, Connection, Page},
    storage,
};
use whisker::prelude::*;
use whisker_input::{AutoCapitalize, Input, KeyboardType};

#[component]
pub fn connection_screen() -> Element {
    let app = use_context::<AppState>().expect("AppState context");
    let initial = app.connection().get_untracked().unwrap_or_default();
    let name = signal(initial.name);
    let base_url = signal(initial.base_url);
    let model = signal(initial.model);
    let key = signal(String::new());
    let notice = signal(String::new());
    let checking = signal(false);
    let models = signal(Vec::<String>::new());
    let remember = signal(storage::persistent_keys_available());
    let api = app.client();
    let save_app = app.clone();
    let page = app.page();
    let can_back =
        app.connection().get_untracked().is_some() || !app.turns().with_untracked(Vec::is_empty);
    render! {
        ScrollView(style: theme::fill()) {
            View(style: theme::column().padding(px(24)).flex_shrink(0.0)) {
                Text(value: "WHISKER CHAT", style: theme::muted().margin_bottom(px(20)))
                Text(
                    value: "A quiet place\nto think.",
                    style: theme::text(32.0).margin_bottom(px(16)),
                )
                Text(
                    value: "Your AI, with your own API key.\nConversations stay on this device.",
                    style: theme::muted().margin_bottom(px(28)),
                )
                View(style: theme::row().gap(px(8)).margin_bottom(px(20))) {
                    Button(
                        label: "OpenAI",
                        on_press: move |()| {
                            name.set("OpenAI".into());
                            base_url.set("https://api.openai.com/v1".into());
                            key.set(String::new());
                            model.set(String::new());
                            models.set(Vec::new());
                        },
                    )
                    Button(
                        label: "DeepSeek",
                        on_press: move |()| {
                            name.set("DeepSeek".into());
                            base_url.set("https://api.deepseek.com".into());
                            key.set(String::new());
                            model.set(String::new());
                            models.set(Vec::new());
                        },
                    )
                }
                Text(value: "Connection name", style: theme::muted().margin_bottom(px(6)))
                Input(text: name, style: theme::field())
                Text(
                    value: "API base URL",
                    style: theme::muted().margin_top(px(16)).margin_bottom(px(6)),
                )
                Input(
                    text: base_url,
                    keyboard_type: KeyboardType::Url,
                    auto_capitalize: AutoCapitalize::None,
                    autocorrect: false,
                    style: theme::field(),
                    on_input: move |_: String| {
                        key.set(String::new());
                        models.set(Vec::new());
                        notice.set(String::new());
                    },
                )
                Text(
                    value: "API key",
                    style: theme::muted().margin_top(px(16)).margin_bottom(px(6)),
                )
                Input(
                    text: key,
                    secure: true,
                    auto_capitalize: AutoCapitalize::None,
                    autocorrect: false,
                    placeholder: "Enter your API key",
                    style: theme::field(),
                )
                View(style: theme::column().margin_top(px(12))) {
                    Button(
                        label: computed(move || {
                            if checking.get() {
                                "Checking…".into()
                            } else {
                                "Fetch models".into()
                            }
                        }),
                        on_press: move |()| {
                            if checking.get_untracked() {
                                return;
                            }
                            let mut connection = Connection {
                                name: name.get_untracked(),
                                base_url: base_url.get_untracked(),
                                model: model.get_untracked(),
                            };
                            if let Err(error) = connection.validate() {
                                notice.set(error);
                                return;
                            }
                            let secret = key.get_untracked().trim().to_owned();
                            if secret.is_empty() {
                                notice.set("Enter your API key.".into());
                                return;
                            }
                            let client = match api.clone() {
                                Ok(client) => client,
                                Err(error) => {
                                    notice.set(error.message());
                                    return;
                                }
                            };
                            let requested_url = base_url.get_untracked();
                            let requested_key = key.get_untracked();
                            checking.set(true);
                            notice.set(String::new());
                            spawn_local(async move {
                                let result = client.models(&connection, &secret).await;
                                checking.set(false);
                                if base_url.get_untracked() != requested_url || key.get_untracked() != requested_key {
                                    return;
                                }
                                match result {
                                    Ok(ids) => {
                                        notice.set(format!(
                                            "Models found: {}. Select a model or enter its ID.",
                                            ids.len()
                                        ));
                                        models.set(ids);
                                    }
                                    Err(error) => notice.set(error.message()),
                                }
                            });
                        },
                    )
                }
                Text(
                    value: "Model ID",
                    style: theme::muted().margin_top(px(16)).margin_bottom(px(6)),
                )
                Input(
                    text: model,
                    placeholder: "Enter a model ID",
                    auto_capitalize: AutoCapitalize::None,
                    autocorrect: false,
                    style: theme::field(),
                )
                Show(when: move || !models.with(Vec::is_empty)) {
                    List(
                        each: move || models.get(),
                        key: |id: &String| id.clone(),
                        children: move |id: ReadSignal<String>| render! {
                            Text(
                                value: id,
                                on_tap: move |_| model.set(id.get_untracked()),
                                style: theme::text(14.0).padding(px(10)),
                            )
                        },
                        style: theme::column()
                            .height(px(160))
                            .flex_shrink(0.0)
                            .margin_top(px(8)),
                    )
                }
                Show(when: storage::persistent_keys_available) {
                    View(style: theme::column().margin_top(px(16))) {
                        Button(
                            label: computed(move || {
                                if remember.get() {
                                    "✓ Store key securely".into()
                                } else {
                                    "Use for this session only".into()
                                }
                            }),
                            on_press: move |()| remember.update(|value| *value = !*value),
                        )
                    }
                }
                Show(when: || !storage::persistent_keys_available()) {
                    Text(
                        value: "Your API key is kept for this session only. Enter it again after restarting.",
                        style: theme::muted().margin_top(px(12)),
                    )
                }
                Text(value: notice, style: theme::muted().margin_top(px(12)))
                View(style: theme::column().margin_top(px(20)).gap(px(12))) {
                    Button(
                        label: "Start chatting",
                        primary: true,
                        on_press: move |()| {
                            let connection = Connection {
                                name: name.get_untracked(),
                                base_url: base_url.get_untracked(),
                                model: model.get_untracked(),
                            };
                            if let Err(error) =
                                save_app.configure(connection, key.get_untracked(), remember.get_untracked())
                            {
                                notice.set(error);
                            }
                        },
                    )
                    Show(when: move || can_back) {
                        Button(label: "Back to chat", on_press: move |()| page.set(Page::Chat))
                    }
                }
                Text(
                    value: "API usage is billed to your provider account.\nFetching models does not generate a response.",
                    style: theme::muted().margin_top(px(20)),
                )
            }
        }
    }
}
