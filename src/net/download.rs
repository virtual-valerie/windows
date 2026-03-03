use futures_util::StreamExt;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

use crate::net::client::WorkerError;

/// Sanitize a path component for Windows filesystem.
/// Matches worker.py `_sanitize_component` (lines 117-127).
fn sanitize_component(part: &str) -> String {
    const BAD_CHARS: &str = "<>:\"/\\|?*";
    let mut out = String::with_capacity(part.len());
    for ch in part.chars() {
        if BAD_CHARS.contains(ch) || (ch as u32) < 32 {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    let cleaned = out.trim().trim_end_matches('.').to_string();
    if cleaned.is_empty() {
        "_".to_string()
    } else {
        cleaned
    }
}

/// Compute the local temp path for a download job.
/// Matches worker.py `local_path_for_job` (lines 130-136).
pub fn local_path_for_job(temp_dir: &Path, url: &str, dest_path: &str) -> PathBuf {
    let parsed = url::Url::parse(url).ok();
    let host = parsed
        .as_ref()
        .and_then(|u| u.host_str())
        .map(|h| sanitize_component(h))
        .unwrap_or_else(|| "unknown-host".to_string());

    // URL-decode dest_path and split into components
    let decoded_dest = url_decode_path(dest_path);
    let trimmed = decoded_dest.trim_start_matches('/');

    let mut path = temp_dir.join(&host);
    for part in trimmed.split('/').filter(|p| !p.is_empty()) {
        path = path.join(sanitize_component(part));
    }
    path
}

fn url_decode_path(s: &str) -> String {
    // Simple percent-decoding for paths
    let mut result = String::with_capacity(s.len());
    let mut chars = s.bytes().peekable();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let h1 = chars.next().and_then(|c| (c as char).to_digit(16));
            let h2 = chars.next().and_then(|c| (c as char).to_digit(16));
            if let (Some(h1), Some(h2)) = (h1, h2) {
                result.push((h1 * 16 + h2) as u8 as char);
            }
        } else {
            result.push(b as char);
        }
    }
    result
}

/// Download a file with streaming and progress reporting.
pub async fn download_file<F>(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    on_progress: &F,
) -> Result<u64, WorkerError>
where
    F: Fn(u64, u64),
{
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let resp = client
        .get(url)
        .send()
        .await?
        .error_for_status()
        .map_err(WorkerError::Http)?;

    let total = resp.content_length().unwrap_or(0);
    let mut stream = resp.bytes_stream();
    let mut file = tokio::fs::File::create(dest).await?;
    let mut downloaded: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(WorkerError::Http)?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total);
    }

    file.flush().await?;
    Ok(downloaded)
}
