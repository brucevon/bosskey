//! ui — egui 界面（设计稿风格：zinc 深色 / 靛紫主行动 / 状态徽章）
use std::io::Write;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use crate::backend::Evt;
use crate::core::{self, Engine};

// ---------------- 配色（设计稿 token）----------------

const BG: Color32 = Color32::from_rgb(0x09, 0x09, 0x0B);
const BG_HEADER: Color32 = Color32::from_rgb(0x12, 0x12, 0x15);
const PANEL: Color32 = Color32::from_rgb(0x18, 0x18, 0x1B);
const CARD: Color32 = Color32::from_rgb(0x27, 0x27, 0x2A);
const BORDER: Color32 = Color32::from_rgb(0x3F, 0x3F, 0x46);
const BORDER_SOFT: Color32 = Color32::from_rgb(0x2E, 0x2E, 0x33);

const TEXT: Color32 = Color32::from_rgb(0xF4, 0xF4, 0xF5);
const TEXT_DIM: Color32 = Color32::from_rgb(0xA1, 0xA1, 0xAA);
const TEXT_MUTE: Color32 = Color32::from_rgb(0x71, 0x71, 0x7A);

const INDIGO: Color32 = Color32::from_rgb(0x63, 0x66, 0xF1);
const AMBER: Color32 = Color32::from_rgb(0xF5, 0x9E, 0x0B);
const AMBER_DK: Color32 = Color32::from_rgb(0xD9, 0x77, 0x06);
const ROSE: Color32 = Color32::from_rgb(0xFB, 0x71, 0x85);
const ROSE_BG: Color32 = Color32::from_rgb(0x2A, 0x12, 0x18);
const ROSE_BD: Color32 = Color32::from_rgb(0x8F, 0x2A, 0x3E);
const EMERALD: Color32 = Color32::from_rgb(0x34, 0xD3, 0x99);
const EMERALD_BG: Color32 = Color32::from_rgb(0x05, 0x2E, 0x22);
const AMBER_BG: Color32 = Color32::from_rgb(0x45, 0x1A, 0x03);
const AMBER_FG: Color32 = Color32::from_rgb(0xFB, 0xBF, 0x24);

const ROW_H: f32 = 40.0;

struct Row {
    exe: String,
    title: String,
}

pub struct BossKeyApp {
    pub engine: Engine,
    rx: Receiver<Evt>,
    rows: Vec<Row>,
    search: String,
    toast: Option<(String, Instant)>,
    settings_open: bool,
    self_visible: bool,
    last_scan: Instant,
    maxed: bool,
}

impl BossKeyApp {
    pub fn new(rx: Receiver<Evt>) -> Self {
        let mut app = BossKeyApp {
            engine: Engine::new(),
            rx,
            rows: Vec::new(),
            search: String::new(),
            toast: None,
            settings_open: false,
            self_visible: true,
            last_scan: Instant::now() - Duration::from_secs(10),
            maxed: false,
        };
        app.rescan();
        app
    }

    fn rescan(&mut self) {
        let apps = self.engine.scan_apps();
        self.rows = apps
            .into_iter()
            .map(|(exe, title)| Row { exe, title })
            .collect();
        self.last_scan = Instant::now();
    }

    fn toast(&mut self, msg: &str) {
        self.toast = Some((msg.to_string(), Instant::now()));
    }

    fn filtered_rows(&self) -> Vec<(String, String)> {
        let q = self.search.trim().to_lowercase();
        self.rows
            .iter()
            .filter(|r| {
                q.is_empty()
                    || r.exe.to_lowercase().contains(&q)
                    || r.title.to_lowercase().contains(&q)
            })
            .map(|r| (r.exe.clone(), r.title.clone()))
            .collect()
    }

    fn checked(&self, exe: &str) -> bool {
        self.engine.targets.iter().any(|x| core::exe_matches(x, exe))
    }

    fn toggle_target(&mut self, exe: &str) {
        let mut t = self.engine.targets.clone();
        if let Some(pos) = t.iter().position(|x| core::exe_matches(x, exe)) {
            t.remove(pos);
        } else {
            t.push(exe.to_string());
        }
        self.engine.set_targets(t);
    }
}

impl eframe::App for BossKeyApp {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let win = frame.winit_window().cloned();
        // ---- 后台事件 ----
        while let Ok(evt) = self.rx.try_recv() {
            match evt {
                Evt::ToggleTargets => {
                    let msg = self.engine.do_toggle();
                    self.toast(&msg);
                }
                Evt::ToggleSelf => {
                    self.self_visible = !self.self_visible;
                    let _ = writeln!(std::io::stdout(), "[ui] ToggleSelf -> {}", self.self_visible);
                    core::set_self_visible(self.self_visible);
                }
                Evt::ShowUi => {
                    self.self_visible = true;
                    core::set_self_visible(true);
                }
                Evt::OpenSettings => {
                    self.self_visible = true;
                    core::set_self_visible(true);
                    self.settings_open = true;
                }
                Evt::Exit => {
                    self.engine.do_exit();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        // ---- 关窗（Alt+F4/系统关闭）→ 后台 ----
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.self_visible = false;
            core::set_self_visible(false);
            return;
        }

        // ---- 定时扫描（2s，类似任务管理器）----
        if self.last_scan.elapsed() >= Duration::from_secs(2) {
            self.rescan();
        }
        ctx.request_repaint_after(Duration::from_millis(400));

    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let win = frame.winit_window().cloned();
        // ---- 背景 ----
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, BG);

        // ================= 标题栏 =================
        egui::Panel::top("titlebar")
            .exact_size(44.0)
            .frame(egui::Frame::NONE.fill(BG_HEADER))
            .show(ui, |ui| {
                let drag = ui.interact(ui.max_rect(), ui.id().with("title_drag"), Sense::click_and_drag());
                if drag.dragged() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(28.0, 28.0), Sense::hover());
                    let painter = ui.painter();
                    painter.rect_filled(rect, 7.0, INDIGO);
                    painter.text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        "B",
                        FontId::proportional(15.0),
                        Color32::WHITE,
                    );
                    ui.add_space(8.0);
                    ui.label(RichText::new("BossKey").size(15.0).strong().color(TEXT));
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                            .size(10.0)
                            .monospace()
                            .color(TEXT_MUTE),
                    );
                    ui.add_space(10.0);
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(118.0, 24.0), Sense::hover());
                    ui.painter().rect_filled(rect, 12.0, EMERALD_BG);
                    ui.painter().rect_stroke(
                        rect,
                        12.0,
                        Stroke::new(1.0, Color32::from_rgb(0x05, 0x96, 0x69)),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().circle_filled(
                        Pos2::new(rect.left() + 12.0, rect.center().y),
                        3.0,
                        EMERALD,
                    );
                    ui.painter().text(
                        Pos2::new(rect.left() + 22.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        "后台守护中",
                        FontId::proportional(11.0),
                        EMERALD,
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(8.0);
                        if title_btn(ui, "✕", Some(ROSE)) {
                            self.self_visible = false;
                            core::set_self_visible(false);
                        }
                        if title_btn(ui, "▢", None) {
                            self.maxed = !self.maxed;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(self.maxed));
                        }
                        if title_btn(ui, "—", None) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                    });
                });
            });

        // ================= 快捷键栏 =================
        egui::Panel::top("hotkeys")
            .exact_size(52.0)
            .frame(egui::Frame::NONE.fill(PANEL))
            .show(ui, |ui| {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    ui.label(RichText::new("一键隐藏目标:").size(11.0).color(TEXT_MUTE));
                    kbd_chip(ui, "Ctrl + Alt + H", true);
                    ui.add_space(16.0);
                    ui.label(RichText::new("本体验隐:").size(11.0).color(TEXT_MUTE));
                    kbd_chip(ui, "Ctrl + Alt + B", false);
                    ui.add_space(16.0);
                    ui.label(
                        RichText::new("关闭窗口即退居系统托盘")
                            .size(11.0)
                            .color(TEXT_MUTE),
                    );
                });
            });

        // ================= 底栏 =================
        egui::Panel::bottom("footer")
            .exact_size(76.0)
            .frame(egui::Frame::NONE.fill(BG_HEADER))
            .show(ui, |ui| {
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    if chip_button(ui, "参数设置", CARD, BORDER, TEXT_DIM, 108.0) {
                        self.settings_open = true;
                    }
                    ui.add_space(10.0);
                    if chip_button(ui, "恢复并彻底退出", ROSE_BG, ROSE_BD, ROSE, 160.0) {
                        self.engine.do_exit();
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(20.0);
                        if chip_button(ui, "一键恢复显示", INDIGO, INDIGO, Color32::WHITE, 150.0)
                        {
                            let msg = self.engine.do_show();
                            self.toast(&msg);
                        }
                        ui.add_space(10.0);
                        if chip_button(
                            ui,
                            "立即隐藏选中目标",
                            AMBER_DK,
                            AMBER,
                            Color32::WHITE,
                            170.0,
                        ) {
                            let msg = self.engine.do_hide();
                            self.toast(&msg);
                        }
                    });
                });
            });

        // ================= 主体 =================
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(BG))
            .show(ui, |ui| {
                ui.add_space(14.0);
                // 工具栏
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    let te = egui::TextEdit::singleline(&mut self.search)
                        .hint_text("搜索进程名或窗口标题...")
                        .desired_width(380.0);
                    ui.add(te);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(20.0);
                        if chip_button(ui, "刷新进程", CARD, BORDER, TEXT_DIM, 110.0) {
                            self.rescan();
                        }
                        ui.add_space(8.0);
                        if chip_button(ui, "全选 / 反选", CARD, BORDER, TEXT_DIM, 126.0) {
                            let rows = self.filtered_rows();
                            let all = rows.iter().all(|(exe, _)| self.checked(exe));
                            let mut t = self.engine.targets.clone();
                            for (exe, _) in &rows {
                                let has = t.iter().any(|x| core::exe_matches(x, exe));
                                if all && has {
                                    t.retain(|x| !core::exe_matches(x, exe));
                                } else if !all && !has {
                                    t.push(exe.clone());
                                }
                            }
                            self.engine.set_targets(t);
                        }
                        ui.add_space(16.0);
                        let n = self.engine.targets.len();
                        ui.label(
                            RichText::new(format!("已勾选 {} 项", n))
                                .size(11.0)
                                .color(Color32::from_rgb(0x81, 0x8C, 0xF8)),
                        );
                    });
                });

                ui.add_space(10.0);

                // 列表面板
                let panel_rect = ui.available_rect_before_wrap();
                let rows_rect = Rect::from_min_size(
                    Pos2::new(panel_rect.left() + 20.0, panel_rect.top()),
                    Vec2::new(panel_rect.width() - 40.0, panel_rect.height() - 14.0),
                );
                ui.painter().rect_filled(rows_rect, 12.0, PANEL);
                ui.painter().rect_stroke(rows_rect, 12.0, Stroke::new(1.0, BORDER_SOFT), egui::StrokeKind::Inside);

                let head_y = rows_rect.top() + 16.0;
                let painter = ui.painter();
                painter.text(
                    Pos2::new(rows_rect.left() + 62.0, head_y),
                    Align2::LEFT_CENTER,
                    "应用进程名称",
                    FontId::proportional(11.0),
                    TEXT_MUTE,
                );
                painter.text(
                    Pos2::new(rows_rect.left() + 280.0, head_y),
                    Align2::LEFT_CENTER,
                    "匹配窗口标题 / 描述",
                    FontId::proportional(11.0),
                    TEXT_MUTE,
                );
                painter.text(
                    Pos2::new(rows_rect.right() - 80.0, head_y),
                    Align2::CENTER_CENTER,
                    "状态",
                    FontId::proportional(11.0),
                    TEXT_MUTE,
                );
                painter.line_segment(
                    [
                        Pos2::new(rows_rect.left(), rows_rect.top() + 32.0),
                        Pos2::new(rows_rect.right(), rows_rect.top() + 32.0),
                    ],
                    Stroke::new(1.0, BORDER_SOFT),
                );

                // 行（可滚动）
                let rows = self.filtered_rows();
                let mut action: Option<String> = None;
                egui::ScrollArea::vertical()
                    .id_salt("rows")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (exe, title) in &rows {
                            ui.add_space(4.0);
                            let resp = ui.allocate_response(
                                Vec2::new(rows_rect.width() - 24.0, ROW_H),
                                Sense::click(),
                            );
                            let rect = resp.rect;
                            let checked = self.checked(exe);
                            let hidden = self.engine.is_hidden(exe);
                            let hot = resp.hovered();
                            let painter = ui.painter();
                            let bgc = if checked {
                                Color32::from_rgb(0x1E, 0x1B, 0x2E)
                            } else if hot {
                                Color32::from_rgb(0x23, 0x23, 0x28)
                            } else {
                                PANEL
                            };
                            painter.rect_filled(rect, 4.0, bgc);

                            let cy = rect.center().y;
                            let cb = Rect::from_min_size(
                                Pos2::new(rect.left() + 20.0, cy - 9.0),
                                Vec2::new(18.0, 18.0),
                            );
                            if checked {
                                painter.rect_filled(cb, 5.0, INDIGO);
                                let tick = Stroke::new(2.0, Color32::WHITE);
                                painter.line_segment(
                                    [
                                        Pos2::new(cb.left() + 4.0, cy),
                                        Pos2::new(cb.left() + 8.0, cy + 4.0),
                                    ],
                                    tick,
                                );
                                painter.line_segment(
                                    [
                                        Pos2::new(cb.left() + 8.0, cy + 4.0),
                                        Pos2::new(cb.left() + 14.0, cy - 5.0),
                                    ],
                                    tick,
                                );
                            } else {
                                painter.rect_filled(cb, 5.0, BG);
                                painter.rect_stroke(cb, 5.0, Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
                            }
                            let chip = Rect::from_min_size(
                                Pos2::new(rect.left() + 52.0, cy - 11.0),
                                Vec2::new(22.0, 22.0),
                            );
                            painter.rect_filled(chip, 6.0, CARD);
                            let letter: String =
                                exe.chars().take(1).collect::<String>().to_uppercase();
                            painter.text(
                                chip.center(),
                                Align2::CENTER_CENTER,
                                letter,
                                FontId::monospace(11.0),
                                TEXT_DIM,
                            );
                            painter.text(
                                Pos2::new(rect.left() + 84.0, cy),
                                Align2::LEFT_CENTER,
                                exe,
                                FontId::monospace(13.0),
                                TEXT,
                            );
                            painter.text(
                                Pos2::new(rect.left() + 262.0, cy),
                                Align2::LEFT_CENTER,
                                title,
                                FontId::proportional(12.5),
                                TEXT_DIM,
                            );
                            let (label, fg, bg) = if hidden {
                                ("已隐藏", AMBER_FG, AMBER_BG)
                            } else {
                                ("显示中", EMERALD, EMERALD_BG)
                            };
                            let pill = Rect::from_center_size(
                                Pos2::new(rect.right() - 52.0, cy),
                                Vec2::new(56.0, 22.0),
                            );
                            painter.rect_filled(pill, 11.0, bg);
                            painter.text(
                                pill.center(),
                                Align2::CENTER_CENTER,
                                label,
                                FontId::proportional(11.0),
                                fg,
                            );

                            if resp.clicked() {
                                action = Some(exe.clone());
                            }
                        }
                    });

                if let Some(exe) = action {
                    self.toggle_target(&exe);
                }
            });

        // ================= 设置弹窗 =================
        if self.settings_open {
            let mut open = self.settings_open;
            egui::Window::new(RichText::new("BossKey 快捷键及高级首选项").size(13.0))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ui, |ui| {
                    ui.add_space(6.0);
                    ui.label(RichText::new("全局隐藏目标热键").size(11.0).color(TEXT_DIM));
                    ui.add_space(2.0);
                    kbd_chip(ui, "Ctrl + Alt + H", true);
                    ui.add_space(8.0);
                    ui.label(RichText::new("本体呼出/隐藏热键").size(11.0).color(TEXT_DIM));
                    ui.add_space(2.0);
                    kbd_chip(ui, "Ctrl + Alt + B", false);
                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(6.0);
                    let mut s = self.engine.settings;
                    let mut changed = false;
                    if ui
                        .checkbox(
                            &mut s.restore_targets_on_start,
                            "启动时恢复上次勾选的目标",
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    if ui
                        .checkbox(&mut s.restore_on_exit, "退出时恢复被隐藏的目标")
                        .changed()
                    {
                        changed = true;
                    }
                    if changed {
                        self.engine.set_settings(s);
                    }
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("热键暂不支持自定义 · 改动即时生效并保存")
                            .size(10.0)
                            .color(TEXT_MUTE),
                    );
                });
            self.settings_open = open;
        }

        // ================= Toast =================
        if let Some((msg, at)) = self.toast.clone() {
            if at.elapsed() < Duration::from_secs(2) {
                egui::Area::new(egui::Id::new("toast"))
                    .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-24.0, -92.0))
                    .interactable(false)
                    .show(ui, |ui| {
                        let text_size = ui
                            .painter()
                            .layout_no_wrap(msg.clone(), FontId::proportional(12.5), TEXT)
                            .size();
                        let rect = Rect::from_min_size(
                            ui.cursor().min,
                            Vec2::new(text_size.x + 56.0, 44.0),
                        );
                        ui.allocate_rect(rect, Sense::hover());
                        let painter = ui.painter();
                        painter.rect_filled(rect, 10.0, CARD);
                        painter.rect_stroke(rect, 10.0, Stroke::new(1.0, INDIGO), egui::StrokeKind::Inside);
                        painter.circle_filled(
                            Pos2::new(rect.left() + 20.0, rect.center().y),
                            5.0,
                            EMERALD,
                        );
                        painter.text(
                            Pos2::new(rect.left() + 36.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            msg,
                            FontId::proportional(12.5),
                            TEXT,
                        );
                    });
            } else {
                self.toast = None;
            }
        }
    }
}

// ---------------- 小组件 ----------------

fn title_btn(ui: &mut egui::Ui, glyph: &str, hover: Option<Color32>) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(36.0, 30.0), Sense::click());
    let painter = ui.painter();
    if resp.hovered() {
        if let Some(c) = hover {
            painter.rect_filled(rect, 6.0, c);
        } else {
            painter.rect_filled(rect, 6.0, CARD);
        }
    }
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(13.0),
        if resp.hovered() && hover.is_some() {
            Color32::WHITE
        } else {
            TEXT_DIM
        },
    );
    resp.clicked()
}

fn kbd_chip(ui: &mut egui::Ui, text: &str, accent: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(118.0, 26.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, CARD);
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::monospace(11.0),
        if accent {
            Color32::from_rgb(0xA5, 0xB4, 0xFC)
        } else {
            Color32::from_rgb(0xD4, 0xD4, 0xD8)
        },
    );
}

fn chip_button(
    ui: &mut egui::Ui,
    text: &str,
    bg: Color32,
    border: Color32,
    fg: Color32,
    width: f32,
) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 36.0), Sense::click());
    let painter = ui.painter();
    let bgc = if resp.hovered() && bg == CARD { BORDER } else { bg };
    painter.rect_filled(rect, 8.0, bgc);
    painter.rect_stroke(rect, 8.0, Stroke::new(1.0, border), egui::StrokeKind::Inside);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(12.0),
        fg,
    );
    resp.clicked()
}
