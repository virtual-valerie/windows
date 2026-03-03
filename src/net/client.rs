use reqwest::header::{HeaderMap, HeaderValue};
use std::fmt;
use std::time::Duration;

use crate::worker::config::VERSION;

// Retryable HTTP status codes (matches worker.py line 184)
pub const RETRIABLE_STATUS_CODES: &[u16] = &[
    408, 425, 429, 500, 502, 503, 504, 520, 521, 522, 523, 524,
];

// Retry counts (matches worker.py lines 180-183)
pub const UPLOAD_START_RETRIES: u32 = 12;
pub const UPLOAD_CHUNK_RETRIES: u32 = 30;
pub const UPLOAD_FINISH_RETRIES: u32 = 12;
pub const REPORT_RETRIES: u32 = 20;

#[derive(Debug)]
pub enum WorkerError {
    Http(reqwest::Error),
    Io(std::io::Error),
    Json(serde_json::Error),
    UpgradeRequired(String),
    AuthExpired,
    ServerError(u16, String),
    RetryExhausted(String),
    Other(String),
}

impl fmt::Display for WorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(e) => write!(f, "HTTP error: {e}"),
            Self::Io(e) => write!(f, "IO error: {e}"),
            Self::Json(e) => write!(f, "JSON error: {e}"),
            Self::UpgradeRequired(msg) => write!(f, "Upgrade required: {msg}"),
            Self::AuthExpired => write!(f, "Token expired. Please log in again."),
            Self::ServerError(code, msg) => write!(f, "Server error {code}: {msg}"),
            Self::RetryExhausted(msg) => write!(f, "Retries exhausted: {msg}"),
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl From<reqwest::Error> for WorkerError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e)
    }
}

impl From<std::io::Error> for WorkerError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for WorkerError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

pub fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(300))
        .pool_max_idle_per_host(4)
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .expect("Failed to build HTTP client")
}

pub fn auth_headers(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "Authorization",
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );
    headers.insert(
        "X-Minerva-Worker-Version",
        HeaderValue::from_static(VERSION),
    );
    headers
}

pub fn is_retryable(status: u16) -> bool {
    RETRIABLE_STATUS_CODES.contains(&status)
}

/// Backoff formula matching worker.py line 192: min(cap, 0.85 * attempt + random * 1.25)
pub fn retry_sleep(attempt: u32, cap: f64) -> Duration {
    let secs = (0.85 * attempt as f64 + rand::random::<f64>() * 1.25).min(cap);
    Duration::from_secs_f64(secs)
}

/// Check for 426 Upgrade Required response.
pub fn check_upgrade_required(status: u16, body: &str) -> Result<(), WorkerError> {
    if status == 426 {
        // Try to extract detail from JSON body
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
            if let Some(detail) = v.get("detail").and_then(|d| d.as_str()) {
                return Err(WorkerError::UpgradeRequired(detail.to_string()));
            }
        }
        let msg = if body.trim().is_empty() {
            "Worker update required".to_string()
        } else {
            body.trim().to_string()
        };
        return Err(WorkerError::UpgradeRequired(msg));
    }
    Ok(())
}

/// Extract "detail" field from a JSON error response body.
pub fn response_detail(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(detail) = v.get("detail").and_then(|d| d.as_str()) {
            return detail.to_string();
        }
    }
    body.trim().to_string()
}
