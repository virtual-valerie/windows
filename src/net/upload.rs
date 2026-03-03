use sha2::{Digest, Sha256};
use std::path::Path;

use crate::net::client::{
    auth_headers, check_upgrade_required, is_retryable, retry_sleep, WorkerError,
    UPLOAD_CHUNK_RETRIES, UPLOAD_FINISH_RETRIES, UPLOAD_START_RETRIES,
};
use crate::worker::config::UPLOAD_CHUNK_SIZE;

/// Upload a file using the 3-phase chunked upload protocol.
/// Matches worker.py `upload_file` (lines 216-297).
pub async fn upload_file<F>(
    client: &reqwest::Client,
    upload_server_url: &str,
    token: &str,
    file_id: i64,
    path: &Path,
    on_progress: &F,
) -> Result<(), WorkerError>
where
    F: Fn(u64, u64),
{
    let headers = auth_headers(token);

    // Phase 1: Start session
    let session_id = start_session(client, upload_server_url, &headers, file_id).await?;

    // Phase 2: Send chunks with SHA256 hashing
    let file_size = tokio::fs::metadata(path).await?.len();
    let mut hasher = Sha256::new();
    let mut sent: u64 = 0;

    // Read file in chunks
    let file_data = tokio::fs::read(path).await?;
    for chunk in file_data.chunks(UPLOAD_CHUNK_SIZE) {
        hasher.update(chunk);
        send_chunk(
            client,
            upload_server_url,
            &headers,
            file_id,
            &session_id,
            chunk,
        )
        .await?;
        sent += chunk.len() as u64;
        on_progress(sent, file_size);
    }

    // Phase 3: Finish upload
    let sha256_hex = format!("{:x}", hasher.finalize());
    finish_upload(
        client,
        upload_server_url,
        &headers,
        file_id,
        &session_id,
        &sha256_hex,
    )
    .await?;

    Ok(())
}

async fn start_session(
    client: &reqwest::Client,
    upload_server_url: &str,
    headers: &reqwest::header::HeaderMap,
    file_id: i64,
) -> Result<String, WorkerError> {
    let url = format!("{}/api/upload/{}/start", upload_server_url, file_id);

    for attempt in 1..=UPLOAD_START_RETRIES {
        match client.post(&url).headers(headers.clone()).send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                check_upgrade_required(status, &body)?;

                if status == 401 {
                    return Err(WorkerError::AuthExpired);
                }
                if is_retryable(status) {
                    if attempt == UPLOAD_START_RETRIES {
                        return Err(WorkerError::RetryExhausted(format!(
                            "upload start failed ({status})"
                        )));
                    }
                    tokio::time::sleep(retry_sleep(attempt, 25.0)).await;
                    continue;
                }
                if status >= 400 {
                    return Err(WorkerError::ServerError(status, body));
                }

                let v: serde_json::Value = serde_json::from_str(&body)?;
                let session_id = v["session_id"]
                    .as_str()
                    .ok_or_else(|| WorkerError::Other("Missing session_id".into()))?
                    .to_string();
                return Ok(session_id);
            }
            Err(e) => {
                if attempt == UPLOAD_START_RETRIES {
                    return Err(WorkerError::RetryExhausted(format!(
                        "upload start failed ({e})"
                    )));
                }
                tokio::time::sleep(retry_sleep(attempt, 25.0)).await;
            }
        }
    }
    Err(WorkerError::RetryExhausted(
        "Failed to create upload session".into(),
    ))
}

async fn send_chunk(
    client: &reqwest::Client,
    upload_server_url: &str,
    headers: &reqwest::header::HeaderMap,
    file_id: i64,
    session_id: &str,
    data: &[u8],
) -> Result<(), WorkerError> {
    let url = format!("{}/api/upload/{}/chunk", upload_server_url, file_id);

    for attempt in 1..=UPLOAD_CHUNK_RETRIES {
        let mut req_headers = headers.clone();
        req_headers.insert(
            "Content-Type",
            "application/octet-stream".parse().unwrap(),
        );

        match client
            .post(&url)
            .query(&[("session_id", session_id)])
            .headers(req_headers)
            .body(data.to_vec())
            .send()
            .await
        {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                check_upgrade_required(status, &body)?;

                if status == 401 {
                    return Err(WorkerError::AuthExpired);
                }
                if is_retryable(status) {
                    if attempt == UPLOAD_CHUNK_RETRIES {
                        return Err(WorkerError::RetryExhausted(format!(
                            "upload chunk failed ({status})"
                        )));
                    }
                    tokio::time::sleep(retry_sleep(attempt, 20.0)).await;
                    continue;
                }
                if status >= 400 {
                    return Err(WorkerError::ServerError(status, body));
                }
                return Ok(());
            }
            Err(e) => {
                if attempt == UPLOAD_CHUNK_RETRIES {
                    return Err(WorkerError::RetryExhausted(format!(
                        "upload chunk failed ({e})"
                    )));
                }
                tokio::time::sleep(retry_sleep(attempt, 20.0)).await;
            }
        }
    }
    Ok(())
}

async fn finish_upload(
    client: &reqwest::Client,
    upload_server_url: &str,
    headers: &reqwest::header::HeaderMap,
    file_id: i64,
    session_id: &str,
    expected_sha256: &str,
) -> Result<(), WorkerError> {
    let url = format!("{}/api/upload/{}/finish", upload_server_url, file_id);

    for attempt in 1..=UPLOAD_FINISH_RETRIES {
        match client
            .post(&url)
            .query(&[
                ("session_id", session_id),
                ("expected_sha256", expected_sha256),
            ])
            .headers(headers.clone())
            .send()
            .await
        {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                check_upgrade_required(status, &body)?;

                if status == 401 {
                    return Err(WorkerError::AuthExpired);
                }
                if is_retryable(status) {
                    if attempt == UPLOAD_FINISH_RETRIES {
                        return Err(WorkerError::RetryExhausted(format!(
                            "upload finish failed ({status})"
                        )));
                    }
                    tokio::time::sleep(retry_sleep(attempt, 20.0)).await;
                    continue;
                }
                if status >= 400 {
                    return Err(WorkerError::ServerError(status, body));
                }
                return Ok(());
            }
            Err(e) => {
                if attempt == UPLOAD_FINISH_RETRIES {
                    return Err(WorkerError::RetryExhausted(format!(
                        "upload finish failed ({e})"
                    )));
                }
                tokio::time::sleep(retry_sleep(attempt, 20.0)).await;
            }
        }
    }
    Ok(())
}
