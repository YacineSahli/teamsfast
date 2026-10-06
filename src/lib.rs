mod app;
mod backend;
mod debug;
mod emoji;
mod model;
mod tray;
mod ui;

pub use app::TeamsFastApp;

/// Install the colour-emoji plugin and warm the font lookup. Call once at
/// startup, before the first frame.
pub fn init_emoji(ctx: &egui::Context) {
    ctx.add_plugin(emoji::plugin());
    std::thread::spawn(emoji::warm_up);
}

/// Headless debug entry (`--dump-chats`, `--probe-chat`, `--search`, `--teams`).
pub fn debug_dispatch(args: &[String]) -> Option<eframe::Result<()>> {
    debug::dispatch(args)
}

/// Teams-ish dark theme, applied once at startup.
pub fn apply_style(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.panel_fill = egui::Color32::from_rgb(0x1b, 0x1d, 0x22);
    v.window_fill = egui::Color32::from_rgb(0x20, 0x22, 0x29);
    v.extreme_bg_color = egui::Color32::from_rgb(0x14, 0x16, 0x1a);
    v.selection.bg_fill = egui::Color32::from_rgb(0x5b, 0x5f, 0xc7);
    v.selection.stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(0x8a, 0x88, 0xff));
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
    ] {
        w.corner_radius = egui::CornerRadius::same(6);
    }
    v.widgets.inactive.bg_fill = egui::Color32::from_rgb(0x2b, 0x2d, 0x35);
    v.widgets.hovered.bg_fill = egui::Color32::from_rgb(0x36, 0x39, 0x44);
    v.widgets.active.bg_fill = egui::Color32::from_rgb(0x5b, 0x5f, 0xc7);
    v.hyperlink_color = egui::Color32::from_rgb(0x69, 0xa1, 0xe8);
    v.override_text_color = Some(egui::Color32::from_rgb(0xe8, 0xea, 0xed));
    ctx.set_visuals(v);
}

use egui::Visuals;
