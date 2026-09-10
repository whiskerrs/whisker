use super::{Connection, Library, Session};
use crate::{api::ApiClient, storage};
use futures_util::future::AbortHandle;
use std::{cell::RefCell, rc::Rc};
use whisker::{Owner, RwSignal, on_cleanup};

pub(super) struct Generation {
    pub id: u64,
    pub abort: AbortHandle,
    pub pending: Rc<RefCell<String>>,
    pub session: Session,
    pub turn: RwSignal<super::Turn>,
}

pub(super) struct Inner {
    pub owner: Owner,
    pub connection: RwSignal<Option<Connection>>,
    pub sessions: RwSignal<Vec<Session>>,
    pub active: RwSignal<u64>,
    pub notice: RwSignal<String>,
    pub busy: RwSignal<bool>,
    pub revision: RwSignal<u64>,
    pub key: RefCell<String>,
    pub generation: RefCell<Option<Generation>>,
    pub next_id: RefCell<u64>,
    pub client: Result<ApiClient, crate::api::ApiError>,
}

#[derive(Clone)]
pub struct AppState(pub(super) Rc<Inner>);

impl AppState {
    pub fn new() -> Self {
        let state = Self(Rc::new(Inner {
            owner: Owner::current().expect("AppState requires the app Owner"),
            connection: RwSignal::new(None),
            sessions: RwSignal::new(Vec::new()),
            active: RwSignal::new(0),
            notice: RwSignal::new(String::new()),
            busy: RwSignal::new(false),
            revision: RwSignal::new(0),
            key: RefCell::new(String::new()),
            generation: RefCell::new(None),
            next_id: RefCell::new(1),
            client: ApiClient::new(),
        }));
        let weak = Rc::downgrade(&state.0);
        on_cleanup(move || {
            if let Some(inner) = weak.upgrade()
                && let Some(generation) = inner.generation.borrow_mut().take()
            {
                generation.abort.abort();
            }
        });
        state
    }

    pub fn connection(&self) -> RwSignal<Option<Connection>> {
        self.0.connection
    }
    pub fn notice(&self) -> RwSignal<String> {
        self.0.notice
    }
    pub fn busy(&self) -> RwSignal<bool> {
        self.0.busy
    }
    pub fn revision(&self) -> RwSignal<u64> {
        self.0.revision
    }
    pub fn client(&self) -> Result<ApiClient, crate::api::ApiError> {
        self.0.client.clone()
    }
    pub fn ready(&self) -> bool {
        self.0.connection.get().is_some() && self.has_key()
    }

    pub fn has_key(&self) -> bool {
        !self.0.key.borrow().is_empty()
    }

    pub fn restore(&self) -> Result<(), String> {
        self.0.notice.set(String::new());
        let connection = storage::load_connection()?;
        let mut library = storage::load_library()?;
        library.normalize();
        *self.0.next_id.borrow_mut() = library
            .conversations
            .iter()
            .flat_map(|c| std::iter::once(c.id).chain(c.turns.iter().map(|t| t.id)))
            .max()
            .unwrap_or(0)
            + 1;
        self.0.owner.with(|| {
            self.0.sessions.set(
                library
                    .conversations
                    .into_iter()
                    .map(Session::restore)
                    .collect(),
            );
        });
        self.0.active.set(library.active);
        if library.active == 0 {
            self.new_conversation();
        }
        match connection.as_ref().map(storage::load_key).transpose() {
            Ok(key) => *self.0.key.borrow_mut() = key.flatten().unwrap_or_default(),
            Err(error) => self.0.notice.set(error),
        }
        self.0.connection.set(connection);
        Ok(())
    }

    pub fn configure(
        &self,
        mut connection: Connection,
        key: String,
        remember: bool,
    ) -> Result<(), String> {
        connection.validate()?;
        if connection.model.is_empty() {
            return Err("Enter a model ID.".into());
        }
        let key = self.key_for(&connection, &key);
        if key.is_empty() {
            return Err("Enter your API key.".into());
        }
        storage::save_key(&connection, &key, remember)?;
        storage::save_connection(&connection)?;
        *self.0.key.borrow_mut() = key;
        self.0.connection.set(Some(connection));
        self.0.notice.set(String::new());
        Ok(())
    }

    pub fn key_for(&self, connection: &Connection, entered: &str) -> String {
        if !entered.trim().is_empty() {
            return entered.trim().to_owned();
        }
        if self.0.connection.with_untracked(|current| {
            current
                .as_ref()
                .is_some_and(|c| c.base_url == connection.base_url)
        }) {
            return self.0.key.borrow().clone();
        }
        String::new()
    }

    pub fn persist(&self) {
        let library = Library {
            active: self.0.active.get_untracked(),
            conversations: self.0.sessions.with_untracked(|sessions| {
                sessions.iter().map(|session| session.snapshot()).collect()
            }),
        };
        if let Err(error) = storage::save_library(&library) {
            self.0.notice.set(error);
        }
    }
}
