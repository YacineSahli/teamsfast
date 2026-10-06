//! System tray: icon, show/hide/quit menu.

use fastframe_tray::{Config, Event, MenuItem};

pub const TRAY_SHOW: &str = "show";
pub const TRAY_QUIT: &str = "quit";

/// Procedural app icon: Teams-purple rounded square with a white "T".
pub fn app_icon(size: usize) -> Vec<u8> {
    let s = size.max(8) as f32;
    let radius = (s * 0.22).min(s / 2.0);
    let mut buf = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            if !rounded_rect_contains(px, py, s, radius) {
                continue;
            }
            let idx = (y * size + x) * 4;
            buf[idx] = 0x5b;
            buf[idx + 1] = 0x5f;
            buf[idx + 2] = 0xc7;
            buf[idx + 3] = 255;
            // The "T": bar + stem.
            let bar = px > 0.20 * s && px < 0.80 * s && py > 0.20 * s && py < 0.32 * s;
            let stem = px > 0.44 * s && px < 0.56 * s && py > 0.20 * s && py < 0.80 * s;
            if bar || stem {
                buf[idx] = 255;
                buf[idx + 1] = 255;
                buf[idx + 2] = 255;
            }
        }
    }
    buf
}

fn rounded_rect_contains(px: f32, py: f32, s: f32, r: f32) -> bool {
    if !(0.0..=s).contains(&px) || !(0.0..=s).contains(&py) {
        return false;
    }
    let cx = px.min(s - px);
    let cy = py.min(s - py);
    if cx >= r || cy >= r {
        return true;
    }
    let dx = r - cx;
    let dy = r - cy;
    dx * dx + dy * dy <= r * r
}

pub fn config() -> Config {
    Config {
        id: "teamsfast",
        title: "TeamsFast".into(),
        icon: app_icon,
        template_icon: None,
        themed_icon: false,
        menu_on_click: false,
        menu: vec![
            MenuItem::action(TRAY_SHOW, "Show or hide TeamsFast"),
            MenuItem::Separator,
            MenuItem::action(TRAY_QUIT, "Quit"),
        ],
    }
}

/// What a tray event means for the window.
pub fn action(event: Event, window_hidden: bool) -> Option<TrayAction> {
    Some(match event {
        Event::Show => TrayAction::Show,
        Event::Toggle | Event::Menu(TRAY_SHOW) if window_hidden => TrayAction::Show,
        Event::Toggle | Event::Menu(TRAY_SHOW) => TrayAction::Hide,
        Event::Menu(TRAY_QUIT) => TrayAction::Quit,
        Event::Menu(_) => return None,
    })
}

#[derive(Debug, Clone, Copy)]
pub enum TrayAction {
    Show,
    Hide,
    Quit,
}
