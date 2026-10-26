# bosskey 冒烟测试：界面 + 按钮 + 热键 全链路
# 覆盖：界面出现、进程列表已填充、WM_COMMAND 按钮隐藏/显示、WM_HOTKEY 切换
# 不启动/不关闭 notepad——请先自行打开至少一个 notepad 窗口
param([string]$Exe = "E:\project\bosskey\target\debug\bosskey.exe")
$ErrorActionPreference = 'Stop'

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win32Smoke {
    [DllImport("user32.dll")] public static extern bool PostThreadMessage(uint tid, uint msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr h, int id);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern IntPtr PostMessageW(IntPtr h, uint msg, IntPtr w, IntPtr l);
}
"@

$wmHotkey = 0x0312
$wmCommand = 0x0111
$lvmGetItemCount = 0x1004
$idcList = 101; $idcHide = 105; $idcShow = 106
$bosskey = $Exe
$out = "$env:TEMP\bosskey-out.txt"

function Get-VisibleNotepadCount {
    @(Get-Process Notepad -ErrorAction SilentlyContinue |
        Where-Object { $_.MainWindowHandle -ne 0 -and [Win32Smoke]::IsWindowVisible($_.MainWindowHandle) }).Count
}

$before = Get-VisibleNotepadCount
if ($before -eq 0) { throw "请先打开至少一个 notepad 窗口再运行本测试" }
Write-Host "初始可见 notepad 窗口数: $before"

$boss = Start-Process $bosskey -ArgumentList 'notepad.exe' -PassThru -RedirectStandardOutput $out

# 等待界面窗口就绪，经其句柄反查消息循环线程（GUI 进程 Threads[0] 不一定是主线程）
foreach ($i in 1..20) {
    Start-Sleep -Milliseconds 500
    $boss.Refresh()
    if ($boss.MainWindowHandle -ne 0) { break }
}
if ($boss.MainWindowHandle -eq 0) { Stop-Process -Id $boss.Id -Force -ErrorAction SilentlyContinue; throw "bosskey 界面窗口未出现" }
$uiHwnd = $boss.MainWindowHandle
if (-not [Win32Smoke]::IsWindowVisible($uiHwnd)) { throw "bosskey 界面窗口不可见" }
$pid2 = 0
$tid = [Win32Smoke]::GetWindowThreadProcessId($uiHwnd, [ref]$pid2)
Write-Host "bosskey 界面 hwnd=$uiHwnd 可见=True pid=$pid2 tid=$tid"

# 进程列表已填充（勾选目标的数据源）
$listHwnd = [Win32Smoke]::GetDlgItem($uiHwnd, $idcList)
if ($listHwnd -eq [IntPtr]::Zero) { throw "未找到应用列表控件" }
$itemCount = [Win32Smoke]::SendMessageW($listHwnd, $lvmGetItemCount, [IntPtr]::Zero, [IntPtr]::Zero).ToInt64()
Write-Host "应用列表项数: $itemCount  (期望 > 0，含 notepad)"
if ($itemCount -le 0) { throw "应用列表为空" }

function Toggle-Hotkey {
    [Win32Smoke]::PostThreadMessage($tid, $wmHotkey, [IntPtr]1, [IntPtr]0) | Out-Null
    Start-Sleep -Milliseconds 500
}
function Click-Button([int]$id) {
    # WM_COMMAND，wParam 低位 = 控件 ID（等效 BN_CLICKED）
    [Win32Smoke]::PostMessageW($uiHwnd, $wmCommand, [IntPtr]$id, [IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 500
}

# 1) 按钮隐藏（WM_COMMAND 通路 —— 此前手动添加失效的同一条路）
Click-Button $idcHide
$hiddenCount = Get-VisibleNotepadCount
Write-Host "[按钮隐藏] 可见 notepad 窗口数: $hiddenCount  (期望 0)"

# 2) 热键显示
Toggle-Hotkey
$restoredCount = Get-VisibleNotepadCount
Write-Host "[热键显示] 可见 notepad 窗口数: $restoredCount  (期望 $before)"

# 3) 热键隐藏 + 按钮显示，交叉验证两条通路
Toggle-Hotkey
$afterHide = Get-VisibleNotepadCount
Click-Button $idcShow
$afterShow = Get-VisibleNotepadCount
Write-Host "[热键隐藏/按钮显示] $afterHide -> $afterShow  (期望 0 -> $before)"

Write-Host "--- bosskey 输出 ---"
Get-Content $out -ErrorAction SilentlyContinue
Write-Host "-------------------"

Stop-Process -Id $boss.Id -Force -ErrorAction SilentlyContinue

$pass = ($hiddenCount -eq 0) -and ($restoredCount -eq $before) -and ($afterHide -eq 0) -and ($afterShow -eq $before)
if ($pass) { Write-Host "SMOKE TEST PASSED"; exit 0 }
else { Write-Host "SMOKE TEST FAILED"; exit 1 }
