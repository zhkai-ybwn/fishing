use std::{io, ptr, sync::OnceLock, sync::mpsc, thread, time::Duration};

use windows_sys::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, COLOR_WINDOW, COLOR_WINDOWTEXT, CreateFontW, DT_END_ELLIPSIS, DT_NOPREFIX,
            DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FillRect,
            GetMonitorInfoW, GetSysColor, GetSysColorBrush, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
            MonitorFromWindow, PAINTSTRUCT, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
                GetForegroundWindow, GetMessageW, GetWindowTextW, HWND_TOPMOST, MSG,
                PostQuitMessage, RegisterClassW, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetTimer,
                SetWindowPos, WM_DESTROY, WM_LBUTTONUP, WM_PAINT, WM_TIMER, WNDCLASSW, WS_BORDER,
                WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
            },
        },
    },
    core::PCWSTR,
    w,
};

const CLASS_NAME: PCWSTR = w!("BossSensor.Notification");
const DISPLAY_TIME_MS: u32 = 3500;

/// Display a brief alert in the current monitor's work area without activating it.
pub fn notify(message: &str) -> Result<(), String> {
    let message: Vec<u16> = message
        .encode_utf16()
        .take_while(|code_unit| *code_unit != 0)
        .chain([0])
        .collect();
    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("boss-sensor-notification".to_owned())
        .spawn(move || match show_popup(&message) {
            Ok(()) => {
                let _ = result_sender.send(Ok(()));
                let mut message = MSG::default();
                // SAFETY: This is the window's owning thread. The MSG output is
                // initialized by GetMessageW before dispatching it.
                unsafe {
                    while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
                        DispatchMessageW(&message);
                    }
                }
            }
            Err(error) => {
                let _ = result_sender.send(Err(format!("无法显示桌面提示（{error}）。")));
            }
        })
        .map_err(|error| format!("无法启动桌面提示（{error}）。"))?;
    result_receiver
        .recv_timeout(Duration::from_secs(1))
        .map_err(|_| "等待桌面提示超时。".to_owned())?
}

fn show_popup(message: &[u16]) -> io::Result<()> {
    // SAFETY: The null name requests the handle for this process image.
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    if instance.is_null() {
        return Err(io::Error::last_os_error());
    }
    register_class(instance)?;

    let mut monitor_info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: The monitor info has the required size and a writable address.
    let dpi = unsafe {
        let foreground = GetForegroundWindow();
        let monitor = MonitorFromWindow(foreground, MONITOR_DEFAULTTOPRIMARY);
        if GetMonitorInfoW(monitor, &mut monitor_info) == 0 {
            return Err(io::Error::last_os_error());
        }
        GetDpiForWindow(foreground).max(96)
    };
    let width = scale(365, dpi).min(monitor_info.rcWork.right - monitor_info.rcWork.left);
    let height = scale(58, dpi).min(monitor_info.rcWork.bottom - monitor_info.rcWork.top);
    let margin = scale(16, dpi);
    let left = (monitor_info.rcWork.right - width - margin).max(monitor_info.rcWork.left);
    let top = (monitor_info.rcWork.bottom - height - margin).max(monitor_info.rcWork.top);

    // SAFETY: The registered class and zero-terminated window name remain valid
    // throughout creation. The timer is processed by the window's owning thread.
    unsafe {
        let window = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            CLASS_NAME,
            message.as_ptr(),
            WS_POPUP | WS_BORDER,
            left,
            top,
            width,
            height,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        );
        if window.is_null() {
            return Err(io::Error::last_os_error());
        }
        if SetTimer(window, 1, DISPLAY_TIME_MS, None) == 0
            || SetWindowPos(
                window,
                HWND_TOPMOST,
                left,
                top,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            ) == 0
        {
            let error = io::Error::last_os_error();
            DestroyWindow(window);
            return Err(error);
        }
    }
    Ok(())
}

fn register_class(instance: HINSTANCE) -> io::Result<()> {
    static REGISTERED: OnceLock<Result<(), i32>> = OnceLock::new();
    REGISTERED
        .get_or_init(|| {
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: CLASS_NAME,
                ..Default::default()
            };
            // SAFETY: Win32 copies the class data while the static class name and
            // registered window procedure live for the duration of the process.
            if unsafe { RegisterClassW(&class) } == 0 {
                return Err(io::Error::last_os_error().raw_os_error().unwrap_or(1));
            }
            Ok(())
        })
        .map_err(io::Error::from_raw_os_error)
}

unsafe extern "system" fn window_proc(
    window: HWND,
    event: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: Windows calls this function on the thread that owns the handle,
    // with the message's original parameters.
    unsafe {
        match event {
            WM_PAINT => {
                paint(window);
                0
            }
            WM_TIMER | WM_LBUTTONUP => {
                DestroyWindow(window);
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(window, event, wparam, lparam),
        }
    }
}

fn paint(window: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let mut rect = RECT::default();
    let mut message = [0u16; 512];
    // SAFETY: We handle WM_PAINT on the owning thread. Each GDI object is
    // detached before deletion and the paint DC is released by EndPaint.
    unsafe {
        let dc = BeginPaint(window, &mut paint);
        if dc.is_null() {
            return;
        }
        GetClientRect(window, &mut rect);
        FillRect(dc, &rect, GetSysColorBrush(COLOR_WINDOW));
        SetTextColor(dc, GetSysColor(COLOR_WINDOWTEXT));
        SetBkMode(dc, TRANSPARENT as i32);
        let dpi = GetDpiForWindow(window).max(96);
        rect.left += scale(18, dpi);
        rect.right -= scale(18, dpi);
        let font = CreateFontW(
            -scale(17, dpi),
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            1,
            0,
            0,
            0,
            0,
            w!("Segoe UI"),
        );
        let old_font = if font.is_null() {
            ptr::null_mut()
        } else {
            SelectObject(dc, font)
        };
        let length = GetWindowTextW(window, message.as_mut_ptr(), message.len() as i32);
        DrawTextW(
            dc,
            message.as_ptr(),
            length,
            &mut rect,
            DT_NOPREFIX | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
        );
        if !old_font.is_null() {
            SelectObject(dc, old_font);
        }
        if !font.is_null() {
            DeleteObject(font);
        }
        EndPaint(window, &paint);
    }
}

fn scale(pixels: i32, dpi: u32) -> i32 {
    (pixels * dpi as i32 + 48) / 96
}
