//! Theme: fonts (Inter via fastframe-fonts), Lucide icons
//! (fastframe-icons), colour emoji (fastframe-emoji) and the dark palette.

use egui::{Color32, Visuals};

/// Install fonts, icons, emoji and the base style. Call once at startup.
pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    fastframe_icons::install::<Icon>(ctx);
    ctx.add_plugin(fastframe_emoji::EmojiPlugin::default());
    std::thread::spawn(fastframe_emoji::warm_up);
    apply_style(ctx);
}

/// The chosen interface font: Inter (the fastframe family look), falling
/// back to the platform's own where the desktop provides one.
fn primary_font() -> fastframe_fonts::Primary {
    fastframe_fonts::Primary::Inter
}

fn install_fonts(ctx: &egui::Context) {
    let primary = primary_font();
    let mut fonts = fastframe_fonts::FontSetup::default()
        .primary(primary)
        .definitions();
    fastframe_text::detect().apply_to(&mut fonts);
    ctx.set_fonts(fonts);
}

pub fn apply_style(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.panel_fill = Color32::from_rgb(0x1b, 0x1d, 0x22);
    v.window_fill = Color32::from_rgb(0x20, 0x22, 0x29);
    v.extreme_bg_color = Color32::from_rgb(0x14, 0x16, 0x1a);
    v.selection.bg_fill = Color32::from_rgb(0x5b, 0x5f, 0xc7);
    v.selection.stroke = egui::Stroke::new(1.0, Color32::from_rgb(0x8a, 0x88, 0xff));
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
    ] {
        w.corner_radius = egui::CornerRadius::same(6);
    }
    v.widgets.inactive.bg_fill = Color32::from_rgb(0x2b, 0x2d, 0x35);
    v.widgets.hovered.bg_fill = Color32::from_rgb(0x36, 0x39, 0x44);
    v.widgets.active.bg_fill = Color32::from_rgb(0x5b, 0x5f, 0xc7);
    v.hyperlink_color = Color32::from_rgb(0x69, 0xa1, 0xe8);
    v.override_text_color = Some(Color32::from_rgb(0xe8, 0xea, 0xed));
    ctx.set_visuals(v);
}

fastframe_icons::icons! {
    /// Every icon the interface draws — all from the shared Lucide set.
    pub enum Icon {
        prefix: "teamsfast-icon-",
        directory: "../assets/icons/",
        ArrowLeft => lucide "arrow-left",
        CircleAlert => lucide "circle-alert",
        Copy => lucide "copy",
        ExternalLink => lucide "external-link",
        Pencil => lucide "pencil",
        Plus => lucide "plus",
        Refresh => lucide "refresh-cw",
        Search => lucide "search",
        Trash => lucide "trash-2",
        Users => lucide "users",
        X => lucide "x",
    }
}
