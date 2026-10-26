# bosskey

Windows 老板键工具：按全局热键一键隐藏指定进程的窗口、任务栏按钮、托盘图标，并屏蔽其弹窗与通知；再按一次精确恢复。

## 当前进度

- [x] 技术选型调研（Win10/Win11 窗口/托盘/通知/热键方案对比、总体架构、MVP 路线图）→ `docs/技术选型讨论.md`
- [x] 同类产品分析验证（参考软件 6.1.0 四层托盘隐藏策略、WH_GETMESSAGE 注入机制）→ `docs/参考软件-6.1.0-行为分析.md`
- [ ] P0 真机实验（`NotifyIconSettings\IsPromoted` 即时生效性等，见选型文档 §7）
- [ ] MVP：窗口隐藏/恢复 + 全局热键 + Win11 托盘 + 通知屏蔽

## 技术方向（摘要）

| 决策点 | 结论 |
|---|---|
| 语言栈 | C#/.NET 8 + CsWin32 / Vanara |
| 窗口隐藏 | `ShowWindow(SW_HIDE)` + `GetWindowPlacement` 快照回放 |
| 托盘隐藏 | 能力探测分层：Win10 toolbar 消息 / Win11 `Shell_NotifyIcon(NIM_MODIFY, NIS_HIDDEN)` + 注入兜底 + `NotifyIconSettings` |
| 通知屏蔽 | per-AUMID `Enabled=0` + 全局勿扰 + 窗口兜底三层 |
| 热键 | `RegisterHotKey(MOD_NOREPEAT)` + `WH_KEYBOARD_LL` 兜底 |
