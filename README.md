# BossKey

Windows 老板键工具：一键隐藏 / 恢复指定程序的痕迹。

<img src="assets/screenshot.png" width="720" alt="BossKey 界面" />

## 功能

| 功能 | 说明 |
| --- | --- |
| 一键隐藏 / 恢复 | 隐藏目标程序的全部窗口（任务栏按钮与 Alt-Tab 项同步消失）与托盘图标；恢复时按原位置及最小化 / 最大化状态还原 |
| 应用列表 | 自动识别窗口应用与仅托盘图标应用，支持搜索过滤、全选 / 反选、勾选目标 |
| 自定义热键 | 设置页可为「隐藏 / 显示目标」与「BossKey 显隐」录制任意组合键，即时生效并保存 |
| 后台守护 | 关闭窗口即退居托盘值守；托盘菜单可呼出主窗口或退出（退出时按设置恢复目标） |
| 检查更新 / 立即升级 | 从 GitHub Releases 检查新版本，点「立即升级」自动下载安装包；确认后自动关闭 BossKey、静默安装并重启新版本 |
| 单实例保护 | 重复启动会唤醒已运行的实例，不会出现两个托盘图标与热键冲突 |
| 深浅双主题 | 内置亮色 / 暗色主题一键切换 |
| 安装版 / 绿色版 | 提供安装包与免安装单文件版，配置保存在 exe 同目录 |

默认热键：`Ctrl+Alt+H` 隐藏 / 显示目标，`Ctrl+Alt+B` 隐藏 / 显示 BossKey 本体。

## 安装与升级

- **安装版**：运行 `BossKey_v<版本>.exe` 按向导安装（按用户安装，默认目录 `%LocalAppData%\Programs\BossKey`）。
- **绿色版**：`BossKey_Portable_v<版本>.exe` 免安装，配置写在 exe 同目录，拷走一个文件即带走全部设置。
- **自动升级**：设置 → 检查更新 → 立即升级。下载完成后会提示关闭 BossKey：可选择「自动关闭并安装」或「手动关闭」（退出后自行运行安装包）。
  注意：自动升级统一使用安装包，绿色版升级后会安装到默认目录，原目录下的 exe 与配置不会自动迁移。

## 技术栈

| 组件 | 版本 | 用途 |
| --- | --- | --- |
| Rust / Cargo | 1.99.1 | 编译工具链（edition 2021，MSVC） |
| eframe / egui | 0.37.0 | 窗口与自绘 UI（glow 渲染后端） |
| windows（windows-rs） | 0.61 | Win32 API：全局热键、托盘图标、窗口与进程枚举 |
| winres | 0.1 | 构建期写入 exe 图标与版本资源 |
| PowerShell | 7+ | 打包（`scripts/release.ps1`）与冒烟测试脚本 |
| Inno Setup | 6 | 生成安装版 `BossKey_v<版本>.exe` |

## 编译

环境要求：Windows 10 及以上、[Rust](https://rustup.rs)（MSVC 工具链）。

```powershell
cargo build --release
# 产物：target\release\bosskey.exe
```

可选：按 git tag 版本号出两个产物（安装版需 [Inno Setup 6](https://jrsoftware.org/isinfo.php)）

```powershell
powershell -File scripts\release.ps1
# 安装版：dist\BossKey_v<版本>.exe
# 绿色版：dist\BossKey_Portable_v<版本>.exe
```

## 素材与图标

- 应用图标（`assets/bosskey.ico`、`assets/bosskey-256.rgba`）由 Google Gemini 生成，不含任何第三方商标或授权素材；仅用于本项目。
- 如需替换图标，请同步更新本节说明，确保素材来源可追溯。

## 开源协议

本项目采用 [GPL-3.0](LICENSE) 协议开源：

- **可自由使用、修改、分发，包括商业用途**
- **商业分发必须公开完整源码**（GPL 传染性：任何分发——无论商用与否——均须随附源码）
