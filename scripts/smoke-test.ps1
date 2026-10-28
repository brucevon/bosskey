# bosskey 冒烟测试（egui 版）
# 覆盖：后台消息线程热键（Ctrl+Alt+H 目标显隐 / Ctrl+Alt+B 本体显隐）
# 触发方式：向后台线程（stdout 输出的 MSGTID）投递 WM_HOTKEY
# 不启动/不关闭 notepad——请先自行打开至少一个 notepad 窗口
param([string]$Exe = "E:\project\bosskey\target\debug\bosskey.exe")
$ErrorActionPreference = 'Stop'

Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text;
public class Smoke {
[DllImport("user32.dll")] public static extern bool PostThreadMessage(uint tid, uint msg, IntPtr wParam, IntPtr l);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
public delegate bool EnumProc(IntPtr h, IntPtr l);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
public static IntPtr Found; public static string Want;
public static bool Cb(IntPtr h, IntPtr l) { var sb = new StringBuilder(256); GetWindowTextW(h, sb, 256); if (sb.ToString() == Want) { Found = h; return false; } return true; }
public static IntPtr FindByTitle(string t) { Found = IntPtr.Zero; Want = t; EnumWindows(Cb, IntPtr.Zero); return Found; }
}
"@

$wmHotkey = 0x0312
$bosskey = $Exe
$out = "$env:TEMP\bosskey-out.txt"

function Get-VisibleNotepadCount {
    @(Get-Process Notepad -ErrorAction SilentlyContinue |
        Where-Object { $_.MainWindowHandle -ne 0 -and [Smoke]::IsWindowVisible($_.MainWindowHandle) }).Count
}

$before = Get-VisibleNotepadCount
if ($before -eq 0) { throw "请先打开至少一个 notepad 窗口再运行本测试" }
Write-Host "初始可见 notepad 窗口数: $before"

# 启动并取后台消息线程 TID
$boss = Start-Process $bosskey -ArgumentList 'notepad.exe' -PassThru -RedirectStandardOutput $out
$tid = 0
foreach ($i in 1..30) {
    Start-Sleep -Milliseconds 500
    if (Test-Path $out) {
        $m = Select-String -Path $out -Pattern 'MSGTID=(\d+)' -AllMatches
        if ($m) { $tid = [uint32]$m.Matches[0].Groups[1].Value; break }
    }
}
if ($tid -eq 0) { Stop-Process -Id $boss.Id -Force -ErrorAction SilentlyContinue; throw "未获取到后台消息线程 TID" }
Write-Host "bosskey pid=$($boss.Id) 消息线程 tid=$tid"

function Toggle([int]$id) {
    [Smoke]::PostThreadMessage($tid, $wmHotkey, [IntPtr]$id, [IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 600
}

# 1) 热键隐藏目标（wParam=1）
Toggle 1
$hiddenCount = Get-VisibleNotepadCount
Write-Host "[热键隐藏] 可见 notepad 窗口数: $hiddenCount  (期望 0)"

# 2) 热键显示目标
Toggle 1
$restoredCount = Get-VisibleNotepadCount
Write-Host "[热键显示] 可见 notepad 窗口数: $restoredCount  (期望 $before)"

# 3) 本体显隐（wParam=2）：窗口隐藏再呼回（按标题查真值，MainWindowHandle 在隐藏后不可靠）
function Get-SelfVisible {
    $h = [Smoke]::FindByTitle('BossKey')
    if ($h -eq [IntPtr]::Zero) { return $false }
    return [Smoke]::IsWindowVisible($h)
}
Toggle 2
$selfHidden = -not (Get-SelfVisible)
Write-Host "[本体热键隐藏] IsWindowVisible=$(-not $selfHidden)  (期望 False)"
Toggle 2
$selfShown = Get-SelfVisible
Write-Host "[本体热键呼回] IsWindowVisible=$selfShown  (期望 True)"

Write-Host "--- bosskey 输出 ---"
Get-Content $out -ErrorAction SilentlyContinue
Write-Host "-------------------"

Stop-Process -Id $boss.Id -Force -ErrorAction SilentlyContinue

$pass = ($hiddenCount -eq 0) -and ($restoredCount -eq $before) -and $selfHidden -and $selfShown
if ($pass) { Write-Host "SMOKE TEST PASSED"; exit 0 }
else { Write-Host "SMOKE TEST FAILED"; exit 1 }
