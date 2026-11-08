#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod engine;
mod eq;
mod library;
mod server;

use std::sync::Arc;
use eframe::egui;

fn load_window_icon() -> Option<Arc<egui::IconData>> {
    let bytes = include_bytes!("../assets/icon.png");
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    Some(Arc::new(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    }))
}

fn main() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Hi-Res FLAC Player")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([860.0, 520.0])
        .with_drag_and_drop(true);

    if let Some(icon) = load_window_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Hi-Res FLAC Player",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
