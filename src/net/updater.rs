use serde::Deserialize;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;

use crate::net::client::build_client;
use crate::ui::messages::{UiHandle, UiMessage};
use crate::worker::config::VERSION;

#[derive(Deserialize)]
struct VersionResponse {
    version: String,
}

/// Check the server for a newer EXE version.
/// Sends UiMessage::UpdateAvailable if a newer version exists.
pub async fn check_version(
    server_url: &str,
    ui_tx: &Sender<UiMessage>,
    ui_handle: UiHandle,
) {
    let client = build_client();
    let url = format!("{}/worker/exe/version", server_url);

    let resp = match tokio::time::timeout(
        Duration::from_secs(10),
        client.get(&url).send(),
    )
    .await
    {
        Ok(Ok(r)) => r,
        _ => return, // Network error, skip silently
    };

    let info: VersionResponse = match resp.json().await {
        Ok(v) => v,
        Err(_) => return,
    };

    if info.version != "unknown" && info.version != VERSION {
        ui_tx
            .send(UiMessage::UpdateAvailable {
                current: VERSION.to_string(),
                remote: info.version,
            })
            .ok();
        ui_handle.wake();
    }
}

/// Download the new EXE, rename-and-replace, then relaunch.
///
/// Windows allows renaming a running EXE but not deleting it.
/// Strategy:
///   1. Clean up any previous .exe.old
///   2. Download new EXE to <current>.exe.new
///   3. Rename running <current>.exe -> <current>.exe.old
///   4. Rename <current>.exe.new -> <current>.exe
///   5. Spawn new process and exit
pub async fn download_and_replace(
    server_url: &str,
    ui_tx: &Sender<UiMessage>,
    ui_handle: UiHandle,
) -> Result<(), String> {
    let current_exe =
        std::env::current_exe().map_err(|e| format!("Cannot determine current exe: {e}"))?;

    let new_path = append_ext(&current_exe, "new");
    let old_path = append_ext(&current_exe, "old");

    // Clean up leftover .old from previous update
    let _ = std::fs::remove_file(&old_path);

    ui_tx
        .send(UiMessage::LogLine("Downloading update...".into()))
        .ok();
    ui_handle.wake();

    // Download new EXE
    let client = build_client();
    let url = format!("{}/worker/exe/download", server_url);
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Download failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("Download failed: HTTP {}", resp.status()));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Download read failed: {e}"))?;

    std::fs::write(&new_path, &bytes).map_err(|e| format!("Write new exe failed: {e}"))?;

    // Rename current -> .old
    std::fs::rename(&current_exe, &old_path)
        .map_err(|e| format!("Rename current to .old failed: {e}"))?;

    // Rename .new -> current
    std::fs::rename(&new_path, &current_exe)
        .map_err(|e| format!("Rename .new to current failed: {e}"))?;

    ui_tx
        .send(UiMessage::LogLine("Update installed! Relaunching...".into()))
        .ok();
    ui_handle.wake();

    // Relaunch
    std::process::Command::new(&current_exe)
        .spawn()
        .map_err(|e| format!("Relaunch failed: {e}"))?;

    std::process::exit(0);
}

/// Clean up leftover .exe.old from a previous update.
/// Call this on every startup.
pub fn cleanup_old_exe() {
    if let Ok(current) = std::env::current_exe() {
        let old = append_ext(&current, "old");
        let _ = std::fs::remove_file(old);
    }
}

fn append_ext(path: &PathBuf, ext: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".");
    s.push(ext);
    PathBuf::from(s)
}
