use super::{
    AnswerStatus, AppState, Connection, Conversation, Session, Turn, controller::Generation,
};
use crate::api::{ApiClient, delay};
use futures_util::future::{AbortHandle, Abortable, select};
use std::{cell::RefCell, rc::Rc, time::Duration};
use whisker::{RwSignal, spawn_local};

struct Request {
    connection: Connection,
    key: String,
    messages: Vec<crate::api::Message>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SendError {
    MissingConnection,
}

impl AppState {
    pub fn send(&self, session: Session) -> Result<(), SendError> {
        if self.0.busy.get_untracked() {
            return Ok(());
        }
        let question = session.draft.get_untracked().trim().to_owned();
        if question.is_empty() {
            return Ok(());
        }
        let connection = self.generation_connection()?;
        let id = *self.0.next_id.borrow();
        *self.0.next_id.borrow_mut() += 1;
        session.title_from_question(&question);
        let turn = self.0.owner.with(|| {
            RwSignal::new(Turn {
                id,
                alternatives: Vec::new(),
                question,
                answer: String::new(),
                connection,
                status: AnswerStatus::Running,
            })
        });
        session.turns.update(|turns| turns.push(turn));
        session.draft.set(String::new());
        self.start(session, turn);
        Ok(())
    }

    pub fn retry(&self, session: Session) -> Result<(), SendError> {
        if self.0.busy.get_untracked() {
            return Ok(());
        }
        if let Some(turn) = session.turns.with_untracked(|turns| turns.last().copied()) {
            let connection = self.generation_connection()?;
            turn.update(|turn| {
                let previous = super::Answer {
                    text: turn.answer.clone(),
                    status: turn.status.clone(),
                    connection: turn.connection.clone(),
                };
                turn.alternatives.push(previous);
                turn.answer.clear();
                turn.connection = connection;
                turn.status = AnswerStatus::Running;
            });
            self.start(session, turn);
        }
        Ok(())
    }

    fn generation_connection(&self) -> Result<Connection, SendError> {
        if !self.has_key() {
            return Err(SendError::MissingConnection);
        }
        self.0
            .connection
            .get_untracked()
            .ok_or(SendError::MissingConnection)
    }

    pub fn stop(&self) {
        if let Some(generation) = self.0.generation.borrow_mut().take() {
            generation.abort.abort();
            let remaining = std::mem::take(&mut *generation.pending.borrow_mut());
            generation.turn.update(|turn| {
                turn.answer.push_str(&remaining);
                turn.status = AnswerStatus::Stopped;
            });
        }
        self.0.busy.set(false);
        self.persist();
    }

    fn start(&self, session: Session, turn: RwSignal<Turn>) {
        let client = match self.client() {
            Ok(client) => client,
            Err(error) => {
                turn.update(|turn| turn.status = AnswerStatus::Failed(error.message()));
                self.persist();
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
            session,
            turn,
        });
        self.0.busy.set(true);
        self.persist();
        let request = Request {
            connection: turn.with_untracked(|turn| turn.connection.clone()),
            key: self.0.key.borrow().clone(),
            messages: session.turns.with_untracked(|turns| {
                Conversation::context(
                    &turns
                        .iter()
                        .map(|turn| turn.get_untracked())
                        .collect::<Vec<_>>(),
                )
            }),
        };
        let state = self.clone();
        self.0.owner.with(|| {
            spawn_local(async move {
                let _ = Abortable::new(
                    state.generate(client, id, request, turn, pending),
                    registration,
                )
                .await;
            })
        });
    }

    async fn generate(
        &self,
        client: ApiClient,
        id: u64,
        request: Request,
        turn: RwSignal<Turn>,
        pending: Rc<RefCell<String>>,
    ) {
        let buffer = pending.clone();
        let stream = Box::pin(client.stream(
            &request.connection,
            &request.key,
            request.messages,
            move |text| buffer.borrow_mut().push_str(&text),
        ));
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
