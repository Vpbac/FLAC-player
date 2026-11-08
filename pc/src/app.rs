//! User interface.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use eframe::egui::{
    self, Align, Color32, CornerRadius, Key, Layout, Margin, RichText, Sense, Stroke, TextureHandle,
    TextureOptions, Vec2,
};
use egui_extras::{Column, TableBuilder};
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

use crate::engine::{self, Command, DeviceInfo, EngineEvent, EngineHandle, EngineStatus, PlayState};
use crate::eq::{EqBand, EqConfig, FilterType};
use crate::library::{self, ScanMsg, TrackInfo};
use crate::server::{self, ServerHandle};

const ACCENT: Color32 = Color32::from_rgb(0x4c, 0xc2, 0xff);
const GOLD: Color32 = Color32::from_rgb(0xf5, 0xc5, 0x42);
const GREEN: Color32 = Color32::from_rgb(0x3d, 0xd6, 0x8c);
const DIM: Color32 = Color32::from_rgb(0x9a, 0xa0, 0xa6);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    playlist: Vec<TrackInfo>,
    #[serde(default)]
    watched_folders: Vec<PathBuf>,
    current: Option<usize>,
    position: f64,
    volume: Option<f32>,
    shuffle: bool,
    repeat: Repeat,
    exclusive: Option<bool>,
    device_id: Option<String>,
    #[serde(default)]
    eq_config: Option<EqConfig>,
}

fn state_path() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("FlacHiResPlayer").join("state.json")
}

pub struct App {
    engine: EngineHandle,
    status: EngineStatus,

    playlist: Vec<TrackInfo>,
    current: Option<usize>,
    selected: Option<usize>,
    resume_pos: f64,

    shuffle: bool,
    repeat: Repeat,
    shuffle_order: Vec<usize>,

    volume: f32,
    exclusive: bool,
    device_id: Option<String>,
    devices: Vec<DeviceInfo>,

    search: String,
    scan_tx: Sender<ScanMsg>,
    scan_rx: Receiver<ScanMsg>,
    scanning: usize,

    cover_for: Option<PathBuf>,
    cover: Option<TextureHandle>,
    cover_tx: Sender<(PathBuf, Option<egui::ColorImage>)>,
    cover_rx: Receiver<(PathBuf, Option<egui::ColorImage>)>,

    seek_drag: Option<f64>,
    scroll_to_current: bool,
    fail_streak: usize,
    dirty: bool,
    last_save: Instant,

    watched_folders: std::sync::Arc<std::sync::Mutex<Vec<PathBuf>>>,
    known_paths: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<PathBuf>>>,
    server_playlist: std::sync::Arc<std::sync::Mutex<Vec<TrackInfo>>>,
    server_handle: Option<ServerHandle>,
    show_server_modal: bool,

    eq_config: EqConfig,
    show_eq_modal: bool,
    eq_import_text: String,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_style(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        let engine = engine::spawn(move || ctx.request_repaint());

        let saved: Saved = std::fs::read(state_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();

        // Automatically include parent folders of existing playlist tracks so existing folders are watched immediately
        let mut folders = saved.watched_folders;
        for t in &saved.playlist {
            if let Some(parent) = t.path.parent() {
                let p = parent.to_path_buf();
                if p.is_dir() && !folders.iter().any(|existing| p.starts_with(existing)) {
                    folders.push(p);
                }
            }
        }

        let known_set: std::collections::HashSet<PathBuf> = saved.playlist.iter().map(|t| t.path.clone()).collect();
        let watched_folders = std::sync::Arc::new(std::sync::Mutex::new(folders));
        let known_paths = std::sync::Arc::new(std::sync::Mutex::new(known_set));

        let (scan_tx, scan_rx) = crossbeam_channel::unbounded();
        let (cover_tx, cover_rx) = crossbeam_channel::unbounded();
        let server_playlist = std::sync::Arc::new(std::sync::Mutex::new(saved.playlist.clone()));
        let server_handle = server::start_server(server_playlist.clone(), 8080).ok();

        let watcher_ctx = cc.egui_ctx.clone();
        library::spawn_watcher(
            watched_folders.clone(),
            known_paths.clone(),
            server_playlist.clone(),
            scan_tx.clone(),
            move || watcher_ctx.request_repaint(),
        );

        let eq_config = saved.eq_config.unwrap_or_default();
        let eq_import_text = eq_config.to_autoeq_text();

        let mut app = App {
            engine,
            status: EngineStatus::default(),
            current: saved.current.filter(|&i| i < saved.playlist.len()),
            playlist: saved.playlist,
            selected: None,
            resume_pos: saved.position,
            shuffle: saved.shuffle,
            repeat: saved.repeat,
            shuffle_order: Vec::new(),
            volume: saved.volume.unwrap_or(1.0),
            exclusive: false, // Default to Shared Mode to avoid 'audio renderer error' with YouTube/Spotify/Games
            device_id: saved.device_id,
            devices: Vec::new(),
            search: String::new(),
            scan_tx,
            scan_rx,
            scanning: 0,
            cover_for: None,
            cover: None,
            cover_tx,
            cover_rx,
            seek_drag: None,
            scroll_to_current: true,
            fail_streak: 0,
            dirty: false,
            last_save: Instant::now(),
            watched_folders,
            known_paths,
            server_playlist,
            server_handle,
            show_server_modal: false,
            eq_config: eq_config.clone(),
            show_eq_modal: false,
            eq_import_text,
        };
        app.engine.set_volume(app.volume);
        app.engine.send(Command::SetOutput { device_id: app.device_id.clone(), exclusive: app.exclusive });
        app.engine.send(Command::SetEq(eq_config));
        app.engine.send(Command::ListDevices);
        app.rebuild_shuffle();
        app
    }

    // ------------------------------------------------------------------ state

    fn save(&mut self) {
        let folders = self.watched_folders.lock().map(|g| g.clone()).unwrap_or_default();
        let saved = Saved {
            playlist: self.playlist.clone(),
            watched_folders: folders,
            current: self.current,
            position: self.status.position,
            volume: Some(self.volume),
            shuffle: self.shuffle,
            repeat: self.repeat,
            exclusive: Some(self.exclusive),
            device_id: self.device_id.clone(),
            eq_config: Some(self.eq_config.clone()),
        };
        let path = state_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec(&saved) {
            let _ = std::fs::write(path, json);
        }
        self.dirty = false;
        self.last_save = Instant::now();
    }

    fn rebuild_shuffle(&mut self) {
        let mut order: Vec<usize> = (0..self.playlist.len()).filter(|&i| Some(i) != self.current).collect();
        order.shuffle(&mut rand::rng());
        if let Some(c) = self.current {
            order.insert(0, c);
        }
        self.shuffle_order = order;
    }

    fn next_index(&self, from: Option<usize>, manual: bool) -> Option<usize> {
        let n = self.playlist.len();
        if n == 0 {
            return None;
        }
        let Some(from) = from else { return Some(if self.shuffle { self.shuffle_order[0] } else { 0 }) };
        if self.repeat == Repeat::One && !manual {
            return Some(from);
        }
        let wrap = self.repeat != Repeat::Off || manual;
        if self.shuffle {
            let pos = self.shuffle_order.iter().position(|&i| i == from).unwrap_or(0);
            if pos + 1 < self.shuffle_order.len() {
                Some(self.shuffle_order[pos + 1])
            } else if wrap {
                Some(self.shuffle_order[0])
            } else {
                None
            }
        } else if from + 1 < n {
            Some(from + 1)
        } else if wrap {
            Some(0)
        } else {
            None
        }
    }

    fn prev_index(&self, from: Option<usize>) -> Option<usize> {
        let n = self.playlist.len();
        let from = from?;
        if n == 0 {
            return None;
        }
        if self.shuffle {
            let pos = self.shuffle_order.iter().position(|&i| i == from).unwrap_or(0);
            Some(self.shuffle_order[(pos + self.shuffle_order.len() - 1) % self.shuffle_order.len()])
        } else {
            Some((from + n - 1) % n)
        }
    }

    /// Tell the engine what comes after the current track (for gapless playback).
    fn update_next(&self) {
        let next = self.next_index(self.current, false).map(|i| self.playlist[i].path.clone());
        self.engine.send(Command::SetNext(next));
    }

    fn play_index(&mut self, i: usize) {
        if i >= self.playlist.len() {
            return;
        }
        self.current = Some(i);
        self.engine.send(Command::Play { path: self.playlist[i].path.clone(), start_at: 0.0 });
        self.update_next();
        self.scroll_to_current = true;
        self.dirty = true;
    }

    fn toggle_play(&mut self) {
        match self.status.state {
            PlayState::Playing => self.engine.send(Command::Pause),
            PlayState::Paused => self.engine.send(Command::Resume),
            PlayState::Stopped => {
                if let Some(i) = self.current.or(self.selected).or(if self.playlist.is_empty() { None } else { Some(0) }) {
                    let start = if Some(i) == self.current { std::mem::take(&mut self.resume_pos) } else { 0.0 };
                    self.current = Some(i);
                    self.engine.send(Command::Play { path: self.playlist[i].path.clone(), start_at: start });
                    self.update_next();
                }
            }
        }
    }

    fn next(&mut self) {
        if let Some(i) = self.next_index(self.current, true) {
            self.play_index(i);
        }
    }

    fn prev(&mut self) {
        if self.status.position > 3.0 {
            self.engine.send(Command::Seek(0.0));
            return;
        }
        if let Some(i) = self.prev_index(self.current) {
            self.play_index(i);
        }
    }

    fn add_paths(&mut self, paths: Vec<PathBuf>, ctx: &egui::Context) {
        if paths.is_empty() {
            return;
        }
        if let Ok(mut folders) = self.watched_folders.lock() {
            for p in &paths {
                let dir_opt = if p.is_dir() {
                    Some(p.clone())
                } else {
                    p.parent().map(|d| d.to_path_buf())
                };
                if let Some(dir) = dir_opt {
                    if dir.is_dir() && !folders.iter().any(|existing| dir.starts_with(existing)) {
                        folders.retain(|existing| !existing.starts_with(&dir));
                        folders.push(dir);
                    }
                }
            }
        }
        self.dirty = true;
        self.scanning += 1;
        let c = ctx.clone();
        library::scan(paths, self.scan_tx.clone(), move || c.request_repaint());
    }

    fn sync_server_playlist(&self) {
        if let Ok(mut lock) = self.server_playlist.lock() {
            *lock = self.playlist.clone();
        }
    }

    fn remove_index(&mut self, i: usize) {
        if i >= self.playlist.len() {
            return;
        }
        self.playlist.remove(i);
        self.current = match self.current {
            Some(c) if c == i => None,
            Some(c) if c > i => Some(c - 1),
            c => c,
        };
        self.selected = if self.playlist.is_empty() { None } else { Some(i.min(self.playlist.len() - 1)) };
        self.rebuild_shuffle();
        self.update_next();
        self.sync_server_playlist();
        self.dirty = true;
    }

    fn clear_playlist(&mut self) {
        self.engine.send(Command::Stop);
        self.engine.send(Command::SetNext(None));
        self.playlist.clear();
        if let Ok(mut folders) = self.watched_folders.lock() {
            folders.clear();
        }
        if let Ok(mut known) = self.known_paths.lock() {
            known.clear();
        }
        self.current = None;
        self.selected = None;
        self.shuffle_order.clear();
        self.sync_server_playlist();
        self.dirty = true;
    }

    // ------------------------------------------------------------------ events

    fn poll(&mut self, ctx: &egui::Context) {
        self.status = self.engine.status();

        while let Ok(msg) = self.scan_rx.try_recv() {
            match msg {
                ScanMsg::Tracks(mut t) => {
                    if let Ok(mut known) = self.known_paths.lock() {
                        for item in &t {
                            known.insert(item.path.clone());
                        }
                    }
                    let start_empty = self.playlist.is_empty();
                    // Skip duplicates already in the playlist.
                    t.retain(|x| !self.playlist.iter().any(|p| p.path == x.path));
                    if !t.is_empty() {
                        self.playlist.extend(t);
                        self.rebuild_shuffle();
                        if start_empty && self.current.is_none() && !self.playlist.is_empty() {
                            self.selected = Some(0);
                        }
                        self.update_next();
                        self.sync_server_playlist();
                        self.dirty = true;
                    }
                }
                ScanMsg::Removed(paths) => {
                    for p in paths {
                        if let Some(idx) = self.playlist.iter().position(|t| t.path == p) {
                            self.remove_index(idx);
                        }
                    }
                }
                ScanMsg::Done => self.scanning = self.scanning.saturating_sub(1),
            }
        }

        while let Ok(ev) = self.engine.rx.try_recv() {
            match ev {
                EngineEvent::TrackChanged(path) => {
                    self.fail_streak = 0;
                    let expected = self.next_index(self.current, false);
                    self.current = match expected {
                        Some(i) if self.playlist.get(i).map(|t| &t.path) == Some(&path) => Some(i),
                        _ => self.playlist.iter().position(|t| t.path == path),
                    };
                    self.update_next();
                    self.scroll_to_current = true;
                    self.dirty = true;
                }
                EngineEvent::Finished => {
                    self.dirty = true;
                }
                EngineEvent::Devices(d) => self.devices = d,
                EngineEvent::TrackFailed(path, _err) => {
                    self.fail_streak += 1;
                    if self.fail_streak < self.playlist.len().min(20) {
                        // Skip unreadable file.
                        if let Some(i) = self.playlist.iter().position(|t| t.path == path) {
                            self.current = Some(i);
                            if let Some(n) = self.next_index(Some(i), true) {
                                if n != i {
                                    self.play_index(n);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Cover art follows what's actually playing (or the current track).
        let want = self
            .status
            .path
            .clone()
            .or_else(|| self.current.and_then(|i| self.playlist.get(i)).map(|t| t.path.clone()));
        if want != self.cover_for {
            self.cover_for = want.clone();
            self.cover = None;
            if let Some(p) = want {
                let tx = self.cover_tx.clone();
                let c = ctx.clone();
                std::thread::spawn(move || {
                    let img = library::read_cover(&p).and_then(|bytes| {
                        let img = image::load_from_memory(&bytes).ok()?;
                        let img = img.thumbnail(600, 600).to_rgba8();
                        let (w, h) = img.dimensions();
                        Some(egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw()))
                    });
                    let _ = tx.send((p, img));
                    c.request_repaint();
                });
            }
        }
        while let Ok((p, img)) = self.cover_rx.try_recv() {
            if Some(&p) == self.cover_for.as_ref() {
                self.cover = img.map(|i| ctx.load_texture("cover", i, TextureOptions::LINEAR));
            }
        }

        if self.dirty && self.last_save.elapsed() > Duration::from_secs(3) {
            self.save();
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        if typing {
            return;
        }
        let (space, left, right, del, n, p, enter) = ctx.input(|i| {
            (
                i.key_pressed(Key::Space),
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::Delete),
                i.key_pressed(Key::N) && i.modifiers.ctrl,
                i.key_pressed(Key::P) && i.modifiers.ctrl,
                i.key_pressed(Key::Enter),
            )
        });
        if space {
            self.toggle_play();
        }
        if left {
            self.engine.send(Command::Seek((self.status.position - 5.0).max(0.0)));
        }
        if right {
            self.engine.send(Command::Seek(self.status.position + 5.0));
        }
        if del {
            if let Some(i) = self.selected {
                self.remove_index(i);
            }
        }
        if n {
            self.next();
        }
        if p {
            self.prev();
        }
        if enter {
            if let Some(i) = self.selected {
                self.play_index(i);
            }
        }
    }

    // ------------------------------------------------------------------ UI parts

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("◉ Hi-Res Player").size(18.0).strong().color(ACCENT));
            ui.add_space(12.0);
            if ui.button("📁  Thêm thư mục").clicked() {
                if let Some(dirs) = rfd::FileDialog::new().set_title("Chọn thư mục nhạc").pick_folders() {
                    self.add_paths(dirs, &ui.ctx().clone());
                }
            }
            if ui.button("🎵  Thêm file").clicked() {
                if let Some(files) = rfd::FileDialog::new()
                    .add_filter("Audio", library::AUDIO_EXTS)
                    .set_title("Chọn file nhạc")
                    .pick_files()
                {
                    self.add_paths(files, &ui.ctx().clone());
                }
            }
            if ui.button("🗑  Xóa danh sách").clicked() {
                self.clear_playlist();
            }
            ui.add_space(8.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("🔍 Tìm bài hát, nghệ sĩ, album...")
                    .desired_width(260.0),
            );
            if let Some(srv) = &self.server_handle {
                let ip = srv.lan_ips.first().cloned().unwrap_or_else(|| "127.0.0.1".into());
                let btn_text = format!("📱 iPhone Link: {}:{}", ip, srv.port);
                if ui.button(RichText::new(btn_text).color(GREEN).strong()).on_hover_text("Bấm để xem hướng dẫn kết nối từ iPhone").clicked() {
                    self.show_server_modal = true;
                }
            }

            let eq_col = if self.eq_config.enabled { GOLD } else { DIM };
            let eq_label = if self.eq_config.enabled {
                format!("🎚 EQ: ON ({})", self.eq_config.name)
            } else {
                "🎚 Parametric EQ (Peace / AutoEQ)".to_string()
            };
            if ui
                .button(RichText::new(eq_label).color(eq_col).strong())
                .on_hover_text("Mở bộ cân bằng âm thanh Parametric EQ (Hỗ trợ chuẩn Peace EQ / AutoEQ ParametricEQ.txt)")
                .clicked()
            {
                self.show_eq_modal = !self.show_eq_modal;
            }

            if self.scanning > 0 {
                ui.spinner();
                ui.label(RichText::new("Đang quét...").color(DIM));
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (btn_title, btn_color, tip) = if self.exclusive {
                    (
                        "🔒 Exclusive Mode (Đang chiếm DAC)",
                        GOLD,
                        "Đang chiếm quyền độc quyền thiết bị DAC (Bit-Perfect).\n⚠️ Chú ý: Các ứng dụng khác (YouTube, Spotify, Game) sẽ không phát được âm thanh trong lúc app này phát.\nBấm vào để đổi sang Shared Mode (nghe chung không lỗi).",
                    )
                } else {
                    (
                        "🎧 Shared Mode (Nghe chung đa nhiệm)",
                        GREEN,
                        "Chế độ nghe chung (Mặc định): Bạn có thể nghe nhạc FLAC chất lượng cao cùng lúc với YouTube, Spotify, Game, Discord mà không bao giờ bị lỗi âm thanh.\nBấm vào nếu bạn muốn bật Exclusive Mode.",
                    )
                };
                if ui
                    .button(RichText::new(btn_title).color(btn_color).strong())
                    .on_hover_text(tip)
                    .clicked()
                {
                    self.exclusive = !self.exclusive;
                    self.apply_output();
                }

                let current_name = self
                    .device_id
                    .as_ref()
                    .and_then(|id| self.devices.iter().find(|d| &d.id == id))
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| "Thiết bị mặc định".into());
                let mut chosen = self.device_id.clone();
                let resp = egui::ComboBox::from_id_salt("device")
                    .selected_text(current_name)
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut chosen, None, "Thiết bị mặc định");
                        for d in &self.devices {
                            ui.selectable_value(&mut chosen, Some(d.id.clone()), &d.name);
                        }
                    });
                if resp.response.clicked() {
                    self.engine.send(Command::ListDevices);
                }
                if chosen != self.device_id {
                    self.device_id = chosen;
                    self.apply_output();
                }
                ui.label(RichText::new("🔊 Output:").color(DIM));
            });
        });
    }

    fn apply_output(&mut self) {
        self.engine.send(Command::SetOutput { device_id: self.device_id.clone(), exclusive: self.exclusive });
        self.dirty = true;
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        let w = ui.available_width();
        let size = Vec2::splat(w.min(420.0));
        ui.add_space(6.0);
        if let Some(tex) = &self.cover {
            ui.add(egui::Image::new(tex).fit_to_exact_size(size).corner_radius(10));
        } else {
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            ui.painter().rect_filled(rect, CornerRadius::same(10), Color32::from_gray(38));
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "♫",
                egui::FontId::proportional(size.x * 0.35),
                Color32::from_gray(80),
            );
        }
        ui.add_space(10.0);
        let track = self
            .status
            .path
            .as_ref()
            .and_then(|p| self.playlist.iter().find(|t| &t.path == p))
            .or_else(|| self.current.and_then(|i| self.playlist.get(i)))
            .cloned();
        if let Some(t) = track {
            ui.label(RichText::new(&t.title).size(20.0).strong());
            if !t.artist.is_empty() {
                ui.label(RichText::new(&t.artist).size(15.0).color(ACCENT));
            }
            if !t.album.is_empty() {
                ui.label(RichText::new(&t.album).color(DIM));
            }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                badge(ui, &t.format_label(), if t.is_hires() { GOLD } else { DIM });
                if t.is_hires() {
                    badge(ui, "HI-RES", GOLD);
                }
                if t.bitrate > 0 {
                    badge(ui, &format!("{} kbps", t.bitrate), DIM);
                }
            });
        } else {
            ui.label(RichText::new("Chưa phát bài nào").color(DIM));
        }

        ui.add_space(14.0);
        ui.separator();
        ui.label(RichText::new("Tín hiệu").strong());
        egui::Grid::new("sig").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label(RichText::new("Nguồn").color(DIM));
            ui.label(if self.status.source_info.is_empty() { "—" } else { &self.status.source_info });
            ui.end_row();
            ui.label(RichText::new("Đầu ra").color(DIM));
            ui.add(egui::Label::new(&self.status.output_info).wrap());
            ui.end_row();
        });
        ui.add_space(6.0);
        if self.status.state != PlayState::Stopped || self.status.path.is_some() {
            if self.status.bit_perfect {
                badge(ui, "✔ BIT-PERFECT", GREEN);
            } else {
                let why = if !self.exclusive {
                    "Shared mode: Windows có thể resample/trộn"
                } else if self.volume < 0.9999 {
                    "Âm lượng < 100% (đang xử lý số)"
                } else {
                    "Thiết bị không hỗ trợ định dạng gốc"
                };
                ui.label(RichText::new(format!("⚠ Không bit-perfect: {why}")).color(GOLD).small());
            }
        }
        if let Some(err) = &self.status.error {
            ui.add_space(6.0);
            ui.label(RichText::new(format!("❌ {err}")).color(Color32::from_rgb(255, 110, 110)));
        }
    }

    fn player_bar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        // Seek bar
        let dur = self.status.duration.max(0.0);
        let mut pos = self.seek_drag.unwrap_or(self.status.position);
        ui.horizontal(|ui| {
            ui.label(RichText::new(fmt_time(pos)).monospace().color(DIM));
            let time_w = 60.0;
            ui.spacing_mut().slider_width = (ui.available_width() - time_w).max(100.0);
            let resp = ui.add_enabled(
                dur > 0.0,
                egui::Slider::new(&mut pos, 0.0..=dur.max(0.001)).show_value(false),
            );
            if resp.dragged() {
                self.seek_drag = Some(pos);
            }
            if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                self.engine.send(Command::Seek(pos));
                self.seek_drag = None;
            }
            ui.label(RichText::new(fmt_time(dur)).monospace().color(DIM));
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let btn = Vec2::new(44.0, 36.0);
            let big = Vec2::new(56.0, 40.0);

            let shuffle_col = if self.shuffle { ACCENT } else { DIM };
            if ui
                .add_sized(btn, egui::Button::new(RichText::new("🔀").size(18.0).color(shuffle_col)))
                .on_hover_text("Trộn bài (Shuffle)")
                .clicked()
            {
                self.shuffle = !self.shuffle;
                self.rebuild_shuffle();
                self.update_next();
                self.dirty = true;
            }
            if ui.add_sized(btn, egui::Button::new(RichText::new("⏮").size(18.0))).on_hover_text("Bài trước (Ctrl+P)").clicked() {
                self.prev();
            }
            let icon = if self.status.state == PlayState::Playing { "⏸" } else { "▶" };
            if ui
                .add_sized(big, egui::Button::new(RichText::new(icon).size(22.0).color(Color32::BLACK)).fill(ACCENT))
                .on_hover_text("Phát / Tạm dừng (Space)")
                .clicked()
            {
                self.toggle_play();
            }
            if ui.add_sized(btn, egui::Button::new(RichText::new("⏹").size(18.0))).on_hover_text("Dừng").clicked() {
                self.engine.send(Command::Stop);
            }
            if ui.add_sized(btn, egui::Button::new(RichText::new("⏭").size(18.0))).on_hover_text("Bài tiếp (Ctrl+N)").clicked() {
                self.next();
            }
            let (rep_icon, rep_col, rep_tip) = match self.repeat {
                Repeat::Off => ("🔁", DIM, "Lặp: tắt"),
                Repeat::All => ("🔁", ACCENT, "Lặp: toàn bộ danh sách"),
                Repeat::One => ("🔂", ACCENT, "Lặp: một bài"),
            };
            if ui
                .add_sized(btn, egui::Button::new(RichText::new(rep_icon).size(18.0).color(rep_col)))
                .on_hover_text(rep_tip)
                .clicked()
            {
                self.repeat = match self.repeat {
                    Repeat::Off => Repeat::All,
                    Repeat::All => Repeat::One,
                    Repeat::One => Repeat::Off,
                };
                self.update_next();
                self.dirty = true;
            }

            ui.add_space(16.0);
            // Now playing text
            if let Some(t) = self.current.and_then(|i| self.playlist.get(i)) {
                ui.vertical(|ui| {
                    ui.label(RichText::new(&t.title).strong().size(15.0));
                    ui.label(RichText::new(if t.artist.is_empty() { "—" } else { &t.artist }).color(DIM));
                });
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("{:>3}%", (self.volume * 100.0).round() as i32)).monospace());
                ui.spacing_mut().slider_width = 140.0;
                let r = ui.add(egui::Slider::new(&mut self.volume, 0.0..=1.0).show_value(false));
                if r.changed() {
                    self.engine.set_volume(self.volume);
                    self.dirty = true;
                }
                r.on_hover_text("100% = không xử lý số (bit-perfect)");
                let vicon = if self.volume == 0.0 { "🔇" } else if self.volume < 0.5 { "🔉" } else { "🔊" };
                ui.label(RichText::new(vicon).size(18.0));
            });
        });
        ui.add_space(4.0);
    }

    fn playlist_view(&mut self, ui: &mut egui::Ui) {
        let rows: Vec<usize> = (0..self.playlist.len()).filter(|&i| self.playlist[i].matches(&self.search)).collect();

        if self.playlist.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("Kéo thả thư mục / file nhạc vào đây\nhoặc bấm \"📁 Thêm thư mục\"")
                        .size(18.0)
                        .color(DIM),
                );
            });
            return;
        }

        let total: f64 = self.playlist.iter().map(|t| t.duration).sum();
        let watched_list: Vec<String> = self
            .watched_folders
            .lock()
            .map(|g| g.iter().map(|p| p.display().to_string()).collect())
            .unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} bài • {}{}",
                    self.playlist.len(),
                    fmt_long(total),
                    if self.search.is_empty() { String::new() } else { format!(" • {} kết quả", rows.len()) }
                ))
                .color(DIM),
            );
            if !watched_list.is_empty() {
                ui.label(
                    RichText::new(format!("• 🔄 Tự động nhận nhạc mới ({} thư mục)", watched_list.len()))
                        .small()
                        .color(GREEN),
                )
                .on_hover_text(format!(
                    "Đang tự động theo dõi thư mục (cập nhật ngay khi bạn chép thêm nhạc vào):\n{}",
                    watched_list.join("\n")
                ));
            }
        });

        let playing_path = self.status.path.clone();
        let is_playing = self.status.state != PlayState::Stopped;
        let mut to_play: Option<usize> = None;
        let mut to_remove: Option<usize> = None;
        let mut new_sel = self.selected;

        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(Sense::click())
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::exact(36.0))
            .column(Column::remainder().at_least(160.0).clip(true))
            .column(Column::initial(180.0).at_least(60.0).clip(true))
            .column(Column::initial(200.0).at_least(60.0).clip(true))
            .column(Column::initial(110.0).at_least(60.0))
            .column(Column::exact(56.0));
        if self.scroll_to_current {
            if let Some(r) = self.current.and_then(|c| rows.iter().position(|&i| i == c)) {
                table = table.scroll_to_row(r, Some(Align::Center));
            }
            self.scroll_to_current = false;
        }

        table
            .header(24.0, |mut h| {
                for title in ["#", "Tiêu đề", "Nghệ sĩ", "Album", "Định dạng", "Thời lượng"] {
                    h.col(|ui| {
                        ui.label(RichText::new(title).strong().color(DIM));
                    });
                }
            })
            .body(|body| {
                body.rows(28.0, rows.len(), |mut row| {
                    let idx = rows[row.index()];
                    let t = &self.playlist[idx];
                    let now = is_playing && playing_path.as_ref() == Some(&t.path)
                        || (!is_playing && self.current == Some(idx));
                    row.set_selected(new_sel == Some(idx));
                    let color = if now { ACCENT } else { Color32::from_gray(225) };
                    row.col(|ui| {
                        if now && is_playing {
                            ui.label(RichText::new(if self.status.state == PlayState::Playing { "▶" } else { "⏸" }).color(ACCENT));
                        } else {
                            ui.label(RichText::new(format!("{}", idx + 1)).color(DIM));
                        }
                    });
                    row.col(|ui| {
                        let mut txt = RichText::new(&t.title).color(color);
                        if now {
                            txt = txt.strong();
                        }
                        ui.add(egui::Label::new(txt).truncate().selectable(false));
                    });
                    row.col(|ui| {
                        ui.add(egui::Label::new(RichText::new(&t.artist).color(DIM)).truncate().selectable(false));
                    });
                    row.col(|ui| {
                        ui.add(egui::Label::new(RichText::new(&t.album).color(DIM)).truncate().selectable(false));
                    });
                    row.col(|ui| {
                        let c = if t.is_hires() { GOLD } else { DIM };
                        ui.label(RichText::new(t.format_label()).small().color(c));
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(fmt_time(t.duration)).monospace().color(DIM));
                    });
                    let resp = row.response();
                    if resp.clicked() {
                        new_sel = Some(idx);
                    }
                    if resp.double_clicked() {
                        to_play = Some(idx);
                    }
                    resp.context_menu(|ui| {
                        if ui.button("▶  Phát").clicked() {
                            to_play = Some(idx);
                            ui.close();
                        }
                        if ui.button("📂  Mở thư mục chứa file").clicked() {
                            let _ = std::process::Command::new("explorer")
                                .arg("/select,")
                                .arg(&t.path)
                                .spawn();
                            ui.close();
                        }
                        if ui.button("✖  Xóa khỏi danh sách").clicked() {
                            to_remove = Some(idx);
                            ui.close();
                        }
                    });
                });
            });

        self.selected = new_sel;
        if let Some(i) = to_play {
            self.play_index(i);
        }
        if let Some(i) = to_remove {
            self.remove_index(i);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        self.handle_keys(&ctx);

        // Drag & drop
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            self.add_paths(dropped, &ctx);
        }

        let bar_fill = Color32::from_rgb(0x16, 0x18, 0x1c);
        egui::Panel::top("top")
            .frame(egui::Frame::new().fill(bar_fill).inner_margin(Margin::symmetric(12, 8)))
            .show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("player")
            .frame(
                egui::Frame::new()
                    .fill(bar_fill)
                    .inner_margin(Margin::symmetric(14, 6))
                    .stroke(Stroke::new(1.0, Color32::from_gray(40))),
            )
            .show(ui, |ui| self.player_bar(ui));
        egui::Panel::left("side")
            .resizable(true)
            .default_size(300.0)
            .size_range(220.0..=480.0)
            .frame(egui::Frame::new().fill(Color32::from_rgb(0x1b, 0x1d, 0x22)).inner_margin(Margin::same(12)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.side_panel(ui));
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::from_rgb(0x12, 0x13, 0x16)).inner_margin(Margin::same(10)))
            .show(ui, |ui| self.playlist_view(ui));

        if self.show_server_modal {
            let mut open = true;
            egui::Window::new("📱 Kết Nối iPhone / iPad (Local Streaming)")
                .open(&mut open)
                .resizable(false)
                .collapsible(false)
                .default_width(450.0)
                .show(&ctx, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(8.0, 10.0);
                    ui.label(RichText::new("Máy chủ Streaming đang chạy ngầm trên PC!").strong().size(15.0).color(GREEN));
                    ui.separator();
                    ui.label("Để nghe nhạc từ iPhone cùng mạng Wi-Fi, hãy nhập địa chỉ sau vào App trên iPhone:");
                    
                    if let Some(srv) = &self.server_handle {
                        for ip in &srv.lan_ips {
                            let url = format!("http://{}:{}", ip, srv.port);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&url).strong().size(16.0).color(ACCENT).monospace());
                                if ui.button("📋 Copy").clicked() {
                                    ui.ctx().copy_text(url);
                                }
                            });
                        }
                    }

                    ui.add_space(8.0);
                    ui.label(RichText::new("💡 Gợi ý kết nối:").strong());
                    ui.label("• Đảm bảo iPhone và PC kết nối cùng 1 mạng Wi-Fi nhà bạn.");
                    ui.label("• Không dùng từ khóa 'localhost' trên iPhone vì nó sẽ trỏ vào chính iPhone.");
                    ui.label("• App iOS sẽ tự động tải danh sách bài hát và stream FLAC chất lượng cao trực tiếp từ PC.");
                    
                    ui.add_space(8.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Đóng").clicked() {
                            self.show_server_modal = false;
                        }
                    });
                });
            if !open {
                self.show_server_modal = false;
            }
        }

        self.eq_modal(&ctx);

        if self.status.state == PlayState::Playing {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn on_exit(&mut self) {
        self.save();
        self.engine.send(Command::Quit);
    }
}

impl App {
    fn apply_eq(&mut self) {
        self.eq_import_text = self.eq_config.to_autoeq_text();
        self.engine.send(Command::SetEq(self.eq_config.clone()));
        self.dirty = true;
    }

    fn eq_modal(&mut self, ctx: &egui::Context) {
        if !self.show_eq_modal {
            return;
        }
        let mut open = true;
        let mut changed = false;

        egui::Window::new("🎚 Parametric EQ — Chuẩn Peace EQ / AutoEQ (64-bit DSP)")
            .open(&mut open)
            .resizable(true)
            .default_width(780.0)
            .default_height(560.0)
            .show(ctx, |ui| {
                // Top row: Enable toggle, Preset selector, Import/Export file
                ui.horizontal(|ui| {
                    let en_text = if self.eq_config.enabled { "🟢 ĐANG BẬT EQ" } else { "⚪ ĐANG TẮT (BYPASS)" };
                    let en_col = if self.eq_config.enabled { GREEN } else { DIM };
                    if ui.button(RichText::new(en_text).color(en_col).strong()).clicked() {
                        self.eq_config.enabled = !self.eq_config.enabled;
                        changed = true;
                    }

                    ui.separator();
                    ui.label(RichText::new("Preset AutoEQ:").strong());
                    let presets = EqConfig::builtin_presets();
                    egui::ComboBox::from_id_salt("autoeq_preset")
                        .selected_text(&self.eq_config.name)
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            for p in presets {
                                if ui.selectable_label(self.eq_config.name == p.name, &p.name).clicked() {
                                    let was_enabled = self.eq_config.enabled;
                                    self.eq_config = p;
                                    self.eq_config.enabled = was_enabled || self.eq_config.name != "Flat (Mặc định)";
                                    changed = true;
                                }
                            }
                        });

                    if ui.button("📂 Nạp file ParametricEQ.txt").on_hover_text("Mở file ParametricEQ.txt tải từ AutoEQ.app hoặc Peace EQ").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("AutoEQ / Peace TXT", &["txt", "peq"])
                            .set_title("Chọn file ParametricEQ.txt (AutoEQ / Peace)")
                            .pick_file()
                        {
                            if let Ok(content) = std::fs::read_to_string(&path) {
                                let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Custom AutoEQ");
                                self.eq_config = EqConfig::from_autoeq_text(name, &content);
                                self.eq_config.enabled = true;
                                changed = true;
                            }
                        }
                    }

                    if ui.button("➕ Thêm Band").clicked() {
                        self.eq_config.bands.push(EqBand::default());
                        changed = true;
                    }

                    if ui.button("🔄 Reset Flat").clicked() {
                        self.eq_config = EqConfig::default();
                        changed = true;
                    }
                });

                ui.add_space(6.0);

                // Preamp row
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Preamp (Tiền khuếch đại):").strong());
                    ui.spacing_mut().slider_width = 240.0;
                    if ui
                        .add(egui::Slider::new(&mut self.eq_config.preamp_db, -24.0..=12.0).suffix(" dB").step_by(0.1))
                        .changed()
                    {
                        changed = true;
                    }
                    if ui
                        .button("🛡 Tự động chống rè (Auto Preamp)")
                        .on_hover_text("Tự động hạ Preamp tương ứng với mức tăng cao nhất trên đồ thị để chống vỡ tiếng (Clipping)")
                        .clicked()
                    {
                        let mut max_gain = 0.0_f64;
                        let mut cfg_no_pre = self.eq_config.clone();
                        cfg_no_pre.preamp_db = 0.0;
                        for step in 0..120 {
                            let f = 20.0 * (1000.0_f64).powf(step as f64 / 119.0);
                            max_gain = max_gain.max(cfg_no_pre.response_db(f, 48000.0));
                        }
                        self.eq_config.preamp_db = (-max_gain - 0.2).min(0.0).clamp(-24.0, 12.0);
                        changed = true;
                    }
                });

                ui.add_space(6.0);

                // Live Frequency Response Curve Graph (20 Hz - 20 kHz)
                let graph_h = 135.0;
                let graph_w = ui.available_width().max(300.0);
                let (rect, _) = ui.allocate_exact_size(Vec2::new(graph_w, graph_h), Sense::hover());
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, CornerRadius::same(6), Color32::from_rgb(16, 18, 24));
                painter.rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, Color32::from_gray(45)), egui::StrokeKind::Inside);

                // 0 dB center line and +/- 6, 12 dB lines
                let db_range = 18.0_f64;
                for db_line in [-12.0, -6.0, 0.0, 6.0, 12.0] {
                    let y = rect.center().y - (db_line as f32 / db_range as f32) * (rect.height() * 0.45);
                    let stroke_col = if db_line == 0.0 { Color32::from_gray(80) } else { Color32::from_gray(35) };
                    painter.line_segment([egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)], Stroke::new(1.0, stroke_col));
                    painter.text(
                        egui::pos2(rect.left() + 6.0, y - 2.0),
                        egui::Align2::LEFT_BOTTOM,
                        format!("{:+.0}dB", db_line),
                        egui::FontId::monospace(10.0),
                        Color32::from_gray(110),
                    );
                }

                // Frequency curve points
                let num_pts = 160usize;
                let mut pts = Vec::with_capacity(num_pts);
                for i in 0..num_pts {
                    let t = i as f64 / (num_pts - 1) as f64;
                    let freq = 20.0 * (1000.0_f64).powf(t); // 20 Hz to 20,000 Hz log scale
                    let db = self.eq_config.response_db(freq, 48000.0).clamp(-db_range, db_range);
                    let x = rect.left() + (t as f32) * rect.width();
                    let y = rect.center().y - (db as f32 / db_range as f32) * (rect.height() * 0.45);
                    pts.push(egui::pos2(x, y));
                }
                let curve_col = if self.eq_config.enabled { ACCENT } else { Color32::from_gray(110) };
                painter.add(egui::Shape::line(pts, Stroke::new(2.2, curve_col)));

                ui.add_space(6.0);

                // Interactive Parametric Bands Table
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    let mut remove_idx: Option<usize> = None;
                    egui::Grid::new("peq_bands_grid")
                        .striped(true)
                        .num_columns(7)
                        .spacing([10.0, 6.0])
                        .show(ui, |ui| {
                            for h in ["Bật", "Loại Filter", "Tần số Fc (Hz)", "Gain (dB)", "Hệ số Q", "Thanh trượt Gain", "Xóa"] {
                                ui.label(RichText::new(h).strong().color(DIM));
                            }
                            ui.end_row();

                            for (idx, band) in self.eq_config.bands.iter_mut().enumerate() {
                                if ui.checkbox(&mut band.enabled, format!("#{}", idx + 1)).changed() {
                                    changed = true;
                                }
                                egui::ComboBox::from_id_salt(format!("ft_{idx}"))
                                    .selected_text(band.filter_type.code())
                                    .width(75.0)
                                    .show_ui(ui, |ui| {
                                        for &ft in FilterType::ALL {
                                            if ui.selectable_value(&mut band.filter_type, ft, ft.label()).changed() {
                                                changed = true;
                                            }
                                        }
                                    });
                                if ui
                                    .add(egui::DragValue::new(&mut band.freq).range(20.0..=20000.0).speed(5.0).suffix(" Hz"))
                                    .changed()
                                {
                                    changed = true;
                                }
                                if ui
                                    .add(egui::DragValue::new(&mut band.gain_db).range(-24.0..=24.0).speed(0.1).suffix(" dB"))
                                    .changed()
                                {
                                    changed = true;
                                }
                                if ui
                                    .add(egui::DragValue::new(&mut band.q).range(0.1..=18.0).speed(0.05))
                                    .changed()
                                {
                                    changed = true;
                                }
                                ui.spacing_mut().slider_width = 150.0;
                                if ui.add(egui::Slider::new(&mut band.gain_db, -15.0..=15.0).show_value(false)).changed() {
                                    changed = true;
                                }
                                if ui.button("✖").clicked() {
                                    remove_idx = Some(idx);
                                }
                                ui.end_row();
                            }
                        });
                    if let Some(r) = remove_idx {
                        self.eq_config.bands.remove(r);
                        changed = true;
                    }
                });

                ui.add_space(6.0);
                ui.collapsing("📋 Dán / Xuất cấu hình chuẩn Peace EQ & AutoEQ (ParametricEQ.txt)", |ui| {
                    ui.label(RichText::new("Dán nội dung ParametricEQ.txt từ AutoEQ.app vào ô dưới rồi bấm 'Áp dụng Text':").small().color(DIM));
                    ui.add(
                        egui::TextEdit::multiline(&mut self.eq_import_text)
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(5)
                            .desired_width(f32::INFINITY),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("✔ Áp dụng Text AutoEQ").clicked() {
                            let parsed = EqConfig::from_autoeq_text("Custom AutoEQ", &self.eq_import_text);
                            self.eq_config = parsed;
                            self.eq_config.enabled = true;
                            changed = true;
                        }
                        if ui.button("📋 Copy cấu hình hiện tại").clicked() {
                            ui.ctx().copy_text(self.eq_config.to_autoeq_text());
                        }
                    });
                });
            });

        if !open {
            self.show_eq_modal = false;
        }
        if changed {
            self.apply_eq();
        }
    }
}

fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .stroke(Stroke::new(1.0, color))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().strong().color(color));
        });
}

fn fmt_time(s: f64) -> String {
    let s = s.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn fmt_long(s: f64) -> String {
    let s = s.max(0.0) as u64;
    if s >= 3600 {
        format!("{} giờ {} phút", s / 3600, (s / 60) % 60)
    } else {
        format!("{} phút", s / 60)
    }
}

fn setup_style(ctx: &egui::Context) {
    // Fonts: Segoe UI for full Vietnamese support, CJK fallbacks for Asian titles.
    let mut fonts = egui::FontDefinitions::default();
    let win = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let mut primary = Vec::new();
    for (name, file) in [("segoe", "segoeui.ttf")] {
        if let Ok(b) = std::fs::read(format!("{win}\\Fonts\\{file}")) {
            fonts.font_data.insert(name.into(), std::sync::Arc::new(egui::FontData::from_owned(b)));
            primary.push(name.to_string());
        }
    }
    let mut fallbacks = Vec::new();
    for (name, file) in [("yahei", "msyh.ttc"), ("yugoth", "YuGothM.ttc"), ("malgun", "malgun.ttf"), ("segsym", "seguisym.ttf")] {
        if let Ok(b) = std::fs::read(format!("{win}\\Fonts\\{file}")) {
            fonts.font_data.insert(name.into(), std::sync::Arc::new(egui::FontData::from_owned(b)));
            fallbacks.push(name.to_string());
        }
    }
    if let Some(fam) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        for (i, n) in primary.iter().enumerate() {
            fam.insert(i, n.clone());
        }
        fam.extend(fallbacks.iter().cloned());
    }
    if let Some(fam) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        fam.extend(primary.iter().cloned());
        fam.extend(fallbacks.iter().cloned());
    }
    ctx.set_fonts(fonts);

    let mut visuals = egui::Visuals::dark();
    visuals.selection.bg_fill = Color32::from_rgb(0x1f, 0x4e, 0x6e);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.hyperlink_color = ACCENT;
    visuals.slider_trailing_fill = true;
    visuals.widgets.inactive.corner_radius = CornerRadius::same(6);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(6);
    visuals.widgets.active.corner_radius = CornerRadius::same(6);
    ctx.set_visuals(visuals);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
        s.spacing.button_padding = Vec2::new(10.0, 5.0);
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(14.5));
        s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(14.5));
    });
}
