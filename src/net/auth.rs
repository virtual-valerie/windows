use std::sync::mpsc::Sender;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::ui::messages::{UiHandle, UiMessage};

const OAUTH_TIMEOUT_SECS: u64 = 120;

/// Run the Discord OAuth flow: open browser, listen for callback with token.
pub async fn do_login(
    server_url: &str,
    token_path: &std::path::Path,
    ui_tx: &Sender<UiMessage>,
    ui_handle: UiHandle,
) -> Result<String, String> {
    let listener = TcpListener::bind("127.0.0.1:19283")
        .await
        .map_err(|e| format!("Failed to bind port 19283: {e}"))?;

    let url = format!(
        "{}/auth/discord/login?worker_callback=http://127.0.0.1:19283/",
        server_url
    );

    ui_tx
        .send(UiMessage::LogLine("Opening browser for Discord login...".into()))
        .ok();
    ui_tx
        .send(UiMessage::LogLine(format!("If it doesn't open: {url}")))
        .ok();
    ui_handle.wake();

    open::that(&url).map_err(|e| format!("Failed to open browser: {e}"))?;

    // Wait for the OAuth callback with a timeout
    let result = tokio::time::timeout(
        Duration::from_secs(OAUTH_TIMEOUT_SECS),
        accept_oauth_callback(&listener),
    )
    .await
    .map_err(|_| "Login timed out after 120 seconds".to_string())?;

    let token = result?;

    // Save token
    if let Some(parent) = token_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create token directory: {e}"))?;
    }
    std::fs::write(token_path, &token)
        .map_err(|e| format!("Failed to save token: {e}"))?;

    Ok(token)
}

async fn accept_oauth_callback(listener: &TcpListener) -> Result<String, String> {
    let (mut stream, _addr) = listener
        .accept()
        .await
        .map_err(|e| format!("Accept failed: {e}"))?;

    let mut buf = vec![0u8; 8192];
    let n = stream
        .read(&mut buf)
        .await
        .map_err(|e| format!("Read failed: {e}"))?;
    let request = String::from_utf8_lossy(&buf[..n]);

    // Parse the GET request line to extract ?token=...
    let token = parse_token_from_request(&request)
        .ok_or_else(|| "No token in callback request".to_string())?;

    // Send success response
    let response = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Type: text/html\r\n",
        "Connection: close\r\n",
        "\r\n",
        "<html><body><h1>Logged in! You can close this tab.</h1></body></html>"
    );
    stream.write_all(response.as_bytes()).await.ok();
    stream.flush().await.ok();

    Ok(token)
}

fn parse_token_from_request(request: &str) -> Option<String> {
    // GET /?token=xxx HTTP/1.1
    let first_line = request.lines().next()?;
    let path = first_line.split_whitespace().nth(1)?;

    // Find query string
    let query = path.split('?').nth(1)?;

    // Parse query parameters
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next()?;
        let value = parts.next()?;
        if key == "token" {
            // URL decode the token (though JWT tokens typically don't need it)
            return Some(url_decode(value));
        }
    }
    None
}

fn url_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let h1 = chars.next().and_then(|c| (c as char).to_digit(16));
            let h2 = chars.next().and_then(|c| (c as char).to_digit(16));
            if let (Some(h1), Some(h2)) = (h1, h2) {
                result.push((h1 * 16 + h2) as u8 as char);
            }
        } else if b == b'+' {
            result.push(' ');
        } else {
            result.push(b as char);
        }
    }
    result
}
