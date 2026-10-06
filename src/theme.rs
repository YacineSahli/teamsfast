//! Theme: fonts (Inter via fastframe-fonts), Lucide icons
//! (fastframe-icons), colour emoji (fastframe-emoji) and the dark palette.

use egui::{Color32, Visuals};

/// The app's colour palette. Every field is a themeable name — users edit
/// JSON files in `~/.config/teamsfast/themes/` and the app repaints live.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct Palette {
    pub window: Color32,
    pub panel: Color32,
    pub surface: Color32,
    pub surface_hover: Color32,
    pub surface_active: Color32,
    pub text: Color32,
    pub secondary: Color32,
    pub dim: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub on_accent: Color32,
    pub danger: Color32,
    pub warning: Color32,
    pub link: Color32,
    pub ok: Color32,
    pub bubble_in: Color32,
    pub bubble_out: Color32,
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            window: Color32::from_rgb(0x1b, 0x1d, 0x22),
            panel: Color32::from_rgb(0x1b, 0x1d, 0x22),
            surface: Color32::from_rgb(0x2b, 0x2d, 0x35),
            surface_hover: Color32::from_rgb(0x36, 0x39, 0x44),
            surface_active: Color32::from_rgb(0x5b, 0x5f, 0xc7),
            text: Color32::from_rgb(0xe8, 0xea, 0xed),
            secondary: Color32::from_rgb(0x9a, 0x9d, 0xa3),
            dim: Color32::from_rgb(0x6b, 0x6e, 0x76),
            accent: Color32::from_rgb(0x5b, 0x5f, 0xc7),
            accent_hover: Color32::from_rgb(0x75, 0x79, 0xd6),
            on_accent: Color32::WHITE,
            danger: Color32::from_rgb(0xe0, 0x7a, 0x7a),
            warning: Color32::from_rgb(0xd1, 0xa5, 0x4a),
            link: Color32::from_rgb(0x69, 0xa1, 0xe8),
            ok: Color32::from_rgb(0x6f, 0xd1, 0x94),
            bubble_in: Color32::from_rgb(0x2b, 0x2d, 0x31),
            bubble_out: Color32::from_rgb(0x3b, 0x3e, 0xcf),
        }
    }

    pub fn light() -> Self {
        Self {
            window: Color32::from_rgb(0xf5, 0xf6, 0xf8),
            panel: Color32::from_rgb(0xee, 0xf0, 0xf3),
            surface: Color32::from_rgb(0xff, 0xff, 0xff),
            surface_hover: Color32::from_rgb(0xe2, 0xe6, 0xec),
            surface_active: Color32::from_rgb(0x5b, 0x5f, 0xc7),
            text: Color32::from_rgb(0x1f, 0x22, 0x28),
            secondary: Color32::from_rgb(0x5a, 0x5f, 0x67),
            dim: Color32::from_rgb(0x8a, 0x8f, 0x97),
            accent: Color32::from_rgb(0x5b, 0x5f, 0xc7),
            accent_hover: Color32::from_rgb(0x4a, 0x4e, 0xb5),
            on_accent: Color32::WHITE,
            danger: Color32::from_rgb(0xc0, 0x3a, 0x3a),
            warning: Color32::from_rgb(0x9a, 0x6a, 0x10),
            link: Color32::from_rgb(0x2a, 0x6c, 0xd8),
            ok: Color32::from_rgb(0x1a, 0x7f, 0x4d),
            bubble_in: Color32::from_rgb(0xe8, 0xea, 0xef),
            bubble_out: Color32::from_rgb(0xd6, 0xd9, 0xf5),
        }
    }

    /// Paint the palette onto egui's visuals.
    pub fn apply_visuals(&self, ctx: &egui::Context) {
        let mut v = Visuals::dark();
        // Light palette: window colour is bright.
        let [r, g, b, _] = self.window.to_srgba_unmultiplied();
        if u16::from(r) + u16::from(g) + u16::from(b) > 384 {
            v = Visuals::light();
        }
        v.panel_fill = self.panel;
        v.window_fill = self.surface;
        v.extreme_bg_color = self.window;
        v.selection.bg_fill = self.accent;
        v.selection.stroke = egui::Stroke::new(1.0, self.on_accent);
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
        ] {
            w.corner_radius = egui::CornerRadius::same(6);
        }
        v.widgets.inactive.bg_fill = self.surface;
        v.widgets.hovered.bg_fill = self.surface_hover;
        v.widgets.active.bg_fill = self.accent;
        v.hyperlink_color = self.link;
        v.override_text_color = Some(self.text);
        ctx.set_visuals(v);
    }
}

impl fastframe_theme::Palette for Palette {
    fn base(base: fastframe_theme::Base) -> Self {
        match base {
            fastframe_theme::Base::Dark => Self::dark(),
            fastframe_theme::Base::Light => Self::light(),
        }
    }

    fn set(&mut self, name: &str, color: Color32) -> bool {
        match name {
            "window" => self.window = color,
            "panel" => self.panel = color,
            "surface" => self.surface = color,
            "surface_hover" => self.surface_hover = color,
            "surface_active" => self.surface_active = color,
            "text" => self.text = color,
            "secondary" => self.secondary = color,
            "dim" => self.dim = color,
            "accent" => self.accent = color,
            "accent_hover" => self.accent_hover = color,
            "on_accent" => self.on_accent = color,
            "danger" => self.danger = color,
            "warning" => self.warning = color,
            "link" => self.link = color,
            "ok" => self.ok = color,
            "bubble_in" => self.bubble_in = color,
            "bubble_out" => self.bubble_out = color,
            _ => return false,
        }
        true
    }

    /// Chat colours derive from the interface colours when a shared
    /// Spotifast/ZapFast palette is imported.
    fn derive(&mut self, given: &std::collections::BTreeSet<&str>) {
        if given.contains("surface") && !given.contains("bubble_in") {
            self.bubble_in = self.surface;
        }
        if given.contains("accent") && !given.contains("bubble_out") {
            self.bubble_out = self.accent;
        }
        if given.contains("accent") && !given.contains("link") {
            self.link = self.accent;
        }
    }
}

/// The app's Omarchy bridge: packaged template + shared presets. Omarchy
/// users get a live "Omarchy" palette that follows the desktop theme; the
/// template is what renders when neither Omarchy nor the user has one.
pub const DESKTOP_THEMES: fastframe_theme::DesktopThemes = fastframe_theme::DesktopThemes {
    slug: "teamsfast",
    omarchy_template: include_str!("../contrib/omarchy/teamsfast.json.tpl"),
    omarchy_previous_templates: &[],
    presets: true,
};

/// The shared palettes, as TeamsFast reads them.
pub fn presets() -> impl Iterator<Item = fastframe_theme::CustomTheme<Palette>> {
    fastframe_theme::presets::themes::<Palette>()
}

/// Themes dir: `~/.config/teamsfast/themes/` — user-editable JSON palettes.
pub fn themes_dir() -> std::path::PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| {
                std::path::PathBuf::from(h).join(".config")
            })
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("teamsfast")
        .join("themes")
}

pub use fastframe_theme::display_name;

pub type Catalog = fastframe_theme::Catalog<Palette>;
pub type CustomTheme = fastframe_theme::CustomTheme<Palette>;

/// Persisted app settings: the selected theme file, and whether the live
/// connection starts automatically.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(default = "default_true")]
    pub auto_live: bool,
}

fn default_true() -> bool {
    true
}

pub fn settings_path() -> std::path::PathBuf {
    themes_dir().parent().unwrap().join("settings.json")
}

pub fn load_settings() -> Settings {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Settings {
            theme: None,
            auto_live: true,
        })
}

pub fn save_settings(settings: &Settings) {
    if let Ok(text) = serde_json::to_string_pretty(settings) {
        if let Some(dir) = settings_path().parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(settings_path(), text);
    }
}

/// Build the theme catalog: the shared presets + the app's Omarchy template
/// + user JSON files in `themes_dir()`, live-watched (edits repaint).
pub fn theme_catalog(waker: egui::Context) -> Catalog {
    let mut catalog = Catalog::default();
    catalog.enable_desktop_themes(DESKTOP_THEMES);
    catalog.start(
        themes_dir(),
        None,
        &fastframe_theme::Waker::new(move || waker.request_repaint()),
    );
    catalog
}

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
