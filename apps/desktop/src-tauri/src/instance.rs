//! One Shale at a time.
//!
//! The window's profile is a directory WebView2 locks (`EBWebView`), and a
//! second process that opens it does not get an error it can show: creation
//! fails with `HRESULT(0x800700AA)`, "The requested resource is in use", and
//! Tauri's setup unwinds. No window, no sentence, no hint — double-clicking the
//! icon twice is enough to do it. A named mutex is claimed before anything else
//! opens, and a second instance says so in a box and leaves.
//!
//! The mutex is a courtesy, not a lock on data: if it cannot be created the app
//! starts anyway, because failing to open must never be the more likely outcome.

/// Claim the single-instance mutex. `true` if this process is the only Shale.
#[cfg(windows)]
pub fn claim(name: &str) -> bool {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    /// `ERROR_ALREADY_EXISTS`: `CreateMutexW` succeeded, but the name was taken.
    const ERROR_ALREADY_EXISTS: u32 = 183;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateMutexW(
            attributes: *const core::ffi::c_void,
            initial_owner: i32,
            name: *const u16,
        ) -> *mut core::ffi::c_void;
        fn GetLastError() -> u32;
    }

    let wide: Vec<u16> = OsStr::new(name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // Safe: no attributes and a null-terminated name are what CreateMutexW
    // documents; the handle is held until the process ends, at which point the
    // system releases the name for the next launch.
    unsafe {
        let handle = CreateMutexW(std::ptr::null(), 0, wide.as_ptr());
        if handle.is_null() {
            return true;
        }
        GetLastError() != ERROR_ALREADY_EXISTS
    }
}

/// Bring the window that already has the profile to the front.
///
/// Failing to start is not the user's problem to solve with a dialogue: the
/// second launch means "show me Shale", and Shale is right there. `false` if no
/// window with this title exists, in which case the box is the fallback.
#[cfg(windows)]
pub fn focus_existing(title: &str) -> bool {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    const SW_RESTORE: i32 = 9;

    #[link(name = "user32")]
    extern "system" {
        fn FindWindowW(class: *const u16, title: *const u16) -> *mut core::ffi::c_void;
        fn ShowWindow(window: *mut core::ffi::c_void, command: i32) -> i32;
        fn SetForegroundWindow(window: *mut core::ffi::c_void) -> i32;
    }

    let wide: Vec<u16> = OsStr::new(title)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // Safe: a null class and a null-terminated title are FindWindowW's
    // documented inputs, and both window calls are no-ops on a null handle.
    unsafe {
        let window = FindWindowW(std::ptr::null(), wide.as_ptr());
        if window.is_null() {
            return false;
        }
        ShowWindow(window, SW_RESTORE);
        SetForegroundWindow(window);
        true
    }
}

/// Tell the user, in a box they can dismiss, that another Shale holds the profile.
/// The fallback for when there is no window to bring forward.
#[cfg(windows)]
pub fn say_already_running() {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    /// An information box that comes to the front: a second process has no
    /// right to the foreground, so without these two flags the box opens behind
    /// whatever the user is looking at, which is how a message goes unread.
    const MB_ICONINFORMATION: u32 = 0x40;
    const MB_SETFOREGROUND: u32 = 0x0001_0000;
    const MB_TOPMOST: u32 = 0x0004_0000;

    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(
            window: *mut core::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            kind: u32,
        ) -> i32;
    }

    let text: Vec<u16> = OsStr::new(
        "Shale is already running.\n\nIts window is behind this one — close it, or\nuse the taskbar, before starting another.",
    )
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let caption: Vec<u16> = OsStr::new("Shale")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // Safe: both strings outlive the call and are null-terminated.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_ICONINFORMATION | MB_SETFOREGROUND | MB_TOPMOST,
        );
    }
}

#[cfg(not(windows))]
pub fn focus_existing(_title: &str) -> bool {
    false
}

#[cfg(not(windows))]
pub fn claim(_name: &str) -> bool {
    true
}

#[cfg(not(windows))]
pub fn say_already_running() {}
