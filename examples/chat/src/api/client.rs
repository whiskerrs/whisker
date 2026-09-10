use super::{ApiError, Message, deadline, sse::EventDecoder};
use crate::state::Connection;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone)]
pub struct ApiClient(reqwest::Client);

impl ApiClient {
    pub fn new() -> Result<Self, ApiError> {
        let builder = reqwest::Client::builder();
        #[cfg(not(target_arch = "wasm32"))]
        let builder = builder.redirect(reqwest::redirect::Policy::none());
        builder.build().map(Self).map_err(|_| ApiError::Network)
    }

    pub async fn models(
        &self,
        connection: &Connection,
        key: &str,
    ) -> Result<Vec<String>, ApiError> {
        let response = deadline(
            self.0
                .get(connection.endpoint("models"))
                .bearer_auth(key)
                .send(),
            30,
        )
        .await?
        .map_err(|_| ApiError::Network)?;
        if matches!(response.status().as_u16(), 404 | 405 | 501) {
            return Err(ApiError::ModelsUnsupported);
        }
        check_status(response.status())?;
        #[derive(Deserialize)]
        struct Models {
            data: Vec<Model>,
        }
        #[derive(Deserialize)]
        struct Model {
            id: String,
        }
        let models: Models = deadline(response.json(), 30)
            .await?
            .map_err(|_| ApiError::InvalidResponse)?;
        let mut ids: Vec<_> = models.data.into_iter().map(|model| model.id).collect();
        ids.sort_unstable();
        ids.dedup();
        Ok(ids)
    }

    pub async fn stream(
        &self,
        connection: &Connection,
        key: &str,
        messages: Vec<Message>,
        mut on_text: impl FnMut(String),
    ) -> Result<(), ApiError> {
        let response = deadline(
            self.0
                .post(connection.endpoint("chat/completions"))
                .bearer_auth(key)
                .header("Accept", "text/event-stream")
                .json(&json!({"model": connection.model, "messages": messages, "stream": true}))
                .send(),
            60,
        )
        .await?
        .map_err(|_| ApiError::Network)?;
        check_status(response.status())?;
        if !response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream"))
        {
            return Err(ApiError::InvalidResponse);
        }
        let mut stream = response.bytes_stream();
        let mut decoder = EventDecoder::new();
        let mut completed = false;
        while let Some(chunk) = deadline(stream.next(), 90).await? {
            let chunk = chunk.map_err(|_| ApiError::Network)?;
            for event in decoder.push(&chunk)? {
                if event == "[DONE]" {
                    return if completed {
                        Ok(())
                    } else {
                        Err(ApiError::Interrupted)
                    };
                }
                let value: Value =
                    serde_json::from_str(&event).map_err(|_| ApiError::InvalidResponse)?;
                if value.get("error").is_some() {
                    return Err(ApiError::InvalidResponse);
                }
                if let Some(choice) = value["choices"]
                    .as_array()
                    .and_then(|choices| choices.first())
                {
                    if let Some(text) = choice["delta"]["content"].as_str() {
                        on_text(text.to_owned());
                    }
                    if let Some(reason) = choice["finish_reason"].as_str() {
                        if reason != "stop" {
                            return Err(ApiError::Interrupted);
                        }
                        completed = true;
                    }
                }
            }
        }
        if completed {
            Ok(())
        } else {
            Err(ApiError::Interrupted)
        }
    }
}

fn check_status(status: reqwest::StatusCode) -> Result<(), ApiError> {
    match status.as_u16() {
        200..=299 => Ok(()),
        401 | 403 => Err(ApiError::Unauthorized),
        429 => Err(ApiError::RateLimited),
        status => Err(ApiError::Http(status)),
    }
}

#[cfg(test)]
mod tests;
