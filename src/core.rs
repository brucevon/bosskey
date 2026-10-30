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

// ---------------- 全局热键 ----------------

pub const MOD_ALT: u32 = 0x0001;
pub const MOD_CONTROL: u32 = 0x0002;
pub const MOD_SHIFT: u32 = 0x0004;
pub const MOD_WIN: u32 = 0x0008;

/// 全局热键：Win32 修饰键位 + 虚拟键码
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    pub mods: u32,
    pub vk: u32,
}

impl Hotkey {
    pub fn new(mods: u32, vk: u32) -> Self {
        Hotkey { mods, vk }
    }

    /// 展示用文本，如 "Ctrl + Alt + H"
    pub fn label(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.mods & MOD_CONTROL != 0 {
            parts.push("Ctrl".to_string());
        }
        if self.mods & MOD_ALT != 0 {
            parts.push("Alt".to_string());
        }
        if self.mods & MOD_SHIFT != 0 {
            parts.push("Shift".to_string());
        }
        if self.mods & MOD_WIN != 0 {
            parts.push("Win".to_string());
        }
        parts.push(key_name(self.vk));
        parts.join(" + ")
    }

    /// 从 "Ctrl+Alt+H" 之类的文本解析（忽略大小写与空格）
    pub fn parse(s: &str) -> Option<Self> {
        let mut mods = 0u32;
        let mut vk: Option<u32> = None;
        for part in s.split('+') {
            let p = part.trim();
            if p.is_empty() {
                continue;
            }
            match p.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => mods |= MOD_CONTROL,
                "alt" => mods |= MOD_ALT,
                "shift" => mods |= MOD_SHIFT,
                "win" | "meta" => mods |= MOD_WIN,
                _ => {
                    if let Some(v) = vk_from_name(p) {
                        vk = Some(v);
                    }
                }
            }
        }
        match (vk, mods) {
            (Some(vk), m) if m != 0 => Some(Hotkey { mods: m, vk }),
            _ => None,
        }
    }
}

fn key_name(vk: u32) -> String {
    match vk {
        0x08 => "Backspace".into(),
        0x09 => "Tab".into(),
        0x0D => "Enter".into(),
        0x1B => "Esc".into(),
        0x20 => "Space".into(),
        0x21 => "PageUp".into(),
        0x22 => "PageDown".into(),
        0x23 => "End".into(),
        0x24 => "Home".into(),
        0x25 => "Left".into(),
        0x26 => "Up".into(),
        0x27 => "Right".into(),
        0x28 => "Down".into(),
        0x2E => "Delete".into(),
        0x30..=0x39 | 0x41..=0x5A => (vk as u8 as char).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        _ => format!("VK_{vk:02X}"),
    }
}

fn vk_from_name(name: &str) -> Option<u32> {
    let up = name.trim().to_ascii_uppercase();
    if up.len() == 1 {
        let b = up.as_bytes()[0];
        if b.is_ascii_uppercase() || b.is_ascii_digit() {
            return Some(b as u32);
        }
    }
    if let Some(num) = up.strip_prefix('F') {
        if let Ok(i) = num.parse::<u32>() {
            if (1..=24).contains(&i) {
                return Some(0x6F + i);
            }
        }
    }
    Some(match up.as_str() {
        "BACKSPACE" => 0x08,
        "TAB" => 0x09,
        "ENTER" => 0x0D,
        "ESC" | "ESCAPE" => 0x1B,
        "SPACE" => 0x20,
        "PAGEUP" | "PGUP" => 0x21,
        "PAGEDOWN" | "PGDN" => 0x22,
        "END" => 0x23,
        "HOME" => 0x24,
        "LEFT" => 0x25,
        "UP" => 0x26,
        "RIGHT" => 0x27,
        "DOWN" => 0x28,
        "DELETE" | "DEL" => 0x2E,
        _ => return None,
    })
}

// ---------------- 设置 ----------------

#[derive(Clone, Copy)]
pub struct Settings {
    pub restore_targets_on_start: bool,
    pub restore_on_exit: bool,
    pub hotkey_targets: Hotkey,
    pub hotkey_self: Hotkey,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            restore_targets_on_start: false,
            restore_on_exit: true,
            hotkey_targets: Hotkey::new(MOD_CONTROL | MOD_ALT, 0x48), // Ctrl+Alt+H
            hotkey_self: Hotkey::new(MOD_CONTROL | MOD_ALT, 0x42),    // Ctrl+Alt+B
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
                "hotkey_targets" => {
                    if let Some(h) = Hotkey::parse(v) {
                        s.hotkey_targets = h;
                    }
                }
                "hotkey_self" => {
                    if let Some(h) = Hotkey::parse(v) {
                        s.hotkey_self = h;
                    }
                }
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
            "restore_targets_on_start={}\nrestore_on_exit={}\nhotkey_targets={}\nhotkey_self={}\n",
            s.restore_targets_on_start as u8,
            s.restore_on_exit as u8,
            s.hotkey_targets.label(),
            s.hotkey_self.label()
        ),
    );
}

// ---------------- 更新检查 / 外链 ----------------

/// 用默认浏览器打开 URL
pub fn open_url(url: &str) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    unsafe {
        let op = wide("open");
        let file = wide(url);
        let _ = ShellExecuteW(
            None,
            PCWSTR(op.as_ptr()),
            PCWSTR(file.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// 检查 GitHub 最新 Release 的 tag（如 "v0.1.15"）
/// 走 releases.atom（避开 API 未登录限流），使用系统自带 curl.exe 发起 HTTPS 请求
pub fn check_latest_release() -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("curl.exe")
        .args([
            "-sS",
            "-L",
            "--max-time",
            "15",
            "-A",
            "BossKey",
            "https://github.com/brucevon/bosskey/releases.atom",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("无法调用 curl：{e}"))?;
    if !out.status.success() {
        return Err(format!(
            "请求失败（curl 退出码 {}）",
            out.status.code().unwrap_or(-1)
        ));
    }
    let body = String::from_utf8_lossy(&out.stdout);
    body.split("releases/tag/")
        .nth(1)
        .and_then(|s| s.split(['"', '<', '?']).next())
        .map(str::to_string)
        .ok_or_else(|| "无法解析版本信息".to_string())
}

/// a 是否比 b 新（按 "1.2.3" 数字段比较，忽略前缀 v）
pub fn version_newer(a: &str, b: &str) -> bool {
    fn parts(v: &str) -> Vec<u32> {
        v.trim()
            .trim_start_matches('v')
            .split('.')
            .map(|s| s.parse::<u32>().unwrap_or(0))
            .collect()
    }
    let (a, b) = (parts(a), parts(b));
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
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

/// 真实应用窗口判定（任务管理器逻辑）：矩形 >=50x50 且与屏幕相交，
/// 排除后台进程的 0x0 / 离屏幽灵「可见」窗口
fn has_real_window(hwnd: HWND) -> bool {
    unsafe {
        // 任务栏按钮判定（任务管理器「应用」逻辑）：
        // 无属主窗口 或 显式 WS_EX_APPWINDOW；被拥有窗口无任务栏按钮，属后台
        let owner = GetWindow(hwnd, GW_OWNER).unwrap_or_default();
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if !owner.0.is_null() && (ex & WS_EX_APPWINDOW.0 as isize) == 0 {
            return false;
        }
        // DWM 遮蔽窗口（输入法覆盖层/UWP 后台等）对用户不可见——任务管理器同款排除
        let mut cloaked = 0u32;
        if windows::Win32::Graphics::Dwm::DwmGetWindowAttribute(
            hwnd,
            windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE(14), // DWMWA_CLOAKED
            &mut cloaked as *mut u32 as *mut c_void,
            size_of::<u32>() as u32,
        )
        .is_ok()
            && cloaked != 0
        {
            return false;
        }
        let mut rc = RECT::default();
        if GetWindowRect(hwnd, &mut rc).is_err() {
            return false;
        }
        if rc.right - rc.left < 50 || rc.bottom - rc.top < 50 {
            return false;
        }
        let sx = GetSystemMetrics(SM_CXSCREEN);
        let sy = GetSystemMetrics(SM_CYSCREEN);
        rc.right > 0 && rc.bottom > 0 && rc.left < sx && rc.top < sy
    }
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
        // 语言对（缺失时用常见代码页兜底）
        let mut pairs_buf: Vec<u16> = Vec::new();
        unsafe {
            let mut tp: *mut c_void = std::ptr::null_mut();
            let mut tl = 0u32;
            let tkey = wide(r"\VarFileInfo\Translation");
            if VerQueryValueW(buf.as_ptr() as *const c_void, PCWSTR(tkey.as_ptr()), &mut tp, &mut tl)
                .as_bool()
                && !tp.is_null()
            {
                pairs_buf = std::slice::from_raw_parts(tp as *const u16, tl as usize / 2).to_vec();
            }
        }
        if pairs_buf.is_empty() {
            pairs_buf = vec![0x0409, 0x04B0, 0x0409, 0x04E4, 0x0804, 0x04B0, 0x0000, 0x04B0];
        }
        // FileDescription -> ProductName -> None（调用方回落 exe 名，对齐任务管理器）
        for field in [r"\FileDescription", r"\ProductName"] {
            for chunk in pairs_buf.chunks_exact(2) {
                let key = wide(&format!(
                    r"\StringFileInfo\{:04x}{:04x}{}",
                    chunk[0], chunk[1], field
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
///
/// 过滤：
/// 1. NotifyIconSettings 是历史记录 → 探测图标当前存活（Shell_NotifyIconGetRect）
///    注：Windows 无「图标可见性」API——GetRect 对所有图标（含可见）返回同一
///    占位矩形，故与 Windows 设置「其他系统托盘图标」同语义：存活即算托盘应用
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
        if !has_real_window(hwnd) {
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
            if !has_real_window(hwnd) {
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
