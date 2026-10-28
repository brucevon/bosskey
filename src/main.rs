//! bosskey — Windows 老板键（绿色版，egui 界面）
//!
//! 架构：
//! - `core`    业务引擎（窗口/托盘隐藏恢复、进程扫描、设置）
//! - `backend` Win32 消息线程（全局热键 Ctrl+Alt+H/B + 托盘图标）
//! - `ui`      egui 界面（eframe）
#![windows_subsystem = "windows"]

mod backend;
mod core;
mod ui;

use std::sync::mpsc;

use eframe::egui;

fn main() -> eframe::Result {
    // 中文字体：从系统字体目录加载（保证 exe 仍是单文件绿色版）
    let font_data = [
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\Deng.ttf",
        r"C:\Windows\Fonts\msyh.ttc",
    ]
    .iter()
    .find_map(|p| std::fs::read(p).ok());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1080.0, 720.0])
            .with_min_inner_size([860.0, 560.0])
            .with_decorations(false)
            .with_title("BossKey")
            .with_icon(egui::IconData {
                rgba: include_bytes!("../assets/bosskey-256.rgba").to_vec(),
                width: 256,
                height: 256,
            }),
        ..Default::default()
    };

    let (tx, rx) = mpsc::channel();
    // Win32 消息线程：热键 + 托盘
    backend::spawn(tx);

    eframe::run_native(
        "BossKey",
        options,
        Box::new(move |cc| {
            if let Some(data) = font_data {
                let mut fonts = egui::FontDefinitions::default();
                fonts.font_data.insert(
                    "cjk".to_owned(),
                    std::sync::Arc::new(egui::FontData::from_owned(data)),
                );
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .push("cjk".to_owned());
                fonts
                    .families
                    .entry(egui::FontFamily::Monospace)
                    .or_default()
                    .push("cjk".to_owned());
                cc.egui_ctx.set_fonts(fonts);
            }
            Ok(Box::new(ui::BossKeyApp::new(rx)))
        }),
    )
}
