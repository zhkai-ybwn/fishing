use std::{
    ptr, thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HWND, LPARAM},
    Globalization::{CSTR_EQUAL, CompareStringOrdinal},
    Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute},
    System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    },
    UI::WindowsAndMessaging::{
        EnumWindows, GW_HWNDPREV, GW_OWNER, GWL_EXSTYLE, GetForegroundWindow, GetWindow,
        GetWindowLongW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, HWND_TOP,
        IsIconic, IsWindowVisible, SW_RESTORE, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOOWNERZORDER, SWP_NOSIZE, SetForegroundWindow, SetWindowPos, ShowWindowAsync,
        WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    },
};

const RESTORE_WAIT: Duration = Duration::from_millis(500);
const POSITION_WAIT: Duration = Duration::from_millis(250);
const WINDOW_POLL_INTERVAL: Duration = Duration::from_millis(15);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowInfo {
    pub title: String,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowTarget {
    pub title: Option<String>,
    pub path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowActivation {
    Foreground,
    Raised,
}

struct DesktopWindow {
    handle: HWND,
    process_id: u32,
    info: WindowInfo,
}

pub fn windows() -> Vec<WindowInfo> {
    let mut entries: Vec<_> = desktop_windows()
        .into_iter()
        .map(|window| window.info)
        .collect();
    entries.sort_by(|first, second| {
        first
            .path
            .cmp(&second.path)
            .then(first.title.cmp(&second.title))
    });
    entries.dedup();
    entries
}

pub fn activate(target: &WindowTarget) -> Result<WindowActivation, String> {
    let candidates = desktop_windows();
    let window = select_target(&candidates, target)?;
    let handle = window.handle;

    // The program can close or replace a window between enumeration and activation.
    validate_target(window, target)?;

    // SAFETY: The handle was validated above. This async request is followed
    // by bounded observation, because success only means it was submitted.
    if unsafe { IsIconic(handle) } != 0 {
        let requested = unsafe { ShowWindowAsync(handle, SW_RESTORE) } != 0;
        if wait_for(requested, RESTORE_WAIT, || {
            window_is_displayed(window).then_some(())
        })
        .is_none()
        {
            return Err("未能从最小化状态恢复目标窗口。".into());
        }
    }

    validate_target(window, target)?;
    if let Some(activation) = observed_activation(window, false) {
        return Ok(activation);
    }
    // SAFETY: The selected window's identity was revalidated. Win32 may decline
    // this request under the system foreground policy.
    let requested = unsafe { SetForegroundWindow(handle) } != 0;
    if let Some(activation) = wait_for(requested, POSITION_WAIT, || {
        observed_activation(window, false)
    }) {
        return Ok(activation);
    }

    validate_target(window, target)?;
    // SAFETY: HWND_TOP keeps the window in its current topmost or non-topmost
    // group. The async request retains size, position, owner order and focus.
    let requested = unsafe {
        SetWindowPos(
            handle,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_ASYNCWINDOWPOS | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOOWNERZORDER,
        )
    } != 0;
    wait_for(requested, POSITION_WAIT, || {
        observed_activation(window, true)
    })
    .ok_or_else(|| "Windows 未将目标窗口切到前台或移到当前窗口前面。".into())
}

fn validate_target(window: &DesktopWindow, target: &WindowTarget) -> Result<(), String> {
    if inspect_window(window.handle).is_some_and(|candidate| {
        candidate.process_id == window.process_id && matches_target(&candidate.info, target)
    }) {
        Ok(())
    } else {
        Err("目标窗口已关闭或发生变化。".into())
    }
}

fn window_is_displayed(window: &DesktopWindow) -> bool {
    let mut process_id = 0;
    // SAFETY: The process ID pointer is writable and stale handles cause Win32
    // queries to return failure. A recycled handle must still belong to the target.
    unsafe {
        GetWindowThreadProcessId(window.handle, &mut process_id) != 0
            && process_id == window.process_id
            && IsWindowVisible(window.handle) != 0
            && IsIconic(window.handle) == 0
            && !is_cloaked(window.handle)
    }
}

fn observed_activation(window: &DesktopWindow, allow_raise: bool) -> Option<WindowActivation> {
    if !window_is_displayed(window) {
        return None;
    }
    // SAFETY: GetForegroundWindow has no arguments or preconditions.
    let foreground = unsafe { GetForegroundWindow() };
    if foreground == window.handle {
        return Some(WindowActivation::Foreground);
    }
    if !allow_raise || foreground.is_null() {
        return None;
    }

    let raised = is_above(foreground, window.handle, |handle| {
        // SAFETY: The previous window query handles a window closing concurrently.
        unsafe { GetWindow(handle, GW_HWNDPREV) }
    });
    // SAFETY: Recheck that the foreground has not changed during the Z-order walk.
    (raised && unsafe { GetForegroundWindow() } == foreground && window_is_displayed(window))
        .then_some(WindowActivation::Raised)
}

fn is_above(mut position: HWND, target: HWND, mut previous: impl FnMut(HWND) -> HWND) -> bool {
    // The live Z-order may change mid-walk. Keep each observation bounded.
    for _ in 0..4096 {
        position = previous(position);
        if position.is_null() {
            return false;
        }
        if position == target {
            return true;
        }
    }
    false
}

fn wait_for<T>(
    requested: bool,
    timeout: Duration,
    mut observe: impl FnMut() -> Option<T>,
) -> Option<T> {
    let deadline = Instant::now() + if requested { timeout } else { Duration::ZERO };
    loop {
        if let Some(value) = observe() {
            return Some(value);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return None;
        }
        thread::sleep(remaining.min(WINDOW_POLL_INTERVAL));
    }
}

fn select_target<'a>(
    candidates: &'a [DesktopWindow],
    target: &WindowTarget,
) -> Result<&'a DesktopWindow, String> {
    let mut matches = candidates
        .iter()
        .filter(|candidate| matches_target(&candidate.info, target));
    let first = matches
        .next()
        .ok_or_else(|| "找不到目标窗口，请打开目标应用或重新选择。".to_owned())?;
    if matches.next().is_some() {
        return Err("找到多个符合条件的窗口，请选择可区分的窗口。".into());
    }
    Ok(first)
}

fn matches_target(info: &WindowInfo, target: &WindowTarget) -> bool {
    let first: Vec<u16> = info.path.encode_utf16().collect();
    let second: Vec<u16> = target.path.encode_utf16().collect();
    // SAFETY: Both buffers are valid for their explicitly supplied lengths.
    let same_path = unsafe {
        CompareStringOrdinal(
            first.as_ptr(),
            first.len() as i32,
            second.as_ptr(),
            second.len() as i32,
            1,
        ) == CSTR_EQUAL
    };
    same_path
        && target
            .title
            .as_ref()
            .is_none_or(|title| info.title == *title)
}

fn desktop_windows() -> Vec<DesktopWindow> {
    let mut windows = Vec::new();
    // SAFETY: EnumWindows invokes the callback synchronously while `windows`
    // remains valid and exclusively available to the callback.
    unsafe {
        EnumWindows(
            Some(collect_window),
            (&mut windows as *mut Vec<DesktopWindow>) as LPARAM,
        )
    };
    windows
}

unsafe extern "system" fn collect_window(handle: HWND, context: LPARAM) -> i32 {
    if let Some(window) = inspect_window(handle) {
        // SAFETY: desktop_windows provides the live vector as callback context.
        unsafe { &mut *(context as *mut Vec<DesktopWindow>) }.push(window);
    }
    1
}

fn inspect_window(handle: HWND) -> Option<DesktopWindow> {
    let mut process_id = 0;
    // SAFETY: Win32 receives valid, correctly sized output buffers. The calls
    // handle windows that close during enumeration by returning failure.
    let text_length = unsafe {
        if IsWindowVisible(handle) == 0
            || GetWindowThreadProcessId(handle, &mut process_id) == 0
            || process_id == std::process::id()
        {
            return None;
        }

        let style = GetWindowLongW(handle, GWL_EXSTYLE) as u32;
        if style & WS_EX_APPWINDOW == 0
            && (style & WS_EX_TOOLWINDOW != 0 || !GetWindow(handle, GW_OWNER).is_null())
        {
            return None;
        }

        if is_cloaked(handle) {
            return None;
        }

        GetWindowTextLengthW(handle)
    };
    if text_length <= 0 {
        return None;
    }

    let mut title = vec![0u16; text_length.min(32_767) as usize + 1];
    // SAFETY: The title buffer is writable for the supplied number of items.
    let written = unsafe { GetWindowTextW(handle, title.as_mut_ptr(), title.len() as i32) };
    if written <= 0 {
        return None;
    }
    let title = String::from_utf16_lossy(&title[..written as usize]);
    if title.trim().is_empty() {
        return None;
    }
    Some(DesktopWindow {
        handle,
        process_id,
        info: WindowInfo {
            title,
            path: executable_path(process_id)?,
        },
    })
}

fn is_cloaked(handle: HWND) -> bool {
    let mut cloaked = 0u32;
    // SAFETY: The DWORD output buffer is writable and correctly sized.
    let result = unsafe {
        DwmGetWindowAttribute(
            handle,
            DWMWA_CLOAKED as u32,
            ptr::from_mut(&mut cloaked).cast(),
            std::mem::size_of_val(&cloaked) as u32,
        )
    };
    result >= 0 && cloaked != 0
}

fn executable_path(process_id: u32) -> Option<String> {
    let mut path = vec![0u16; 32_768];
    let mut length = path.len() as u32;
    // SAFETY: The output buffer is large enough for the supplied length and
    // the process handle is closed exactly once after the query.
    let ok = unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
        if process.is_null() {
            return None;
        }
        let ok = QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length);
        CloseHandle(process);
        ok != 0
    };
    ok.then(|| String::from_utf16_lossy(&path[..length as usize]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(title: &str) -> DesktopWindow {
        DesktopWindow {
            handle: ptr::null_mut(),
            process_id: 0,
            info: WindowInfo {
                title: title.to_owned(),
                path: "C:\\Programs\\Editor.exe".to_owned(),
            },
        }
    }

    #[test]
    fn ambiguous_apps_require_a_specific_window() {
        let candidates = [window("Test notes"), window("Work notes")];
        let mut target = WindowTarget {
            title: None,
            path: "c:\\programs\\editor.EXE".to_owned(),
        };
        assert!(select_target(&candidates, &target).is_err());

        target.title = Some("Work notes".to_owned());
        assert_eq!(
            select_target(&candidates, &target).unwrap().info.title,
            "Work notes"
        );

        target.title = Some("Gone".to_owned());
        assert!(select_target(&candidates, &target).is_err());
    }

    #[test]
    fn z_order_check_requires_target_above_foreground() {
        let higher = ptr::without_provenance_mut(1);
        let foreground = ptr::without_provenance_mut(2);
        let lower = ptr::without_provenance_mut(3);
        let previous = |position| match position {
            position if position == lower => foreground,
            position if position == foreground => higher,
            _ => ptr::null_mut(),
        };
        assert!(is_above(foreground, higher, previous));
        assert!(!is_above(foreground, lower, previous));
        assert!(!is_above(foreground, foreground, previous));
    }
}
