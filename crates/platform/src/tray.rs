use std::{
    cell::Cell,
    io, ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use windows::Win32::{
    Foundation::HWND as ComHwnd,
    System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
        CoInitializeEx, CoUninitialize,
    },
    UI::Shell::{ITaskbarList, TaskbarList},
};
use windows_sys::{
    Win32::{
        Foundation::{
            ERROR_CLASS_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, POINT,
            RECT, WPARAM,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Shell::{
                NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETFOCUS,
                NIM_SETVERSION, NIN_SELECT, NOTIFYICON_VERSION_4, NOTIFYICONDATAW,
                NOTIFYICONIDENTIFIER, Shell_NotifyIconGetRect, Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                AppendMenuW, CREATESTRUCTW, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
                DestroyMenu, DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetCursorPos,
                GetMessageW, GetSystemMetrics, GetWindowLongPtrW, HICON, IDI_APPLICATION,
                IMAGE_ICON, IsIconic, IsWindow, KillTimer, LR_SHARED, LoadIconW, LoadImageW,
                MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW,
                RegisterWindowMessageW, SM_CXSMICON, SM_CYSMICON, SW_RESTORE, SetForegroundWindow,
                SetMenuDefaultItem, SetTimer, SetWindowLongPtrW, ShowWindowAsync, TPM_NONOTIFY,
                TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP, WM_CLOSE,
                WM_CONTEXTMENU, WM_DESTROY, WM_LBUTTONDBLCLK, WM_NCCREATE, WM_NULL, WM_RBUTTONUP,
                WM_TIMER, WNDCLASSW, WS_EX_TOOLWINDOW, WS_OVERLAPPED,
            },
        },
    },
    core::PCWSTR,
    w,
};

const CLASS_NAME: PCWSTR = w!("fishing.Tray");
const ICON_RESOURCE: usize = 1;
const ICON_ID: u32 = 1;
const TRAY_CALLBACK: u32 = WM_APP + 14;
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
const MENU_OPEN: usize = 1;
const MENU_TOGGLE: usize = 2;
const MENU_QUIT: usize = 3;
const RESTORE_TASKBAR_TIMER: usize = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    Open,
    Toggle,
    Quit,
}

pub struct Tray {
    window: isize,
    main_window: isize,
    actions: mpsc::Receiver<TrayAction>,
    running: Arc<AtomicBool>,
    taskbar_hidden: Arc<AtomicBool>,
}

impl Tray {
    /// Create the system-tray icon. The supplied HWND identifies the settings window.
    pub fn new(main_window: isize, wake: impl Fn() + Send + 'static) -> Result<Self, String> {
        if main_window == 0 {
            return Err("找不到设置窗口，无法创建系统托盘图标。".into());
        }
        let (action_sender, actions) = mpsc::channel();
        let (started, startup) = mpsc::sync_channel(1);
        let running = Arc::new(AtomicBool::new(false));
        let taskbar_hidden = Arc::new(AtomicBool::new(false));
        let worker_running = Arc::clone(&running);
        let worker_taskbar = Arc::clone(&taskbar_hidden);
        thread::Builder::new()
            .name("fishing-tray".into())
            .spawn(move || {
                tray_loop(
                    main_window,
                    started,
                    action_sender,
                    worker_running,
                    worker_taskbar,
                    wake,
                )
            })
            .map_err(|error| format!("无法启动系统托盘（{error}）。"))?;

        let window = startup
            .recv_timeout(Duration::from_secs(3))
            .map_err(|error| format!("无法连接系统托盘（{error}）。"))?
            .map_err(|error| format!("无法创建系统托盘图标（{error}）。"))?;
        Ok(Self {
            window,
            main_window,
            actions,
            running,
            taskbar_hidden,
        })
    }

    pub fn next_action(&self) -> Option<TrayAction> {
        self.actions.try_recv().ok()
    }

    pub fn set_running(&self, running: bool) {
        self.running.store(running, Ordering::Relaxed);
    }

    /// Change the settings window's taskbar entry while retaining its HWND.
    pub fn set_taskbar_visible(&self, visible: bool) -> Result<(), String> {
        let window = hwnd(self.main_window);
        // SAFETY: IsWindow accepts an HWND that may already have been destroyed.
        if unsafe { IsWindow(window) } == 0 {
            return Err("设置窗口已关闭。".into());
        }
        change_taskbar(window, visible)
            .map_err(|error| format!("无法更新任务栏图标（{error}）。"))?;
        self.taskbar_hidden.store(!visible, Ordering::Relaxed);
        Ok(())
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let window = hwnd(self.window);
        remove_icon(window);
        // SAFETY: The tray HWND belongs to our message thread. If it is already
        // gone, PostMessageW fails without touching another resource.
        unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
    }
}

struct TrayState {
    main_window: HWND,
    icon: HICON,
    taskbar_created: u32,
    actions: mpsc::Sender<TrayAction>,
    running: Arc<AtomicBool>,
    taskbar_hidden: Arc<AtomicBool>,
    pending_wake: Cell<bool>,
}

fn tray_loop(
    main_window: isize,
    started: mpsc::SyncSender<io::Result<isize>>,
    actions: mpsc::Sender<TrayAction>,
    running: Arc<AtomicBool>,
    taskbar_hidden: Arc<AtomicBool>,
    wake: impl Fn(),
) {
    // SAFETY: These calls use static names and shared resources from the process
    // image or the system module.
    let (instance, icon, taskbar_created) = unsafe {
        let instance = GetModuleHandleW(ptr::null());
        let icon = LoadImageW(
            instance,
            ptr::without_provenance(ICON_RESOURCE),
            IMAGE_ICON,
            GetSystemMetrics(SM_CXSMICON),
            GetSystemMetrics(SM_CYSMICON),
            LR_SHARED,
        );
        let icon = if icon.is_null() {
            LoadIconW(ptr::null_mut(), IDI_APPLICATION)
        } else {
            icon
        };
        (instance, icon, RegisterWindowMessageW(w!("TaskbarCreated")))
    };
    let mut state = TrayState {
        main_window: hwnd(main_window),
        icon,
        taskbar_created,
        actions,
        running,
        taskbar_hidden,
        pending_wake: Cell::new(false),
    };
    let window = match create_window(instance, &mut state) {
        Ok(window) => window,
        Err(error) => {
            let _ = started.send(Err(error));
            return;
        }
    };
    if !add_icon(window, icon) {
        let _ = started.send(Err(io::Error::other("Windows 托盘未接受图标")));
        // SAFETY: This is the window's message thread.
        unsafe { DestroyWindow(window) };
        return;
    }
    if started.send(Ok(window as isize)).is_err() {
        // SAFETY: The listener timed out or quit. Destroy the icon's owning window.
        unsafe { DestroyWindow(window) };
        return;
    }

    let mut message = MSG::default();
    // SAFETY: State lives throughout the window's message loop. TranslateMessage
    // and DispatchMessageW use the MSG initialized by GetMessageW.
    unsafe {
        while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
            if state.pending_wake.replace(false) {
                wake();
            }
        }
        DestroyWindow(window);
    }
}

fn create_window(instance: HINSTANCE, state: &mut TrayState) -> io::Result<HWND> {
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    // SAFETY: Windows copies the class data. The window state pointer remains
    // valid on the creating thread until after the window is destroyed.
    unsafe {
        if RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(io::Error::last_os_error());
        }
        // A hidden top-level window receives Explorer's TaskbarCreated broadcast.
        let window = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CLASS_NAME,
            w!("fishing"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::from_mut(state).cast(),
        );
        if window.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(window)
    }
}

fn icon_data(window: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: ICON_ID,
        ..Default::default()
    }
}

fn add_icon(window: HWND, icon: HICON) -> bool {
    let mut data = icon_data(window);
    data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
    data.uCallbackMessage = TRAY_CALLBACK;
    data.hIcon = icon;
    for (slot, character) in data.szTip.iter_mut().zip("fishing".encode_utf16()) {
        *slot = character;
    }
    // SAFETY: The structure has its documented size and uses a shared icon.
    unsafe {
        if Shell_NotifyIconW(NIM_ADD, &data) == 0 {
            return false;
        }
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        Shell_NotifyIconW(NIM_SETVERSION, &data);
    }
    true
}

fn remove_icon(window: HWND) {
    // SAFETY: The stable tray key is this window handle and icon ID.
    unsafe { Shell_NotifyIconW(NIM_DELETE, &icon_data(window)) };
}

fn change_taskbar(window: HWND, visible: bool) -> windows::core::Result<()> {
    // SAFETY: Initialize COM for this call only. The closure releases the
    // ITaskbarList before the matching CoUninitialize on the same thread.
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).ok()?;
        let result = (|| {
            let taskbar: ITaskbarList = CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)?;
            taskbar.HrInit()?;
            if visible {
                taskbar.AddTab(ComHwnd(window))
            } else {
                taskbar.DeleteTab(ComHwnd(window))
            }
        })();
        CoUninitialize();
        result
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: The system dispatches to the owning thread. The window userdata
    // points to the thread's TrayState, which outlives all window messages.
    unsafe {
        if message == WM_NCCREATE {
            let creation = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(window, GWLP_USERDATA, creation.lpCreateParams as isize);
        }
        if message == WM_DESTROY {
            remove_icon(window);
            PostQuitMessage(0);
            return 0;
        }
        let state = GetWindowLongPtrW(window, GWLP_USERDATA) as *const TrayState;
        if let Some(state) = state.as_ref() {
            if message == state.taskbar_created && message != 0 {
                add_icon(window, state.icon);
                if state.taskbar_hidden.load(Ordering::Relaxed) {
                    // Give the settings HWND time to handle the same Explorer
                    // restart broadcast before restoring its hidden tab state.
                    SetTimer(window, RESTORE_TASKBAR_TIMER, 200, None);
                }
                return 0;
            }
            if message == WM_TIMER && wparam == RESTORE_TASKBAR_TIMER {
                KillTimer(window, RESTORE_TASKBAR_TIMER);
                if state.taskbar_hidden.load(Ordering::Relaxed) {
                    let _ = change_taskbar(state.main_window, false);
                }
                return 0;
            }
            if message == TRAY_CALLBACK {
                let event = lparam as u32 & 0xFFFF;
                let action = match event {
                    NIN_SELECT | NIN_KEYSELECT | WM_LBUTTONDBLCLK => Some(TrayAction::Open),
                    WM_CONTEXTMENU | WM_RBUTTONUP => show_menu(window, state),
                    _ => None,
                };
                if let Some(action) = action {
                    if action == TrayAction::Open {
                        // Restore natively so this works even when a repaint
                        // requested from Rust won't wake a hidden settings window.
                        if IsIconic(state.main_window) != 0 {
                            ShowWindowAsync(state.main_window, SW_RESTORE);
                        }
                        SetForegroundWindow(state.main_window);
                    }
                    if state.actions.send(action).is_ok() {
                        state.pending_wake.set(true);
                    }
                    if action == TrayAction::Quit {
                        // A native close reaches winit even when the main window
                        // is minimized and not currently painting.
                        PostMessageW(state.main_window, WM_CLOSE, 0, 0);
                    }
                }
                return 0;
            }
        }
        DefWindowProcW(window, message, wparam, lparam)
    }
}

fn show_menu(window: HWND, state: &TrayState) -> Option<TrayAction> {
    let toggle = if state.running.load(Ordering::Relaxed) {
        w!("暂停")
    } else {
        w!("启动")
    };
    let anchor = menu_anchor(window);
    // SAFETY: All menu strings are static and the menu is destroyed after use.
    // TrackPopupMenu runs on the window's owning thread.
    let selection = unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return None;
        }
        let populated = AppendMenuW(menu, MF_STRING, MENU_OPEN, w!("打开设置")) != 0
            && AppendMenuW(menu, MF_STRING, MENU_TOGGLE, toggle) != 0
            && AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null()) != 0
            && AppendMenuW(menu, MF_STRING, MENU_QUIT, w!("退出 fishing")) != 0;
        let mut selection = 0;
        if populated {
            SetMenuDefaultItem(menu, MENU_OPEN as u32, 0);
            SetForegroundWindow(window);
            selection = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                anchor.x,
                anchor.y,
                0,
                window,
                ptr::null(),
            );
            // Microsoft's tray menu guidance requires posting a benign message
            // to reset the menu's foreground behavior for repeated openings.
            PostMessageW(window, WM_NULL, 0, 0);
            if selection == 0 {
                Shell_NotifyIconW(NIM_SETFOCUS, &icon_data(window));
            }
        }
        DestroyMenu(menu);
        selection
    };
    action_from_menu(selection as usize)
}

fn menu_anchor(window: HWND) -> POINT {
    let identifier = NOTIFYICONIDENTIFIER {
        cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
        hWnd: window,
        uID: ICON_ID,
        ..Default::default()
    };
    let mut bounds = RECT::default();
    let mut position = POINT::default();
    // SAFETY: The tray identifier and position output pointers are valid.
    unsafe {
        if Shell_NotifyIconGetRect(&identifier, &mut bounds) == 0 {
            position.x = (bounds.left + bounds.right) / 2;
            position.y = (bounds.top + bounds.bottom) / 2;
        } else {
            GetCursorPos(&mut position);
        }
    }
    position
}

fn action_from_menu(selection: usize) -> Option<TrayAction> {
    match selection {
        MENU_OPEN => Some(TrayAction::Open),
        MENU_TOGGLE => Some(TrayAction::Toggle),
        MENU_QUIT => Some(TrayAction::Quit),
        _ => None,
    }
}

fn hwnd(handle: isize) -> HWND {
    ptr::with_exposed_provenance_mut(handle as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_actions_distinguish_cancel_from_a_selection() {
        assert_eq!(action_from_menu(0), None);
        assert_eq!(action_from_menu(MENU_OPEN), Some(TrayAction::Open));
        assert_eq!(action_from_menu(MENU_TOGGLE), Some(TrayAction::Toggle));
        assert_eq!(action_from_menu(MENU_QUIT), Some(TrayAction::Quit));
    }
}
