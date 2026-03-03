#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{HWND, WPARAM, LPARAM};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

/// Custom Windows message ID for waking the UI thread.
pub const WM_APP_UI_MSG: u32 = 0x8001; // WM_APP + 1

/// The phase of a transfer (determines progress bar color).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferPhase {
    Download,
    Upload,
}

/// Messages sent from async background tasks to the UI thread.
#[derive(Debug)]
pub enum UiMessage {
    /// Append a line to the log area.
    LogLine(String),

    /// A transfer started — add it to the active transfers list.
    TransferStarted {
        file_id: i64,
        label: String,
        phase: TransferPhase,
    },

    /// Update progress for a specific transfer.
    TransferProgress {
        file_id: i64,
        current: u64,
        total: u64,
        phase: TransferPhase,
    },

    /// A transfer completed — remove it from the active list.
    TransferDone {
        file_id: i64,
    },

    /// Authentication succeeded.
    AuthSuccess,

    /// Authentication failed.
    AuthFailed(String),

    /// Worker loop started.
    WorkerStarted,

    /// Worker loop stopped.
    WorkerStopped,

    /// A job completed successfully.
    JobCompleted {
        file_id: i64,
        dest_path: String,
        bytes: u64,
    },

    /// A job failed after all retries.
    JobFailed {
        file_id: i64,
        error: String,
    },

    /// A newer version is available.
    UpdateAvailable {
        current: String,
        remote: String,
    },

    /// Update the status bar text.
    StatusText(String),
}

/// Handle to the Win32 window for posting messages from async code.
#[derive(Clone, Copy)]
pub struct UiHandle {
    #[cfg(target_os = "windows")]
    pub hwnd: HWND,
    #[cfg(not(target_os = "windows"))]
    _dummy: (),
}

unsafe impl Send for UiHandle {}
unsafe impl Sync for UiHandle {}

impl UiHandle {
    #[cfg(target_os = "windows")]
    pub fn new(hwnd: HWND) -> Self {
        Self { hwnd }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn new_dummy() -> Self {
        Self { _dummy: () }
    }

    /// Wake the UI thread by posting a custom message.
    pub fn wake(&self) {
        #[cfg(target_os = "windows")]
        unsafe {
            let _ = PostMessageW(self.hwnd, WM_APP_UI_MSG, WPARAM(0), LPARAM(0));
        }
    }
}
