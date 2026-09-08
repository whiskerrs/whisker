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
            url::Url::parse(self.base_url.trim()).map_err(|_| "接続先URLを確認してください。")?;
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
            return Err("認証情報やクエリを含まないHTTPSのURLを指定してください。".into());
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
    pub id: u64,
    pub question: String,
    pub answer: String,
    pub connection: Connection,
    pub status: AnswerStatus,
}

impl Turn {
    pub fn status_text(&self) -> String {
        match &self.status {
            AnswerStatus::Running if self.answer.is_empty() => "応答を待っています…".into(),
            AnswerStatus::Running => "生成中…".into(),
            AnswerStatus::Complete => String::new(),
            AnswerStatus::Stopped => "生成を停止しました".into(),
            AnswerStatus::Failed(message) => message.clone(),
            AnswerStatus::Interrupted => "生成が中断されました".into(),
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct Conversation {
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
                question: "question".into(),
                answer: "partial".into(),
                connection: Connection::default(),
                status: AnswerStatus::Running,
            }],
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
