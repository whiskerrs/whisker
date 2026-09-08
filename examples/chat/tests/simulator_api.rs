use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
#[ignore = "Manual loopback API for simulator testing; runs until interrupted"]
async fn serve_simulator() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8787")
        .await
        .unwrap();
    println!("Mock API: http://127.0.0.1:8787/v1; key: test-key; model: test-model");
    loop {
        let (socket, _) = listener.accept().await.unwrap();
        tokio::spawn(async move {
            let _ = respond(socket).await;
        });
    }
}

async fn respond(mut socket: tokio::net::TcpStream) -> std::io::Result<()> {
    let mut request = Vec::new();
    loop {
        let mut bytes = [0u8; 4096];
        let count = socket.read(&mut bytes).await?;
        if count == 0 {
            return Ok(());
        }
        request.extend_from_slice(&bytes[..count]);
        if request.len() > 1_048_576 {
            return Ok(());
        }
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
    let request = String::from_utf8_lossy(&request);
    if !request
        .to_ascii_lowercase()
        .contains("authorization: bearer test-key\r\n")
    {
        socket
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await?;
    } else if request.starts_with("GET /v1/models ") {
        let body = r#"{"data":[{"id":"test-model"}]}"#;
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await?;
    } else {
        let json: serde_json::Value =
            serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap_or("{}"))
                .unwrap_or_default();
        let question = json["messages"]
            .as_array()
            .and_then(|messages| messages.last())
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        if question.eq_ignore_ascii_case("rate-limit") {
            socket.write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await?;
            return Ok(());
        }
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await?;
        for index in 1..=30 {
            let value = serde_json::json!({"choices":[{"delta":{"content":format!("{index}. これはローカルAPIの回答です。日本語の表示とスクロールを確認できます。\n\n")},"finish_reason":null}]});
            let event = format!("data: {value}\n\n");
            for bytes in event.as_bytes().chunks(13) {
                socket.write_all(bytes).await?;
            }
            tokio::time::sleep(std::time::Duration::from_millis(160)).await;
            if question.eq_ignore_ascii_case("disconnect") && index == 3 {
                return Ok(());
            }
        }
        socket.write_all(b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n").await?;
    }
    Ok(())
}
