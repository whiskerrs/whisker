use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn server(status: &str, body: String) -> (Connection, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        loop {
            let mut bytes = [0u8; 1024];
            let n = socket.read(&mut bytes).await.unwrap();
            if n == 0 {
                break;
            }
            request.extend_from_slice(&bytes[..n]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|v| v.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        for chunk in response.as_bytes().chunks(7) {
            if socket.write_all(chunk).await.is_err() {
                break;
            }
            tokio::task::yield_now().await;
        }
        String::from_utf8(request).unwrap()
    });
    (
        Connection {
            name: "Test".into(),
            base_url: format!("http://{address}"),
            model: "test-model".into(),
        },
        task,
    )
}

#[tokio::test]
async fn streams_text_and_sends_only_the_supported_request_fields() {
    let body = concat!(
        ": heartbeat\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"日本語\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let (connection, request) = server("200 OK", body.into()).await;
    let mut output = String::new();
    ApiClient::new()
        .unwrap()
        .stream(
            &connection,
            "test-key",
            vec![Message {
                role: "user",
                content: "Hello".into(),
            }],
            |text| output.push_str(&text),
        )
        .await
        .unwrap();
    assert_eq!(output, "日本語");
    let request = request.await.unwrap();
    assert!(request.starts_with("POST /chat/completions HTTP/1.1"));
    let json: Value = serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(
        json,
        json!({"model":"test-model", "stream":true, "messages":[{"role":"user","content":"Hello"}]})
    );
}

#[tokio::test]
async fn classifies_authentication_and_rate_limits_without_echoing_provider_bodies() {
    for (status, expected) in [
        ("401 Unauthorized", ApiError::Unauthorized),
        ("429 Too Many Requests", ApiError::RateLimited),
    ] {
        let (connection, request) = server(status, "provider body must not be logged".into()).await;
        assert_eq!(
            ApiClient::new()
                .unwrap()
                .stream(&connection, "test-key", vec![], |_| {})
                .await,
            Err(expected)
        );
        request.await.unwrap();
    }
}

#[tokio::test]
async fn preserves_partial_output_but_rejects_an_unfinished_stream() {
    let (connection, request) = server(
        "200 OK",
        "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n".into(),
    )
    .await;
    let mut output = String::new();
    assert_eq!(
        ApiClient::new()
            .unwrap()
            .stream(&connection, "test-key", vec![], |text| output
                .push_str(&text))
            .await,
        Err(ApiError::Interrupted)
    );
    assert_eq!(output, "partial");
    request.await.unwrap();
}
