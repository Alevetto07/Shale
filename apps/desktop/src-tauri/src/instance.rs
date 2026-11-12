//! One Shale at a time.
//!
//! The window's profile is a directory WebView2 locks (`EBWebView`), and a
//! second process that opens it does not get an error it can show: creation
//! fails with `HRESULT(0x800700AA)`, "The requested resource is in use", and
//! Tauri's setup unwinds. No window, no sentence, no hint — double-clicking the
//! So: a named mutex is claimed before anything opens, a second launch brings
//! the window that already exists to the front, and a start that fails for any
//! other reason — a `msedgewebview2` left behind by a crash, most of the time —
//! gets one retry as a fresh process and then a native box, not a panic.
//!
//! The mutex is a courtesy, not a lock on data: if it cannot be created the app
//! starts anyway, because failing to open must never be the more likely outcome.

use std::sync::atomic::{AtomicPtr, Ordering};

/// The mutex handle, so a retry can let go of the name before it re-execs.
static MUTEX: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

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
        MUTEX.store(handle, Ordering::SeqCst);
        GetLastError() != ERROR_ALREADY_EXISTS
    }
}

/// Let go of the mutex before spawning the process that will claim it again.
#[cfg(windows)]
pub fn release() {
    #[link(name = "kernel32")]
    extern "system" {
        fn CloseHandle(handle: *mut core::ffi::c_void) -> i32;
    }

    let handle = MUTEX.swap(std::ptr::null_mut(), Ordering::SeqCst);
    if !handle.is_null() {
        // Safe: the handle came from CreateMutexW and is closed once.
        unsafe {
            CloseHandle(handle);
        }
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

/// Show a box, in the foreground, and return when it is dismissed.
#[cfg(windows)]
pub fn say(caption: &str, message: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    /// An information box that comes to the front: a starting process has no
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

    let wide = |text: &str| -> Vec<u16> {
        OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let text = wide(message);
    let caption = wide(caption);
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

/// The box for a second launch that has no window to raise.
/// The fallback for when there is no window to bring forward.
#[cfg(windows)]
pub fn say_already_running() {
    say(
        "Shale",
        "Shale is already running.\n\nIts window is behind this one — close it, or\nuse the taskbar, before starting another.",
    );
}

/// The sentence a failed start is worth, given the error behind it.
///
/// The `0x800700AA` case is WebView2 refusing a profile that a process still
/// holds — nearly always a `msedgewebview2` left behind when Shale was killed
/// rather than closed. Nothing about the number is useful to the person reading
/// it and the fix is a wait, so the sentence says that instead.
pub fn start_failed_message(detail: &str) -> String {
    let lower = detail.to_ascii_lowercase();
    let in_use = lower.contains("0x800700aa") || lower.contains("in use");
    if in_use {
        "Shale could not open its window because an earlier Shale is still closing.\n\n\
         Wait a moment, then start Shale again. If this comes back, end the\n\
         msedgewebview2 processes in Task Manager and start Shale once more."
            .to_string()
    } else {
        format!(
            "Shale could not open its window.\n\n{}\n\nThe details are also on standard error.",
            first_lines(detail, 6)
        )
    }
}

/// The box for a window that could not be made, with the reason in plain words.
#[cfg(windows)]
pub fn say_start_failed(detail: &str) {
    say("Shale did not start", &start_failed_message(detail));
}

/// The first `count` non-empty lines of a long error, for a box that must stay
/// readable: Tauri's webview error arrives as one very long debug string.
fn first_lines(text: &str, count: usize) -> String {
    let mut lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.len() > count {
        lines.truncate(count);
        lines.push("…");
    }
    lines.join("\n")
}

#[cfg(not(windows))]
pub fn claim(_name: &str) -> bool {
    true
}

#[cfg(not(windows))]
pub fn release() {}

#[cfg(not(windows))]
pub fn focus_existing(_title: &str) -> bool {
    false
}

#[cfg(not(windows))]
pub fn say(caption: &str, message: &str) {
    eprintln!("{caption}: {message}");
}

#[cfg(not(windows))]
pub fn say_already_running() {}

#[cfg(not(windows))]
pub fn say_start_failed(detail: &str) {
    eprintln!("Shale did not start: {}", start_failed_message(detail));
}

#[cfg(test)]
mod tests {
    use super::{first_lines, start_failed_message};

    #[test]
    fn a_held_profile_is_explained_as_a_wait_not_as_a_number() {
        let message = start_failed_message(
            "failed to create webview: WebView2 error: WindowsError(Error { \
             code: HRESULT(0x800700AA), message: \"The requested resource is in use.\" })",
        );
        assert!(
            message.contains("an earlier Shale is still closing"),
            "{message}"
        );
        assert!(message.contains("start Shale again"), "{message}");
        assert!(
            !message.contains("0x800700AA"),
            "the number is not the sentence: {message}"
        );
    }

    #[test]
    fn any_other_failure_is_quoted_rather_than_summarised() {
        let detail = "failed to create webview: WebView2 error: WindowsError(Error { \
                      code: HRESULT(0x80070005), message: \"Access is denied.\" })";
        let message = start_failed_message(detail);
        assert!(
            message.contains("Shale could not open its window"),
            "{message}"
        );
        assert!(message.contains("0x80070005"), "{message}");
        assert!(message.contains("standard error"), "{message}");
    }

    #[test]
    fn a_long_error_is_cut_to_six_lines_with_an_ellipsis() {
        let detail = (1..=10)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let cut = first_lines(&detail, 6);
        assert_eq!(cut.lines().count(), 7, "{cut}");
        assert!(cut.ends_with('…'), "{cut}");
        assert!(cut.contains("line 6") && !cut.contains("line 7"), "{cut}");
    }
}
