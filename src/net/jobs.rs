use serde::Deserialize;

use crate::net::client::{
    auth_headers, check_upgrade_required, is_retryable, response_detail, retry_sleep, WorkerError,
    REPORT_RETRIES,
};

#[derive(Debug, Clone, Deserialize)]
pub struct FileJob {
    pub file_id: i64,
    pub url: String,
    pub dest_path: String,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct JobBatchResponse {
    pub jobs: Vec<FileJob>,
    #[serde(default)]
    pub lease_timeout_minutes: u32,
}

/// Fetch available jobs from the server.
/// Matches worker.py producer logic (lines 448-461).
pub async fn fetch_jobs(
    client: &reqwest::Client,
    server_url: &str,
    token: &str,
    count: u32,
) -> Result<JobBatchResponse, WorkerError> {
    let url = format!("{}/api/jobs", server_url);
    let resp = client
        .get(&url)
        .query(&[("count", count)])
        .headers(auth_headers(token))
        .send()
        .await?;

    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();

    check_upgrade_required(status, &body)?;

    if status == 401 {
        return Err(WorkerError::AuthExpired);
    }
    if status >= 400 {
        return Err(WorkerError::ServerError(status, body));
    }

    let batch: JobBatchResponse = serde_json::from_str(&body)?;
    Ok(batch)
}

/// Report job completion/failure to the server.
/// Matches worker.py `report_job` (lines 300-336) including the 409 "not finalized" retry.
pub async fn report_job(
    client: &reqwest::Client,
    server_url: &str,
    token: &str,
    file_id: i64,
    status: &str,
    bytes_downloaded: Option<u64>,
    error: Option<&str>,
) -> Result<(), WorkerError> {
    let url = format!("{}/api/jobs/report", server_url);
    let payload = serde_json::json!({
        "file_id": file_id,
        "status": status,
        "bytes_downloaded": bytes_downloaded,
        "error": error,
    });

    for attempt in 1..=REPORT_RETRIES {
        match client
            .post(&url)
            .headers(auth_headers(token))
            .json(&payload)
            .send()
            .await
        {
            Ok(resp) => {
                let resp_status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();

                check_upgrade_required(resp_status, &body)?;

                if resp_status == 401 {
                    return Err(WorkerError::AuthExpired);
                }

                // Special 409 handling for async finalize race (worker.py lines 318-325)
                if resp_status == 409 && status == "completed" {
                    let detail = response_detail(&body).to_lowercase();
                    if detail.contains("not finalized") || detail.contains("upload") {
                        if attempt == REPORT_RETRIES {
                            return Err(WorkerError::ServerError(resp_status, body));
                        }
                        let delay = (0.25 + attempt as f64 * 0.1).min(2.0);
                        tokio::time::sleep(std::time::Duration::from_secs_f64(delay)).await;
                        continue;
                    }
                }

                if is_retryable(resp_status) {
                    if attempt == REPORT_RETRIES {
                        return Err(WorkerError::ServerError(resp_status, body));
                    }
                    tokio::time::sleep(retry_sleep(attempt, 20.0)).await;
                    continue;
                }

                if resp_status >= 400 {
                    return Err(WorkerError::ServerError(resp_status, body));
                }

                return Ok(());
            }
            Err(e) => {
                if attempt == REPORT_RETRIES {
                    return Err(WorkerError::Http(e));
                }
                tokio::time::sleep(retry_sleep(attempt, 20.0)).await;
            }
        }
    }
    Ok(())
}
