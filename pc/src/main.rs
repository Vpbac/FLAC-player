#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod engine;
mod eq;
mod library;
mod server;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Hi-Res FLAC Player")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([860.0, 520.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Hi-Res FLAC Player",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
