//! Win32 quirks for a borderless main window: bare Alt must not enter the
//! system-menu modal loop, and `WM_CLOSE` on helper HWNDs must not steal
//! shutdown from the real main window.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use tauri::WebviewWindow;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, EnableMenuItem, EnumThreadWindows, GetSystemMenu, GetWindowRect, HMENU,
    IsIconic, IsZoomed, PostMessageW, SetWindowLongPtrW, TrackPopupMenuEx, GWLP_WNDPROC,
    MF_BYCOMMAND, MF_ENABLED, MF_GRAYED, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_CLOSE,
    WM_SYSCOMMAND,
};

const SC_KEYMENU: u32 = 0xF100;
const SC_SIZE: u32 = 0xF000;
const SC_MOVE: u32 = 0xF010;
const SC_MINIMIZE: u32 = 0xF020;
const SC_MAXIMIZE: u32 = 0xF030;
const SC_RESTORE: u32 = 0xF120;
const VK_SPACE: u32 = 0x20;
/// Matches `.titlebar { height: 34px; }` in the desktop shell.
const CUSTOM_TITLEBAR_HEIGHT_PX: i32 = 34;
/// `GetSystemMenu` with `bRevert == TRUE` resets the menu and the returned handle is
/// undefined (null in practice), so Alt+Space would hit the early return in
/// `track_system_menu` and show nothing.
const GET_SYSTEM_MENU_REVERT: BOOL = BOOL(0);

type WndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeymenuDisposition {
    Swallow,
    PopupSystemMenu,
    Default,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SystemMenuItemState {
    command: u32,
    enabled: bool,
}

/// Same enable/disable pattern `DefWindowProc` applies before showing the system menu.
fn system_menu_item_states(maximized: bool, minimized: bool) -> [SystemMenuItemState; 5] {
    let restore = maximized || minimized;
    if minimized {
        return [
            SystemMenuItemState {
                command: SC_RESTORE,
                enabled: restore,
            },
            SystemMenuItemState {
                command: SC_MOVE,
                enabled: true,
            },
            SystemMenuItemState {
                command: SC_SIZE,
                enabled: false,
            },
            SystemMenuItemState {
                command: SC_MINIMIZE,
                enabled: false,
            },
            SystemMenuItemState {
                command: SC_MAXIMIZE,
                enabled: true,
            },
        ];
    }
    if maximized {
        return [
            SystemMenuItemState {
                command: SC_RESTORE,
                enabled: restore,
            },
            SystemMenuItemState {
                command: SC_MOVE,
                enabled: false,
            },
            SystemMenuItemState {
                command: SC_SIZE,
                enabled: false,
            },
            SystemMenuItemState {
                command: SC_MINIMIZE,
                enabled: true,
            },
            SystemMenuItemState {
                command: SC_MAXIMIZE,
                enabled: false,
            },
        ];
    }
    [
        SystemMenuItemState {
            command: SC_RESTORE,
            enabled: restore,
        },
        SystemMenuItemState {
            command: SC_MOVE,
            enabled: true,
        },
        SystemMenuItemState {
            command: SC_SIZE,
            enabled: true,
        },
        SystemMenuItemState {
            command: SC_MINIMIZE,
            enabled: true,
        },
        SystemMenuItemState {
            command: SC_MAXIMIZE,
            enabled: true,
        },
    ]
}

fn keymenu_disposition(lparam: LPARAM) -> KeymenuDisposition {
    if lparam.0 == 0 {
        KeymenuDisposition::Swallow
    } else if lparam.0 as u32 == VK_SPACE {
        KeymenuDisposition::PopupSystemMenu
    } else {
        KeymenuDisposition::Default
    }
}

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

unsafe fn apply_system_menu_item_states(menu: HMENU, hwnd: HWND) {
    let maximized = IsZoomed(hwnd).as_bool();
    let minimized = IsIconic(hwnd).as_bool();
    for item in system_menu_item_states(maximized, minimized) {
        let flags = if item.enabled {
            MF_ENABLED
        } else {
            MF_GRAYED
        };
        let _ = EnableMenuItem(menu, item.command, MF_BYCOMMAND | flags);
    }
}

unsafe fn track_system_menu(hwnd: HWND) -> LRESULT {
    let menu = GetSystemMenu(hwnd, GET_SYSTEM_MENU_REVERT);
    debug_assert!(
        !menu.0.is_null(),
        "GetSystemMenu returned null; bRevert must be FALSE"
    );
    if menu.0.is_null() {
        return LRESULT(0);
    }
    apply_system_menu_item_states(menu, hwnd);
    let mut point = POINT {
        x: 0,
        y: CUSTOM_TITLEBAR_HEIGHT_PX,
    };
    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_ok() {
        point.x = rect.left;
        point.y = rect.top + CUSTOM_TITLEBAR_HEIGHT_PX;
    }
    let flags = TPM_RETURNCMD.0 | TPM_RIGHTBUTTON.0;
    let picked = TrackPopupMenuEx(menu, flags, point.x, point.y, hwnd, None);
    let command = picked.0 as u32;
    if command != 0 {
        let _ = PostMessageW(hwnd, WM_SYSCOMMAND, WPARAM(command as usize), LPARAM(0));
    }
    LRESULT(0)
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
            if cmd == SC_KEYMENU {
                match keymenu_disposition(lparam) {
                    KeymenuDisposition::Swallow => return LRESULT(0),
                    KeymenuDisposition::PopupSystemMenu => {
                        return unsafe { track_system_menu(hwnd) };
                    }
                    KeymenuDisposition::Default => {}
                }
            }
        }
        WM_CLOSE if key != main => {
            return LRESULT(0);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_alt_is_swallowed() {
        assert_eq!(keymenu_disposition(LPARAM(0)), KeymenuDisposition::Swallow);
    }

    #[test]
    fn alt_space_uses_popup_not_modal_loop() {
        assert_eq!(
            keymenu_disposition(LPARAM(VK_SPACE as isize)),
            KeymenuDisposition::PopupSystemMenu
        );
    }

    #[test]
    fn alt_letter_shortcuts_still_pass_through() {
        assert_eq!(
            keymenu_disposition(LPARAM(b'F' as isize)),
            KeymenuDisposition::Default
        );
    }

    fn enabled(commands: &[SystemMenuItemState], cmd: u32) -> bool {
        commands
            .iter()
            .find(|item| item.command == cmd)
            .expect("command")
            .enabled
    }

    #[test]
    fn system_menu_normal_state_matches_defwindowproc() {
        let states = system_menu_item_states(false, false);
        assert!(!enabled(&states, SC_RESTORE));
        assert!(enabled(&states, SC_MOVE));
        assert!(enabled(&states, SC_SIZE));
        assert!(enabled(&states, SC_MINIMIZE));
        assert!(enabled(&states, SC_MAXIMIZE));
    }

    #[test]
    fn system_menu_maximized_state_matches_defwindowproc() {
        let states = system_menu_item_states(true, false);
        assert!(enabled(&states, SC_RESTORE));
        assert!(!enabled(&states, SC_MOVE));
        assert!(!enabled(&states, SC_SIZE));
        assert!(enabled(&states, SC_MINIMIZE));
        assert!(!enabled(&states, SC_MAXIMIZE));
    }

    #[test]
    fn system_menu_minimized_state_matches_defwindowproc() {
        let states = system_menu_item_states(false, true);
        assert!(enabled(&states, SC_RESTORE));
        assert!(enabled(&states, SC_MOVE));
        assert!(!enabled(&states, SC_SIZE));
        assert!(!enabled(&states, SC_MINIMIZE));
        assert!(enabled(&states, SC_MAXIMIZE));
    }

    #[test]
    fn get_system_menu_revert_is_false() {
        assert!(
            !GET_SYSTEM_MENU_REVERT.as_bool(),
            "GetSystemMenu(..., true) resets the menu and returns a null handle"
        );
    }
}