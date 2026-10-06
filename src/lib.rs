mod app;
mod backend;
mod debug;
mod emoji;
mod model;
mod theme;
mod tray;
mod ui;

pub use app::TeamsFastApp;

/// Install fonts, icons, emoji and the theme. Call once at startup.
pub fn init_theme(ctx: &egui::Context) {
    theme::install(ctx);
}

/// Theme only (no emoji) — the TEAMSFAST_NO_EMOJI escape hatch.
pub fn apply_style(ctx: &egui::Context) {
    crate::theme::apply_style(ctx);
}

/// Headless debug entry (`--dump-chats`, `--probe-chat`, `--search`, `--teams`).
pub fn debug_dispatch(args: &[String]) -> Option<eframe::Result<()>> {
    debug::dispatch(args)
}


