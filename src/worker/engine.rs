use std::collections::HashSet;
use std::sync::mpsc::Sender;
use std::sync::Arc;

use crate::net::client::{build_client, WorkerError};
use crate::net::download::{download_file, local_path_for_job};
use crate::net::jobs::{fetch_jobs, report_job, FileJob};
use crate::net::upload::upload_file;
use crate::ui::messages::{TransferPhase, UiHandle, UiMessage};
use crate::worker::config::{AppState, MAX_RETRIES, QUEUE_PREFETCH, RETRY_DELAY_SECS};

/// Run the producer-consumer worker loop.
/// Matches worker.py `worker_loop` (lines 414-513).
pub async fn run_worker(
    state: Arc<AppState>,
    ui_tx: Sender<UiMessage>,
    ui_handle: UiHandle,
) {
    let client = build_client();
    let concurrency = state.config.concurrency;
    let batch_size = state.config.batch_size;
    let queue_cap = concurrency * QUEUE_PREFETCH;

    let (job_tx, job_rx) = async_channel::bounded::<FileJob>(queue_cap);
    let seen_ids: Arc<tokio::sync::Mutex<HashSet<i64>>> =
        Arc::new(tokio::sync::Mutex::new(HashSet::new()));

    // Log startup info
    ui_tx
        .send(UiMessage::LogLine(format!(
            "Server:      {}",
            state.config.server_url
        )))
        .ok();
    ui_tx
        .send(UiMessage::LogLine(format!(
            "Upload API:  {}",
            state.config.upload_server_url
        )))
        .ok();
    ui_tx
        .send(UiMessage::LogLine(format!(
            "Concurrency: {}",
            concurrency
        )))
        .ok();
    ui_handle.wake();

    // Create temp dir
    let _ = std::fs::create_dir_all(&state.config.temp_dir);

    // Spawn producer
    let producer_state = state.clone();
    let producer_client = client.clone();
    let producer_tx = ui_tx.clone();
    let producer_seen = seen_ids.clone();
    let producer_job_tx = job_tx.clone();
    let producer = tokio::spawn(async move {
        producer_loop(
            producer_state,
            producer_client,
            producer_tx,
            ui_handle,
            producer_seen,
            producer_job_tx,
            batch_size,
        )
        .await;
    });

    // Spawn consumer workers
    let mut consumers = Vec::new();
    for _ in 0..concurrency {
        let s = state.clone();
        let c = client.clone();
        let tx = ui_tx.clone();
        let rx = job_rx.clone();
        let seen = seen_ids.clone();
        consumers.push(tokio::spawn(async move {
            consumer_loop(s, c, tx, ui_handle, rx, seen).await;
        }));
    }

    // Wait for producer to finish (it exits when state.running becomes false)
    let _ = producer.await;

    // Close the channel so consumers drain and exit
    job_tx.close();

    // Wait for all consumers
    for c in consumers {
        let _ = c.await;
    }
}

async fn producer_loop(
    state: Arc<AppState>,
    client: reqwest::Client,
    ui_tx: Sender<UiMessage>,
    ui_handle: UiHandle,
    seen_ids: Arc<tokio::sync::Mutex<HashSet<i64>>>,
    job_tx: async_channel::Sender<FileJob>,
    batch_size: usize,
) {
    let mut no_jobs_warned = false;

    while state.is_running() {
        // Don't over-fill the queue
        if job_tx.len() >= state.config.concurrency {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            continue;
        }

        let token = match state.get_token() {
            Some(t) => t,
            None => {
                ui_tx
                    .send(UiMessage::LogLine(
                        "No token available. Please login.".into(),
                    ))
                    .ok();
                ui_handle.wake();
                state.set_running(false);
                break;
            }
        };

        // Fetch count: min(4, batch_size, max(1, capacity - current_size))
        let remaining_cap = job_tx.capacity().unwrap_or(4) - job_tx.len();
        let count = 4.min(batch_size).min(remaining_cap.max(1)) as u32;

        match fetch_jobs(&client, &state.config.server_url, &token, count).await {
            Ok(batch) => {
                if batch.jobs.is_empty() {
                    if !no_jobs_warned {
                        ui_tx
                            .send(UiMessage::LogLine(
                                "No jobs available, waiting...".into(),
                            ))
                            .ok();
                        ui_tx
                            .send(UiMessage::StatusText("Waiting for jobs...".into()))
                            .ok();
                        ui_handle.wake();
                        no_jobs_warned = true;
                    }
                    // 12 + random(0..8) seconds
                    let wait =
                        12.0 + rand::random::<f64>() * 8.0;
                    tokio::time::sleep(std::time::Duration::from_secs_f64(wait)).await;
                    continue;
                }

                no_jobs_warned = false;
                for job in batch.jobs {
                    let file_id = job.file_id;
                    {
                        let mut seen = seen_ids.lock().await;
                        if seen.contains(&file_id) {
                            continue;
                        }
                        seen.insert(file_id);
                    }
                    if job_tx.send(job).await.is_err() {
                        break; // Channel closed
                    }
                }
            }
            Err(WorkerError::AuthExpired) => {
                ui_tx
                    .send(UiMessage::LogLine(
                        "Token expired. Please login again.".into(),
                    ))
                    .ok();
                ui_handle.wake();
                state.set_running(false);
                break;
            }
            Err(WorkerError::UpgradeRequired(msg)) => {
                ui_tx
                    .send(UiMessage::LogLine(format!("Upgrade required: {}", msg)))
                    .ok();
                ui_handle.wake();
                state.set_running(false);
                break;
            }
            Err(e) => {
                ui_tx
                    .send(UiMessage::LogLine(format!("Server error: {}. Retrying...", e)))
                    .ok();
                ui_handle.wake();
                // 6 + random(0..4) seconds
                let wait = 6.0 + rand::random::<f64>() * 4.0;
                tokio::time::sleep(std::time::Duration::from_secs_f64(wait)).await;
            }
        }
    }
}

async fn consumer_loop(
    state: Arc<AppState>,
    client: reqwest::Client,
    ui_tx: Sender<UiMessage>,
    ui_handle: UiHandle,
    job_rx: async_channel::Receiver<FileJob>,
    seen_ids: Arc<tokio::sync::Mutex<HashSet<i64>>>,
) {
    while let Ok(job) = job_rx.recv().await {
        if !state.is_running() {
            seen_ids.lock().await.remove(&job.file_id);
            continue;
        }

        process_job(&state, &client, &ui_tx, ui_handle, &job).await;
        seen_ids.lock().await.remove(&job.file_id);
    }
}

/// Process a single job: download, upload, report.
/// Matches worker.py `process_job` (lines 342-408).
async fn process_job(
    state: &AppState,
    client: &reqwest::Client,
    ui_tx: &Sender<UiMessage>,
    ui_handle: UiHandle,
    job: &FileJob,
) {
    let file_id = job.file_id;
    let dest_path = &job.dest_path;
    let label = if dest_path.len() <= 60 {
        dest_path.clone()
    } else {
        format!("...{}", &dest_path[dest_path.len() - 57..])
    };

    let local_path = local_path_for_job(&state.config.temp_dir, &job.url, dest_path);
    let token = match state.get_token() {
        Some(t) => t,
        None => return,
    };

    let mut last_err: Option<String> = None;
    let mut file_size: u64 = 0;
    let mut uploaded = false;

    for attempt in 1..=MAX_RETRIES {
        // Download
        ui_tx
            .send(UiMessage::TransferStarted {
                file_id,
                label: label.clone(),
                phase: TransferPhase::Download,
            })
            .ok();
        ui_handle.wake();

        let dl_tx = ui_tx.clone();
        let dl_handle = ui_handle;
        match download_file(client, &job.url, &local_path, &|current, total| {
            dl_tx
                .send(UiMessage::TransferProgress {
                    file_id,
                    current,
                    total,
                    phase: TransferPhase::Download,
                })
                .ok();
            dl_handle.wake();
        })
        .await
        {
            Ok(size) => {
                file_size = size;
            }
            Err(e) => {
                last_err = Some(format!("{}", e));
                let _ = tokio::fs::remove_file(&local_path).await;
                if attempt < MAX_RETRIES {
                    let err_short = &format!("{}", e)[..format!("{}", e).len().min(72)];
                    ui_tx
                        .send(UiMessage::LogLine(format!(
                            "RETRY {}/{} {} ({})",
                            attempt, MAX_RETRIES, label, err_short
                        )))
                        .ok();
                    ui_handle.wake();
                    tokio::time::sleep(std::time::Duration::from_secs(
                        RETRY_DELAY_SECS * attempt as u64,
                    ))
                    .await;
                }
                continue;
            }
        }

        // Upload — switch the transfer phase to Upload
        ui_tx
            .send(UiMessage::TransferStarted {
                file_id,
                label: label.clone(),
                phase: TransferPhase::Upload,
            })
            .ok();
        ui_handle.wake();

        let ul_tx = ui_tx.clone();
        let ul_handle = ui_handle;
        match upload_file(
            client,
            &state.config.upload_server_url,
            &token,
            file_id,
            &local_path,
            &|current, total| {
                ul_tx
                    .send(UiMessage::TransferProgress {
                        file_id,
                        current,
                        total,
                        phase: TransferPhase::Upload,
                    })
                    .ok();
                ul_handle.wake();
            },
        )
        .await
        {
            Ok(()) => {
                uploaded = true;
                break;
            }
            Err(e) => {
                last_err = Some(format!("{}", e));
                let _ = tokio::fs::remove_file(&local_path).await;
                if attempt < MAX_RETRIES {
                    let err_short = &format!("{}", e)[..format!("{}", e).len().min(72)];
                    ui_tx
                        .send(UiMessage::LogLine(format!(
                            "RETRY {}/{} {} ({})",
                            attempt, MAX_RETRIES, label, err_short
                        )))
                        .ok();
                    ui_handle.wake();
                    tokio::time::sleep(std::time::Duration::from_secs(
                        RETRY_DELAY_SECS * attempt as u64,
                    ))
                    .await;
                }
            }
        }
    }

    if !uploaded {
        // All retries exhausted — remove from active transfers
        ui_tx
            .send(UiMessage::TransferDone { file_id })
            .ok();
        ui_tx
            .send(UiMessage::JobFailed {
                file_id,
                error: last_err.clone().unwrap_or_default(),
            })
            .ok();
        ui_handle.wake();

        let err_msg = last_err.map(|e| e[..e.len().min(500)].to_string());
        let _ = report_job(
            client,
            &state.config.server_url,
            &token,
            file_id,
            "failed",
            None,
            err_msg.as_deref(),
        )
        .await;

        // Update stats
        if let Ok(mut stats) = state.stats.lock() {
            stats.jobs_failed += 1;
        }
        return;
    }

    // Success — remove from active transfers
    ui_tx
        .send(UiMessage::TransferDone { file_id })
        .ok();
    ui_tx
        .send(UiMessage::JobCompleted {
            file_id,
            dest_path: dest_path.clone(),
            bytes: file_size,
        })
        .ok();
    ui_handle.wake();

    // Clean up temp file
    let _ = tokio::fs::remove_file(&local_path).await;

    // Report completion (best-effort)
    if let Err(e) = report_job(
        client,
        &state.config.server_url,
        &token,
        file_id,
        "completed",
        Some(file_size),
        None,
    )
    .await
    {
        ui_tx
            .send(UiMessage::LogLine(format!(
                "{}: uploaded but report delayed ({})",
                label,
                &format!("{}", e)[..format!("{}", e).len().min(120)]
            )))
            .ok();
        ui_handle.wake();
    }

    // Update stats
    if let Ok(mut stats) = state.stats.lock() {
        stats.jobs_completed += 1;
        stats.bytes_downloaded += file_size;
    }
}
