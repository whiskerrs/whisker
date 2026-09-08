use super::{AppState, Conversation, Session};
use whisker::RwSignal;

impl AppState {
    pub fn sessions(&self) -> RwSignal<Vec<Session>> {
        self.0.sessions
    }
    pub fn active_id(&self) -> RwSignal<u64> {
        self.0.active
    }
    pub fn active(&self) -> Option<Session> {
        let id = self.0.active.get();
        self.0
            .sessions
            .with(|sessions| sessions.iter().find(|s| s.id == id).copied())
    }
    pub fn new_conversation(&self) {
        let id = *self.0.next_id.borrow();
        *self.0.next_id.borrow_mut() += 1;
        let session = self.0.owner.with(|| {
            Session::restore(Conversation {
                id,
                title: "New conversation".into(),
                ..Conversation::default()
            })
        });
        self.0
            .sessions
            .update(|sessions| sessions.insert(0, session));
        self.0.active.set(id);
        self.persist();
    }
    pub fn select(&self, id: u64) {
        if self.0.sessions.with_untracked(|sessions| {
            sessions
                .iter()
                .any(|s| s.id == id && !s.trashed.get_untracked())
        }) {
            self.0.active.set(id);
            self.persist();
        }
    }
    pub fn trash(&self, session: Session) {
        let generating = self
            .0
            .generation
            .borrow()
            .as_ref()
            .is_some_and(|g| g.session.id == session.id);
        if generating {
            self.stop();
        }
        session.trashed.set(true);
        if self.0.active.get_untracked() == session.id {
            let next = self.0.sessions.with_untracked(|sessions| {
                sessions
                    .iter()
                    .find(|s| !s.trashed.get_untracked())
                    .copied()
            });
            if let Some(next) = next {
                self.select(next.id);
            } else {
                self.new_conversation();
            }
        }
        self.persist();
    }
    pub fn restore_conversation(&self, session: Session) {
        session.trashed.set(false);
        self.select(session.id);
    }
    pub fn rename(&self, session: Session, title: &str) {
        let title = title.trim();
        if !title.is_empty() {
            session.title.set(title.chars().take(120).collect());
            self.persist();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AnswerStatus, Connection, Turn, controller::Generation};
    use futures_util::future::AbortHandle;
    use std::{cell::RefCell, rc::Rc};
    use whisker::{Owner, RwSignal};

    fn with_app(test: impl FnOnce(AppState)) {
        let runtime =
            whisker::runtime::RuntimeContext::new(whisker::runtime::RuntimeWakeHandle::new(|| {}));
        runtime.enter(|| {
            let owner = Owner::new(None);
            owner.with(|| test(AppState::new()));
            owner.dispose();
        });
    }

    #[test]
    fn switching_and_trashing_preserve_other_drafts_and_restore_identity() {
        with_app(|app| {
            app.new_conversation();
            let first = app.active().unwrap();
            first.draft.set("unsent".into());
            app.new_conversation();
            let second = app.active().unwrap();
            assert_ne!(first.id, second.id);
            app.select(first.id);
            assert_eq!(app.active().unwrap().draft.get_untracked(), "unsent");
            app.trash(first);
            assert_eq!(app.active().unwrap().id, second.id);
            app.restore_conversation(first);
            assert_eq!(app.active().unwrap().id, first.id);
            assert_eq!(first.draft.get_untracked(), "unsent");
        });
    }

    #[test]
    fn stopping_after_switching_updates_the_original_conversation() {
        with_app(|app| {
            app.new_conversation();
            let original = app.active().unwrap();
            let turn = RwSignal::new(Turn {
                id: 1,
                question: "Q".into(),
                answer: "A".into(),
                connection: Connection::default(),
                status: AnswerStatus::Running,
                alternatives: vec![],
            });
            original.turns.set(vec![turn]);
            let (abort, _) = AbortHandle::new_pair();
            *app.0.generation.borrow_mut() = Some(Generation {
                id: 1,
                abort,
                session: original,
                turn,
                pending: Rc::new(RefCell::new("B".into())),
            });
            app.0.busy.set(true);
            app.new_conversation();
            app.stop();
            assert_eq!(turn.get_untracked().answer, "AB");
            assert_eq!(turn.get_untracked().status, AnswerStatus::Stopped);
            assert!(app.active().unwrap().turns.with_untracked(Vec::is_empty));
        });
    }

    #[test]
    fn saved_credentials_are_never_reused_for_a_different_endpoint() {
        with_app(|app| {
            app.0.connection.set(Some(Connection::default()));
            *app.0.key.borrow_mut() = "test-key".into();
            assert_eq!(app.key_for(&Connection::default(), ""), "test-key");
            let other = Connection {
                base_url: "https://example.com".into(),
                ..Connection::default()
            };
            assert!(app.key_for(&other, "").is_empty());
        });
    }
}
