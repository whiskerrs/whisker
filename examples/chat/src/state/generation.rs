use super::{AnswerStatus, AppState, Conversation, Page, Turn, controller::Generation};
use crate::api::{ApiClient, delay};
use futures_util::future::{AbortHandle, Abortable, select};
use std::{cell::RefCell, rc::Rc, time::Duration};
use whisker::{RwSignal, spawn_local};

impl AppState {
    pub fn send(&self) {
        if self.0.busy.get_untracked() {
            return;
        }
        let question = self.0.draft.get_untracked().trim().to_owned();
        if question.is_empty() {
            return;
        }
        let Some(connection) = self.0.connection.get_untracked() else {
            self.0.page.set(Page::Connection);
            return;
        };
        if !self.has_key() {
            self.0.page.set(Page::Connection);
            return;
        }
        let id = *self.0.next_id.borrow();
        *self.0.next_id.borrow_mut() += 1;
        let turn = self.0.owner.with(|| {
            RwSignal::new(Turn {
                id,
                question,
                answer: String::new(),
                connection,
                status: AnswerStatus::Running,
            })
        });
        self.0.turns.update(|turns| turns.push(turn));
        self.0.draft.set(String::new());
        self.start(turn);
    }

    pub fn retry(&self) {
        if self.0.busy.get_untracked() || !self.has_key() {
            return;
        }
        if let Some(turn) = self.0.turns.with_untracked(|turns| turns.last().copied()) {
            let Some(connection) = self.0.connection.get_untracked() else {
                return;
            };
            turn.update(|turn| {
                turn.answer.clear();
                turn.connection = connection;
                turn.status = AnswerStatus::Running;
            });
            self.start(turn);
        }
    }

    pub fn stop(&self) {
        if let Some(generation) = self.0.generation.borrow_mut().take() {
            generation.abort.abort();
            if let Some(turn) = self.0.turns.with_untracked(|turns| turns.last().copied()) {
                let remaining = std::mem::take(&mut *generation.pending.borrow_mut());
                turn.update(|turn| {
                    turn.answer.push_str(&remaining);
                    turn.status = AnswerStatus::Stopped;
                });
            }
        }
        self.0.busy.set(false);
        self.persist();
    }

    fn start(&self, turn: RwSignal<Turn>) {
        let client = match self.client() {
            Ok(client) => client,
            Err(error) => {
                turn.update(|turn| turn.status = AnswerStatus::Failed(error.message()));
                return;
            }
        };
        let id = *self.0.next_id.borrow();
        *self.0.next_id.borrow_mut() += 1;
        let (abort, registration) = AbortHandle::new_pair();
        let pending = Rc::new(RefCell::new(String::new()));
        *self.0.generation.borrow_mut() = Some(Generation {
            id,
            abort,
            pending: pending.clone(),
        });
        self.0.busy.set(true);
        self.persist();
        let state = self.clone();
        self.0.owner.with(|| {
            spawn_local(async move {
                let _ =
                    Abortable::new(state.generate(client, id, turn, pending), registration).await;
            })
        });
    }

    async fn generate(
        &self,
        client: ApiClient,
        id: u64,
        turn: RwSignal<Turn>,
        pending: Rc<RefCell<String>>,
    ) {
        let messages = self.0.turns.with_untracked(|turns| {
            Conversation::context(
                &turns
                    .iter()
                    .map(|turn| turn.get_untracked())
                    .collect::<Vec<_>>(),
            )
        });
        let connection = turn.with_untracked(|turn| turn.connection.clone());
        let key = self.0.key.borrow().clone();
        let buffer = pending.clone();
        let stream = Box::pin(client.stream(&connection, &key, messages, move |text| {
            buffer.borrow_mut().push_str(&text)
        }));
        let refresh = Box::pin(async {
            let mut frames = 0;
            loop {
                delay(Duration::from_millis(32)).await;
                self.flush_text(id, turn, &pending);
                frames += 1;
                if frames % 32 == 0 {
                    self.persist();
                }
            }
        });
        let result = match select(stream, refresh).await {
            futures_util::future::Either::Left((result, _)) => result,
            futures_util::future::Either::Right(_) => unreachable!(),
        };
        if !self.is_current(id) {
            return;
        }
        self.flush_text(id, turn, &pending);
        turn.update(|turn| {
            turn.status = match result {
                Ok(()) => AnswerStatus::Complete,
                Err(error) => AnswerStatus::Failed(error.message()),
            }
        });
        self.0.generation.borrow_mut().take();
        self.0.busy.set(false);
        self.persist();
    }

    fn is_current(&self, id: u64) -> bool {
        self.0
            .generation
            .borrow()
            .as_ref()
            .is_some_and(|generation| generation.id == id)
    }

    fn flush_text(&self, id: u64, turn: RwSignal<Turn>, pending: &RefCell<String>) {
        if !self.is_current(id) || pending.borrow().is_empty() {
            return;
        }
        let text = std::mem::take(&mut *pending.borrow_mut());
        turn.update(|turn| turn.answer.push_str(&text));
        self.0.revision.update(|revision| *revision += 1);
    }
}
