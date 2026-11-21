// Witch It+: the launcher of the Witch It+ mods. It finds the game (Steam or Epic), installs,
// updates and removes mods from their sources (choosing versions that work together and asking
// first), keeps presets, and starts and stops the game (with mods, or vanilla for one start).
// Playing never needs it: the store's own start loads the installed mods too.
#![cfg_attr(windows, windows_subsystem = "windows")]
mod app;
mod care;
mod config;
mod game;
mod install;
mod mods;
mod pages;
mod registry;
mod resolve;
mod secret;
mod theme;
mod update;

use eframe::egui;
use std::time::Duration;

impl eframe::App for app::App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick();
        self.draw(ctx);
        let soon = self.busy() || self.indexes.len() < self.config.sources.len();
        ctx.request_repaint_after(if soon { Duration::from_millis(200) } else { Duration::from_secs(2) });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([980.0, 680.0]).with_min_inner_size([760.0, 520.0]).with_title("Witch It+"),
        ..Default::default()
    };
    eframe::run_native("Witch It+", options, Box::new(|cc| {
        theme::apply(&cc.egui_ctx);
        update::sweep();
        Ok(Box::new(app::App::new()))
    }))
}
