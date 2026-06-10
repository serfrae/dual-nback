#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod config;
mod game;
mod stats;
mod theme;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Dual N-Back")
            .with_inner_size([920.0, 760.0])
            .with_min_inner_size([700.0, 620.0]),
        ..Default::default()
    };
    eframe::run_native("dual-nback", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
