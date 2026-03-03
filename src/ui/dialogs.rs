#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;
#[cfg(target_os = "windows")]
use windows::core::*;

/// Show a Yes/No dialog asking the user to update.
/// Returns true if the user clicked Yes.
#[cfg(target_os = "windows")]
pub unsafe fn show_update_dialog(parent: HWND, current: &str, remote: &str) -> bool {
    let msg = format!(
        "Update available: v{} \u{2192} v{}\n\nUpdate and restart now?",
        current, remote
    );
    let wide_msg: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
    let wide_title: Vec<u16> = "Minerva DPN Worker - Update"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let result = MessageBoxW(
        parent,
        PCWSTR(wide_msg.as_ptr()),
        PCWSTR(wide_title.as_ptr()),
        MB_YESNO | MB_ICONQUESTION,
    );

    result == IDYES
}

/// Show a simple info/error message box.
#[cfg(target_os = "windows")]
pub unsafe fn show_message(parent: HWND, title: &str, message: &str, is_error: bool) {
    let wide_msg: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    let wide_title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();

    let flags = if is_error {
        MB_OK | MB_ICONERROR
    } else {
        MB_OK | MB_ICONINFORMATION
    };

    MessageBoxW(
        parent,
        PCWSTR(wide_msg.as_ptr()),
        PCWSTR(wide_title.as_ptr()),
        flags,
    );
}

/// Show settings info dialog.
#[cfg(target_os = "windows")]
pub unsafe fn show_settings_info(parent: HWND, server_url: &str, upload_url: &str, concurrency: usize) {
    let msg = format!(
        "Current Settings:\n\n\
         Server: {}\n\
         Upload Server: {}\n\
         Concurrency: {}\n\n\
         To change settings, edit:\n\
         %APPDATA%\\minerva-dpn\\settings.json",
        server_url, upload_url, concurrency
    );
    let wide_msg: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
    let wide_title: Vec<u16> = "Minerva DPN Worker - Settings"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    MessageBoxW(
        parent,
        PCWSTR(wide_msg.as_ptr()),
        PCWSTR(wide_title.as_ptr()),
        MB_OK | MB_ICONINFORMATION,
    );
}
