//! backend — Win32 消息线程：全局热键 + 托盘图标
//!
//! 与 winit/eframe 事件循环隔离：本线程自建隐藏窗口与 GetMessage 泵，
//! 事件经 channel 发往 egui 层（附带线程 TID 供冒烟测试投递 WM_HOTKEY）。
use std::mem::size_of;
use std::sync::mpsc::Sender;
use std::thread;

use std::io::Write;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey,
};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_STATE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW, NOTIFY_ICON_DATA_FLAGS, NOTIFY_ICON_STATE, NIS_HIDDEN, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use crate::core::Hotkey;

pub enum Evt {
    ToggleTargets,
    ToggleSelf,
    ShowUi,
    OpenSettings,
    Exit,
    /// 重新注册后的热键状态（false = 注册失败，通常被其它程序占用）
    HotkeyStatus { targets_ok: bool, self_ok: bool },
}

const WM_TRAYICON: u32 = WM_APP + 1;
/// 通知消息线程改注册热键（UI → backend）
const WM_HOTKEY_RELOAD: u32 = WM_APP + 2;
const TRAY_UID: u32 = 1;
const HOTKEY_TARGETS: i32 = 1;
const HOTKEY_SELF: i32 = 2;
const IDM_SHOW: usize = 301;
const IDM_SETTINGS: usize = 302;
const IDM_EXIT: usize = 303;

/// 托盘图标宿主窗口句柄（消息线程创建后写入，0 表示尚未就绪）
static TRAY_HWND: AtomicIsize = AtomicIsize::new(0);

/// 热键配置：UI 修改后写这里，并通知消息线程重新注册
pub struct HotkeyCtl {
    tid: AtomicU32,
    cfg: Mutex<(Hotkey, Hotkey)>,
}

impl HotkeyCtl {
    pub fn new(targets: Hotkey, self_hk: Hotkey) -> Arc<Self> {
        Arc::new(HotkeyCtl {
            tid: AtomicU32::new(0),
            cfg: Mutex::new((targets, self_hk)),
        })
    }

    pub fn set(&self, targets: Hotkey, self_hk: Hotkey) {
        *self.cfg.lock().unwrap() = (targets, self_hk);
    }
}

/// 请求消息线程重新注册全局热键
pub fn reload(ctl: &HotkeyCtl) {
    let tid = ctl.tid.load(Ordering::SeqCst);
    if tid != 0 {
        unsafe {
            let _ = PostThreadMessageW(tid, WM_HOTKEY_RELOAD, WPARAM(0), LPARAM(0));
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 启动消息线程（返回线程句柄；TID 通过 stdout 输出供测试）
pub fn spawn(tx: Sender<Evt>, ctl: Arc<HotkeyCtl>) -> thread::JoinHandle<()> {
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
        let status_tx = tx.clone();
        let tx_ptr = Box::into_raw(Box::new(tx));
        let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, tx_ptr as isize);
        TRAY_HWND.store(hwnd.0 as isize, Ordering::SeqCst);

        // 线程热键（可在设置页自定义）+ 托盘图标
        ctl.tid.store(GetCurrentThreadId(), Ordering::SeqCst);
        register_hotkeys(&ctl, &status_tx);

        // 托盘图标
        tray_add(hwnd);

        // 冒烟/测试用：消息线程 TID
        let _ = writeln!(std::io::stdout(), "MSGTID={}", GetCurrentThreadId());

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            // 热键配置变更：重注册后继续
            if msg.message == WM_HOTKEY_RELOAD {
                register_hotkeys(&ctl, &status_tx);
                continue;
            }
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
        TRAY_HWND.store(0, Ordering::SeqCst);
        let _ = UnregisterHotKey(Some(HWND::default()), HOTKEY_TARGETS);
        let _ = UnregisterHotKey(Some(HWND::default()), HOTKEY_SELF);
        let _ = Box::from_raw(tx_ptr);
    })
}

/// 按当前配置（重）注册两个全局热键，并把结果回传 UI
unsafe fn register_hotkeys(ctl: &HotkeyCtl, tx: &Sender<Evt>) {
    let (targets, self_hk) = *ctl.cfg.lock().unwrap();
    let _ = UnregisterHotKey(Some(HWND::default()), HOTKEY_TARGETS);
    let _ = UnregisterHotKey(Some(HWND::default()), HOTKEY_SELF);
    let targets_ok = RegisterHotKey(
        Some(HWND::default()),
        HOTKEY_TARGETS,
        HOT_KEY_MODIFIERS(targets.mods | MOD_NOREPEAT.0),
        targets.vk,
    )
    .is_ok();
    let self_ok = RegisterHotKey(
        Some(HWND::default()),
        HOTKEY_SELF,
        HOT_KEY_MODIFIERS(self_hk.mods | MOD_NOREPEAT.0),
        self_hk.vk,
    )
    .is_ok();
    let _ = tx.send(Evt::HotkeyStatus {
        targets_ok,
        self_ok,
    });
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

/// 显示 / 隐藏本程序托盘图标（与 BossKey 本体显隐联动）
pub fn set_tray_visible(visible: bool) {
    let h = TRAY_HWND.load(Ordering::SeqCst);
    if h == 0 {
        return;
    }
    unsafe { tray_set_hidden(HWND(h as *mut std::ffi::c_void), !visible) }
}

unsafe fn tray_set_hidden(hwnd: HWND, hidden: bool) {
    let nid = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_UID,
        uFlags: NOTIFY_ICON_DATA_FLAGS(NIF_STATE.0),
        dwState: if hidden {
            NIS_HIDDEN
        } else {
            NOTIFY_ICON_STATE(0)
        },
        dwStateMask: NIS_HIDDEN,
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
}
