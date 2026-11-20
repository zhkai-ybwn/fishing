use std::{ptr, thread, time::Duration};

use serde::{Deserialize, Serialize};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HWND, LPARAM},
    Globalization::{CSTR_EQUAL, CompareStringOrdinal},
    Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute},
    System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    },
    UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GWL_EXSTYLE, GetForegroundWindow, GetWindow, GetWindowLongW,
        GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
        SW_RESTORE, SetForegroundWindow, ShowWindowAsync, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    },
};

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

struct DesktopWindow {
    handle: HWND,
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

pub fn activate(target: &WindowTarget) -> Result<(), String> {
    let candidates = desktop_windows();
    let window = select_target(&candidates, target)?;
    let handle = window.handle;

    // The program can close or replace a window between enumeration and activation.
    if !inspect_window(handle).is_some_and(|candidate| matches_target(&candidate.info, target)) {
        return Err("目标窗口已关闭或发生变化。".into());
    }

    // SAFETY: These Win32 calls use a window handle examined above. A concurrent
    // window destruction produces a Win32 error, and the result is checked below.
    let accepted = unsafe {
        if GetForegroundWindow() == handle {
            return Ok(());
        }
        if IsIconic(handle) != 0 {
            ShowWindowAsync(handle, SW_RESTORE);
        }
        SetForegroundWindow(handle) != 0
    };

    if is_foreground(handle) {
        return Ok(());
    }
    if accepted {
        for _ in 0..8 {
            thread::sleep(Duration::from_millis(15));
            if is_foreground(handle) {
                return Ok(());
            }
        }
    }
    Err("未能将目标窗口切到前台，Windows 可能限制了后台切换。".into())
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

fn is_foreground(handle: HWND) -> bool {
    // SAFETY: GetForegroundWindow has no arguments or preconditions.
    unsafe { GetForegroundWindow() == handle }
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
    let mut cloaked = 0u32;
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

        if DwmGetWindowAttribute(
            handle,
            DWMWA_CLOAKED as u32,
            ptr::from_mut(&mut cloaked).cast(),
            std::mem::size_of_val(&cloaked) as u32,
        ) >= 0
            && cloaked != 0
        {
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
        info: WindowInfo {
            title,
            path: executable_path(process_id)?,
        },
    })
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
}
