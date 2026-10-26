//! bosskey — Windows 老板键（绿色版）
//!
//! 运行后出现界面：列出正在运行的应用（带可见窗口的进程，不含 bosskey 自身），
//! 复选框勾选目标，点「隐藏 / 显示」或按 Ctrl+Alt+H 操作；
//! Ctrl+Alt+B 单独控制 bosskey 本窗口的显隐。
//! 配置保存在 exe 同目录 bosskey.targets.txt。
//!
//! 行为：
//! - 目标进程有可见顶层窗口：隐藏全部（SW_HIDE，任务栏按钮随之消失），记录 WINDOWPLACEMENT
//! - 恢复：按原状态还原（位置 / 最小化 / 最大化）
//! - 目标未运行或无窗口：不做处理；退出程序时自动恢复仍隐藏的窗口
#![windows_subsystem = "windows"]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::io::Write;
use std::mem::size_of;
use std::path::PathBuf;

use windows::Win32::Foundation::{CloseHandle, HINSTANCE, HWND, LPARAM, LRESULT, RECT, TRUE, WPARAM};
use windows::Win32::Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::Controls::{
    LIST_VIEW_ITEM_STATE_FLAGS, LVCOLUMNW, LVCF_TEXT, LVCF_WIDTH, LVIF_STATE, LVIF_TEXT, LVITEMW,
    LVM_DELETEALLITEMS, LVM_GETITEMCOUNT, LVM_GETITEMRECT, LVM_GETITEMSTATE, LVM_GETNEXTITEM,
    LVM_INSERTCOLUMNW, LVM_INSERTITEMW, LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMW,
    LVM_SETITEMSTATE, LVS_EX_CHECKBOXES, LVS_EX_FULLROWSELECT, LVS_REPORT, LVIS_FOCUSED,
    LVIS_SELECTED, LVN_ITEMCHANGED, LVIS_STATEIMAGEMASK, LVNI_SELECTED, NM_CLICK, NMHDR,
    NMITEMACTIVATE, NMLISTVIEW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey, VK_B,
    VK_H,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, PWSTR, w};

const HOTKEY_ID: i32 = 1; // Ctrl+Alt+H：隐藏/显示目标
const HOTKEY_ID_SELF: i32 = 2; // Ctrl+Alt+B：隐藏/显示 bosskey 本窗口
const TIMER_ID: usize = 100; // 列表自动刷新（约任务管理器节奏）
const REFRESH_MS: u32 = 2000;

const IDC_LIST: usize = 101;
const IDC_REFRESH: usize = 102;
const IDC_HIDE: usize = 105;
const IDC_SHOW: usize = 106;
const IDC_STATUS: usize = 107;
const IDC_HINT: usize = 108;
const IDC_EXIT: usize = 109;

struct HiddenWindow {
    hwnd: HWND,
    placement: WINDOWPLACEMENT,
}

#[derive(Default)]
struct Ui {
    main: HWND,
    list: HWND,
    status: HWND,
}

thread_local! {
    // 所有窗口操作都在 UI 线程上执行
    static HIDDEN: RefCell<Vec<HiddenWindow>> = const { RefCell::new(Vec::new()) };
    static TARGETS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    // 列表项与 exe 名一一对应
    static DISPLAYED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static POPULATING: Cell<bool> = const { Cell::new(false) };
    static UI: RefCell<Ui> = const { RefCell::new(Ui {
        main: HWND(std::ptr::null_mut()),
        list: HWND(std::ptr::null_mut()),
        status: HWND(std::ptr::null_mut()),
    }) };
}

fn main() {
    // 默认全部不勾选：启动时不从配置预载，勾选仅本次会话生效
    // （启动参数可预置目标：bosskey.exe notepad.exe ...）
    let args: Vec<String> = std::env::args().skip(1).collect();
    TARGETS.with(|t| {
        let mut targets: Vec<String> = Vec::new();
        for a in args {
            if !targets.iter().any(|x| x == &a) {
                targets.push(a);
            }
        }
        *t.borrow_mut() = targets;
    });

    let module = unsafe { GetModuleHandleW(None).unwrap_or_default() };
    let instance = HINSTANCE(module.0);
    let (hotkey_ok, hotkey_self_ok) = unsafe {
        let mods: HOT_KEY_MODIFIERS = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
        let a = RegisterHotKey(Some(HWND::default()), HOTKEY_ID, mods, VK_H.0 as u32).is_ok();
        let b = RegisterHotKey(Some(HWND::default()), HOTKEY_ID_SELF, mods, VK_B.0 as u32).is_ok();
        (a, b)
    };

    unsafe {
        let class = w!("BossKeyMainWindow");
        // 资源 1 = build.rs 嵌入的图标
        let icon = LoadIconW(Some(HINSTANCE(module.0)), PCWSTR(1usize as *mut u16)).unwrap_or_default();
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance,
            hIcon: icon,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: class,
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            std::process::exit(1);
        }

        let (w, h) = (456i32, 430i32);
        let (x, y) = (
            (GetSystemMetrics(SM_CXSCREEN) - w) / 2,
            (GetSystemMetrics(SM_CYSCREEN) - h) / 2,
        );
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!("bosskey — 老板键"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_VISIBLE,
            x,
            y,
            w,
            h,
            None,
            None,
            Some(instance),
            None,
        )
        .expect("CreateWindowExW 失败");

        build_controls(hwnd, instance);
        populate_list();
        let _ = SetTimer(Some(hwnd), TIMER_ID, REFRESH_MS, None);

        if hotkey_ok && hotkey_self_ok {
            out("就绪 — Ctrl+Alt+H 隐藏/显示目标，Ctrl+Alt+B 隐藏/显示本窗口");
        } else if hotkey_ok {
            out("就绪 — Ctrl+Alt+B 注册失败（被占用），本窗口显隐热键不可用");
        } else {
            out("热键注册失败（被占用），仅界面按钮可用");
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            // 线程热键的 WM_HOTKEY 带空 hwnd，DispatchMessage 不会路由到窗口过程，
            // 必须在这里直接处理
            if msg.message == WM_HOTKEY {
                match msg.wParam.0 as i32 {
                    x if x == HOTKEY_ID => do_toggle(),
                    x if x == HOTKEY_ID_SELF => do_toggle_self(),
                    _ => {}
                }
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = UnregisterHotKey(Some(HWND::default()), HOTKEY_ID);
        let _ = UnregisterHotKey(Some(HWND::default()), HOTKEY_ID_SELF);
    }
}

// ---------------- UI ----------------

unsafe fn build_controls(parent: HWND, instance: HINSTANCE) {
    let font = GetStockObject(DEFAULT_GUI_FONT);

    let mk = |class: PCWSTR, text: PCWSTR, style: WINDOW_STYLE, x: i32, y: i32, w: i32, h: i32, id: usize| unsafe {
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            text,
            WS_CHILD | WS_VISIBLE | style,
            x,
            y,
            w,
            h,
            Some(parent),
            Some(HMENU(id as *mut c_void)),
            Some(instance),
            None,
        )
        .unwrap_or_default();
        let _ = SendMessageW(hwnd, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
        hwnd
    };

    mk(w!("STATIC"), w!("运行中的应用（勾选为目标）："), WINDOW_STYLE(0), 12, 10, 300, 18, 0);
    let list = mk(
        w!("SysListView32"),
        w!(""),
        WS_BORDER | WINDOW_STYLE(LVS_REPORT),
        12, 30, 432, 200,
        IDC_LIST,
    );
    let _ = SendMessageW(
        list,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        Some(WPARAM(0)),
        Some(LPARAM((LVS_EX_CHECKBOXES | LVS_EX_FULLROWSELECT) as isize)),
    );
    // 单列
    let col_title = wide("应用进程");
    let mut col = LVCOLUMNW {
        mask: LVCF_TEXT | LVCF_WIDTH,
        cx: 410,
        pszText: PWSTR(col_title.as_ptr() as _),
        ..Default::default()
    };
    let _ = SendMessageW(list, LVM_INSERTCOLUMNW, Some(WPARAM(0)), Some(LPARAM(&mut col as *mut _ as isize)));

    mk(w!("BUTTON"), w!("刷新列表"), WS_TABSTOP, 12, 242, 98, 30, IDC_REFRESH);
    mk(w!("BUTTON"), w!("隐藏"), WS_TABSTOP, 120, 242, 98, 30, IDC_HIDE);
    mk(w!("BUTTON"), w!("显示"), WS_TABSTOP, 228, 242, 98, 30, IDC_SHOW);
    mk(w!("BUTTON"), w!("退出并恢复"), WS_TABSTOP, 336, 242, 108, 30, IDC_EXIT);

    let status = mk(w!("STATIC"), w!(""), WINDOW_STYLE(0), 12, 288, 432, 36, IDC_STATUS);
    mk(
        w!("STATIC"),
        w!("热键 Ctrl+Alt+H 隐藏/显示目标 · Ctrl+Alt+B 隐藏/显示本窗口 · 勾选即保存 · 退出时自动恢复已隐藏窗口"),
        WINDOW_STYLE(0),
        12, 336, 432, 40,
        IDC_HINT,
    );

    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        ui.main = parent;
        ui.list = list;
        ui.status = status;
    });
}

/// 扫描运行中的应用：有可见顶层窗口的进程，按 exe 归并
fn scan_apps() -> Vec<(String, String)> {
    struct Ctx {
        cache: HashMap<u32, String>,
        found: HashMap<String, String>, // exe -> 首个窗口标题
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let ctx = &mut *(lparam.0 as *mut Ctx);
        if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) == 0 {
            return TRUE;
        }
        if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_TOOLWINDOW.0 as isize != 0 {
            return TRUE;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return TRUE;
        }
        let exe = ctx.cache.entry(pid).or_insert_with(|| exe_name_of(pid).unwrap_or_default());
        if exe.is_empty() || is_self(exe) {
            return TRUE;
        }
        ctx.found.entry(exe.clone()).or_insert_with(|| {
            let mut buf = [0u16; 128];
            let n = GetWindowTextW(hwnd, &mut buf) as usize;
            String::from_utf16_lossy(&buf[..n])
        });
        TRUE
    }

    let mut ctx = Ctx {
        cache: HashMap::new(),
        found: HashMap::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut ctx as *mut _ as isize));
    }

    let mut apps: Vec<(String, String)> = ctx
        .found
        .into_iter()
        .map(|(exe, title)| {
            let label = if title.is_empty() {
                exe.clone()
            } else {
                format!("{exe} — {title}")
            };
            (exe, label)
        })
        .collect();
    apps.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    // 只显示正在运行的应用（类似任务管理器）：
    // 已配置但未运行的目标不展示、也不从配置中删除——进程再启动时自动出现并保持勾选
    apps
}

/// 重建列表（含勾选状态）
fn populate_list() {
    let apps = scan_apps();
    POPULATING.with(|p| p.set(true));
    UI.with(|ui| unsafe {
        let list = ui.borrow().list;
        let _ = SendMessageW(list, LVM_DELETEALLITEMS, Some(WPARAM(0)), Some(LPARAM(0)));
        for (i, (exe, label)) in apps.iter().enumerate() {
            let checked = TARGETS.with(|t| t.borrow().iter().any(|x| exe_matches(x, exe)));
            let text = wide(label);
            let mut item = LVITEMW {
                mask: LVIF_TEXT,
                iItem: i as i32,
                pszText: PWSTR(text.as_ptr() as _),
                ..Default::default()
            };
            let _ = SendMessageW(list, LVM_INSERTITEMW, Some(WPARAM(0)), Some(LPARAM(&mut item as *mut _ as isize)));
            // LVM_INSERTITEMW 的初始 state 不可靠，勾选态插入后显式设置
            let mut st = LVITEMW {
                iItem: i as i32,
                stateMask: LVIS_STATEIMAGEMASK,
                state: LIST_VIEW_ITEM_STATE_FLAGS(if checked { 2u32 << 12 } else { 1u32 << 12 }),
                ..Default::default()
            };
            let _ = SendMessageW(
                list,
                LVM_SETITEMSTATE,
                Some(WPARAM(i)),
                Some(LPARAM(&mut st as *mut _ as isize)),
            );
        }
    });
    DISPLAYED.with(|d| *d.borrow_mut() = apps.into_iter().map(|(exe, _)| exe).collect());
    POPULATING.with(|p| p.set(false));
}

/// 定时自动刷新（类似任务管理器）：
/// 进程集合没变 -> 只原位更新文本（窗口标题会变，不闪、不动勾选与选中）
/// 集合变了 -> 整表重建（勾选由 TARGETS 保持），并恢复选中项
fn refresh_tick() {
    if POPULATING.with(|p| p.get()) {
        return;
    }
    let apps = scan_apps();
    let same = DISPLAYED.with(|d| {
        let d = d.borrow();
        d.len() == apps.len() && d.iter().zip(apps.iter()).all(|(a, (b, _))| a == b)
    });
    if same {
        UI.with(|ui| unsafe {
            let list = ui.borrow().list;
            for (i, (_, label)) in apps.iter().enumerate() {
                let text = wide(label);
                let mut item = LVITEMW {
                    mask: LVIF_TEXT,
                    iItem: i as i32,
                    pszText: PWSTR(text.as_ptr() as _),
                    ..Default::default()
                };
                let _ = SendMessageW(
                    list,
                    LVM_SETITEMW,
                    Some(WPARAM(0)),
                    Some(LPARAM(&mut item as *mut _ as isize)),
                );
            }
        });
    } else {
        let selected = selected_exe();
        populate_list();
        if let Some(exe) = selected {
            select_exe(&exe);
        }
    }
}

/// 当前选中行对应的 exe
fn selected_exe() -> Option<String> {
    UI.with(|ui| unsafe {
        let idx = SendMessageW(
            ui.borrow().list,
            LVM_GETNEXTITEM,
            Some(WPARAM(usize::MAX)), // -1：从头查找
            Some(LPARAM(LVNI_SELECTED as isize)),
        )
        .0;
        if idx < 0 {
            None
        } else {
            DISPLAYED.with(|d| d.borrow().get(idx as usize).cloned())
        }
    })
}

/// 重建后恢复选中（只动选中/焦点，不动勾选态）
fn select_exe(exe: &str) {
    let idx = DISPLAYED.with(|d| d.borrow().iter().position(|e| e == exe));
    if let Some(i) = idx {
        UI.with(|ui| unsafe {
            let mut item = LVITEMW {
                iItem: i as i32,
                stateMask: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0),
                state: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0),
                ..Default::default()
            };
            let _ = SendMessageW(
                ui.borrow().list,
                LVM_SETITEMSTATE,
                Some(WPARAM(i)),
                Some(LPARAM(&mut item as *mut _ as isize)),
            );
        });
    }
}

/// 勾选变化 -> 同步 TARGETS 并保存
fn sync_targets_from_list() {
    let count = UI.with(|ui| unsafe {
        SendMessageW(ui.borrow().list, LVM_GETITEMCOUNT, Some(WPARAM(0)), Some(LPARAM(0))).0
    });
    let checked: Vec<String> = DISPLAYED.with(|d| {
        let displayed = d.borrow();
        (0..count as usize)
            .filter_map(|i| {
                let state = UI.with(|ui| unsafe {
                    SendMessageW(
                        ui.borrow().list,
                        LVM_GETITEMSTATE,
                        Some(WPARAM(i)),
                        Some(LPARAM(LVIS_STATEIMAGEMASK.0 as isize)),
                    )
                    .0
                });
                let is_checked = ((state >> 12) & 0xF) == 2;
                match (is_checked, displayed.get(i)) {
                    (true, Some(exe)) => Some(exe.clone()),
                    _ => None,
                }
            })
            .collect()
    });
    TARGETS.with(|t| {
        // 保留未显示的既有配置（如未运行目标），其余以勾选为准
        let mut next: Vec<String> = t
            .borrow()
            .iter()
            .filter(|x| {
                !is_self(x) && !DISPLAYED.with(|d| d.borrow().iter().any(|e| exe_matches(e, x)))
            })
            .cloned()
            .collect();
        for e in checked {
            if !next.iter().any(|x| exe_matches(x, &e)) {
                next.push(e);
            }
        }
        *t.borrow_mut() = next.clone();
        save_targets(&next);
    });
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            match w.0 & 0xFFFF {
                x if x == IDC_REFRESH as usize => {
                    populate_list();
                    out("已刷新应用列表");
                }
                x if x == IDC_HIDE as usize => do_hide(),
                x if x == IDC_SHOW as usize => do_show(),
                x if x == IDC_EXIT as usize => {
                    let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_NOTIFY => {
            let hdr = &*(l.0 as *const NMHDR);
            if hdr.code == LVN_ITEMCHANGED as u32 {
                let nmlv = &*(l.0 as *const NMLISTVIEW);
                let state_changed = (nmlv.uChanged.0 & LVIF_STATE.0) != 0
                    && ((nmlv.uNewState ^ nmlv.uOldState) & LVIS_STATEIMAGEMASK.0) != 0;
                if state_changed && !POPULATING.with(|p| p.get()) {
                    sync_targets_from_list();
                }
            } else if hdr.code == NM_CLICK as u32 {
                // 勾选框只有 ~16px，点行文字只选中不切换——这里补刀：点行任意处即切换勾选。
                // 点在勾选框上由控件自己切换（会触发 LVN_ITEMCHANGED），此处不重复处理。
                let nmi = &*(l.0 as *const NMITEMACTIVATE);
                if nmi.iItem >= 0 && !POPULATING.with(|p| p.get()) {
                    let list = UI.with(|ui| ui.borrow().list);
                    let mut rc = RECT::default();
                    let got = SendMessageW(
                        list,
                        LVM_GETITEMRECT,
                        Some(WPARAM(nmi.iItem as usize)),
                        Some(LPARAM(&mut rc as *mut _ as isize)),
                    )
                    .0
                        != 0;
                    // 勾选框约在行首 22px 内（点中时由控件自己切换），右侧区域点击才手动切换
                    if got && nmi.ptAction.x >= rc.left + 22 {
                        let state = SendMessageW(
                            list,
                            LVM_GETITEMSTATE,
                            Some(WPARAM(nmi.iItem as usize)),
                            Some(LPARAM(LVIS_STATEIMAGEMASK.0 as isize)),
                        )
                        .0;
                        let checked_now = ((state >> 12) & 0xF) == 2;
                        // LVM_SETITEMSTATE 要求 mask=0，只用 stateMask/state
                        let mut item = LVITEMW {
                            iItem: nmi.iItem,
                            stateMask: LVIS_STATEIMAGEMASK,
                            state: LIST_VIEW_ITEM_STATE_FLAGS(
                                if checked_now { 1u32 << 12 } else { 2u32 << 12 },
                            ),
                            ..Default::default()
                        };
                        let _ = SendMessageW(
                            list,
                            LVM_SETITEMSTATE,
                            Some(WPARAM(nmi.iItem as usize)),
                            Some(LPARAM(&mut item as *mut _ as isize)),
                        );
                        sync_targets_from_list(); // 程序化改状态不发通知，显式同步
                    }
                }
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if w.0 == TIMER_ID {
                refresh_tick();
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            do_show(); // 退出前恢复仍隐藏的窗口
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

// ---------------- 隐藏 / 显示 ----------------

/// bosskey 本窗口显隐（独立热键 Ctrl+Alt+B）
fn do_toggle_self() {
    let hwnd = UI.with(|ui| ui.borrow().main);
    unsafe {
        if IsWindowVisible(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_HIDE);
            let _ = writeln!(std::io::stdout(), "本窗口已隐藏（Ctrl+Alt+B 呼回）");
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            out("本窗口已显示");
        }
    }
}

fn do_toggle() {
    let has_hidden = HIDDEN.with(|h| !h.borrow().is_empty());
    if has_hidden {
        do_show();
    } else {
        do_hide();
    }
}

fn do_hide() {
    // 以列表当前勾选为准（不依赖通知链路，点击时实时读取）
    sync_targets_from_list();
    let targets = TARGETS.with(|t| t.borrow().clone());
    if targets.is_empty() {
        out("尚未勾选目标——点击列表行即可勾选，再点「隐藏」");
        return;
    }
    let windows = collect_visible_windows(&targets);
    if windows.is_empty() {
        out("未找到可隐藏的窗口（目标未运行或已隐藏）");
        return;
    }

    let mut hid = 0u32;
    HIDDEN.with(|h| {
        let mut hidden = h.borrow_mut();
        for hwnd in windows {
            unsafe {
                let mut wp = WINDOWPLACEMENT {
                    length: size_of::<WINDOWPLACEMENT>() as u32,
                    ..Default::default()
                };
                if GetWindowPlacement(hwnd, &mut wp).is_err() {
                    continue;
                }
                // SW_HIDE 同时摘除任务栏按钮与 Alt-Tab 项，无需 ITaskbarList
                let _ = ShowWindow(hwnd, SW_HIDE);
                hidden.push(HiddenWindow {
                    hwnd,
                    placement: wp,
                });
                hid += 1;
            }
        }
    });
    out(&format!("已隐藏 {hid} 个窗口"));
}

fn do_show() {
    let mut restored = 0u32;
    let mut foreground: Option<HWND> = None;
    HIDDEN.with(|h| {
        for hw in h.borrow_mut().drain(..) {
            unsafe {
                if !IsWindow(Some(hw.hwnd)).as_bool() {
                    continue;
                }
                if SetWindowPlacement(hw.hwnd, &hw.placement).is_ok() {
                    restored += 1;
                    if foreground.is_none() && hw.placement.showCmd != SW_SHOWMINIMIZED.0 as u32 {
                        foreground = Some(hw.hwnd);
                    }
                }
            }
        }
    });
    if let Some(hwnd) = foreground {
        unsafe {
            let _ = SetForegroundWindow(hwnd);
        }
    }
    if restored > 0 {
        out(&format!("已显示 {restored} 个窗口"));
    } else {
        out("当前没有已隐藏的窗口");
    }
}

// ---------------- 核心：窗口收集 / 进程匹配 ----------------

/// 枚举目标进程的可见顶层窗口
fn collect_visible_windows(targets: &[String]) -> Vec<HWND> {
    struct EnumCtx<'a> {
        targets: &'a [String],
        exe_cache: HashMap<u32, String>,
        out: Vec<HWND>,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let ctx = &mut *(lparam.0 as *mut EnumCtx);
        if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) == 0 {
            return TRUE;
        }
        if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_TOOLWINDOW.0 as isize != 0 {
            return TRUE;
        }

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return TRUE;
        }

        let exe = ctx.exe_cache.entry(pid).or_insert_with(|| exe_name_of(pid).unwrap_or_default());
        if !exe.is_empty() && !is_self(exe) && ctx.targets.iter().any(|t| exe_matches(t, exe)) {
            ctx.out.push(hwnd);
        }
        TRUE
    }

    let mut ctx = EnumCtx {
        targets,
        exe_cache: HashMap::new(),
        out: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut ctx as *mut _ as isize));
    }
    ctx.out
}

/// 进程 PID -> exe 文件名（如 notepad.exe）
fn exe_name_of(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 260];
        let mut size = buf.len() as u32;
        let fmt: PROCESS_NAME_FORMAT = PROCESS_NAME_FORMAT(0); // PROCESS_NAME_WIN32
        let ok = QueryFullProcessImageNameW(handle, fmt, PWSTR(buf.as_mut_ptr()), &mut size);
        let _ = CloseHandle(handle);
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        let name = path.rsplit(['\\', '/']).next().unwrap_or("").to_string();
        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    }
}

/// 比较目标名与进程 exe 名：忽略大小写，".exe" 可省略
fn exe_matches(target: &str, exe: &str) -> bool {
    fn norm(s: &str) -> String {
        let s = s.trim().to_ascii_lowercase();
        s.strip_suffix(".exe").unwrap_or(&s).to_string()
    }
    norm(target) == norm(exe)
}

/// 是否是 bosskey 自身进程（从列表与隐藏目标中排除）
fn is_self(exe: &str) -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .map(|own| own.trim().eq_ignore_ascii_case(exe.trim()))
        .unwrap_or(false)
}

// ---------------- 配置 / 辅助 ----------------

fn config_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .map(|p| p.with_file_name("bosskey.targets.txt"))
        .unwrap_or_else(|| PathBuf::from("bosskey.targets.txt"))
}

/// 勾选记录写入配置（仅留档，启动时不预载）
fn save_targets(targets: &[String]) {
    let _ = std::fs::write(config_path(), format!("{}\n", targets.join("\n")));
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 状态输出：写到界面状态栏；stdout 留给冒烟测试（重定向时可用），永不 panic
fn out(msg: &str) {
    UI.with(|ui| unsafe {
        let w = wide(msg);
        let _ = SetWindowTextW(ui.borrow().status, PCWSTR(w.as_ptr()));
    });
    let _ = writeln!(std::io::stdout(), "{msg}");
}

#[cfg(test)]
mod tests {
    use super::exe_matches;

    #[test]
    fn matches_with_or_without_exe_suffix() {
        assert!(exe_matches("notepad.exe", "notepad.exe"));
        assert!(exe_matches("notepad", "notepad.exe"));
        assert!(exe_matches("NOTEPAD.EXE", "Notepad.exe"));
        assert!(!exe_matches("notepad", "calc.exe"));
    }
}
