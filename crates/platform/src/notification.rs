use std::{io, ptr, sync::OnceLock, sync::mpsc, thread, time::Duration};

use windows::{
    UI::ViewManagement::{AccessibilitySettings, UIColorType, UISettings},
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
};
use windows_sys::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Dwm::{
            DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
        },
        Graphics::Gdi::{
            BeginPaint, COLOR_WINDOW, COLOR_WINDOWTEXT, CreateFontW, CreatePen, CreateSolidBrush,
            DT_CALCRECT, DT_EDITCONTROL, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER,
            DT_WORDBREAK, DeleteObject, DrawTextW, Ellipse, EndPaint, FW_NORMAL, FW_SEMIBOLD,
            FillRect, GetDC, GetMonitorInfoW, GetStockObject, GetSysColor, GetSysColorBrush, HDC,
            LineTo, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow, MoveToEx, NULL_BRUSH,
            NULL_PEN, PAINTSTRUCT, PS_SOLID, ReleaseDC, SelectObject, SetBkMode, SetTextColor,
            TRANSPARENT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
                GetClientRect, GetForegroundWindow, GetMessageW, GetWindowLongPtrW, GetWindowTextW,
                HWND_TOPMOST, MA_NOACTIVATE, MSG, PostQuitMessage, RegisterClassW, SWP_NOACTIVATE,
                SWP_SHOWWINDOW, SetTimer, SetWindowLongPtrW, SetWindowPos, WM_DESTROY,
                WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_PAINT, WM_TIMER, WNDCLASSW, WS_BORDER,
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
const VERTICAL_PADDING: i32 = 14;
const CLOCK_SIZE: i32 = 30;
const TEXT_LEFT: i32 = HORIZONTAL_PADDING + CLOCK_SIZE + 12;
const BODY_TOP: i32 = VERTICAL_PADDING + 22;

#[derive(Clone, Copy)]
#[repr(isize)]
enum Appearance {
    Light,
    Dark,
    System,
}

struct Palette {
    background: u32,
    title: u32,
    body: u32,
    clock: u32,
    clock_background: u32,
    border: u32,
}

impl Appearance {
    fn detect() -> Self {
        // SAFETY: Successful initialization of the popup thread is paired with RoUninitialize.
        unsafe {
            if RoInitialize(RO_INIT_MULTITHREADED).is_err() {
                return Self::System;
            }
            let appearance = Self::query_windows_theme().unwrap_or(Self::System);
            RoUninitialize();
            appearance
        }
    }

    fn query_windows_theme() -> windows::core::Result<Self> {
        if AccessibilitySettings::new()?.HighContrast()? {
            return Ok(Self::System);
        }
        let color = UISettings::new()?.GetColorValue(UIColorType::Foreground)?;
        let foreground_brightness =
            2 * u32::from(color.R) + 5 * u32::from(color.G) + u32::from(color.B);
        Ok(if foreground_brightness > 8 * 128 {
            Self::Dark
        } else {
            Self::Light
        })
    }

    fn palette(self) -> Palette {
        match self {
            Self::Light => Palette {
                background: rgb(248, 250, 249),
                title: rgb(69, 85, 81),
                body: rgb(22, 35, 33),
                clock: rgb(45, 92, 82),
                clock_background: rgb(226, 238, 234),
                border: rgb(209, 221, 217),
            },
            Self::Dark => Palette {
                background: rgb(31, 38, 36),
                title: rgb(190, 202, 198),
                body: rgb(242, 247, 244),
                clock: rgb(171, 217, 203),
                clock_background: rgb(47, 65, 60),
                border: rgb(71, 88, 82),
            },
            Self::System => {
                // SAFETY: These color indexes correspond to the selected contrast theme.
                let background = unsafe { GetSysColor(COLOR_WINDOW) };
                let ink = unsafe { GetSysColor(COLOR_WINDOWTEXT) };
                Palette {
                    background,
                    title: ink,
                    body: ink,
                    clock: ink,
                    clock_background: background,
                    border: ink,
                }
            }
        }
    }
}

const fn rgb(red: u8, green: u8, blue: u8) -> u32 {
    red as u32 | ((green as u32) << 8) | ((blue as u32) << 16)
}

/// Display a brief alert in the current monitor's work area without activating it.
pub fn notify(message: &str) -> Result<(), String> {
    let message: String = message
        .chars()
        .take_while(|character| *character != '\0')
        .take(240)
        .collect();
    let display_time =
        DISPLAY_TIME_MS + (message.chars().count().saturating_sub(40) as u32 * 50).min(2500);
    let message: Vec<u16> = message.encode_utf16().chain([0]).collect();
    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("boss-sensor-notification".to_owned())
        .spawn(move || match show_popup(&message, display_time) {
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

fn show_popup(message: &[u16], display_time: u32) -> io::Result<()> {
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
    let appearance = Appearance::detect();

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
        SetWindowLongPtrW(window, GWLP_USERDATA, appearance as isize);
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
        let border = appearance.palette().border;
        let _ = DwmSetWindowAttribute(
            window,
            DWMWA_BORDER_COLOR as u32,
            ptr::from_ref(&border).cast(),
            std::mem::size_of_val(&border) as u32,
        );
        if SetTimer(window, 1, display_time, None) == 0
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
    let horizontal_inset = scale(TEXT_LEFT, dpi) + scale(HORIZONTAL_PADDING, dpi) + frame;
    let vertical_inset = scale(BODY_TOP, dpi) + scale(VERTICAL_PADDING, dpi) + frame;
    let mut bounds = RECT {
        right: (max_width - horizontal_inset).max(1),
        ..Default::default()
    };
    // SAFETY: The hidden popup is owned by this thread, and message is null-terminated.
    unsafe {
        let dc = GetDC(window);
        if !dc.is_null() {
            with_text_font(dc, scale(16, dpi), FW_NORMAL, || {
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
    let width = (bounds.right + horizontal_inset).max(scale(232, dpi));
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
            WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
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
        let appearance = match GetWindowLongPtrW(window, GWLP_USERDATA) {
            appearance if appearance == Appearance::Dark as isize => Appearance::Dark,
            appearance if appearance == Appearance::System as isize => Appearance::System,
            _ => Appearance::Light,
        };
        let palette = appearance.palette();
        let background = CreateSolidBrush(palette.background);
        if background.is_null() {
            FillRect(dc, &rect, GetSysColorBrush(COLOR_WINDOW));
        } else {
            FillRect(dc, &rect, background);
            DeleteObject(background);
        }
        SetBkMode(dc, TRANSPARENT as i32);
        let dpi = GetDpiForWindow(window).max(96);
        paint_clock(dc, dpi, &palette);

        let left = scale(TEXT_LEFT, dpi);
        let right = rect.right - scale(HORIZONTAL_PADDING, dpi);
        let mut title = RECT {
            left,
            top: scale(VERTICAL_PADDING, dpi),
            right,
            bottom: scale(BODY_TOP - 2, dpi),
        };
        SetTextColor(dc, palette.title);
        with_text_font(dc, scale(14, dpi), FW_SEMIBOLD, || {
            DrawTextW(
                dc,
                w!("专注提醒"),
                -1,
                &mut title,
                DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS,
            );
        });

        let mut bounds = RECT {
            left,
            top: scale(BODY_TOP, dpi),
            right,
            bottom: rect.bottom - scale(VERTICAL_PADDING, dpi),
        };
        let length = GetWindowTextW(window, message.as_mut_ptr(), message.len() as i32);
        if length > 0 {
            SetTextColor(dc, palette.body);
            with_text_font(dc, scale(16, dpi), FW_NORMAL, || {
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

fn paint_clock(dc: HDC, dpi: u32, palette: &Palette) {
    let left = scale(HORIZONTAL_PADDING, dpi);
    let top = scale(VERTICAL_PADDING, dpi);
    let diameter = scale(CLOCK_SIZE, dpi);
    let x = left + diameter / 2;
    let y = top + diameter / 2;
    let radius = scale(8, dpi);
    // SAFETY: The caller owns the paint DC. Restore its pen and brush before deleting ours.
    unsafe {
        let background = CreateSolidBrush(palette.clock_background);
        let stroke = CreatePen(PS_SOLID, scale(1, dpi).max(1), palette.clock);
        if !background.is_null() && !stroke.is_null() {
            let saved_pen = SelectObject(dc, GetStockObject(NULL_PEN));
            let saved_brush = SelectObject(dc, background);
            Ellipse(dc, left, top, left + diameter, top + diameter);

            SelectObject(dc, stroke);
            SelectObject(dc, GetStockObject(NULL_BRUSH));
            Ellipse(dc, x - radius, y - radius, x + radius + 1, y + radius + 1);
            MoveToEx(dc, x, y - scale(4, dpi), ptr::null_mut());
            LineTo(dc, x, y);
            LineTo(dc, x + scale(4, dpi), y + scale(2, dpi));

            SelectObject(dc, saved_pen);
            SelectObject(dc, saved_brush);
        }
        if !background.is_null() {
            DeleteObject(background);
        }
        if !stroke.is_null() {
            DeleteObject(stroke);
        }
    }
}

fn with_text_font(dc: HDC, size: i32, weight: u32, operation: impl FnOnce()) {
    // SAFETY: The caller holds the DC for the duration of the operation.
    // The original font is restored before disposing of the temporary font.
    unsafe {
        let font = CreateFontW(
            -size,
            0,
            0,
            0,
            weight as i32,
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
