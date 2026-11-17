//! Win32 quirks for a borderless main window: bare Alt must not enter the
//! system-menu modal loop, and `WM_CLOSE` on helper HWNDs must not steal
//! shutdown from the real main window.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use tauri::WebviewWindow;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, EnumThreadWindows, PostMessageW, SetWindowLongPtrW, GWLP_WNDPROC, WM_CANCELMODE,
    WM_CLOSE, WM_EXITMENULOOP, WM_SYSCOMMAND,
};

const SC_KEYMENU: u32 = 0xF100;

type WndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

fn hwnd_key(hwnd: HWND) -> usize {
    hwnd.0 as usize
}

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
    main_hwnd: usize,
    prev: HashMap<usize, WndProc>,
}

pub fn attach_main_window(window: &WebviewWindow) -> Result<(), String> {
    let raw = window.hwnd().map_err(|err| err.to_string())?;
    let hwnd = HWND(raw.0);
    state()
        .lock()
        .map_err(|_| "window chrome state poisoned".to_string())?
        .main_hwnd = hwnd_key(hwnd);
    subclass_hwnd(hwnd)?;
    subclass_thread_windows()?;
    Ok(())
}

pub fn subclass_thread_windows() -> Result<(), String> {
    let ok = unsafe { EnumThreadWindows(GetCurrentThreadId(), Some(enum_subclass), LPARAM(0)) };
    if !ok.as_bool() {
        return Err("EnumThreadWindows failed".into());
    }
    Ok(())
}

unsafe extern "system" fn enum_subclass(hwnd: HWND, _: LPARAM) -> BOOL {
    let _ = subclass_hwnd(hwnd);
    BOOL(1)
}

fn subclass_hwnd(hwnd: HWND) -> Result<(), String> {
    let key = hwnd_key(hwnd);
    let Ok(mut chrome) = state().lock() else {
        return Err("window chrome state poisoned".into());
    };
    if chrome.prev.contains_key(&key) {
        return Ok(());
    }
    unsafe {
        let prev = SetWindowLongPtrW(
            hwnd,
            GWLP_WNDPROC,
            shim_wndproc as *const () as isize,
        );
        if prev == 0 {
            return Ok(());
        }
        chrome.prev.insert(
            key,
            std::mem::transmute::<isize, WndProc>(prev),
        );
    }
    Ok(())
}

unsafe extern "system" fn shim_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let key = hwnd_key(hwnd);
    let main = {
        let Ok(chrome) = state().lock() else {
            return LRESULT(0);
        };
        chrome.main_hwnd
    };

    match msg {
        WM_SYSCOMMAND if key == main => {
            let cmd = wparam.0 as u32 & 0xFFF0;
            if cmd == SC_KEYMENU && lparam.0 == 0 {
                return LRESULT(0);
            }
        }
        WM_CLOSE if key != main => {
            return LRESULT(0);
        }
        WM_EXITMENULOOP if key == main => {
            // One Esc after Alt+Space must leave the frame, not a half-latched menu mode.
            let _ = unsafe { PostMessageW(hwnd, WM_CANCELMODE, WPARAM(0), LPARAM(0)) };
        }
        _ => {}
    }

    let Ok(chrome) = state().lock() else {
        return LRESULT(0);
    };
    let Some(prev) = chrome.prev.get(&key).copied() else {
        return LRESULT(0);
    };
    drop(chrome);
    unsafe { CallWindowProcW(Some(prev), hwnd, msg, wparam, lparam) }
}