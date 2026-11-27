use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

#[cfg(windows)]
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
#[cfg(windows)]
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(windows)]
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, FindWindowW, GetCursorPos, GetMessageW, LoadIconW, PostQuitMessage,
    RegisterClassW, SetForegroundWindow, ShowWindow, TrackPopupMenu, TranslateMessage,
    MF_SEPARATOR, MF_STRING, SW_RESTORE, SW_SHOW, TPM_BOTTOMALIGN, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, WM_APP, WM_COMMAND, WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONUP,
    WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    RestoreWindow,
    PauseAll,
    ResumeAll,
    ExitApp,
}

static TRAY_SENDER: OnceLock<UnboundedSender<TrayEvent>> = OnceLock::new();
static EGUI_CTX: OnceLock<eframe::egui::Context> = OnceLock::new();

pub fn set_egui_context(ctx: eframe::egui::Context) {
    let _ = EGUI_CTX.set(ctx);
}

pub const APP_TITLE: &str = "XDM-Rust Download Manager v1.0";

#[cfg(windows)]
pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
pub fn restore_main_window() {
    unsafe {
        let title = to_wide(APP_TITLE);
        let mut hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if hwnd == 0 {
            let legacy_title = to_wide("XDM-Rust Download Manager");
            hwnd = FindWindowW(std::ptr::null(), legacy_title.as_ptr());
        }
        if hwnd != 0 {
            ShowWindow(hwnd, SW_RESTORE);
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        }
    }
    if let Some(ctx) = EGUI_CTX.get() {
        ctx.request_repaint();
    }
}

#[cfg(not(windows))]
pub fn restore_main_window() {
    if let Some(ctx) = EGUI_CTX.get() {
        ctx.request_repaint();
    }
}

#[cfg(windows)]
unsafe extern "system" fn tray_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        msg if msg == WM_APP + 100 => {
            let event = lparam as u32;
            if event == WM_LBUTTONUP || event == WM_LBUTTONDBLCLK {
                restore_main_window();
                if let Some(tx) = TRAY_SENDER.get() {
                    let _ = tx.send(TrayEvent::RestoreWindow);
                }
                if let Some(ctx) = EGUI_CTX.get() {
                    ctx.request_repaint();
                }
            } else if event == WM_RBUTTONUP {
                let mut pt: POINT = std::mem::zeroed();
                GetCursorPos(&mut pt);
                SetForegroundWindow(hwnd);

                let hmenu = CreatePopupMenu();
                let m_show = to_wide("⚡ Open XDM-Rust");
                let m_pause = to_wide("⏸ Pause All Downloads");
                let m_resume = to_wide("▶ Resume All Downloads");
                let m_exit = to_wide("❌ Exit");

                AppendMenuW(hmenu, MF_STRING, 101, m_show.as_ptr());
                AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());
                AppendMenuW(hmenu, MF_STRING, 102, m_pause.as_ptr());
                AppendMenuW(hmenu, MF_STRING, 103, m_resume.as_ptr());
                AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());
                AppendMenuW(hmenu, MF_STRING, 104, m_exit.as_ptr());

                let cmd = TrackPopupMenu(
                    hmenu,
                    TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_RETURNCMD,
                    pt.x,
                    pt.y,
                    0,
                    hwnd,
                    std::ptr::null(),
                );
                DestroyMenu(hmenu);

                match cmd {
                    101 => {
                        restore_main_window();
                        if let Some(tx) = TRAY_SENDER.get() {
                            let _ = tx.send(TrayEvent::RestoreWindow);
                        }
                    }
                    102 => {
                        if let Some(tx) = TRAY_SENDER.get() {
                            let _ = tx.send(TrayEvent::PauseAll);
                        }
                    }
                    103 => {
                        if let Some(tx) = TRAY_SENDER.get() {
                            let _ = tx.send(TrayEvent::ResumeAll);
                        }
                    }
                    104 => {
                        if let Some(tx) = TRAY_SENDER.get() {
                            let _ = tx.send(TrayEvent::ExitApp);
                        }
                    }
                    _ => {}
                }

                if let Some(ctx) = EGUI_CTX.get() {
                    ctx.request_repaint();
                }
            }
            0
        }
        WM_COMMAND => {
            let cmd = (wparam & 0xFFFF) as usize;
            if let Some(tx) = TRAY_SENDER.get() {
                match cmd {
                    101 => {
                        restore_main_window();
                        let _ = tx.send(TrayEvent::RestoreWindow);
                    }
                    102 => {
                        let _ = tx.send(TrayEvent::PauseAll);
                    }
                    103 => {
                        let _ = tx.send(TrayEvent::ResumeAll);
                    }
                    104 => {
                        let _ = tx.send(TrayEvent::ExitApp);
                    }
                    _ => {}
                }
            }
            if let Some(ctx) = EGUI_CTX.get() {
                ctx.request_repaint();
            }
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub struct TrayManager {
    _shutdown: Arc<AtomicBool>,
}

#[cfg(windows)]
impl TrayManager {
    pub fn start() -> (Self, UnboundedReceiver<TrayEvent>) {
        let (tx, rx) = unbounded_channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();

        let _ = TRAY_SENDER.set(tx);

        std::thread::spawn(move || unsafe {
            let class_name = to_wide("XDMRustTrayWindowClass");
            let hinstance = GetModuleHandleW(std::ptr::null());

            let mut wc: WNDCLASSW = std::mem::zeroed();
            wc.lpfnWndProc = Some(tray_wnd_proc);
            wc.hInstance = hinstance;
            wc.lpszClassName = class_name.as_ptr();

            RegisterClassW(&wc);

            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                class_name.as_ptr(),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                0,
                0,
                hinstance,
                std::ptr::null(),
            );

            let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
            nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = hwnd;
            nid.uID = 1001;
            nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            nid.uCallbackMessage = WM_APP + 100;

            // Load application icon resource (ID 1) embedded into exe
            let hicon = LoadIconW(hinstance, 1 as *const u16);
            nid.hIcon = if hicon != 0 {
                hicon
            } else {
                LoadIconW(0, 32512 as *const u16) // Default application icon fallback
            };

            let tip = to_wide(APP_TITLE);
            for (i, c) in tip.iter().take(nid.szTip.len() - 1).enumerate() {
                nid.szTip[i] = *c;
            }

            Shell_NotifyIconW(NIM_ADD, &nid);

            let mut msg = std::mem::zeroed();
            while !shutdown_clone.load(Ordering::Relaxed) && GetMessageW(&mut msg, 0, 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            // Cleanup tray icon on exit
            Shell_NotifyIconW(NIM_DELETE, &nid);
            DestroyWindow(hwnd);
        });

        (Self { _shutdown: shutdown }, rx)
    }
}

#[cfg(not(windows))]
impl TrayManager {
    pub fn start() -> (Self, UnboundedReceiver<TrayEvent>) {
        let (_tx, rx) = unbounded_channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        (Self { _shutdown: shutdown }, rx)
    }
}
