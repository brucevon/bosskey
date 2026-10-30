//! ui — egui 界面（设计稿风格：zinc 深色 / 靛紫主行动 / 状态徽章）
use std::io::Write;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use crate::backend::Evt;
use crate::core::{self, Engine};

// ---------------- 配色（深色=设计稿 token；浅色=同构亮色）----------------

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub bg_header: Color32,
    pub panel: Color32,
    pub card: Color32,
    pub border: Color32,
    pub border_soft: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_mute: Color32,
    pub indigo: Color32,
    pub indigo_soft: Color32,
    pub amber: Color32,
    pub amber_dk: Color32,
    pub rose: Color32,
    pub rose_bg: Color32,
    pub rose_bd: Color32,
    pub emerald: Color32,
    pub emerald_bg: Color32,
    pub emerald_bd: Color32,
    pub amber_bg: Color32,
    pub amber_fg: Color32,
    pub checked_bg: Color32,
    pub hot_bg: Color32,
    pub kbd_accent: Color32,
}

fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

impl Palette {
    fn dark() -> Self {
        Palette {
            bg: rgb(0x09, 0x09, 0x0B),
            bg_header: rgb(0x12, 0x12, 0x15),
            panel: rgb(0x18, 0x18, 0x1B),
            card: rgb(0x27, 0x27, 0x2A),
            border: rgb(0x3F, 0x3F, 0x46),
            border_soft: rgb(0x2E, 0x2E, 0x33),
            text: rgb(0xF4, 0xF4, 0xF5),
            text_dim: rgb(0xA1, 0xA1, 0xAA),
            text_mute: rgb(0x71, 0x71, 0x7A),
            indigo: rgb(0x63, 0x66, 0xF1),
            indigo_soft: rgb(0x81, 0x8C, 0xF8),
            amber: rgb(0xF5, 0x9E, 0x0B),
            amber_dk: rgb(0xD9, 0x77, 0x06),
            rose: rgb(0xFB, 0x71, 0x85),
            rose_bg: rgb(0x2A, 0x12, 0x18),
            rose_bd: rgb(0x8F, 0x2A, 0x3E),
            emerald: rgb(0x34, 0xD3, 0x99),
            emerald_bg: rgb(0x05, 0x2E, 0x22),
            emerald_bd: rgb(0x05, 0x96, 0x69),
            amber_bg: rgb(0x45, 0x1A, 0x03),
            amber_fg: rgb(0xFB, 0xBF, 0x24),
            checked_bg: rgb(0x1E, 0x1B, 0x2E),
            hot_bg: rgb(0x23, 0x23, 0x28),
            kbd_accent: rgb(0xA5, 0xB4, 0xFC),
        }
    }

    fn light() -> Self {
        Palette {
            bg: rgb(0xF8, 0xFA, 0xFC),
            bg_header: rgb(0xEE, 0xF2, 0xF6),
            panel: rgb(0xFF, 0xFF, 0xFF),
            card: rgb(0xE7, 0xED, 0xF3),
            border: rgb(0xCB, 0xD5, 0xE1),
            border_soft: rgb(0xDC, 0xE3, 0xEB),
            text: rgb(0x0F, 0x17, 0x2A),
            text_dim: rgb(0x47, 0x55, 0x69),
            text_mute: rgb(0x8A, 0x97, 0xA5),
            indigo: rgb(0x4F, 0x46, 0xE5),
            indigo_soft: rgb(0x4F, 0x46, 0xE5),
            amber: rgb(0xF5, 0x9E, 0x0B),
            amber_dk: rgb(0xD9, 0x77, 0x06),
            rose: rgb(0xE1, 0x1D, 0x48),
            rose_bg: rgb(0xFF, 0xE4, 0xE6),
            rose_bd: rgb(0xFB, 0x71, 0x85),
            emerald: rgb(0x05, 0x96, 0x69),
            emerald_bg: rgb(0xD1, 0xFA, 0xE5),
            emerald_bd: rgb(0x34, 0xD3, 0x99),
            amber_bg: rgb(0xFE, 0xF3, 0xC7),
            amber_fg: rgb(0xB4, 0x53, 0x09),
            checked_bg: rgb(0xEE, 0xF2, 0xFF),
            hot_bg: rgb(0xE7, 0xED, 0xF3),
            kbd_accent: rgb(0x4F, 0x46, 0xE5),
        }
    }
}

const ROW_H: f32 = 40.0;

struct Row {
    exe: String,
    name: String,
    title: String,
}

pub struct BossKeyApp {
    pub engine: Engine,
    rx: Receiver<Evt>,
    rows: Vec<Row>,
    search: String,
    toast: Option<(String, Instant)>,
    page: u8, // 0=列表 1=设置
    confirm_exit: bool,
    self_visible: bool,
    last_scan: Instant,
    maxed: bool,
    theme_light: bool,
    logo: Option<egui::TextureHandle>,
    exiting: bool,
    names: std::collections::HashMap<String, String>,
    icons: std::collections::HashMap<String, egui::TextureHandle>,
    pending_icons: std::collections::HashMap<String, Vec<u8>>,
    procs: Vec<(u32, String)>,
    hotkeys: std::sync::Arc<crate::backend::HotkeyCtl>,
    capture: Option<u8>, // Some(0)=一键隐藏目标 Some(1)=BossKey 显隐
}

impl BossKeyApp {
    pub fn new(
        rx: Receiver<Evt>,
        hotkeys: std::sync::Arc<crate::backend::HotkeyCtl>,
    ) -> Self {
        let mut app = BossKeyApp {
            engine: Engine::new(),
            rx,
            rows: Vec::new(),
            search: String::new(),
            toast: None,
            page: 0,
            confirm_exit: false,
            self_visible: true,
            last_scan: Instant::now() - Duration::from_secs(10),
            maxed: false,
            theme_light: true,
            logo: None,
            exiting: false,
            names: std::collections::HashMap::new(),
            icons: std::collections::HashMap::new(),
            pending_icons: std::collections::HashMap::new(),
            procs: Vec::new(),
            hotkeys,
            capture: None,
        };
        app.rescan();
        app
    }

    fn rescan(&mut self) {
        self.procs = core::running_processes();
        let apps = self.engine.scan_apps();
        let mut rows = Vec::with_capacity(apps.len());
        for (exe, title) in apps {
            if !self.names.contains_key(&exe) {
                let (name, icon) = core::exe_shell_info(&exe);
                if let Some(n) = name {
                    self.names.insert(exe.clone(), n);
                }
                if let Some(rgba) = icon {
                    self.pending_icons.insert(exe.clone(), rgba);
                }
            }
            let mut name = self.names.get(&exe).cloned().unwrap_or_else(|| exe.clone());
            // 任务管理器样式：同名进程 >1 时聚合显示「名称 (N)」
            let n = self
                .procs
                .iter()
                .filter(|(_, nm)| core::exe_matches(&exe, nm))
                .count();
            if n > 1 {
                name = format!("{name} ({n})");
            }
            rows.push(Row { exe, name, title });
        }
        self.rows = rows;
        self.last_scan = Instant::now();
    }

    fn toast(&mut self, msg: &str) {
        self.toast = Some((msg.to_string(), Instant::now()));
    }

    fn filtered_rows(&self) -> Vec<(String, String, String)> {
        let q = self.search.trim().to_lowercase();
        self.rows
            .iter()
            .filter(|r| {
                q.is_empty()
                    || r.exe.to_lowercase().contains(&q)
                    || r.name.to_lowercase().contains(&q)
                    || r.title.to_lowercase().contains(&q)
            })
            .map(|r| (r.exe.clone(), r.name.clone(), r.title.clone()))
            .collect()
    }

    fn palette(light: bool) -> Palette {
        if light {
            Palette::light()
        } else {
            Palette::dark()
        }
    }

    /// 将待处理图标转为 egui 纹理（每 exe 一次）
    fn ensure_icons(&mut self, ctx: &egui::Context) {
        let pending: Vec<(String, Vec<u8>)> = self
            .pending_icons
            .drain()
            .collect();
        for (exe, rgba) in pending {
            if self.icons.contains_key(&exe) {
                continue;
            }
            let img = egui::ColorImage::from_rgba_unmultiplied([32, 32], &rgba);
            let handle = ctx.load_texture(format!("app-icon-{exe}"), img, Default::default());
            self.icons.insert(exe, handle);
        }
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

    /// 录制全局热键：读取本帧按下的组合键，成功则保存并通知后台重注册
    fn poll_hotkey_capture(&mut self, ctx: &egui::Context) {
        let which = match self.capture {
            Some(w) => w,
            None => return,
        };
        ctx.request_repaint();

        let mut pressed: Option<(egui::Key, egui::Modifiers)> = None;
        ctx.input(|i| {
            for ev in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = ev
                {
                    pressed = Some((*key, *modifiers));
                }
            }
        });
        let (key, m) = match pressed {
            Some(v) => v,
            None => return,
        };
        if key == egui::Key::Escape {
            self.capture = None;
            return;
        }
        let vk = match vk_of(key) {
            Some(v) => v,
            None => return,
        };
        let mut mods = 0u32;
        if m.ctrl {
            mods |= core::MOD_CONTROL;
        }
        if m.alt {
            mods |= core::MOD_ALT;
        }
        if m.shift {
            mods |= core::MOD_SHIFT;
        }
        if mods == 0 {
            self.toast("请至少按住 Ctrl / Alt / Shift 之一");
            return;
        }
        let hk = core::Hotkey::new(mods, vk);
        let mut s = self.engine.settings;
        let other = if which == 0 {
            s.hotkey_self
        } else {
            s.hotkey_targets
        };
        if hk == other {
            self.toast("两个快捷键不能相同");
            return;
        }
        if which == 0 {
            s.hotkey_targets = hk;
        } else {
            s.hotkey_self = hk;
        }
        self.engine.set_settings(s);
        self.hotkeys.set(s.hotkey_targets, s.hotkey_self);
        crate::backend::reload(&self.hotkeys);
        self.capture = None;
        let msg = format!("快捷键已更新：{}", hk.label());
        self.toast(&msg);
    }
}

/// egui 按键 → Win32 虚拟键码（仅常用键；不支持返回 None）
fn vk_of(key: egui::Key) -> Option<u32> {
    use egui::Key as K;
    Some(match key {
        K::A => 0x41, K::B => 0x42, K::C => 0x43, K::D => 0x44, K::E => 0x45,
        K::F => 0x46, K::G => 0x47, K::H => 0x48, K::I => 0x49, K::J => 0x4A,
        K::K => 0x4B, K::L => 0x4C, K::M => 0x4D, K::N => 0x4E, K::O => 0x4F,
        K::P => 0x50, K::Q => 0x51, K::R => 0x52, K::S => 0x53, K::T => 0x54,
        K::U => 0x55, K::V => 0x56, K::W => 0x57, K::X => 0x58, K::Y => 0x59,
        K::Z => 0x5A,
        K::Num0 => 0x30, K::Num1 => 0x31, K::Num2 => 0x32, K::Num3 => 0x33,
        K::Num4 => 0x34, K::Num5 => 0x35, K::Num6 => 0x36, K::Num7 => 0x37,
        K::Num8 => 0x38, K::Num9 => 0x39,
        K::F1 => 0x70, K::F2 => 0x71, K::F3 => 0x72, K::F4 => 0x73,
        K::F5 => 0x74, K::F6 => 0x75, K::F7 => 0x76, K::F8 => 0x77,
        K::F9 => 0x78, K::F10 => 0x79, K::F11 => 0x7A, K::F12 => 0x7B,
        K::Space => 0x20,
        K::Enter => 0x0D,
        K::Tab => 0x09,
        K::Backspace => 0x08,
        K::Delete => 0x2E,
        K::Insert => 0x2D,
        K::Home => 0x24,
        K::End => 0x23,
        K::PageUp => 0x21,
        K::PageDown => 0x22,
        K::ArrowLeft => 0x25,
        K::ArrowUp => 0x26,
        K::ArrowRight => 0x27,
        K::ArrowDown => 0x28,
        _ => return None,
    })
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
                    self.page = 1;
                }
                Evt::Exit => {
                    self.engine.do_exit();
                    self.exiting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        // ---- 关窗（Alt+F4/系统关闭）→ 后台 ----
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.exiting {
                return; // 真正退出：放行关闭
            }
            // 后台模式：拦截关窗，转驻托盘
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
        let c = Self::palette(self.theme_light);
        self.poll_hotkey_capture(&ctx);
        self.ensure_icons(&ctx);
        if self.logo.is_none() {
            let img = egui::ColorImage::from_rgba_unmultiplied(
                [64, 64],
                include_bytes!("../assets/bosskey-64.rgba"),
            );
            self.logo = Some(ctx.load_texture("bosskey-logo", img, Default::default()));
        }
        ctx.set_visuals(if self.theme_light {
            egui::Visuals::light()
        } else {
            egui::Visuals::dark()
        });
        // ---- 背景 ----
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, c.bg);

        // ================= 标题栏 =================
        egui::Panel::top("titlebar")
            .exact_size(44.0)
            .frame(egui::Frame::NONE.fill(c.bg_header))
            .show(ui, |ui| {
                let drag = ui.interact(ui.max_rect(), ui.id().with("title_drag"), Sense::click_and_drag());
                if drag.dragged() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add_space(14.0);
                    if let Some(tex) = &self.logo {
                        ui.add(
                            egui::Image::new((tex.id(), Vec2::new(30.0, 30.0)))
                                .fit_to_exact_size(Vec2::new(30.0, 30.0)),
                        );
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new("BossKey").size(15.0).strong().color(c.text));
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                            .size(10.0)
                            .monospace()
                            .color(c.text_mute),
                    );
                    ui.add_space(10.0);
                    // 徽章宽度随文字自适应
                    let badge_text = "后台守护中";
                    let tw = ui
                        .painter()
                        .layout_no_wrap(
                            badge_text.to_string(),
                            FontId::proportional(11.0),
                            c.emerald,
                        )
                        .size()
                        .x;
                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::new(tw + 44.0, 24.0), Sense::hover());
                    ui.painter().rect_filled(rect, 12.0, c.emerald_bg);
                    ui.painter().rect_stroke(
                        rect,
                        12.0,
                        Stroke::new(1.0, c.emerald_bd),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().circle_filled(
                        Pos2::new(rect.left() + 12.0, rect.center().y),
                        3.0,
                        c.emerald,
                    );
                    ui.painter().text(
                        Pos2::new(rect.left() + 22.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        "后台守护中",
                        FontId::proportional(11.0),
                        c.emerald,
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(8.0);
                        if win_btn(ui, c, 2) {
                            self.self_visible = false;
                            core::set_self_visible(false);
                        }
                        if win_btn(ui, c, 1) {
                            self.maxed = !self.maxed;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(self.maxed));
                        }
                        if win_btn(ui, c, 0) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                        ui.add_space(10.0);
                        if theme_btn(ui, c, self.theme_light) {
                            self.theme_light = !self.theme_light;
                        }
                    });
                });
            });

        // ================= 菜单栏（标题下方）=================
        egui::Panel::top("menubar")
            .exact_size(34.0)
            .frame(egui::Frame::NONE.fill(c.bg_header))
            .show(ui, |ui| {
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    if menu_item(ui, c, "设置") {
                        self.page = 1;
                    }
                });
            });

        // ================= 底栏（左：快捷键提示；右：动作按钮）=================
        egui::Panel::bottom("footer")
            .exact_size(76.0)
            .frame(egui::Frame::NONE.fill(c.bg_header))
            .show(ui, |ui| {
                ui.add_space(15.0);
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    let hk_targets = self.engine.settings.hotkey_targets.label();
                    let hk_self = self.engine.settings.hotkey_self.label();
                    label_chip(ui, c, "一键隐藏目标:", &hk_targets, true);
                    ui.add_space(16.0);
                    label_chip(ui, c, "BossKey显隐:", &hk_self, false);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(20.0);
                        if chip_button(ui, "退出BossKey", c.rose_bg, c.rose_bg, c.rose_bd, c.rose, 108.0)
                        {
                            self.confirm_exit = true;
                        }
                        ui.add_space(10.0);
                        if chip_button(ui, "恢复选中", c.indigo, c.indigo, c.indigo, Color32::WHITE, 96.0)
                        {
                            let msg = self.engine.do_show();
                            self.toast(&msg);
                        }
                        ui.add_space(10.0);
                        if chip_button(ui, "隐藏选中", c.amber_dk, c.amber, c.amber, Color32::WHITE, 96.0)
                        {
                            let msg = self.engine.do_hide();
                            self.toast(&msg);
                        }
                    });
                });
            });

        // ================= 主体 =================
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(c.bg))
            .show(ui, |ui| {
                if self.page == 0 {
                ui.add_space(14.0);
                // 工具栏
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    search_field(ui, c, &mut self.search, 380.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(20.0);
                        if chip_button(ui, "刷新进程", c.card, c.border, c.border, c.text_dim, 110.0) {
                            self.rescan();
                        }
                        ui.add_space(8.0);
                        if chip_button(ui, "全选 / 反选", c.card, c.border, c.border, c.text_dim, 126.0) {
                            let rows = self.filtered_rows();
                            let all = rows.iter().all(|(exe, _, _)| self.checked(exe));
                            let mut t = self.engine.targets.clone();
                            for (exe, _, _) in &rows {
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
                                .color(c.indigo_soft),
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
                ui.painter().rect_filled(rows_rect, 12.0, c.panel);
                ui.painter().rect_stroke(rows_rect, 12.0, Stroke::new(1.0, c.border_soft), egui::StrokeKind::Inside);

                let head_y = rows_rect.top() + 16.0;
                let painter = ui.painter();
                painter.text(
                    Pos2::new(rows_rect.left() + 88.0, head_y),
                    Align2::LEFT_CENTER,
                    "应用名称",
                    FontId::proportional(11.0),
                    c.text_mute,
                );
                painter.text(
                    Pos2::new(rows_rect.left() + 352.0, head_y),
                    Align2::LEFT_CENTER,
                    "进程名称",
                    FontId::proportional(11.0),
                    c.text_mute,
                );
                painter.text(
                    Pos2::new(rows_rect.right() - 80.0, head_y),
                    Align2::CENTER_CENTER,
                    "状态",
                    FontId::proportional(11.0),
                    c.text_mute,
                );
                painter.line_segment(
                    [
                        Pos2::new(rows_rect.left(), rows_rect.top() + 32.0),
                        Pos2::new(rows_rect.right(), rows_rect.top() + 32.0),
                    ],
                    Stroke::new(1.0, c.border_soft),
                );

                // 行区：严格约束在圆角面板内（左侧/底部不溢出）
                let body = Rect::from_min_max(
                    Pos2::new(rows_rect.left() + 1.0, rows_rect.top() + 34.0),
                    Pos2::new(rows_rect.right() - 1.0, rows_rect.bottom() - 1.0),
                );
                let rows = self.filtered_rows();
                let mut action: Option<String> = None;
                ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("rows")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (exe, name, _title) in &rows {
                            ui.add_space(4.0);
                            let resp = ui.allocate_response(
                                Vec2::new(body.width() - 8.0, ROW_H),
                                Sense::click(),
                            );
                            let rect = resp.rect;
                            let checked = self.checked(exe);
                            let hidden = self.engine.is_hidden(exe);
                            let hot = resp.hovered();
                            let painter = ui.painter();
                            let bgc = if checked {
                                c.checked_bg
                            } else if hot {
                                c.hot_bg
                            } else {
                                c.panel
                            };
                            painter.rect_filled(rect, 4.0, bgc);

                            let cy = rect.center().y;
                            let cb = Rect::from_min_size(
                                Pos2::new(rect.left() + 20.0, cy - 9.0),
                                Vec2::new(18.0, 18.0),
                            );
                            if checked {
                                painter.rect_filled(cb, 5.0, c.indigo);
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
                                painter.rect_filled(cb, 5.0, c.bg);
                                painter.rect_stroke(cb, 5.0, Stroke::new(1.0, c.border), egui::StrokeKind::Inside);
                            }
                            // 应用图标（真实 exe 图标；缺省回落字母块）
                            let icon_rect = Rect::from_min_size(
                                Pos2::new(rect.left() + 52.0, cy - 12.0),
                                Vec2::new(24.0, 24.0),
                            );
                            if let Some(tex) = self.icons.get(exe) {
                                painter.image(
                                    tex.id(),
                                    icon_rect,
                                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                    Color32::WHITE,
                                );
                            } else {
                                painter.rect_filled(icon_rect, 6.0, c.card);
                                let letter: String =
                                    exe.chars().take(1).collect::<String>().to_uppercase();
                                painter.text(
                                    icon_rect.center(),
                                    Align2::CENTER_CENTER,
                                    letter,
                                    FontId::monospace(11.0),
                                    c.text_dim,
                                );
                            }
                            // 应用名称 / 进程名称
                            painter.text(
                                Pos2::new(rect.left() + 88.0, cy),
                                Align2::LEFT_CENTER,
                                ellipsize(name, 22),
                                FontId::proportional(12.5),
                                c.text,
                            );
                            painter.text(
                                Pos2::new(rect.left() + 352.0, cy),
                                Align2::LEFT_CENTER,
                                exe,
                                FontId::monospace(12.5),
                                c.text_dim,
                            );
                            let (label, fg, bg) = if hidden {
                                ("已隐藏", c.amber_fg, c.amber_bg)
                            } else {
                                ("显示中", c.emerald, c.emerald_bg)
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
                });

                if let Some(exe) = action {
                    self.toggle_target(&exe);
                }
                } else {
                    // ================= 设置页（新页面，非弹窗）=================
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        if chip_button(ui, "← 返回", c.card, c.border, c.border, c.text_dim, 84.0) {
                            self.page = 0;
                        }
                    });
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        ui.label(RichText::new("设置").size(16.0).strong().color(c.text));
                    });
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        ui.label(RichText::new("快捷键").size(11.0).color(c.text_mute));
                    });
                    ui.add_space(6.0);
                    let hk_t = self.engine.settings.hotkey_targets;
                    let hk_s = self.engine.settings.hotkey_self;
                    let cap = self.capture;
                    let mut action: Option<u8> = None;
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        if hotkey_row(ui, c, "一键隐藏目标", hk_t, cap == Some(0)) {
                            action = Some(0);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        if hotkey_row(ui, c, "BossKey 显隐", hk_s, cap == Some(1)) {
                            action = Some(1);
                        }
                    });
                    if let Some(which) = action {
                        self.capture = if cap == Some(which) { None } else { Some(which) };
                    }
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        ui.label(RichText::new("行为").size(11.0).color(c.text_mute));
                    });
                    ui.add_space(6.0);
                    let mut s = self.engine.settings;
                    let mut changed = false;
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        if ui
                            .checkbox(&mut s.restore_targets_on_start, "启动时恢复上次勾选的目标")
                            .changed()
                        {
                            changed = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        if ui
                            .checkbox(&mut s.restore_on_exit, "退出时恢复被隐藏的目标")
                            .changed()
                        {
                            changed = true;
                        }
                    });
                    if changed {
                        self.engine.set_settings(s);
                    }
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        ui.label(
                            RichText::new("改动即时生效并保存 · 快捷键点「修改」后按下新组合键")
                                .size(10.0)
                                .color(c.text_mute),
                        );
                    });
                }
            });

        // ================= 退出确认弹窗（自绘卡片，与整体风格一致）=================
        if self.confirm_exit {
            let screen = ui.max_rect();
            // 背景压暗：浅色主题略淡，深色主题更深
            let dim = if self.theme_light {
                Color32::from_rgba_unmultiplied(15, 23, 42, 110)
            } else {
                Color32::from_black_alpha(170)
            };
            egui::Area::new(egui::Id::new("exit_modal"))
                .order(egui::Order::Foreground)
                .fixed_pos(screen.min)
                .interactable(true)
                .show(ui.ctx(), |ui| {
                    // 遮罩同时吞掉下层点击
                    ui.allocate_rect(screen, Sense::click());
                    ui.painter().rect_filled(screen, 0.0, dim);

                    let card = Rect::from_center_size(screen.center(), Vec2::new(404.0, 206.0));
                    let p = ui.painter().clone();
                    // 投影 + 卡片
                    p.rect_filled(
                        card.translate(Vec2::new(0.0, 12.0)),
                        16.0,
                        Color32::from_black_alpha(if self.theme_light { 40 } else { 80 }),
                    );
                    p.rect_filled(card, 16.0, c.panel);
                    p.rect_stroke(
                        card,
                        16.0,
                        Stroke::new(1.0, c.border),
                        egui::StrokeKind::Inside,
                    );

                    // 警示图标
                    let ic = Pos2::new(card.left() + 42.0, card.top() + 44.0);
                    p.circle_filled(ic, 18.0, c.rose_bg);
                    p.circle_stroke(ic, 18.0, Stroke::new(1.0, c.rose_bd));
                    p.text(ic, Align2::CENTER_CENTER, "!", FontId::proportional(19.0), c.rose);

                    // 标题 / 说明（左对齐，与警示图标同轴）
                    let title = p.layout_no_wrap(
                        "退出 BossKey".to_string(),
                        FontId::proportional(15.0),
                        c.text,
                    );
                    p.galley(Pos2::new(card.left() + 74.0, card.top() + 33.0), title, c.text);
                    let msg = p.layout_no_wrap(
                        "退出后将恢复被隐藏的软件窗口与托盘图标。".to_string(),
                        FontId::proportional(12.5),
                        c.text_dim,
                    );
                    p.galley(Pos2::new(card.left() + 74.0, card.top() + 64.0), msg, c.text_dim);

                    // 分隔线
                    let dy = card.bottom() - 72.0;
                    p.line_segment(
                        [
                            Pos2::new(card.left() + 24.0, dy),
                            Pos2::new(card.right() - 24.0, dy),
                        ],
                        Stroke::new(1.0, c.border_soft),
                    );

                    // 按钮：右对齐，取消在左、确认在右
                    let confirm = Rect::from_min_size(
                        Pos2::new(card.right() - 24.0 - 104.0, card.bottom() - 60.0),
                        Vec2::new(104.0, 36.0),
                    );
                    let cancel = Rect::from_min_size(
                        Pos2::new(confirm.left() - 12.0 - 88.0, confirm.top()),
                        Vec2::new(88.0, 36.0),
                    );
                    let cresp = ui.interact(cancel, egui::Id::new("exit_cancel"), Sense::click());
                    let okresp = ui.interact(confirm, egui::Id::new("exit_confirm"), Sense::click());

                    // 取消按钮
                    let (cbg, cbd, cfg) = if cresp.hovered() {
                        (c.hot_bg, c.border, c.text)
                    } else {
                        (c.card, c.border, c.text_dim)
                    };
                    p.rect_filled(cancel, 8.0, cbg);
                    p.rect_stroke(cancel, 8.0, Stroke::new(1.0, cbd), egui::StrokeKind::Inside);
                    p.text(
                        cancel.center(),
                        Align2::CENTER_CENTER,
                        "取消",
                        FontId::proportional(12.0),
                        cfg,
                    );

                    // 确认按钮
                    let (obg, obd, ofg) = if okresp.hovered() {
                        (c.rose, c.rose, Color32::WHITE)
                    } else {
                        (c.rose_bg, c.rose_bd, c.rose)
                    };
                    p.rect_filled(confirm, 8.0, obg);
                    p.rect_stroke(confirm, 8.0, Stroke::new(1.0, obd), egui::StrokeKind::Inside);
                    p.text(
                        confirm.center(),
                        Align2::CENTER_CENTER,
                        "确认退出",
                        FontId::proportional(12.0),
                        ofg,
                    );

                    if cresp.clicked() {
                        self.confirm_exit = false;
                    }
                    if okresp.clicked() {
                        self.engine.do_exit();
                        self.exiting = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            // Esc 取消
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.confirm_exit = false;
            }
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
                            .layout_no_wrap(msg.clone(), FontId::proportional(12.5), c.text)
                            .size();
                        let rect = Rect::from_min_size(
                            ui.cursor().min,
                            Vec2::new(text_size.x + 56.0, 44.0),
                        );
                        ui.allocate_rect(rect, Sense::hover());
                        let painter = ui.painter();
                        painter.rect_filled(rect, 10.0, c.card);
                        painter.rect_stroke(rect, 10.0, Stroke::new(1.0, c.indigo), egui::StrokeKind::Inside);
                        painter.circle_filled(
                            Pos2::new(rect.left() + 20.0, rect.center().y),
                            5.0,
                            c.emerald,
                        );
                        painter.text(
                            Pos2::new(rect.left() + 36.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            msg,
                            FontId::proportional(12.5),
                            c.text,
                        );
                    });
            } else {
                self.toast = None;
            }
        }
    }
}

// ---------------- 小组件 ----------------

/// 窗口按钮：矢量绘制（0=最小化 1=最大化 2=关闭），不依赖字形
/// 按字符数截断并加省略号
fn ellipsize(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

fn win_btn(ui: &mut egui::Ui, c: Palette, icon: u8) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(36.0, 30.0), Sense::click());
    let painter = ui.painter();
    let hot = resp.hovered();
    if hot {
        painter.rect_filled(rect, 6.0, if icon == 2 { c.rose } else { c.card });
    }
    let col = if hot && icon == 2 {
        Color32::WHITE
    } else {
        c.text_dim
    };
    let ctr = rect.center();
    let st = Stroke::new(1.4, col);
    match icon {
        0 => {
            // 最小化：横线
            painter.line_segment(
                [Pos2::new(ctr.x - 5.5, ctr.y), Pos2::new(ctr.x + 5.5, ctr.y)],
                st,
            );
        }
        1 => {
            // 最大化：方框
            painter.rect_stroke(
                Rect::from_center_size(ctr, Vec2::new(11.0, 11.0)),
                1.5,
                st,
                egui::StrokeKind::Inside,
            );
        }
        _ => {
            // 关闭：叉
            painter.line_segment(
                [Pos2::new(ctr.x - 5.0, ctr.y - 5.0), Pos2::new(ctr.x + 5.0, ctr.y + 5.0)],
                st,
            );
            painter.line_segment(
                [Pos2::new(ctr.x - 5.0, ctr.y + 5.0), Pos2::new(ctr.x + 5.0, ctr.y - 5.0)],
                st,
            );
        }
    }
    resp.clicked()
}

/// 主题切换按钮：太阳（亮色激活）/ 月牙（暗色激活）
fn theme_btn(ui: &mut egui::Ui, c: Palette, light: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(36.0, 30.0), Sense::click());
    let painter = ui.painter();
    let hot = resp.hovered();
    if hot {
        painter.rect_filled(rect, 6.0, c.card);
    }
    let col = if hot { c.text } else { c.text_dim };
    let ctr = rect.center();
    let st = Stroke::new(1.3, col);
    if light {
        // 太阳：圆 + 八道光芒
        painter.circle_stroke(ctr, 4.5, st);
        for i in 0..8 {
            let a = std::f32::consts::FRAC_PI_4 * i as f32;
            let (s, cs) = a.sin_cos();
            painter.line_segment(
                [
                    Pos2::new(ctr.x + 6.5 * cs, ctr.y + 6.5 * s),
                    Pos2::new(ctr.x + 9.0 * cs, ctr.y + 9.0 * s),
                ],
                st,
            );
        }
    } else {
        // 月牙：大圆 + 底色小圆遮出缺口
        painter.circle_stroke(ctr, 6.0, st);
        painter.circle_filled(Pos2::new(ctr.x + 3.0, ctr.y - 2.5), 5.0, c.bg_header);
    }
    resp.clicked()
}

fn paint_kbd(painter: &egui::Painter, c: Palette, rect: Rect, text: &str, accent: bool) {
    painter.rect_filled(rect, 4.0, c.card);
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0, c.border), egui::StrokeKind::Inside);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::monospace(11.0),
        if accent { c.kbd_accent } else { c.text_dim },
    );
}

/// 菜单项（标题下方一行）：滑杆图标 + 文字，圆角描边胶囊（与整体按钮同语言）
fn menu_item(ui: &mut egui::Ui, c: Palette, text: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(76.0, 26.0), Sense::click());
    let p = ui.painter();
    let hovered = resp.hovered();
    let fg = if hovered { c.text } else { c.text_dim };
    p.rect_filled(rect, 8.0, if hovered { c.hot_bg } else { c.panel });
    p.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, if hovered { c.border } else { c.border_soft }),
        egui::StrokeKind::Inside,
    );
    // 滑杆图标：三条横线 + 错位旋钮
    let st = Stroke::new(1.2, fg);
    let x0 = rect.left() + 11.0;
    let x1 = x0 + 12.0;
    let ly = rect.center().y;
    for (dy, kx) in [(ly - 5.0, x0 + 7.0), (ly, x0 + 3.0), (ly + 5.0, x0 + 8.5)] {
        p.line_segment([Pos2::new(x0, dy), Pos2::new(x1, dy)], st);
        p.circle_filled(Pos2::new(kx, dy), 1.9, fg);
    }
    p.text(
        Pos2::new(rect.left() + 30.0, ly),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(12.0),
        fg,
    );
    resp.clicked()
}

/// 自绘搜索框：圆角面板底 + 细描边 + 放大镜；聚焦时描边转靛紫
fn search_field(ui: &mut egui::Ui, c: Palette, text: &mut String, width: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 36.0), Sense::hover());
    let box_id = ui.id().with("search_box");
    let hit = ui.interact(rect, box_id.with("hit"), Sense::click());

    ui.painter().rect_filled(rect, 8.0, c.panel);
    ui.painter().rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, c.border_soft),
        egui::StrokeKind::Inside,
    );

    // 输入区：左侧留出放大镜位
    let text_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 32.0, rect.top() + 8.0),
        Vec2::new(width - 32.0 - 12.0, 20.0),
    );
    let edit = ui.put(
        text_rect,
        egui::TextEdit::singleline(text)
            .id(box_id.with("edit"))
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .desired_width(text_rect.width())
            .font(FontId::proportional(12.5))
            .text_color(c.text)
            .horizontal_align(egui::Align::LEFT)
            .vertical_align(egui::Align::Center)
            .hint_text("搜索进程名或窗口标题..."),
    );
    if hit.clicked() && !edit.has_focus() {
        edit.request_focus();
    }

    // 放大镜 + 聚焦描边
    let p = ui.painter();
    let focused = edit.has_focus();
    let col = if focused { c.indigo } else { c.text_mute };
    let ctr = Pos2::new(rect.left() + 18.0, rect.center().y - 1.0);
    p.circle_stroke(ctr, 5.0, Stroke::new(1.2, col));
    p.line_segment(
        [
            Pos2::new(ctr.x + 3.6, ctr.y + 3.6),
            Pos2::new(ctr.x + 7.0, ctr.y + 7.0),
        ],
        Stroke::new(1.2, col),
    );
    if focused {
        p.rect_stroke(rect, 8.0, Stroke::new(1.0, c.indigo), egui::StrokeKind::Inside);
    }
}

fn kbd_chip(ui: &mut egui::Ui, c: Palette, text: &str, accent: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(118.0, 26.0), Sense::hover());
    paint_kbd(ui.painter(), c, rect, text, accent);
}

/// 设置页一行：名称 + 键帽 + 修改/取消按钮（返回按钮是否被点击）
/// 名称列与键帽宽度固定，保证两行按钮左右对齐
fn hotkey_row(ui: &mut egui::Ui, c: Palette, name: &str, hk: core::Hotkey, capturing: bool) -> bool {
    const NAME_W: f32 = 106.0;
    const CHIP_W: f32 = 156.0;
    const ROW_H: f32 = 36.0; // 与「修改」按钮等高，避免视觉错位
    let (lrect, _) = ui.allocate_exact_size(Vec2::new(NAME_W, ROW_H), Sense::hover());
    ui.painter().text(
        Pos2::new(lrect.left(), lrect.center().y),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(11.5),
        c.text_dim,
    );
    let text = if capturing {
        "按下组合键…".to_string()
    } else {
        hk.label()
    };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(CHIP_W, ROW_H), Sense::hover());
    // 键帽与「修改」按钮同尺寸同圆角，整行视觉齐平
    let p = ui.painter();
    p.rect_filled(rect, 8.0, c.card);
    p.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, c.border),
        egui::StrokeKind::Inside,
    );
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        &text,
        FontId::monospace(11.0),
        c.kbd_accent,
    );
    ui.add_space(10.0);
    let label = if capturing { "取消" } else { "修改" };
    chip_button(ui, label, c.card, c.border, c.border, c.text_dim, 72.0)
}

/// 「标签 + 键帽」同轴行：文字与键帽垂直居中对齐（键帽宽度随内容自适应）
fn label_chip(ui: &mut egui::Ui, c: Palette, label: &str, keys: &str, accent: bool) {
    let lw = ui
        .painter()
        .layout_no_wrap(label.to_string(), FontId::proportional(11.0), c.text_mute)
        .size()
        .x;
    let kw = ui
        .painter()
        .layout_no_wrap(
            keys.to_string(),
            FontId::monospace(11.0),
            if accent { c.kbd_accent } else { c.text_dim },
        )
        .size()
        .x
        + 22.0;
    let kw = kw.max(118.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(lw + 8.0 + kw, 26.0), Sense::hover());
    let painter = ui.painter();
    let cy = rect.center().y;
    painter.text(
        Pos2::new(rect.left(), cy),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(11.0),
        c.text_mute,
    );
    let chip = Rect::from_min_size(
        Pos2::new(rect.left() + lw + 8.0, rect.top()),
        Vec2::new(kw, 26.0),
    );
    paint_kbd(painter, c, chip, keys, accent);
}

fn chip_button(
    ui: &mut egui::Ui,
    text: &str,
    bg: Color32,
    hover_bg: Color32,
    border: Color32,
    fg: Color32,
    width: f32,
) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 36.0), Sense::click());
    let painter = ui.painter();
    let bgc = if resp.hovered() { hover_bg } else { bg };
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
