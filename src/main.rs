#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod brand;
mod engine;
mod rules;
mod settings;
mod ui;

use eframe::egui;

fn main() -> eframe::Result {
    if std::env::args().any(|arg| arg == "--self-test") {
        match test_inference() {
            Ok(()) => return Ok(()),
            Err(error) => {
                eprintln!("BossSensor self-test: {error}");
                std::process::exit(1);
            }
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_icon(brand::window_icon())
            .with_inner_size([548.0, 624.0])
            .with_min_inner_size([416.0, 440.0]),
        ..Default::default()
    };
    eframe::run_native(
        "BossSensor",
        options,
        Box::new(|context| Ok(Box::new(app::BossSensor::new(&context.egui_ctx)))),
    )
}

fn test_inference() -> Result<(), String> {
    use boss_sensor_vision::{EMBEDDING_LEN, Vision};

    let _ = brand::window_icon();
    let portrait = include_bytes!("../crates/vision/tests/data/portrait.rgb");
    let observation = Vision::new()?.analyze(portrait, 160, 160, true)?;
    match observation.embeddings.as_slice() {
        [embedding] if observation.face_count == 1 && embedding.len() == EMBEDDING_LEN => Ok(()),
        _ => Err("unexpected face detection or recognition result".into()),
    }
}
