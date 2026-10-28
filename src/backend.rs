//! backend — Win32 消息线程：全局热键 + 托盘图标
//!
//! 与 winit/eframe 事件循环隔离：本线程自建隐藏窗口与 GetMessage 泵，
//! 事件经 channel 发往 egui 层（附带线程 TID 供冒烟测试投递 WM_HOTKEY）。
use std::mem::size_of;
use std::sync::mpsc::Sender;
use std::thread;

use std::io::Write;

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey, VK_B,
    VK_H,
};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, NOTIFY_ICON_DATA_FLAGS,
    Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

pub enum Evt {
    ToggleTargets,
    ToggleSelf,
    ShowUi,
    OpenSettings,
    Exit,
}

const WM_TRAYICON: u32 = WM_APP + 1;
const TRAY_UID: u32 = 1;
const HOTKEY_TARGETS: i32 = 1;
const HOTKEY_SELF: i32 = 2;
const IDM_SHOW: usize = 301;
const IDM_SETTINGS: usize = 302;
const IDM_EXIT: usize = 303;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 启动消息线程（返回线程句柄；TID 通过 stdout 输出供测试）
pub fn spawn(tx: Sender<Evt>) -> thread::JoinHandle<()> {
    thread::spawn(move || unsafe {
        let module = GetModuleHandleW(None).unwrap_or_default();
        let instance = HINSTANCE(module.0);

        // 隐藏的顶层窗口：托盘回调与命令菜单宿主
        let class = w!("BossKeyMsgWindow");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(msg_wnd_proc),
            hInstance: instance,
            lpszClassName: class,
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!("BossKeyMsg"),
            WS_OVERLAPPED,
            -32000,
            -32000,
            1,
            1,
            None,
            None,
            Some(instance),
            None,
        ) {
            Ok(h) => h,
            Err(_) => return,
        };
        // wnd_proc 通过 GWLP_USERDATA 拿 Sender
        let tx_ptr = Box::into_raw(Box::new(tx));
        let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, tx_ptr as isize);

        // 线程热键
        let mods: HOT_KEY_MODIFIERS = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
        let _ = RegisterHotKey(Some(HWND::default()), HOTKEY_TARGETS, mods, VK_H.0 as u32);
        let _ = RegisterHotKey(Some(HWND::default()), HOTKEY_SELF, mods, VK_B.0 as u32);

        // 托盘图标
        tray_add(hwnd);

        // 冒烟/测试用：消息线程 TID
        let _ = writeln!(std::io::stdout(), "MSGTID={}", GetCurrentThreadId());

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            // 线程热键的 WM_HOTKEY 带空 hwnd，DispatchMessage 不路由——直接处理
            if msg.message == WM_HOTKEY {
                let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Sender<Evt>;
                if !p.is_null() {
                    match msg.wParam.0 as i32 {
                        x if x == HOTKEY_TARGETS => {
                            let _ = (*p).send(Evt::ToggleTargets);
                        }
                        x if x == HOTKEY_SELF => {
                            let _ = (*p).send(Evt::ToggleSelf);
                        }
                        _ => {}
                    }
                }
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        tray_delete(hwnd);
        let _ = UnregisterHotKey(Some(HWND::default()), HOTKEY_TARGETS);
        let _ = UnregisterHotKey(Some(HWND::default()), HOTKEY_SELF);
        let _ = Box::from_raw(tx_ptr);
    })
}

unsafe extern "system" fn msg_wnd_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let send = |evt: Evt| {
        let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Sender<Evt>;
        if !p.is_null() {
            let _ = (*p).send(evt);
        }
    };
    match msg {
        WM_HOTKEY => {
            match w.0 as i32 {
                x if x == HOTKEY_TARGETS => send(Evt::ToggleTargets),
                x if x == HOTKEY_SELF => send(Evt::ToggleSelf),
                _ => {}
            }
            LRESULT(0)
        }
        WM_TRAYICON => {
            match l.0 as u32 {
                x if x == WM_LBUTTONUP || x == WM_LBUTTONDBLCLK => send(Evt::ShowUi),
                x if x == WM_RBUTTONUP => {
                    tray_menu(hwnd);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            match w.0 & 0xFFFF {
                x if x == IDM_SHOW => send(Evt::ShowUi),
                x if x == IDM_SETTINGS => send(Evt::OpenSettings),
                x if x == IDM_EXIT => send(Evt::Exit),
                _ => {}
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

unsafe fn tray_menu(hwnd: HWND) {
    let menu = CreatePopupMenu().unwrap_or_default();
    let t1 = wide("显示主窗口");
    let t2 = wide("设置");
    let t3 = wide("退出（恢复目标）");
    let _ = AppendMenuW(menu, MF_STRING, IDM_SHOW, PCWSTR(t1.as_ptr()));
    let _ = AppendMenuW(menu, MF_STRING, IDM_SETTINGS, PCWSTR(t2.as_ptr()));
    let _ = AppendMenuW(menu, MF_STRING, IDM_EXIT, PCWSTR(t3.as_ptr()));
    let mut pt = windows::Win32::Foundation::POINT::default();
    let _ = GetCursorPos(&mut pt);
    let _ = SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(
        menu,
        TRACK_POPUP_MENU_FLAGS(TPM_RIGHTBUTTON.0 | TPM_RETURNCMD.0),
        pt.x,
        pt.y,
        None,
        hwnd,
        None,
    );
    let _ = DestroyMenu(menu);
    let _ = PostMessageW(Some(hwnd), WM_COMMAND, WPARAM(cmd.0 as usize), LPARAM(0));
}

unsafe fn tray_add(hwnd: HWND) {
    let module = GetModuleHandleW(None).unwrap_or_default();
    let icon = LoadIconW(
        Some(HINSTANCE(module.0)),
        PCWSTR(1usize as *mut u16),
    )
    .unwrap_or_default();
    let mut nid = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_UID,
        uFlags: NOTIFY_ICON_DATA_FLAGS(NIF_ICON.0 | NIF_MESSAGE.0 | NIF_TIP.0),
        uCallbackMessage: WM_TRAYICON,
        hIcon: icon,
        ..Default::default()
    };
    let tip = wide("BossKey — 老板键");
    let n = tip.len().min(127);
    nid.szTip[..n].copy_from_slice(&tip[..n]);
    let _ = Shell_NotifyIconW(NIM_ADD, &nid);
}

unsafe fn tray_delete(hwnd: HWND) {
    let nid = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_UID,
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
}

// 避免未使用告警
#[allow(dead_code)]
fn _anchor(_: HOT_KEY_MODIFIERS) {}
