#[cfg(target_os = "windows")]
use std::sync::mpsc::Receiver;
#[cfg(target_os = "windows")]
use std::sync::{mpsc::Sender, Arc};

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Controls::*;
#[cfg(target_os = "windows")]
use windows::core::*;

#[cfg(target_os = "windows")]
use crate::ui::controls::*;
#[cfg(target_os = "windows")]
use crate::ui::dialogs;
#[cfg(target_os = "windows")]
use crate::ui::messages::*;
#[cfg(target_os = "windows")]
use crate::worker::config::AppState;

#[cfg(target_os = "windows")]
struct WindowData {
    controls: AppControls,
    state: Arc<AppState>,
    ui_rx: Receiver<UiMessage>,
    ui_tx: Sender<UiMessage>,
    ui_handle: UiHandle,
    runtime: Arc<tokio::runtime::Runtime>,
}

#[cfg(target_os = "windows")]
static mut WINDOW_DATA: Option<*mut WindowData> = None;

#[cfg(target_os = "windows")]
unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            LRESULT(0)
        }

        WM_SIZE => {
            if let Some(data_ptr) = WINDOW_DATA {
                let data = &*data_ptr;
                let width = (lparam.0 & 0xFFFF) as i32;
                let height = ((lparam.0 >> 16) & 0xFFFF) as i32;
                data.controls.on_resize(width, height);
            }
            LRESULT(0)
        }

        WM_DRAWITEM => {
            if wparam.0 as u16 == ID_TRANSFER_LIST {
                if let Some(data_ptr) = WINDOW_DATA {
                    let data = &*data_ptr;
                    let dis = &*(lparam.0 as *const DRAWITEMSTRUCT);
                    data.controls.draw_transfer_item(dis);
                    return LRESULT(1); // We handled it
                }
            }
            LRESULT(0)
        }

        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as u16;
            let notification = ((wparam.0 >> 16) & 0xFFFF) as u16;

            if notification == BN_CLICKED as u16 {
                if let Some(data_ptr) = WINDOW_DATA {
                    let data = &mut *data_ptr;
                    match id {
                        ID_BTN_LOGIN => {
                            handle_login(data);
                        }
                        ID_BTN_START => {
                            handle_start_stop(data);
                        }
                        ID_BTN_SETTINGS => {
                            dialogs::show_settings_info(
                                hwnd,
                                &data.state.config.server_url,
                                &data.state.config.upload_server_url,
                                data.state.config.concurrency,
                            );
                        }
                        _ => {}
                    }
                }
            }
            LRESULT(0)
        }

        WM_APP_UI_MSG => {
            if let Some(data_ptr) = WINDOW_DATA {
                let data = &mut *data_ptr;
                drain_ui_messages(hwnd, data);
            }
            LRESULT(0)
        }

        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[cfg(target_os = "windows")]
unsafe fn handle_login(data: &WindowData) {
    let state = data.state.clone();
    let ui_tx = data.ui_tx.clone();
    let ui_handle = data.ui_handle;
    let server_url = state.config.server_url.clone();
    let token_path = state.config.token_path.clone();

    data.runtime.spawn(async move {
        match crate::net::auth::do_login(&server_url, &token_path, &ui_tx, ui_handle).await {
            Ok(token) => {
                state.set_token(token);
                ui_tx.send(UiMessage::AuthSuccess).ok();
                ui_handle.wake();
            }
            Err(e) => {
                ui_tx.send(UiMessage::AuthFailed(e)).ok();
                ui_handle.wake();
            }
        }
    });
}

#[cfg(target_os = "windows")]
unsafe fn handle_start_stop(data: &mut WindowData) {
    if data.state.is_running() {
        data.state.set_running(false);
        data.ui_tx.send(UiMessage::LogLine("Stopping worker...".into())).ok();
        data.ui_handle.wake();
    } else {
        if data.state.get_token().is_none() {
            data.controls.append_log("Please login first.");
            return;
        }
        data.state.set_running(true);
        data.controls.set_running(true);

        let state = data.state.clone();
        let ui_tx = data.ui_tx.clone();
        let ui_handle = data.ui_handle;

        data.runtime.spawn(async move {
            ui_tx.send(UiMessage::WorkerStarted).ok();
            ui_handle.wake();

            crate::worker::engine::run_worker(state.clone(), ui_tx.clone(), ui_handle).await;

            state.set_running(false);
            ui_tx.send(UiMessage::WorkerStopped).ok();
            ui_handle.wake();
        });
    }
}

#[cfg(target_os = "windows")]
unsafe fn drain_ui_messages(hwnd: HWND, data: &mut WindowData) {
    while let Ok(msg) = data.ui_rx.try_recv() {
        match msg {
            UiMessage::LogLine(text) => {
                data.controls.append_log(&text);
            }
            UiMessage::TransferStarted { file_id, label, phase } => {
                data.controls.add_transfer(TransferInfo {
                    file_id,
                    label,
                    phase,
                    current: 0,
                    total: 0,
                });
            }
            UiMessage::TransferProgress { file_id, current, total, phase } => {
                data.controls.update_transfer(file_id, current, total, phase);
            }
            UiMessage::TransferDone { file_id } => {
                data.controls.remove_transfer(file_id);
            }
            UiMessage::AuthSuccess => {
                data.controls.set_logged_in(true);
                data.controls.set_status("Logged in - Ready");
                data.controls.append_log("Login successful!");
            }
            UiMessage::AuthFailed(err) => {
                data.controls.append_log(&format!("Login failed: {}", err));
            }
            UiMessage::WorkerStarted => {
                data.controls.set_running(true);
                let c = data.state.config.concurrency;
                data.controls.set_status(&format!("Running ({} workers)", c));
                data.controls.append_log(&format!(
                    "Worker started (concurrency={}, server={})",
                    c, data.state.config.server_url
                ));
            }
            UiMessage::WorkerStopped => {
                data.controls.set_running(false);
                data.controls.set_status("Stopped");
                data.controls.append_log("Worker stopped.");
            }
            UiMessage::JobCompleted {
                file_id: _,
                dest_path,
                bytes,
            } => {
                let label = if dest_path.len() > 60 {
                    format!("...{}", &dest_path[dest_path.len() - 57..])
                } else {
                    dest_path
                };
                data.controls.append_log(&format!("OK {} ({} bytes)", label, bytes));
            }
            UiMessage::JobFailed { file_id: _, error } => {
                data.controls.append_log(&format!("FAIL: {}", &error[..error.len().min(120)]));
            }
            UiMessage::UpdateAvailable { current, remote } => {
                if dialogs::show_update_dialog(hwnd, &current, &remote) {
                    let server_url = data.state.config.server_url.clone();
                    let ui_tx = data.ui_tx.clone();
                    let ui_handle = data.ui_handle;
                    data.runtime.spawn(async move {
                        if let Err(e) =
                            crate::net::updater::download_and_replace(&server_url, &ui_tx, ui_handle)
                                .await
                        {
                            ui_tx
                                .send(UiMessage::LogLine(format!("Update failed: {}", e)))
                                .ok();
                            ui_handle.wake();
                        }
                    });
                }
            }
            UiMessage::StatusText(text) => {
                data.controls.set_status(&text);
            }
        }
    }
}

/// Create the main window and run the Win32 message loop.
#[cfg(target_os = "windows")]
pub fn run_message_loop(
    state: Arc<AppState>,
    runtime: Arc<tokio::runtime::Runtime>,
    ui_tx: Sender<UiMessage>,
    ui_rx: Receiver<UiMessage>,
) {
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None).unwrap().into();

        let class_name = w!("MinervaWorkerWindow");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as _),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: class_name,
            hIconSm: HICON::default(),
        };

        RegisterClassExW(&wc);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Minerva DPN Worker"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            700,
            550,
            HWND::default(),
            HMENU::default(),
            instance,
            None,
        )
        .unwrap();

        let controls = AppControls::create(hwnd, instance);
        let ui_handle = UiHandle::new(hwnd);

        let has_token = state.get_token().is_some();
        controls.set_logged_in(has_token);
        if has_token {
            controls.set_status("Logged in - Ready");
        }

        let mut window_data = Box::new(WindowData {
            controls,
            state: state.clone(),
            ui_rx,
            ui_tx: ui_tx.clone(),
            ui_handle,
            runtime: runtime.clone(),
        });

        WINDOW_DATA = Some(&mut *window_data as *mut WindowData);

        // Spawn version check
        let server_url = state.config.server_url.clone();
        let tx = ui_tx.clone();
        runtime.spawn(async move {
            crate::net::updater::check_version(&server_url, &tx, ui_handle).await;
        });

        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        WINDOW_DATA = None;
    }
}
