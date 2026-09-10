use crate::api::Message;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Connection {
    pub name: String,
    pub base_url: String,
    pub model: String,
}

impl Connection {
    pub fn validate(&mut self) -> Result<(), String> {
        let url =
            url::Url::parse(self.base_url.trim()).map_err(|_| "Enter a valid API base URL.")?;
        let loopback = cfg!(debug_assertions)
            && url.scheme() == "http"
            && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if (url.scheme() != "https" && !loopback)
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(
                "Use an HTTPS URL without credentials, query parameters, or a fragment.".into(),
            );
        }
        self.base_url = url.as_str().trim_end_matches('/').to_owned();
        self.name = self.name.trim().to_owned();
        self.model = self.model.trim().to_owned();
        if self.name.is_empty() {
            self.name = url.host_str().unwrap_or("Custom").to_owned();
        }
        Ok(())
    }

    pub fn endpoint(&self, path: &str) -> String {
        format!("{}/{path}", self.base_url)
    }
}

impl Default for Connection {
    fn default() -> Self {
        Self {
            name: "OpenAI".into(),
            base_url: "https://api.openai.com/v1".into(),
            model: String::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AnswerStatus {
    Running,
    Complete,
    Stopped,
    Failed(String),
    Interrupted,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Turn {
    #[serde(default)]
    pub alternatives: Vec<Answer>,
    pub id: u64,
    pub question: String,
    pub answer: String,
    pub connection: Connection,
    pub status: AnswerStatus,
}

impl AnswerStatus {
    pub fn label(&self, empty: bool) -> String {
        match self {
            AnswerStatus::Running if empty => "Waiting for a response…".into(),
            AnswerStatus::Running => "Generating…".into(),
            AnswerStatus::Complete => String::new(),
            AnswerStatus::Stopped => "Generation stopped".into(),
            AnswerStatus::Failed(message) => message.clone(),
            AnswerStatus::Interrupted => "Generation interrupted".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Answer {
    pub text: String,
    pub status: AnswerStatus,
    pub connection: Connection,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Conversation {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub trashed: bool,
    pub turns: Vec<Turn>,
    pub draft: String,
}

impl Conversation {
    pub fn restore(&mut self) {
        for turn in &mut self.turns {
            if turn.status == AnswerStatus::Running {
                turn.status = AnswerStatus::Interrupted;
            }
        }
    }

    pub fn context(turns: &[Turn]) -> Vec<Message> {
        turns
            .iter()
            .flat_map(|turn| {
                let mut messages = vec![Message {
                    role: "user",
                    content: turn.question.clone(),
                }];
                if turn.status == AnswerStatus::Complete {
                    messages.push(Message {
                        role: "assistant",
                        content: turn.answer.clone(),
                    });
                }
                messages
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_credential_bearing_urls_and_retains_custom_paths() {
        for url in [
            "https://user:password@example.com",
            "https://example.com?key=secret",
            "https://example.com#fragment",
            "http://example.com",
        ] {
            let mut connection = Connection {
                base_url: url.into(),
                ..Connection::default()
            };
            assert!(connection.validate().is_err());
        }
        let mut connection = Connection {
            base_url: "https://example.com/tenant/v1/".into(),
            ..Connection::default()
        };
        connection.validate().unwrap();
        assert_eq!(
            connection.endpoint("models"),
            "https://example.com/tenant/v1/models"
        );
    }

    #[test]
    fn restore_interrupts_generation_without_losing_text_or_draft() {
        let mut conversation = Conversation {
            draft: "draft".into(),
            turns: vec![Turn {
                id: 1,
                alternatives: Vec::new(),
                question: "question".into(),
                answer: "partial".into(),
                connection: Connection::default(),
                status: AnswerStatus::Running,
            }],
            ..Conversation::default()
        };
        conversation.restore();
        assert_eq!(conversation.turns[0].status, AnswerStatus::Interrupted);
        assert_eq!(conversation.turns[0].answer, "partial");
        assert_eq!(conversation.draft, "draft");
        assert_eq!(Conversation::context(&conversation.turns).len(), 1);
        conversation.turns[0].status = AnswerStatus::Complete;
        assert_eq!(
            Conversation::context(&conversation.turns)[1].content,
            "partial"
        );
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct Library {
    pub active: u64,
    pub conversations: Vec<Conversation>,
}

impl Library {
    pub fn normalize(&mut self) {
        let mut next = self.conversations.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        let mut used = std::collections::HashSet::new();
        for conversation in &mut self.conversations {
            if conversation.id == 0 || !used.insert(conversation.id) {
                conversation.id = next;
                used.insert(next);
                next += 1;
            }
            conversation.restore();
            if conversation.title.is_empty() {
                conversation.title = conversation
                    .turns
                    .first()
                    .map(|t| t.question.chars().take(52).collect())
                    .unwrap_or_else(|| "New conversation".into());
            }
        }
        if !self
            .conversations
            .iter()
            .any(|c| c.id == self.active && !c.trashed)
        {
            self.active = self
                .conversations
                .iter()
                .find(|c| !c.trashed)
                .map(|c| c.id)
                .unwrap_or(0);
        }
    }
}

#[cfg(test)]
mod library_tests {
    use super::*;
    #[test]
    fn migrates_old_conversation_without_losing_content() {
        let legacy = r#"{"draft":"keep this","turns":[{"id":1,"question":"First question","answer":"partial","connection":{"name":"Local","base_url":"https://example.com","model":"test"},"status":"Running"}]}"#;
        let conversation: Conversation = serde_json::from_str(legacy).unwrap();
        let mut library = Library {
            active: 0,
            conversations: vec![conversation],
        };
        library.normalize();
        let c = &library.conversations[0];
        assert_eq!(library.active, c.id);
        assert_ne!(c.id, 0);
        assert_eq!(c.title, "First question");
        assert_eq!(c.draft, "keep this");
        assert_eq!(c.turns[0].status, AnswerStatus::Interrupted);
        assert!(c.turns[0].alternatives.is_empty());
    }
}
