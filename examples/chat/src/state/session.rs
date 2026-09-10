//! Reactive state for one conversation, independent of the currently visible route.
use super::{Conversation, Turn};
use whisker::RwSignal;

#[derive(Clone, Copy, PartialEq)]
pub struct Session {
    pub id: u64,
    pub title: RwSignal<String>,
    pub turns: RwSignal<Vec<RwSignal<Turn>>>,
    pub draft: RwSignal<String>,
    pub trashed: RwSignal<bool>,
}

impl Session {
    pub fn restore(mut value: Conversation) -> Self {
        value.restore();
        Self {
            id: value.id,
            title: RwSignal::new(value.title),
            turns: RwSignal::new(value.turns.into_iter().map(RwSignal::new).collect()),
            draft: RwSignal::new(value.draft),
            trashed: RwSignal::new(value.trashed),
        }
    }

    pub fn snapshot(self) -> Conversation {
        Conversation {
            id: self.id,
            title: self.title.get_untracked(),
            turns: self
                .turns
                .with_untracked(|turns| turns.iter().map(|turn| turn.get_untracked()).collect()),
            draft: self.draft.get_untracked(),
            trashed: self.trashed.get_untracked(),
        }
    }

    pub fn title_from_question(self, question: &str) {
        if self.turns.with_untracked(Vec::is_empty) {
            self.title.set(
                question
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(52)
                    .collect(),
            );
        }
    }
}
