#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod net;
mod ui;
mod worker;

use std::sync::Arc;

fn main() {
    // 1. Clean up leftover .old file from previous update
    net::updater::cleanup_old_exe();

    // 2. Build tokio runtime
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime"),
    );

    // 3. Create UI message channel
    let (ui_tx, ui_rx) = std::sync::mpsc::channel::<ui::messages::UiMessage>();

    // 4. Load config and state
    let state = Arc::new(worker::config::AppState::new());

    // 5. Run Win32 message loop on the main thread (blocks until window closes)
    #[cfg(target_os = "windows")]
    {
        ui::window::run_message_loop(state, runtime, ui_tx, ui_rx);
    }

    // Non-Windows: headless console mode for development/testing
    #[cfg(not(target_os = "windows"))]
    {
        eprintln!("Minerva DPN Worker v{}", worker::config::VERSION);
        eprintln!("This is a Windows GUI application.");
        eprintln!("On non-Windows platforms, use worker.py instead.");

        // For development: run the worker in headless mode if a token exists
        if state.get_token().is_some() {
            eprintln!("Token found. Starting headless worker...");
            let ui_handle = ui::messages::UiHandle::new_dummy();
            state.set_running(true);

            // Drain UI messages in a background thread
            std::thread::spawn(move || {
                while let Ok(msg) = ui_rx.recv() {
                    match msg {
                        ui::messages::UiMessage::LogLine(text) => {
                            eprintln!("{}", text);
                        }
                        ui::messages::UiMessage::JobCompleted {
                            dest_path, bytes, ..
                        } => {
                            eprintln!("OK {} ({} bytes)", dest_path, bytes);
                        }
                        ui::messages::UiMessage::JobFailed { error, .. } => {
                            eprintln!("FAIL: {}", error);
                        }
                        ui::messages::UiMessage::WorkerStopped => {
                            eprintln!("Worker stopped.");
                            break;
                        }
                        _ => {}
                    }
                }
            });

            runtime.block_on(async {
                // Check for updates
                net::updater::check_version(
                    &state.config.server_url,
                    &ui_tx,
                    ui_handle,
                )
                .await;

                // Run worker
                worker::engine::run_worker(state, ui_tx, ui_handle).await;
            });
        } else {
            eprintln!("No token found. Please use worker.py to login first.");
        }
    }
}
