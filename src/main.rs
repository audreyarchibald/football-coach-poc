// football-coach-poc/src/main.rs
// Entry point for Football Coach PoC

#![allow(dead_code)]

mod detection;
mod export;
mod gui;
mod library;
mod live_capture;
mod metrics;
mod pitch_awareness;
mod pitch_mapping;
mod tactical_insights;
mod tracker;
mod video_processor;

use anyhow::Result;
use log::info;

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    info!("Starting Football Coach PoC v{}", env!("CARGO_PKG_VERSION"));

    // Initialize ffmpeg
    ffmpeg_next::init().expect("Failed to initialize ffmpeg");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([1000.0, 700.0])
            .with_title("Football Coach PoC — Tactical Analysis"),
        ..Default::default()
    };

    eframe::run_native(
        "Football Coach PoC",
        options,
        Box::new(|cc| Ok(Box::new(gui::app::CoachApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {}", e))?;

    Ok(())
}
