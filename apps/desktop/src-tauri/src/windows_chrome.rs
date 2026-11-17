//! Win32 quirks for a borderless main window: bare Alt must not enter the
//! system-menu modal loop, and `WM_CLOSE` on helper HWNDs must not steal
//! shutdown from the real main window.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use tauri::WebviewWindow;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, EnumThreadWindows, SetWindowLongPtrW, GWLP_WNDPROC, WM_CLOSE, WM_SYSCOMMAND,
};

const SC_KEYMENU: u32 = 0xF100;

type WndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

fn state() -> &'static Mutex<ChromeState> {
    static STATE: OnceLock<Mutex<ChromeState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(ChromeState {
            main_hwnd: 0,
            prev: HashMap::new(),
        })
    })
}

struct ChromeState {
    main_hwnd: isize,
    prev: HashMap<isize, WndProc>,
}

pub fn attach_main_window(window: &WebviewWindow) -> Result<(), String> {
    let raw = window.hwnd().map_err(|err| err.to_string())?;
    let hwnd = HWND(raw.0);
    state()
        .lock()
        .map_err(|_| "window chrome state poisoned".to_string())?
        .main_hwnd = hwnd.0;
    subclass_hwnd(hwnd)?;
    subclass_thread_windows()?;
    Ok(())
}

pub fn subclass_thread_windows() -> Result<(), String> {
    unsafe {
        EnumThreadWindows(
            GetCurrentThreadId(),
            Some(enum_subclass),
            LPARAM(0),
        )
        .map_err(|err| err.to_string())?;
    }
    Ok(())
}

unsafe extern "system" fn enum_subclass(hwnd: HWND, _: LPARAM) -> BOOL {
    let _ = subclass_hwnd(hwnd);
    BOOL(1)
}

fn subclass_hwnd(hwnd: HWND) -> Result<(), String> {
    let Ok(mut chrome) = state().lock() else {
        return Err("window chrome state poisoned".into());
    };
    if chrome.prev.contains_key(&hwnd.0) {
        return Ok(());
    }
    unsafe {
        let prev = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, shim_wndproc as isize);
        if prev == 0 {
            // Some HWNDs on the UI thread cannot be subclassed; skip them quietly.
            return Ok(());
        }
        chrome.prev.insert(hwnd.0, std::mem::transmute(prev));
    }
    Ok(())
}

unsafe extern "system" fn shim_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let Ok(chrome) = state().lock() else {
        return LRESULT(0);
    };
    let main = chrome.main_hwnd;
  drop(chrome);

    match msg {
        WM_SYSCOMMAND if hwnd.0 == main => {
            let cmd = wparam.0 as u32 & 0xFFF0;
            // Bare Alt on a decorations:false window posts SC_KEYMENU with
            // lParam 0 and traps the UI in the menu loop until Esc or a click.
            if cmd == SC_KEYMENU && lparam.0 == 0 {
                return LRESULT(0);
            }
        }
        WM_CLOSE if hwnd.0 != main => {
            // A close delivered to a helper or message-only HWND must not mark
            // the app as shutting down while the main window is still open.
            return LRESULT(0);
        }
        _ => {}
    }

    let Ok(chrome) = state().lock() else {
        return LRESULT(0);
    };
    let Some(prev) = chrome.prev.get(&hwnd.0).copied() else {
        return LRESULT(0);
    };
    drop(chrome);
    unsafe { CallWindowProcW(Some(prev), hwnd, msg, wparam, lparam) }
}