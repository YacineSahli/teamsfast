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
    pub outline: Color32,
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
            danger: Color32::from_rgb(0xcf, 0x44, 0x44),
            warning: Color32::from_rgb(0xd1, 0xa5, 0x4a),
            outline: Color32::from_rgb(0x3a, 0x3d, 0x47),
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
            outline: Color32::from_rgb(0xc5, 0xca, 0xd3),
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
        v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, self.outline);
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
            "outline" => self.outline = color,
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

/// Persisted app settings. Every field defaults so older settings files
/// keep loading as new preferences appear.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    /// Selected theme filename in `~/.config/teamsfast/themes/` (or a
    /// shared preset); None = the built-in dark palette.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// Built-in palette when `theme` is None: "light" or "dark".
    #[serde(default = "default_builtin")]
    pub builtin: String,
    /// Start the live (trouter) connection automatically.
    #[serde(default = "default_true")]
    pub auto_live: bool,
    /// Desktop notifications for incoming messages.
    #[serde(default = "default_true")]
    pub notify: bool,
    /// Show the message body in notifications (off = sender only).
    #[serde(default = "default_true")]
    pub notify_preview: bool,
    /// Skip notifications while the window is focused.
    #[serde(default = "default_true")]
    pub skip_focused: bool,
    /// Interface zoom factor (egui zoom; 1.0 = native).
    #[serde(default = "default_zoom")]
    pub zoom: f32,
    /// Launch with the window hidden to the tray.
    #[serde(default)]
    pub start_in_tray: bool,
    /// The window close button hides to the tray instead of quitting.
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    /// Unread count badges on chat rows.
    #[serde(default = "default_true")]
    pub unread_badges: bool,
    /// Locally pinned chat ids (rendered first).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pinned_chats: Vec<String>,
    /// Locally known muted chat ids (server state mirrored for icons).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub muted_chats: Vec<String>,
    /// Locally pinned messages: chat id → message ids (top-of-view pins).
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub pinned_messages: std::collections::HashMap<String, Vec<String>>,
    /// Per-chat notification level: "all" (default) | "mentions" | "off".
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub notification_levels: std::collections::HashMap<String, String>,
    /// Ghost mode: hold back read receipts (no mark-read calls).
    #[serde(default)]
    pub ghost_mode: bool,
    /// Quiet hours window ("HH:MM", "HH:MM"); notifications suppressed
    /// inside it. None = always notify.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiet_hours: Option<(String, String)>,
    /// Check GitHub for a newer release once per launch.
    #[serde(default = "default_true")]
    pub update_checks: bool,
}

fn default_true() -> bool {
    true
}

fn default_builtin() -> String {
    "dark".into()
}

fn default_zoom() -> f32 {
    1.0
}

impl Default for Settings {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap_or(Self {
            theme: None,
            builtin: default_builtin(),
            auto_live: true,
            notify: true,
            notify_preview: true,
            skip_focused: true,
            zoom: 1.0,
            start_in_tray: false,
            close_to_tray: true,
            unread_badges: true,
            pinned_chats: Vec::new(),
            muted_chats: Vec::new(),
            pinned_messages: std::collections::HashMap::new(),
            notification_levels: std::collections::HashMap::new(),
            ghost_mode: false,
            quiet_hours: None,
            update_checks: true,
        })
    }
}

pub fn settings_path() -> std::path::PathBuf {
    themes_dir().parent().unwrap().join("settings.json")
}

/// State-directory root (`~/.local/state/teamsfast`): logs + archive.
pub fn state_dir() -> std::path::PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state"))
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("teamsfast")
}

pub fn load_settings() -> Settings {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
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
    egui_extras::install_image_loaders(ctx);
    fastframe_icons::install::<Icon>(ctx);
    // Register the bundled Noto Color Emoji BEFORE the plugin so both the
    // text pipeline and the raster cache have a colour font.
    crate::emoji::setup();
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
    // Disabled buttons must still read as buttons (was: near-invisible
    // gray-on-gray, e.g. the Teams dialog's disabled "Create").
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.2, Color32::from_rgb(0x8a, 0x8f, 0x99));
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, Color32::from_rgb(0x3a, 0x3d, 0x45));
    v.widgets.noninteractive.fg_stroke =
        egui::Stroke::new(1.0, Color32::from_rgb(0x9a, 0x9f, 0xa8));
    v.hyperlink_color = Color32::from_rgb(0x69, 0xa1, 0xe8);
    v.override_text_color = Some(Color32::from_rgb(0xe8, 0xea, 0xed));
    ctx.set_visuals(v);
}

/// Render `emoji` to RGBA at `height` px through the installed emoji font
/// (bypasses egui's text pipeline, so bar/chip emoji are always full colour).
pub fn raster_emoji(emoji: &str, height: u32) -> Option<(Vec<u8>, [usize; 2])> {
    crate::emoji::setup();
    let emoji_font = fastframe_emoji::get();
    if std::env::var_os("TEAMSFAST_EMOJI_PROBE").is_some() {
        for h in [16u32, 24, 32, 48, 64, 96, 128, 136, 160] {
            let r = emoji_font.render(emoji, h);
            log::warn!("EMOJI PROBE {emoji:?} @{h}px -> {}", r.is_some());
        }
        log::warn!("EMOJI PROBE available={}", fastframe_emoji::available());
    }
    match emoji_font.render(emoji, height) {
        Some(p) => Some((p.rgba, p.size)),
        None => {
            log::warn!("emoji raster failed for {emoji:?} at {height}px");
            None
        }
    }
}

fastframe_icons::icons! {
    /// Every icon the interface draws — Lucide outlines, from the shared
    /// fastframe set or the app's own `assets/icons/` files.
    pub enum Icon {
        prefix: "teamsfast-icon-",
        directory: "../assets/icons/",
        Paperclip => "paperclip",
        Send => "send",
        Reply => "reply",
        Smile => "smile",
        Activity => "activity",
        Archive => "archive",
        AtSign => "at-sign",
        Bell => "bell",
        BellOff => "bell-off",
        Calendar => "calendar",
        Download => "download",
        FileText => "file-text",
        Forward => "forward",
        ListTodo => "list-todo",
        MessageSquare => "message-square",
        Phone => "phone",
        SquareCheck => "square-check",
        Video => "video",
        ArrowLeft => lucide "arrow-left",
        Check => lucide "check",
        CircleAlert => lucide "circle-alert",
        CircleCheck => lucide "circle-check",
        Clock => lucide "clock",
        Copy => lucide "copy",
        Ellipsis => lucide "ellipsis",
        ExternalLink => lucide "external-link",
        Info => lucide "info",
        Lock => lucide "lock",
        LogOut => lucide "log-out",
        Mic => lucide "mic",
        Moon => lucide "moon",
        Pencil => lucide "pencil",
        Pin => lucide "pin",
        PinOff => lucide "pin-off",
        Plus => lucide "plus",
        Refresh => lucide "refresh-cw",
        Search => lucide "search",
        Settings => lucide "settings",
        Sun => lucide "sun",
        Trash => lucide "trash-2",
        User => lucide "user",
        Users => lucide "users",
        Volume => lucide "volume-2",
        VolumeX => lucide "volume-x",
        X => lucide "x",
    }
}
