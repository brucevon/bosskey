# bosskey

Windows 老板键工具：按全局热键一键隐藏指定进程的窗口、任务栏按钮、托盘图标，并屏蔽其弹窗与通知；再按一次精确恢复。

技术栈：Rust + [windows-rs](https://github.com/microsoft/windows-rs)。

## 用法（绿色版）

```powershell
cargo build --release
.\target\release\bosskey.exe            # 双击即用，无需安装
```

- 界面：自动列出运行中的应用——**有窗口的进程** + **仅托盘图标的应用**（来源 `NotifyIconSettings`，仅保留仍在运行的）；不含 bosskey 自身。约 2 秒自动刷新（类似任务管理器）；**默认全部不勾选**，勾选仅本次会话生效；进程退出即从列表消失。按钮：**刷新列表 / 隐藏 / 显示 / 退出并恢复**
- 全局热键：`Ctrl+Alt+H` 隐藏/显示目标，`Ctrl+Alt+B` 隐藏/显示 bosskey 本体（**窗口 + 托盘图标一起显隐**）
- 后台运行：点窗口 **X 不退出**，驻留托盘图标（左键点托盘图标呼出主窗口，右键菜单可退出）；「退出并恢复」按钮或托盘菜单退出时自动恢复目标窗口
- 目标自动识别**窗口 + 托盘图标**，隐藏时一起藏（不必两者都有）：窗口走 `SW_HIDE`（任务栏按钮/Alt-Tab 项随之消失）+ `WINDOWPLACEMENT` 状态快照；托盘图标走跨进程 `NIS_HIDDEN`（身份来自 `NotifyIconSettings` + `Shell_NotifyIconGetRect` 探测）
- 恢复 → 按原状态还原（位置/最小化/最大化），并激活到前台
- 目标未运行或无窗口 → 不做处理；退出程序时自动恢复仍隐藏的窗口
- 勾选记录写入 exe 同目录 `bosskey.targets.txt`（仅留档，启动时不预载）；启动参数可预置勾选（`bosskey.exe notepad.exe`）

单文件 `bosskey.exe`（约 180KB，纯 windows-rs 无其它依赖）即完整绿色版。

冒烟测试（先打开至少一个 notepad 窗口）：`powershell -File scripts\smoke-test.ps1 -Exe target\release\bosskey.exe`

## 当前进度

- [x] 技术选型调研（Win10/Win11 窗口/托盘/通知/热键方案对比、总体架构、MVP 路线图）→ `docs/技术选型讨论.md`
- [x] 同类产品分析验证（参考软件 6.1.0 四层托盘隐藏策略、WH_GETMESSAGE 注入机制）→ `docs/参考软件-6.1.0-行为分析.md`
- [x] MVP v0：窗口隐藏/显示切换 + 任务栏按钮随窗口隐藏 + 状态恢复 + 全局热键（真机冒烟通过）
- [x] 绿色版 GUI：界面手动操作（目标管理/隐藏/显示/退出恢复）+ 配置持久化，单文件约 180KB
- [x] 目标选取改为运行进程列表 + 复选框勾选；程序图标（窗口+划掉的眼睛）已嵌入 exe
- [x] 进程列表排除 bosskey 自身；本窗口显隐独立热键 Ctrl+Alt+B
- [x] 后台托盘模式：关窗驻托盘、托盘菜单、Ctrl+Alt+B 连托盘图标一起显隐
- [x] 目标托盘图标隐藏/恢复（跨进程 NIS_HIDDEN），窗口与托盘自动识别一起处理（IDMan 实测通过）
- [ ] P0 真机实验（`NotifyIconSettings\IsPromoted` 即时生效性等，见选型文档 §7）
- [ ] MVP v1：托盘图标隐藏 + 通知屏蔽

## 技术方向（摘要）

| 决策点 | 结论 |
|---|---|
| 语言栈 | C#/.NET 8 + CsWin32 / Vanara |
| 窗口隐藏 | `ShowWindow(SW_HIDE)` + `GetWindowPlacement` 快照回放 |
| 托盘隐藏 | 能力探测分层：Win10 toolbar 消息 / Win11 `Shell_NotifyIcon(NIM_MODIFY, NIS_HIDDEN)` + 注入兜底 + `NotifyIconSettings` |
| 通知屏蔽 | per-AUMID `Enabled=0` + 全局勿扰 + 窗口兜底三层 |
| 热键 | `RegisterHotKey(MOD_NOREPEAT)` + `WH_KEYBOARD_LL` 兜底 |
