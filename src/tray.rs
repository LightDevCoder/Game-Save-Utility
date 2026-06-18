#[cfg(windows)]
mod platform {
    use crate::models::Language;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::mem;
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;
    use winapi::shared::minwindef::{LPARAM, LRESULT, UINT, WPARAM};
    use winapi::shared::windef::{HICON, HWND, POINT};
    use winapi::um::libloaderapi::GetModuleHandleW;
    use winapi::um::shellapi::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
        NOTIFYICONDATAW,
    };
    use winapi::um::winuser::{
        AppendMenuW, CallWindowProcW, CreatePopupMenu, DestroyMenu, GetCursorPos, LoadIconW,
        PostMessageW, SetForegroundWindow, SetWindowLongPtrW, ShowWindow, TrackPopupMenu,
        GWLP_WNDPROC, IDI_APPLICATION, MF_STRING, SW_HIDE, SW_RESTORE, SW_SHOW, TPM_LEFTALIGN,
        TPM_RIGHTBUTTON, WM_APP, WM_COMMAND, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP,
    };

    const TRAY_UID: UINT = 1;
    const TRAY_CALLBACK_MESSAGE: UINT = WM_APP + 42;
    const MENU_SHOW: usize = 1001;
    const MENU_EXIT: usize = 1002;

    pub type WindowHandle = HWND;

    #[derive(Clone, Copy)]
    struct TrayState {
        hwnd: HWND,
        old_wnd_proc: isize,
        icon_added: bool,
    }

    // SAFETY: TrayState only stores opaque Win32 handles and an integer window-procedure pointer.
    // Access is serialized by TRAY_STATE, and Win32 operations are dispatched to the UI thread.
    unsafe impl Send for TrayState {}

    static TRAY_STATE: Mutex<Option<TrayState>> = Mutex::new(None);
    static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
    static TRAY_LANGUAGE_IS_ENGLISH: AtomicBool = AtomicBool::new(false);

    pub fn hwnd_from_creation_context(cc: &eframe::CreationContext<'_>) -> Option<WindowHandle> {
        let handle = cc.window_handle().ok()?.as_raw();
        match handle {
            RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as HWND),
            _ => None,
        }
    }

    pub fn init(hwnd: WindowHandle) {
        if hwnd.is_null() {
            return;
        }

        let mut state = match TRAY_STATE.lock() {
            Ok(state) => state,
            Err(_) => return,
        };

        if let Some(existing) = *state {
            if existing.hwnd == hwnd {
                return;
            }
            // SAFETY: existing.hwnd is the window handle previously registered in TRAY_STATE.
            unsafe {
                delete_icon(existing.hwnd);
            }
        }

        // SAFETY: hwnd comes from eframe's live Win32 window handle. The replacement procedure
        // uses the Win32 WNDPROC ABI and the returned procedure is retained for forwarding.
        let old_wnd_proc = unsafe {
            SetWindowLongPtrW(hwnd, GWLP_WNDPROC, tray_window_proc as *const () as isize)
        };
        // SAFETY: hwnd is valid and add_or_modify_icon fully initializes NOTIFYICONDATAW.
        let icon_added = unsafe { add_or_modify_icon(hwnd, NIM_ADD) };
        *state = Some(TrayState {
            hwnd,
            old_wnd_proc,
            icon_added,
        });
    }

    pub fn set_language(language: Language) {
        TRAY_LANGUAGE_IS_ENGLISH.store(matches!(language, Language::EnUs), Ordering::SeqCst);
        if let Some(existing) = TRAY_STATE.lock().ok().and_then(|state| *state) {
            if existing.icon_added {
                // SAFETY: the icon was previously added for this valid stored window handle.
                unsafe {
                    add_or_modify_icon(existing.hwnd, NIM_MODIFY);
                }
            }
        }
    }

    pub fn hide_window(hwnd: WindowHandle) {
        if !hwnd.is_null() {
            // SAFETY: callers pass the live eframe Win32 window handle and only visibility changes.
            unsafe {
                ShowWindow(hwnd, SW_HIDE);
            }
        }
    }

    pub fn show_window(hwnd: WindowHandle) {
        if !hwnd.is_null() {
            // SAFETY: callers pass the live eframe Win32 window handle.
            unsafe {
                ShowWindow(hwnd, SW_SHOW);
                ShowWindow(hwnd, SW_RESTORE);
                SetForegroundWindow(hwnd);
            }
        }
    }

    pub fn take_exit_requested() -> bool {
        EXIT_REQUESTED.swap(false, Ordering::SeqCst)
    }

    pub fn shutdown() {
        let mut state = match TRAY_STATE.lock() {
            Ok(state) => state,
            Err(_) => return,
        };
        let Some(existing) = *state else {
            return;
        };

        // SAFETY: the stored handle and old procedure were produced during init for this window.
        unsafe {
            if existing.icon_added {
                delete_icon(existing.hwnd);
            }
            if existing.old_wnd_proc != 0 {
                SetWindowLongPtrW(existing.hwnd, GWLP_WNDPROC, existing.old_wnd_proc);
            }
        }
        *state = None;
    }

    unsafe extern "system" fn tray_window_proc(
        hwnd: HWND,
        msg: UINT,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            TRAY_CALLBACK_MESSAGE => {
                match lparam as UINT {
                    WM_LBUTTONUP | WM_LBUTTONDBLCLK => show_window(hwnd),
                    WM_RBUTTONUP => show_menu(hwnd),
                    _ => {}
                }
                0
            }
            WM_COMMAND => {
                match low_word(wparam) {
                    MENU_SHOW => {
                        show_window(hwnd);
                        return 0;
                    }
                    MENU_EXIT => {
                        request_exit(hwnd);
                        return 0;
                    }
                    _ => {}
                }
                call_old_proc(hwnd, msg, wparam, lparam)
            }
            _ => call_old_proc(hwnd, msg, wparam, lparam),
        }
    }

    unsafe fn call_old_proc(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let old_wnd_proc = TRAY_STATE
            .lock()
            .ok()
            .and_then(|state| state.map(|state| state.old_wnd_proc))
            .unwrap_or_default();

        if old_wnd_proc == 0 {
            return 0;
        }

        // SAFETY: SetWindowLongPtrW returned the previous WNDPROC encoded as an isize.
        let old_proc = mem::transmute::<isize, winapi::um::winuser::WNDPROC>(old_wnd_proc);
        // SAFETY: old_proc is the previous procedure for hwnd and receives the original message.
        CallWindowProcW(old_proc, hwnd, msg, wparam, lparam)
    }

    unsafe fn request_exit(hwnd: HWND) {
        EXIT_REQUESTED.store(true, Ordering::SeqCst);
        // SAFETY: hwnd is supplied by the active tray callback.
        ShowWindow(hwnd, SW_SHOW);
        ShowWindow(hwnd, SW_RESTORE);
        SetForegroundWindow(hwnd);
        PostMessageW(hwnd, WM_NULL, 0, 0);
    }

    unsafe fn show_menu(hwnd: HWND) {
        // SAFETY: CreatePopupMenu returns an owned HMENU that is destroyed before returning.
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }

        let show_text = wide(if is_english() {
            "Show Window"
        } else {
            "显示窗口"
        });
        let exit_text = wide(if is_english() { "Exit" } else { "退出" });
        // SAFETY: menu is valid and the UTF-16 buffers remain alive through these calls.
        AppendMenuW(menu, MF_STRING, MENU_SHOW, show_text.as_ptr());
        AppendMenuW(menu, MF_STRING, MENU_EXIT, exit_text.as_ptr());

        let mut point = POINT { x: 0, y: 0 };
        // SAFETY: point is writable and hwnd/menu remain valid for the popup-menu interaction.
        if GetCursorPos(&mut point) != 0 {
            SetForegroundWindow(hwnd);
            TrackPopupMenu(
                menu,
                TPM_LEFTALIGN | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                0,
                hwnd,
                ptr::null(),
            );
        }
        // SAFETY: menu was created successfully above and is no longer in use.
        DestroyMenu(menu);
    }

    unsafe fn add_or_modify_icon(hwnd: HWND, message: u32) -> bool {
        // SAFETY: zero is a valid initial state for NOTIFYICONDATAW before required fields are set.
        let mut data: NOTIFYICONDATAW = mem::zeroed();
        data.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = hwnd;
        data.uID = TRAY_UID;
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = TRAY_CALLBACK_MESSAGE;
        data.hIcon = load_icon();

        let tip = wide(if is_english() {
            "Game Save Utility"
        } else {
            "单机游戏存档备份与恢复工具"
        });
        for (target, ch) in data.szTip.iter_mut().zip(tip) {
            *target = ch;
        }

        // SAFETY: data is fully initialized for NIM_ADD/NIM_MODIFY and hwnd is live.
        Shell_NotifyIconW(message, &mut data) != 0
    }

    unsafe fn delete_icon(hwnd: HWND) {
        // SAFETY: zero is a valid initial state and the identifying fields are set below.
        let mut data: NOTIFYICONDATAW = mem::zeroed();
        data.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = hwnd;
        data.uID = TRAY_UID;
        // SAFETY: data identifies the tray icon previously associated with hwnd.
        Shell_NotifyIconW(NIM_DELETE, &mut data);
    }

    unsafe fn load_icon() -> HICON {
        // SAFETY: a null module name requests the current process module.
        let module = GetModuleHandleW(ptr::null());
        let resource_id = ptr::with_exposed_provenance::<u16>(1);
        // SAFETY: resource_id uses Win32 MAKEINTRESOURCE semantics for icon resource 1.
        let icon = LoadIconW(module, resource_id);
        if icon.is_null() {
            // SAFETY: a null module with IDI_APPLICATION requests the shared system icon.
            LoadIconW(ptr::null_mut(), IDI_APPLICATION)
        } else {
            icon
        }
    }

    fn low_word(value: WPARAM) -> usize {
        value & 0xffff
    }

    fn is_english() -> bool {
        TRAY_LANGUAGE_IS_ENGLISH.load(Ordering::SeqCst)
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }
}

#[cfg(not(windows))]
mod platform {
    use crate::models::Language;

    pub type WindowHandle = *mut std::ffi::c_void;

    pub fn hwnd_from_creation_context(_cc: &eframe::CreationContext<'_>) -> Option<WindowHandle> {
        None
    }

    pub fn init(_hwnd: WindowHandle) {}
    pub fn set_language(_language: Language) {}
    pub fn hide_window(_hwnd: WindowHandle) {}
    pub fn show_window(_hwnd: WindowHandle) {}
    pub fn take_exit_requested() -> bool {
        false
    }
    pub fn shutdown() {}
}

pub use platform::*;
