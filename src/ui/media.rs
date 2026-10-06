//! Fullscreen image lightbox.

/// Draw a lightbox over everything when open. Returns the URL to close
/// (`None` when the user dismissed it this frame or it should stay open).
pub fn lightbox(
    ui: &mut egui::Ui,
    tex: &egui::TextureHandle,
    size: [usize; 2],
    url: &str,
) -> bool {
    let screen = ui.max_rect();
    let resp = ui.allocate_rect(screen, egui::Sense::click());
    ui.painter().rect_filled(
        screen,
        0.0,
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 235),
    );
    // Fit to screen with margins.
    let avail = screen.shrink2(egui::vec2(60.0, 60.0));
    let avail_size = egui::vec2(avail.width(), avail.height());
    let scale = (avail_size.x / size[0].max(1) as f32)
        .min(avail_size.y / size[1].max(1) as f32)
        .min(1.0);
    let disp = egui::vec2(size[0] as f32 * scale, size[1] as f32 * scale);
    let center = screen.center();
    let rect = egui::Rect::from_center_size(center, disp);
    ui.painter().image(
        tex.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
    ui.painter().text(
        egui::pos2(screen.center().x, screen.top() + 24.0),
        egui::Align2::CENTER_CENTER,
        format!("{url}  —  click anywhere or press Esc to close"),
        egui::FontId::proportional(12.0),
        egui::Color32::from_rgb(0xaa, 0xad, 0xb2),
    );
    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
    resp.clicked() || esc
}
