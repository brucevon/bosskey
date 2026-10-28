//! core — 业务引擎：目标管理 / 窗口与托盘隐藏恢复 / 进程扫描 / 设置存储
//!
//! 与 UI 框架无关（egui/Win32 壳皆可挂载）；Win32 调用沿用 windows-rs。
use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::size_of;
use std::io::Write;
use std::path::PathBuf;

use windows::Win32::Foundation::{
    CloseHandle, COLORREF, ERROR_SUCCESS, HWND, LPARAM, POINT, RECT, TRUE, WPARAM,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::{
    NIF_GUID, NIF_STATE, NIM_MODIFY, NOTIFYICONDATAW, NOTIFYICONIDENTIFIER, NOTIFY_ICON_DATA_FLAGS,
    NOTIFY_ICON_STATE, NIS_HIDDEN, Shell_NotifyIconGetRect, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{GUID, PCWSTR, PWSTR, w};

// ---------------- 设置 ----------------

#[derive(Clone, Copy)]
pub struct Settings {
    pub restore_targets_on_start: bool,
    pub restore_on_exit: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            restore_targets_on_start: false,
            restore_on_exit: true,
        }
    }
}

fn settings_path() -> PathBuf {
    exe_dir_file("bosskey.settings.txt")
}

pub fn load_settings() -> Settings {
    let mut s = Settings::default();
    if let Ok(text) = std::fs::read_to_string(settings_path()) {
        for line in text.lines() {
            let mut it = line.splitn(2, '=');
            let (k, v) = (it.next().unwrap_or(""), it.next().unwrap_or(""));
            match k.trim() {
                "restore_targets_on_start" => s.restore_targets_on_start = v.trim() == "1",
                "restore_on_exit" => s.restore_on_exit = v.trim() == "1",
                _ => {}
            }
        }
    }
    s
}

pub fn save_settings(s: &Settings) {
    let _ = std::fs::write(
        settings_path(),
        format!(
            "restore_targets_on_start={}\nrestore_on_exit={}\n",
            s.restore_targets_on_start as u8, s.restore_on_exit as u8
        ),
    );
}

// ---------------- 配置（目标记录）----------------

fn exe_dir_file(name: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .map(|p| p.with_file_name(name))
        .unwrap_or_else(|| PathBuf::from(name))
}

pub fn save_targets(targets: &[String]) {
    let _ = std::fs::write(exe_dir_file("bosskey.targets.txt"), format!("{}\n", targets.join("\n")));
}

pub fn load_targets() -> Vec<String> {
    std::fs::read_to_string(exe_dir_file("bosskey.targets.txt"))
        .map(|s| {
            s.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

// ---------------- 匹配 / 进程 ----------------

/// 比较目标名与进程 exe 名：忽略大小写，".exe" 可省略
pub fn exe_matches(target: &str, exe: &str) -> bool {
    fn norm(s: &str) -> String {
        let s = s.trim().to_ascii_lowercase();
        s.strip_suffix(".exe").unwrap_or(&s).to_string()
    }
    norm(target) == norm(exe)
}

/// 是否是 bosskey 自身进程（从列表与隐藏目标中排除）
pub fn is_self(exe: &str) -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .map(|own| own.trim().eq_ignore_ascii_case(exe.trim()))
        .unwrap_or(false)
}

/// 进程 PID -> exe 文件名
pub fn exe_name_of(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 260];
        let mut size = buf.len() as u32;
        let fmt: PROCESS_NAME_FORMAT = PROCESS_NAME_FORMAT(0);
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

/// 进程 PID -> exe 全路径
pub fn full_path_of(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 260];
        let mut size = buf.len() as u32;
        let fmt: PROCESS_NAME_FORMAT = PROCESS_NAME_FORMAT(0);
        let ok = QueryFullProcessImageNameW(handle, fmt, PWSTR(buf.as_mut_ptr()), &mut size);
        let _ = CloseHandle(handle);
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        if path.is_empty() {
            None
        } else {
            Some(path)
        }
    }
}

/// 按 exe 名找运行中进程的全路径
pub fn find_exe_path(exe: &str) -> Option<String> {
    running_processes()
        .into_iter()
        .find(|(_, name)| exe_matches(exe, name))
        .and_then(|(pid, _)| full_path_of(pid))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// exe 版本资源 FileDescription（任务管理器「应用名称」同源）
fn file_description(wpath: &[u16]) -> Option<String> {
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };
    unsafe {
        let mut dummy = 0u32;
        let size = GetFileVersionInfoSizeW(PCWSTR(wpath.as_ptr()), Some(&mut dummy));
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        if GetFileVersionInfoW(PCWSTR(wpath.as_ptr()), Some(0), size, buf.as_mut_ptr() as *mut c_void).is_err() {
            return None;
        }
        let mut tp: *mut c_void = std::ptr::null_mut();
        let mut tl = 0u32;
        let tkey = wide(r"\VarFileInfo\Translation");
        if !VerQueryValueW(buf.as_ptr() as *const c_void, PCWSTR(tkey.as_ptr()), &mut tp, &mut tl).as_bool()
            || tp.is_null()
        {
            return None;
        }
        let pairs = std::slice::from_raw_parts(tp as *const u16, (tl as usize / 2) as usize);
        for chunk in pairs.chunks_exact(2) {
            let key = wide(&format!(
                r"\StringFileInfo\{:04x}{:04x}\FileDescription",
                chunk[0], chunk[1]
            ));
            let mut fp: *mut c_void = std::ptr::null_mut();
            let mut fl = 0u32;
            if VerQueryValueW(buf.as_ptr() as *const c_void, PCWSTR(key.as_ptr()), &mut fp, &mut fl)
                .as_bool()
                && fl > 1
                && !fp.is_null()
            {
                let s = std::slice::from_raw_parts(fp as *const u16, fl.saturating_sub(1) as usize);
                let name = String::from_utf16_lossy(s);
                if !name.trim().is_empty() {
                    return Some(name);
                }
            }
        }
        None
    }
}

/// Shell 展示名（资源管理器同款友好名）+ 应用图标 RGBA（SHGetFileInfoW）
pub fn exe_shell_info(exe: &str) -> (Option<String>, Option<Vec<u8>>) {
    use windows::Win32::Graphics::Gdi as g;
    use windows::Win32::UI::Shell::{SHGFI_ICON, SHGFI_LARGEICON, SHFILEINFOW, SHGetFileInfoW};
    use windows::Win32::UI::WindowsAndMessaging::{DI_NORMAL, DestroyIcon, DrawIconEx};
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;

    let path = match find_exe_path(exe) {
        Some(p) => p,
        None => return (None, None),
    };
    let wpath: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let name = file_description(&wpath);

        let mut shfi2 = SHFILEINFOW::default();
        let _ = SHGetFileInfoW(
            PCWSTR(wpath.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut shfi2),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        let hicon = shfi2.hIcon;
        if hicon.is_invalid() {
            return (name, None);
        }
        let hdc = g::GetDC(None);
        if hdc.is_invalid() {
            let _ = DestroyIcon(hicon);
            return (name, None);
        }
        let mem = g::CreateCompatibleDC(Some(hdc));
        let mut bits: *mut c_void = std::ptr::null_mut();
        let bmi = g::BITMAPINFO {
            bmiHeader: g::BITMAPINFOHEADER {
                biSize: size_of::<g::BITMAPINFOHEADER>() as u32,
                biWidth: 32,
                biHeight: -32,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut rgba = None;
        if let Ok(hbmp) = g::CreateDIBSection(Some(hdc), &bmi, g::DIB_RGB_COLORS, &mut bits, None, 0) {
            let old = g::SelectObject(mem, g::HGDIOBJ(hbmp.0));
            let _ = DrawIconEx(mem, 0, 0, hicon, 32, 32, 0, None, DI_NORMAL);
            if !bits.is_null() {
                let mut out = vec![0u8; 32 * 32 * 4];
                let src = std::slice::from_raw_parts(bits as *const u8, 32 * 32 * 4);
                let mut alpha_sum = 0u32;
                for i in 0..32 * 32 {
                    out[i * 4] = src[i * 4 + 2];
                    out[i * 4 + 1] = src[i * 4 + 1];
                    out[i * 4 + 2] = src[i * 4];
                    out[i * 4 + 3] = src[i * 4 + 3];
                    alpha_sum += src[i * 4 + 3] as u32;
                }
                if alpha_sum == 0 {
                    for px in out.chunks_exact_mut(4) {
                        px[3] = 255;
                    }
                }
                rgba = Some(out);
            }
            let _ = g::SelectObject(mem, old);
            let _ = g::DeleteObject(g::HGDIOBJ(hbmp.0));
        }
        let _ = g::DeleteDC(mem);
        let _ = g::ReleaseDC(None, hdc);
        let _ = DestroyIcon(hicon);
        (name, rgba)
    }
}

/// 当前运行中的 (pid, exe 名)
pub fn running_processes() -> Vec<(u32, String)> {
    let mut list = Vec::new();
    unsafe {
        let snap = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(h) => h,
            Err(_) => return list,
        };
        let mut pe = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut pe).is_ok() {
            loop {
                let name = String::from_utf16_lossy(&pe.szExeFile)
                    .trim_end_matches('\0')
                    .to_string();
                if !name.is_empty() {
                    list.push((pe.th32ProcessID, name));
                }
                if Process32NextW(snap, &mut pe).is_err() {
                    break;
                }
            }
        }
        CloseHandle(snap).ok();
    }
    list
}

pub fn running_exe_names() -> Vec<String> {
    running_processes().into_iter().map(|(_, n)| n).collect()
}

// ---------------- 托盘图标身份与隐藏 ----------------

pub struct TrayRegEntry {
    pub exe: String,
    pub uid: Option<u32>,
    pub guid: Option<GUID>,
}

#[derive(Clone, Copy)]
pub struct IconId {
    pub hwnd: HWND,
    pub uid: u32,
    pub guid: Option<GUID>,
}

/// 读取 Win11 托盘图标注册表（每个托盘图标一条）
pub fn tray_registry_entries() -> Vec<TrayRegEntry> {
    use windows::Win32::System::Registry::{
        HKEY, KEY_READ, REG_DWORD, REG_SZ, REG_VALUE_TYPE, RegCloseKey, RegEnumKeyExW,
        RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER,
    };

    let mut out: Vec<TrayRegEntry> = Vec::new();
    unsafe {
        let mut root = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Control Panel\\NotifyIconSettings"),
            None,
            KEY_READ,
            &mut root,
        ) != ERROR_SUCCESS
        {
            return out;
        }
        let mut idx = 0u32;
        loop {
            let mut name = [0u16; 256];
            let mut name_len = name.len() as u32;
            if RegEnumKeyExW(
                root,
                idx,
                Some(PWSTR(name.as_mut_ptr())),
                &mut name_len,
                None,
                None,
                None,
                None,
            ) != ERROR_SUCCESS
            {
                break;
            }
            idx += 1;
            let mut sub = HKEY::default();
            if RegOpenKeyExW(root, windows::core::PCWSTR(name.as_ptr()), None, KEY_READ, &mut sub)
                != ERROR_SUCCESS
            {
                continue;
            }

            let mut buf = [0u16; 520];
            let mut size = (buf.len() * 2) as u32;
            let mut ty = REG_VALUE_TYPE::default();
            let q = RegQueryValueExW(
                sub,
                w!("ExecutablePath"),
                None,
                Some(&mut ty),
                Some(buf.as_mut_ptr() as *mut u8),
                Some(&mut size),
            );
            if q != ERROR_SUCCESS || ty != REG_SZ {
                RegCloseKey(sub).ok();
                continue;
            }
            let n = (size / 2) as usize;
            let path = String::from_utf16_lossy(&buf[..n.saturating_sub(1)]);
            let path = path.trim().trim_matches('"');
            let exe = path.rsplit(['\\', '/']).next().unwrap_or("").to_string();
            if exe.is_empty() || is_self(&exe) {
                RegCloseKey(sub).ok();
                continue;
            }

            let mut uid: Option<u32> = None;
            let mut dv = [0u8; 4];
            let mut dsize = 4u32;
            let mut dty = REG_VALUE_TYPE::default();
            if RegQueryValueExW(
                sub,
                w!("UID"),
                None,
                Some(&mut dty),
                Some(dv.as_mut_ptr()),
                Some(&mut dsize),
            ) == ERROR_SUCCESS
                && dty == REG_DWORD
            {
                uid = Some(u32::from_le_bytes(dv));
            }

            let mut guid: Option<GUID> = None;
            let mut gbuf = [0u16; 64];
            let mut gsize = (gbuf.len() * 2) as u32;
            let mut gty = REG_VALUE_TYPE::default();
            if RegQueryValueExW(
                sub,
                w!("IconGuid"),
                None,
                Some(&mut gty),
                Some(gbuf.as_mut_ptr() as *mut u8),
                Some(&mut gsize),
            ) == ERROR_SUCCESS
                && gty == REG_SZ
            {
                let gn = (gsize / 2) as usize;
                let gs = String::from_utf16_lossy(&gbuf[..gn.saturating_sub(1)]);
                if !gs.trim().is_empty() {
                    guid = parse_guid(&gs);
                }
            }
            RegCloseKey(sub).ok();

            if guid.is_some() || uid.is_some() {
                out.push(TrayRegEntry { exe, uid, guid });
            }
        }
        RegCloseKey(root).ok();
    }
    out
}

pub fn parse_guid(s: &str) -> Option<GUID> {
    let s = s.trim().trim_matches(['{', '}']);
    let p: Vec<&str> = s.split('-').collect();
    if p.len() != 5 {
        return None;
    }
    let h = format!("{}{}", p[3], p[4]);
    let mut data4 = [0u8; 8];
    for i in 0..8 {
        data4[i] = u8::from_str_radix(h.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(GUID {
        data1: u32::from_str_radix(p[0], 16).ok()?,
        data2: u16::from_str_radix(p[1], 16).ok()?,
        data3: u16::from_str_radix(p[2], 16).ok()?,
        data4,
    })
}

/// 托盘图标属主进程（无可见窗口、仅托盘图标的应用）
/// NotifyIconSettings 是历史记录——必须逐个探测图标当前存活（Shell_NotifyIconGetRect），
/// 否则纯后台进程（曾注册过图标但已注销）会混入列表
pub fn tray_apps() -> Vec<String> {
    let procs = running_processes();
    let mut exes: Vec<String> = Vec::new();
    for e in tray_registry_entries() {
        if exes.iter().any(|x| exe_matches(x, &e.exe)) {
            continue;
        }
        let pids: Vec<u32> = procs
            .iter()
            .filter(|(_, n)| exe_matches(&e.exe, n))
            .map(|(p, _)| *p)
            .collect();
        if !pids.is_empty() && !discover_tray_icons(&e.exe, &pids).is_empty() {
            exes.push(e.exe);
        }
    }
    exes
}

/// 目标进程的候选图标属主窗口：全部顶层窗口（含隐藏）+ 消息窗口
pub fn candidate_windows(pids: &[u32]) -> Vec<HWND> {
    struct Ctx<'a> {
        pids: &'a [u32],
        out: Vec<HWND>,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let ctx = &mut *(lparam.0 as *mut Ctx);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != 0 && ctx.pids.contains(&pid) {
            ctx.out.push(hwnd);
        }
        TRUE
    }

    let mut ctx = Ctx {
        pids,
        out: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut ctx as *mut _ as isize));
        let msg_parent = HWND(-3isize as *mut c_void); // HWND_MESSAGE
        let mut prev = HWND::default();
        loop {
            match FindWindowExW(
                Some(msg_parent),
                Some(prev),
                windows::core::PCWSTR::null(),
                windows::core::PCWSTR::null(),
            ) {
                Ok(h) => {
                    let mut pid = 0u32;
                    GetWindowThreadProcessId(h, Some(&mut pid));
                    if pid != 0 && ctx.pids.contains(&pid) {
                        ctx.out.push(h);
                    }
                    prev = h;
                }
                Err(_) => break,
            }
        }
    }
    ctx.out
}

/// 解析 exe 的托盘图标身份（Shell_NotifyIconGetRect 只读探测属主窗口）
pub fn discover_tray_icons(exe: &str, pids: &[u32]) -> Vec<IconId> {
    let entries: Vec<TrayRegEntry> = tray_registry_entries()
        .into_iter()
        .filter(|e| exe_matches(&e.exe, exe))
        .collect();
    if entries.is_empty() || pids.is_empty() {
        return Vec::new();
    }
    let candidates = if entries.iter().any(|e| e.guid.is_none()) {
        candidate_windows(pids)
    } else {
        Vec::new()
    };
    let mut found = Vec::new();
    for e in entries {
        if let Some(g) = e.guid {
            let ident = NOTIFYICONIDENTIFIER {
                cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
                hWnd: HWND::default(),
                uID: 0,
                guidItem: g,
            };
            if unsafe { Shell_NotifyIconGetRect(&ident).is_ok() } {
                found.push(IconId {
                    hwnd: HWND::default(),
                    uid: 0,
                    guid: Some(g),
                });
            }
        } else if let Some(uid) = e.uid {
            for &hwnd in &candidates {
                let ident = NOTIFYICONIDENTIFIER {
                    cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
                    hWnd: hwnd,
                    uID: uid,
                    guidItem: GUID::zeroed(),
                };
                if unsafe { Shell_NotifyIconGetRect(&ident).is_ok() } {
                    found.push(IconId {
                        hwnd,
                        uid,
                        guid: None,
                    });
                    break;
                }
            }
        }
    }
    found
}

/// 隐藏/显示 托盘图标：跨进程 NIM_MODIFY + NIS_HIDDEN（可逆）
pub fn set_icon_hidden(id: &IconId, hidden: bool) -> bool {
    unsafe {
        let mut nid = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: id.hwnd,
            uID: id.uid,
            uFlags: NOTIFY_ICON_DATA_FLAGS(
                NIF_STATE.0 | if id.guid.is_some() { NIF_GUID.0 } else { 0 },
            ),
            dwState: if hidden {
                NIS_HIDDEN
            } else {
                NOTIFY_ICON_STATE(0)
            },
            dwStateMask: NIS_HIDDEN,
            ..Default::default()
        };
        if let Some(g) = id.guid {
            nid.guidItem = g;
        }
        Shell_NotifyIconW(NIM_MODIFY, &nid).as_bool()
    }
}

// ---------------- 窗口收集 / 隐藏 ----------------

pub struct HiddenWindow {
    pub hwnd: HWND,
    pub placement: WINDOWPLACEMENT,
    pub exe: String,
}

/// 枚举目标进程的可见顶层窗口
pub fn collect_visible_windows(targets: &[String]) -> Vec<HWND> {
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
        let exe = ctx
            .exe_cache
            .entry(pid)
            .or_insert_with(|| exe_name_of(pid).unwrap_or_default());
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

// ---------------- 引擎 ----------------

pub struct Engine {
    pub targets: Vec<String>,
    pub settings: Settings,
    pub hidden: Vec<HiddenWindow>,
    pub hidden_tray: Vec<IconId>,
}

impl Engine {
    pub fn new() -> Self {
        let settings = load_settings();
        let mut targets = if settings.restore_targets_on_start {
            load_targets()
        } else {
            Vec::new()
        };
        for a in std::env::args().skip(1) {
            if !targets.iter().any(|x| x == &a) {
                targets.push(a);
            }
        }
        Engine {
            targets,
            settings,
            hidden: Vec::new(),
            hidden_tray: Vec::new(),
        }
    }

    pub fn set_targets(&mut self, t: Vec<String>) {
        self.targets = t;
        save_targets(&self.targets);
    }

    pub fn set_settings(&mut self, s: Settings) {
        self.settings = s;
        save_settings(&s);
    }

    /// 隐藏目标的窗口 + 托盘图标，返回状态消息
    pub fn do_hide(&mut self) -> String {
        if self.targets.is_empty() {
            return "请至少先勾选一个目标应用进程！".to_string();
        }
        let windows = collect_visible_windows(&self.targets);
        let mut icons: Vec<IconId> = Vec::new();
        let procs = running_processes();
        for t in &self.targets {
            let pids: Vec<u32> = procs
                .iter()
                .filter(|(_, name)| exe_matches(t, name))
                .map(|(pid, _)| *pid)
                .collect();
            for id in discover_tray_icons(t, &pids) {
                if !icons.iter().any(|x| x.hwnd == id.hwnd && x.uid == id.uid) {
                    icons.push(id);
                }
            }
        }
        if windows.is_empty() && icons.is_empty() {
            return "未找到可隐藏的窗口或托盘图标（目标未运行或已隐藏）".to_string();
        }

        let mut hid = 0u32;
        for hwnd in windows {
            unsafe {
                let mut wp = WINDOWPLACEMENT {
                    length: size_of::<WINDOWPLACEMENT>() as u32,
                    ..Default::default()
                };
                if GetWindowPlacement(hwnd, &mut wp).is_err() {
                    continue;
                }
                let _ = ShowWindow(hwnd, SW_HIDE);
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                self.hidden.push(HiddenWindow {
                    hwnd,
                    placement: wp,
                    exe: exe_name_of(pid).unwrap_or_default(),
                });
                hid += 1;
            }
        }

        let mut hid_icons = 0u32;
        for id in icons {
            if set_icon_hidden(&id, true) {
                self.hidden_tray.push(id);
                hid_icons += 1;
            }
        }
        format!("已成功隐藏 {hid} 个目标应用窗口、{hid_icons} 个托盘图标")
    }

    /// 恢复全部，返回状态消息
    pub fn do_show(&mut self) -> String {
        let mut restored = 0u32;
        let mut foreground: Option<HWND> = None;
        for hw in self.hidden.drain(..) {
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
        if let Some(hwnd) = foreground {
            unsafe {
                let _ = SetForegroundWindow(hwnd);
            }
        }
        let mut restored_icons = 0u32;
        for id in self.hidden_tray.drain(..) {
            if set_icon_hidden(&id, false) {
                restored_icons += 1;
            }
        }
        if restored > 0 || restored_icons > 0 {
            format!("已显示 {restored} 个窗口、{restored_icons} 个托盘图标")
        } else {
            "当前没有已隐藏的窗口或托盘图标".to_string()
        }
    }

    pub fn do_toggle(&mut self) -> String {
        if !self.hidden.is_empty() || !self.hidden_tray.is_empty() {
            self.do_show()
        } else {
            self.do_hide()
        }
    }

    /// 退出（按设置恢复目标）
    pub fn do_exit(&mut self) {
        if self.settings.restore_on_exit {
            let _ = self.do_show();
        }
    }

    /// 列表数据（运行中应用：窗口 + 仅托盘）
    pub fn scan_apps(&self) -> Vec<(String, String)> {
        use std::collections::HashMap as Map;
        struct Ctx {
            cache: Map<u32, String>,
            found: Map<String, String>,
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
            let exe = ctx
                .cache
                .entry(pid)
                .or_insert_with(|| exe_name_of(pid).unwrap_or_default());
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
            cache: Map::new(),
            found: Map::new(),
        };
        unsafe {
            let _ = EnumWindows(Some(enum_proc), LPARAM(&mut ctx as *mut _ as isize));
        }
        let mut apps: Vec<(String, String)> = ctx
            .found
            .into_iter()
            .map(|(exe, title)| (exe, title))
            .collect();
        for exe in tray_apps() {
            if !apps.iter().any(|(e, _)| exe_matches(e, &exe)) {
                apps.push((exe.clone(), String::new()));
            }
        }
        apps.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        apps
    }

    /// 该 exe 当前是否有被隐藏的窗口/图标（状态徽章用）
    pub fn is_hidden(&self, exe: &str) -> bool {
        self.hidden.iter().any(|h| exe_matches(&h.exe, exe))
    }
}

/// 本体（主窗口）显隐：枚举本进程标题为 BossKey 的顶层窗口
pub fn set_self_visible(visible: bool) {
    struct Ctx {
        pid: u32,
        out: Vec<HWND>,
    }
    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let ctx = &mut *(lparam.0 as *mut Ctx);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == ctx.pid {
            let mut buf = [0u16; 64];
            let n = GetWindowTextW(hwnd, &mut buf) as usize;
            if String::from_utf16_lossy(&buf[..n]) == "BossKey" {
                ctx.out.push(hwnd);
            }
        }
        TRUE
    }
    let mut ctx = Ctx {
        pid: std::process::id(),
        out: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut ctx as *mut _ as isize));
        let _ = writeln!(std::io::stdout(), "[selfvis] visible={} found={}", visible, ctx.out.len());
        for h in ctx.out {
            let _ = ShowWindow(h, if visible { SW_SHOW } else { SW_HIDE });
            if visible {
                let _ = SetForegroundWindow(h);
            }
        }
    }
}

// 部分 Win32 调用的短导入（仅 core 使用）
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, GetWindowPlacement, GetWindowThreadProcessId, IsWindow,
    IsWindowVisible, SetForegroundWindow, SetWindowPlacement, ShowWindow, SW_HIDE,
    SW_SHOWMINIMIZED, WINDOWPLACEMENT,
};

#[allow(dead_code)]
fn _anchor(_: COLORREF, _: POINT, _: RECT) {}
