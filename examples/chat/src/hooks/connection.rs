//! Connection editing and model discovery share one screen-owned lifecycle.
use crate::{
    state::{AppState, Connection},
    storage,
};
use whisker::prelude::*;

#[derive(Clone, Copy)]
pub struct ConnectionForm {
    pub name: RwSignal<String>,
    pub base_url: RwSignal<String>,
    pub model: RwSignal<String>,
    pub key: RwSignal<String>,
    pub notice: RwSignal<String>,
    pub checking: RwSignal<bool>,
    pub models: RwSignal<Vec<String>>,
    pub remember: RwSignal<bool>,
    pub discover: Callback,
    pub save: Callback,
}

pub fn use_connection_form(saved: Callback) -> ConnectionForm {
    let app = use_context::<AppState>().expect("AppState context");
    let initial = app.connection().get_untracked().unwrap_or_default();
    let saved_key = storage::load_key(&initial);
    let remember =
        storage::secure_keys_available() || saved_key.as_ref().is_ok_and(|key| key.is_some());
    let mut form = ConnectionForm {
        name: signal(initial.name),
        base_url: signal(initial.base_url),
        model: signal(initial.model),
        key: signal(String::new()),
        notice: signal(saved_key.err().unwrap_or_default()),
        checking: signal(false),
        models: signal(Vec::new()),
        remember: signal(remember),
        discover: Callback::new(|()| {}),
        save: Callback::new(|()| {}),
    };
    let discover_app = app.clone();
    form.discover = Callback::new(move |()| {
        if form.checking.get_untracked() {
            return;
        }
        let mut connection = form.value();
        if let Err(error) = connection.validate() {
            form.notice.set(error);
            return;
        }
        let secret = discover_app.key_for(&connection, &form.key.get_untracked());
        if secret.is_empty() {
            form.notice.set("Enter your API key.".into());
            return;
        }
        let client = match discover_app.client() {
            Ok(client) => client,
            Err(error) => {
                form.notice.set(error.message());
                return;
            }
        };
        let requested_url = form.base_url.get_untracked();
        let requested_key = form.key.get_untracked();
        form.checking.set(true);
        form.notice.set(String::new());
        spawn_local(async move {
            let result = client.models(&connection, &secret).await;
            form.checking.set(false);
            if form.base_url.get_untracked() != requested_url
                || form.key.get_untracked() != requested_key
            {
                return;
            }
            match result {
                Ok(ids) => {
                    if form.model.with_untracked(String::is_empty)
                        && let Some(id) = ids.first()
                    {
                        form.model.set(id.clone());
                    }
                    form.notice
                        .set(format!("Connected. {} models available.", ids.len()));
                    form.models.set(ids);
                }
                Err(error) => form.notice.set(error.message()),
            }
        });
    });
    form.save = Callback::new(move |()| {
        match app.configure(
            form.value(),
            form.key.get_untracked(),
            form.remember.get_untracked(),
        ) {
            Ok(()) => saved.call(),
            Err(error) => form.notice.set(error),
        }
    });
    form
}

impl ConnectionForm {
    pub fn value(self) -> Connection {
        Connection {
            name: self.name.get_untracked(),
            base_url: self.base_url.get_untracked(),
            model: self.model.get_untracked(),
        }
    }
    pub fn endpoint_changed(self) {
        self.remember.set(storage::secure_keys_available());
        self.key.set(String::new());
        self.models.set(Vec::new());
        self.notice.set(String::new());
    }
    pub fn preset(self, deepseek: bool) {
        self.name
            .set(if deepseek { "DeepSeek" } else { "OpenAI" }.into());
        self.base_url.set(
            if deepseek {
                "https://api.deepseek.com"
            } else {
                "https://api.openai.com/v1"
            }
            .into(),
        );
        self.model.set(String::new());
        self.endpoint_changed();
    }
}
