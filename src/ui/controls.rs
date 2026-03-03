#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Controls::*;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::*;
#[cfg(target_os = "windows")]
use windows::core::*;

#[cfg(target_os = "windows")]
use crate::ui::messages::TransferPhase;

/// Control IDs
pub const ID_BTN_LOGIN: u16 = 101;
pub const ID_BTN_START: u16 = 102;
pub const ID_BTN_SETTINGS: u16 = 103;
pub const ID_TRANSFER_LIST: u16 = 201;
pub const ID_LOG_EDIT: u16 = 202;
pub const ID_STATUSBAR: u16 = 203;

/// Info about an active transfer, stored on the UI side.
#[cfg(target_os = "windows")]
#[derive(Clone)]
pub struct TransferInfo {
    pub file_id: i64,
    pub label: String,
    pub phase: TransferPhase,
    pub current: u64,
    pub total: u64,
}

#[cfg(target_os = "windows")]
pub struct AppControls {
    pub btn_login: HWND,
    pub btn_start: HWND,
    pub btn_settings: HWND,
    pub transfer_list: HWND,
    pub log_edit: HWND,
    pub statusbar: HWND,
    pub mono_font: HFONT,
    /// Active transfers keyed by file_id, in insertion order via Vec.
    pub transfers: Vec<TransferInfo>,
}

#[cfg(target_os = "windows")]
impl AppControls {
    pub unsafe fn create(parent: HWND, instance: HINSTANCE) -> Self {
        InitCommonControlsEx(&INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_BAR_CLASSES | ICC_PROGRESS_CLASS | ICC_LISTVIEW_CLASSES,
        });

        let btn_login = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Login with Discord"),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_PUSHBUTTON as u32),
            10, 10, 160, 30,
            parent, HMENU(ID_BTN_LOGIN as _), instance, None,
        ).unwrap();

        let btn_start = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Start"),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_PUSHBUTTON as u32),
            180, 10, 100, 30,
            parent, HMENU(ID_BTN_START as _), instance, None,
        ).unwrap();

        let btn_settings = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("Settings"),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_PUSHBUTTON as u32),
            290, 10, 100, 30,
            parent, HMENU(ID_BTN_SETTINGS as _), instance, None,
        ).unwrap();

        // Owner-drawn listbox for active transfers with per-item progress bars
        let transfer_list = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            w!("LISTBOX"),
            w!(""),
            WS_CHILD | WS_VISIBLE | WS_VSCROLL
                | WINDOW_STYLE(LBS_OWNERDRAWFIXED as u32)
                | WINDOW_STYLE(LBS_NOINTEGRALHEIGHT as u32)
                | WINDOW_STYLE(LBS_NOSEL as u32),
            10, 50, 560, 180,
            parent, HMENU(ID_TRANSFER_LIST as _), instance, None,
        ).unwrap();

        // Set item height to 28px for progress bars
        SendMessageW(transfer_list, LB_SETITEMHEIGHT, WPARAM(0), LPARAM(28));

        // Log area (bottom half)
        let log_edit = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            w!("EDIT"),
            w!(""),
            WS_CHILD | WS_VISIBLE | WS_VSCROLL
                | WINDOW_STYLE(ES_MULTILINE as u32)
                | WINDOW_STYLE(ES_READONLY as u32)
                | WINDOW_STYLE(ES_AUTOVSCROLL as u32),
            10, 240, 560, 160,
            parent, HMENU(ID_LOG_EDIT as _), instance, None,
        ).unwrap();

        let mono_font = CreateFontW(
            14, 0, 0, 0,
            FW_NORMAL.0 as i32, 0, 0, 0,
            DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            CLEARTYPE_QUALITY.0 as u32,
            (FF_MODERN.0 | FIXED_PITCH.0) as u32,
            w!("Consolas"),
        );
        if !mono_font.is_invalid() {
            SendMessageW(log_edit, WM_SETFONT, WPARAM(mono_font.0 as _), LPARAM(1));
        }

        let statusbar = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("msctls_statusbar32"),
            w!("Not logged in"),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(SBARS_SIZEGRIP as u32),
            0, 0, 0, 0,
            parent, HMENU(ID_STATUSBAR as _), instance, None,
        ).unwrap();

        Self {
            btn_login,
            btn_start,
            btn_settings,
            transfer_list,
            log_edit,
            statusbar,
            mono_font,
            transfers: Vec::new(),
        }
    }

    /// Resize controls when the parent window is resized.
    pub unsafe fn on_resize(&self, width: i32, height: i32) {
        let _ = SendMessageW(self.statusbar, WM_SIZE, WPARAM(0), LPARAM(0));

        let mut status_rect = RECT::default();
        let _ = GetWindowRect(self.statusbar, &mut status_rect);
        let status_height = status_rect.bottom - status_rect.top;

        let usable = height - 50 - status_height - 10; // below toolbar, above statusbar
        // Split: top 40% transfers, bottom 60% log
        let transfer_h = (usable * 40 / 100).max(80);
        let log_h = usable - transfer_h - 5; // 5px gap

        let _ = MoveWindow(self.transfer_list, 10, 50, width - 20, transfer_h, TRUE);
        let _ = MoveWindow(self.log_edit, 10, 50 + transfer_h + 5, width - 20, log_h, TRUE);
    }

    // ── Transfer list management ─────────────────────────────────────────

    /// Add or update a transfer in the active list.
    pub unsafe fn add_transfer(&mut self, info: TransferInfo) {
        // Check if already exists
        if let Some(existing) = self.transfers.iter_mut().find(|t| t.file_id == info.file_id) {
            existing.phase = info.phase;
            existing.label = info.label;
            existing.current = 0;
            existing.total = 0;
            self.sync_listbox();
            return;
        }
        self.transfers.push(info);
        self.sync_listbox();
    }

    /// Update progress for a transfer.
    pub unsafe fn update_transfer(&mut self, file_id: i64, current: u64, total: u64, phase: TransferPhase) {
        if let Some(t) = self.transfers.iter_mut().find(|t| t.file_id == file_id) {
            t.current = current;
            t.total = total;
            t.phase = phase;
            // Invalidate just this item's area for repaint
            self.invalidate_transfer_list();
        }
    }

    /// Remove a transfer from the active list.
    pub unsafe fn remove_transfer(&mut self, file_id: i64) {
        self.transfers.retain(|t| t.file_id != file_id);
        self.sync_listbox();
    }

    /// Sync listbox item count with our transfers vec.
    unsafe fn sync_listbox(&self) {
        let current_count = SendMessageW(self.transfer_list, LB_GETCOUNT, WPARAM(0), LPARAM(0)).0 as usize;
        let target = self.transfers.len();

        if current_count < target {
            for _ in current_count..target {
                // Add dummy items (content drawn by owner-draw)
                let empty: Vec<u16> = vec![0];
                SendMessageW(self.transfer_list, LB_ADDSTRING, WPARAM(0), LPARAM(empty.as_ptr() as _));
            }
        } else if current_count > target {
            for _ in target..current_count {
                SendMessageW(self.transfer_list, LB_DELETESTRING, WPARAM(target), LPARAM(0));
            }
        }
        self.invalidate_transfer_list();
    }

    unsafe fn invalidate_transfer_list(&self) {
        let _ = InvalidateRect(self.transfer_list, None, FALSE);
    }

    /// Owner-draw: paint a single transfer item with colored progress bar + label.
    /// Called from WM_DRAWITEM handler.
    pub unsafe fn draw_transfer_item(&self, dis: &DRAWITEMSTRUCT) {
        let index = dis.itemID as usize;
        if index >= self.transfers.len() {
            return;
        }
        let transfer = &self.transfers[index];
        let hdc = dis.hDC;
        let rc = dis.rcItem;

        // Background
        let bg_brush = CreateSolidBrush(COLORREF(0x00F0F0F0)); // light gray
        FillRect(hdc, &rc, bg_brush);
        let _ = DeleteObject(bg_brush);

        // Progress bar area: left 60% of the item
        let bar_right = rc.left + ((rc.right - rc.left) * 60 / 100);
        let bar_rect = RECT {
            left: rc.left + 4,
            top: rc.top + 3,
            right: bar_right - 2,
            bottom: rc.bottom - 3,
        };

        // Bar background (dark gray)
        let bar_bg = CreateSolidBrush(COLORREF(0x00D0D0D0));
        FillRect(hdc, &bar_rect, bar_bg);
        let _ = DeleteObject(bar_bg);

        // Filled portion
        if transfer.total > 0 {
            let fraction = (transfer.current as f64 / transfer.total as f64).min(1.0);
            let fill_width = ((bar_rect.right - bar_rect.left) as f64 * fraction) as i32;

            let color = match transfer.phase {
                TransferPhase::Download => COLORREF(0x00D0A030), // cyan-ish (BGR: teal)
                TransferPhase::Upload => COLORREF(0x0020B0E0),   // yellow-orange (BGR)
            };
            let fill_brush = CreateSolidBrush(color);
            let fill_rect = RECT {
                left: bar_rect.left,
                top: bar_rect.top,
                right: bar_rect.left + fill_width,
                bottom: bar_rect.bottom,
            };
            FillRect(hdc, &fill_rect, fill_brush);
            let _ = DeleteObject(fill_brush);
        }

        // Percentage text on the bar
        let pct_text = if transfer.total > 0 {
            let pct = (transfer.current as f64 / transfer.total as f64 * 100.0) as u32;
            format!("{}%", pct)
        } else {
            "...".to_string()
        };
        let mut pct_wide: Vec<u16> = pct_text.encode_utf16().chain(std::iter::once(0)).collect();
        let pct_len = pct_wide.len() - 1;
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, COLORREF(0x00000000));
        if !self.mono_font.is_invalid() {
            SelectObject(hdc, self.mono_font);
        }
        let mut text_rc = bar_rect;
        DrawTextW(hdc, &mut pct_wide[..pct_len], &mut text_rc,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE);

        // Label text: right 40% of the item
        let phase_prefix = match transfer.phase {
            TransferPhase::Download => "DL ",
            TransferPhase::Upload => "UL ",
        };
        let label_text = format!("{}{}", phase_prefix, transfer.label);
        let mut label_wide: Vec<u16> = label_text.encode_utf16().chain(std::iter::once(0)).collect();
        let label_len = label_wide.len() - 1;

        let mut label_rc = RECT {
            left: bar_right + 4,
            top: rc.top,
            right: rc.right - 4,
            bottom: rc.bottom,
        };

        // Phase color for the label
        let label_color = match transfer.phase {
            TransferPhase::Download => COLORREF(0x00804000), // dark teal
            TransferPhase::Upload => COLORREF(0x00006090), // dark orange
        };
        SetTextColor(hdc, label_color);
        DrawTextW(hdc, &mut label_wide[..label_len], &mut label_rc,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    }

    // ── Log ──────────────────────────────────────────────────────────────

    pub unsafe fn append_log(&self, text: &str) {
        let len = SendMessageW(self.log_edit, WM_GETTEXTLENGTH, WPARAM(0), LPARAM(0));

        if len.0 > 30000 {
            SendMessageW(self.log_edit, EM_SETSEL, WPARAM(0), LPARAM(10000));
            let empty: Vec<u16> = vec![0];
            SendMessageW(self.log_edit, EM_REPLACESEL, WPARAM(0), LPARAM(empty.as_ptr() as _));
            let len = SendMessageW(self.log_edit, WM_GETTEXTLENGTH, WPARAM(0), LPARAM(0));
            SendMessageW(self.log_edit, EM_SETSEL, WPARAM(len.0 as usize), LPARAM(len.0));
        } else {
            SendMessageW(self.log_edit, EM_SETSEL, WPARAM(len.0 as usize), LPARAM(len.0));
        }

        let line = format!("{}\r\n", text);
        let wide: Vec<u16> = line.encode_utf16().chain(std::iter::once(0)).collect();
        SendMessageW(self.log_edit, EM_REPLACESEL, WPARAM(0), LPARAM(wide.as_ptr() as _));
    }

    pub unsafe fn set_status(&self, text: &str) {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        SendMessageW(self.statusbar, SB_SETTEXTW, WPARAM(0), LPARAM(wide.as_ptr() as _));
    }

    pub unsafe fn set_logged_in(&self, logged_in: bool) {
        if logged_in {
            let _ = SetWindowTextW(self.btn_login, w!("Logged In"));
            let _ = EnableWindow(self.btn_login, FALSE);
        } else {
            let _ = SetWindowTextW(self.btn_login, w!("Login with Discord"));
            let _ = EnableWindow(self.btn_login, TRUE);
        }
    }

    pub unsafe fn set_running(&self, running: bool) {
        let text = if running { w!("Stop") } else { w!("Start") };
        let _ = SetWindowTextW(self.btn_start, text);
    }
}

#[cfg(target_os = "windows")]
fn format_bytes(current: u64, total: u64) -> String {
    fn fmt(b: u64) -> String {
        if b >= 1_073_741_824 {
            format!("{:.1} GB", b as f64 / 1_073_741_824.0)
        } else if b >= 1_048_576 {
            format!("{:.1} MB", b as f64 / 1_048_576.0)
        } else if b >= 1024 {
            format!("{:.0} KB", b as f64 / 1024.0)
        } else {
            format!("{} B", b)
        }
    }
    format!("{} / {}", fmt(current), fmt(total))
}
