use std::{io, ptr, sync::OnceLock, sync::mpsc, thread, time::Duration};

use windows_sys::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute},
        Graphics::Gdi::{
            BeginPaint, COLOR_WINDOW, COLOR_WINDOWTEXT, CreateFontW, DT_CALCRECT, DT_EDITCONTROL,
            DT_END_ELLIPSIS, DT_NOPREFIX, DT_WORDBREAK, DeleteObject, DrawTextW, EndPaint,
            FW_NORMAL, FillRect, GetDC, GetMonitorInfoW, GetSysColor, GetSysColorBrush, HDC,
            MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow, PAINTSTRUCT, ReleaseDC,
            SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
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
const TEXT_FLAGS: u32 = DT_NOPREFIX | DT_WORDBREAK | DT_EDITCONTROL;
const HORIZONTAL_PADDING: i32 = 16;
const VERTICAL_PADDING: i32 = 11;

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
    unsafe {
        let foreground = GetForegroundWindow();
        let monitor = MonitorFromWindow(foreground, MONITOR_DEFAULTTOPRIMARY);
        if GetMonitorInfoW(monitor, &mut monitor_info) == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    let work_area = monitor_info.rcWork;

    // SAFETY: The registered class and zero-terminated window name remain valid
    // throughout creation. The timer is processed by the window's owning thread.
    unsafe {
        let window = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            CLASS_NAME,
            message.as_ptr(),
            WS_POPUP | WS_BORDER,
            work_area.left,
            work_area.top,
            1,
            1,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        );
        if window.is_null() {
            return Err(io::Error::last_os_error());
        }
        let dpi = GetDpiForWindow(window).max(96);
        let max_width = scale(368, dpi).min(work_area.right - work_area.left);
        let (width, height) = measure_popup(window, message, dpi, max_width);
        let height = height.min(work_area.bottom - work_area.top);
        let margin = scale(16, dpi);
        let left = (work_area.right - width - margin).max(work_area.left);
        let top = (work_area.bottom - height - margin).max(work_area.top);
        let corner = DWMWCP_ROUND;
        // Older Windows builds can leave the standard window border in place.
        let _ = DwmSetWindowAttribute(
            window,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            ptr::from_ref(&corner).cast(),
            std::mem::size_of_val(&corner) as u32,
        );
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

fn measure_popup(window: HWND, message: &[u16], dpi: u32, max_width: i32) -> (i32, i32) {
    // Allow for the popup's non-client border in addition to the text padding.
    let frame = scale(4, dpi);
    let horizontal_inset = 2 * scale(HORIZONTAL_PADDING, dpi) + frame;
    let vertical_inset = 2 * scale(VERTICAL_PADDING, dpi) + frame;
    let mut bounds = RECT {
        right: (max_width - horizontal_inset).max(1),
        ..Default::default()
    };
    // SAFETY: The hidden popup is owned by this thread, and message is null-terminated.
    unsafe {
        let dc = GetDC(window);
        if !dc.is_null() {
            with_text_font(dc, dpi, || {
                DrawTextW(
                    dc,
                    message.as_ptr(),
                    -1,
                    &mut bounds,
                    TEXT_FLAGS | DT_CALCRECT,
                );
            });
            ReleaseDC(window, dc);
        }
    }
    let width = (bounds.right + horizontal_inset).max(scale(148, dpi));
    (
        width.min(max_width),
        bounds.bottom.max(scale(21, dpi)) + vertical_inset,
    )
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
        let x = scale(HORIZONTAL_PADDING, dpi);
        let y = scale(VERTICAL_PADDING, dpi);
        let mut bounds = RECT {
            left: x,
            top: y,
            right: rect.right - x,
            bottom: rect.bottom - y,
        };
        let length = GetWindowTextW(window, message.as_mut_ptr(), message.len() as i32);
        if length > 0 {
            with_text_font(dc, dpi, || {
                DrawTextW(
                    dc,
                    message.as_ptr(),
                    length,
                    &mut bounds,
                    TEXT_FLAGS | DT_END_ELLIPSIS,
                );
            });
        }
        EndPaint(window, &paint);
    }
}

fn with_text_font(dc: HDC, dpi: u32, operation: impl FnOnce()) {
    // SAFETY: The caller holds the DC for the duration of the operation.
    // The original font is restored before disposing of the temporary font.
    unsafe {
        let font = CreateFontW(
            -scale(16, dpi),
            0,
            0,
            0,
            FW_NORMAL as i32,
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
        operation();
        if !old_font.is_null() {
            SelectObject(dc, old_font);
        }
        if !font.is_null() {
            DeleteObject(font);
        }
    }
}

fn scale(pixels: i32, dpi: u32) -> i32 {
    (pixels * dpi as i32 + 48) / 96
}
