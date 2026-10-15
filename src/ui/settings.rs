//! The Settings window: General / Appearance / Notifications / Account /
//! Storage / About. Pure-view module — it mutates the settings struct and
//! reports [`Action`]s; `App::apply` does the state work.

use crate::theme::{self, Settings};
use crate::ui::conversation::Action;
use egui::{RichText, Ui};

#[derive(PartialEq, Clone, Copy)]
pub enum SettingsTab {
    General,
    Appearance,
    Notifications,
    Account,
    Storage,
    About,
}

impl SettingsTab {
    fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Notifications => "Notifications",
            Self::Account => "Account",
            Self::Storage => "Storage & data",
            Self::About => "About",
        }
    }
}

/// Window-level state (open flag + active tab + confirm guards).
#[derive(Default)]
pub struct SettingsUi {
    pub open: bool,
    pub tab: Option<SettingsTab>,
    confirm_clear: bool,
    confirm_signout: bool,
}

/// Read-only context the window displays.
pub struct SettingsInfo<'a> {
    pub self_name: &'a str,
    pub signed_in: bool,
    pub live: bool,
    pub offline: bool,
    /// (filename, display name) for every catalog theme.
    pub themes: &'a [(String, String)],
    pub chats_cached: usize,
    pub messages_cached: usize,
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn settings_window(
    ui: &mut Ui,
    st: &mut SettingsUi,
    settings: &mut Settings,
    info: &SettingsInfo<'_>,
    pal: &crate::theme::Palette,
    actions: &mut Vec<Action>,
) {
    st.tab.get_or_insert(SettingsTab::General);
    let mut open = st.open;
    let _ = &mut open;

    // Custom frame: the default modal popup fill sat within a few percent
    // of the panels behind it and the content floor was cramped (the
    // General tab's last button nearly touched the edge). Same look as the
    // join dialog: surface fill, 12px radius, deep shadow, even 18px gutters.
    let modal = egui::Modal::new(egui::Id::new("settings_modal")).frame(
        egui::Frame::default()
            .fill(pal.surface)
            .stroke(egui::Stroke::new(1.0, pal.outline))
            .shadow(egui::Shadow {
                offset: [0, 14],
                blur: 36,
                spread: 0,
                color: egui::Color32::from_black_alpha(80),
            })
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(egui::Margin::same(18)),
    );
    let resp = modal.show(ui.ctx(), |ui| {
        ui.set_width(470.0);
        // Header row: title + close affordance (the modal itself also
        // closes on Esc / backdrop click).
        ui.horizontal(|ui| {
            ui.label(RichText::new("Settings").strong().size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(crate::theme::Icon::X.image(pal.secondary, 13.0))
                            .fill(egui::Color32::TRANSPARENT)
                            // Transparent fill alone still draws the
                            // inactive bg_stroke — an outlined box. Ghost
                            // needs stroke NONE too.
                            .stroke(egui::Stroke::NONE)
                            .min_size(egui::vec2(22.0, 22.0)),
                    )
                    .clicked()
                {
                    open = false;
                }
            });
        });
        ui.add_space(4.0);
        {
            // Segmented tab control: recessed track, one accent chip for
            // the active tab, even chip widths.
            let tabs = [
                SettingsTab::General,
                SettingsTab::Appearance,
                SettingsTab::Notifications,
                SettingsTab::Account,
                SettingsTab::Storage,
                SettingsTab::About,
            ];
            ui.add_space(2.0);
            let track_h = 30.0;
            let (track, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), track_h),
                egui::Sense::hover(),
            );
            ui.painter().rect_filled(track, 8, pal.window);
            let chip_w = track.width() / tabs.len() as f32;
            let mut tab = st.tab;
            // The accent chip SLIDES between tabs (ease-out ~180ms) —
            // iOS-segmented-control motion; a teleporting chip read as a
            // state glitch.
            let sel_idx = tabs
                .iter()
                .position(|t| Some(*t) == st.tab)
                .unwrap_or(0) as f32;
            let slide =
                ui.ctx()
                    .animate_value_with_time(egui::Id::new("settings_tab_chip"), sel_idx, 0.18);
            let chip_at = |idx: f32| {
                egui::Rect::from_min_size(
                    track.left_top() + egui::vec2(chip_w * idx + 2.0, 2.0),
                    egui::vec2(chip_w - 4.0, track_h - 4.0),
                )
            };
            ui.painter()
                .rect_filled(chip_at(slide).shrink(1.0), 5, pal.accent);
            for (i, t) in tabs.iter().enumerate() {
                let rect = chip_at(i as f32);
                let sel = tab == Some(*t);
                let resp = ui.allocate_rect(rect, egui::Sense::click());
                let p = ui.painter();
                if !sel && resp.hovered() {
                    p.rect_filled(rect.shrink(1.0), 7, pal.surface_hover);
                }
                p.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    t.title(),
                    egui::FontId::proportional(11.5),
                    if sel { pal.on_accent } else { pal.secondary },
                );
                if resp.clicked() {
                    tab = Some(*t);
                    st.confirm_clear = false;
                    st.confirm_signout = false;
                }
            }
            ui.add_space(6.0);
            // Persist the click the loop picked up. Without this write-back
            // the content pane (and the accent chip) never move — every tab
            // but the current one reads as dead.
            st.tab = tab;
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| match tab {
                    Some(SettingsTab::General) => general(ui, settings, info, pal, actions),
                    Some(SettingsTab::Appearance) => appearance(ui, settings, info, pal, actions),
                    Some(SettingsTab::Notifications) => notifications(ui, settings, pal),
                    Some(SettingsTab::Account) => account(ui, st, info, pal, actions),
                    Some(SettingsTab::Storage) => storage(ui, st, info, pal, actions),
                    _ => about(ui, pal),
                });
        }
    });
    if resp.should_close() {
        open = false;
    }
    st.open = open;
}

/// Checkbox that reports whether it changed (caller persists settings).
///
/// Custom-drawn Fluent-style square: the stock `ui.checkbox` inherits the
/// global widget corner radius (8), which on its ~14px box renders as a
/// CIRCLE — reading as a radio (mutually exclusive) for what are
/// independent booleans.
fn toggle(ui: &mut Ui, pal: &crate::theme::Palette, label: &str, value: &mut bool) -> bool {
    let (rect, mut resp) = ui.allocate_exact_size(
        egui::vec2(16.0 + 8.0 + ui.painter().layout(
            label.into(),
            egui::FontId::proportional(13.0),
            pal.text,
            f32::INFINITY,
        ).rect.width() + 2.0, ui.available_height().min(20.0)),
        egui::Sense::click(),
    );
    if resp.clicked() {
        *value = !*value;
        // Only on an actual flip: an unconditional mark_changed() made
        // `toggle` report "changed" every frame (constant settings writes,
        // and the quiet-hours row re-writing its value each repaint).
        resp.mark_changed();
    }
    let box_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 8.0, rect.center().y),
        egui::vec2(16.0, 16.0),
    );
    let p = ui.painter();
    p.rect(
        box_rect,
        egui::CornerRadius::same(4),
        if *value { pal.accent } else { egui::Color32::TRANSPARENT },
        egui::Stroke::new(
            1.0,
            if resp.hovered() { pal.text } else { pal.dim },
        ),
        egui::StrokeKind::Inside,
    );
    if *value {
        // Check mark: two strokes, Fluent weight.
        let c = box_rect.center();
        let s = 3.2;
        p.line_segment(
            [c - egui::vec2(s, 0.0), c - egui::vec2(0.4, s - 0.8)],
            egui::Stroke::new(1.8, pal.on_accent),
        );
        p.line_segment(
            [c - egui::vec2(0.4, s - 0.8), c + egui::vec2(s + 0.4, -(s - 0.8))],
            egui::Stroke::new(1.8, pal.on_accent),
        );
    }
    p.text(
        egui::pos2(rect.left() + 28.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        pal.text,
    );
    resp.changed()
}

/// Section header: small, letter-spaced-ish secondary label with breathing
/// room above — the rhythm that made settings read as one long list.
fn section(ui: &mut Ui, pal: &crate::theme::Palette, title: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(title.to_uppercase()).small().strong().color(pal.secondary));
    ui.add_space(2.0);
}

fn general(
    ui: &mut Ui,
    settings: &mut Settings,
    info: &SettingsInfo<'_>,
    pal: &crate::theme::Palette,
    actions: &mut Vec<Action>,
) {
    section(ui, pal, "Startup");
    if toggle(
        ui, pal,
        "Connect live (push updates) automatically",
        &mut settings.auto_live,
    ) {
        theme::save_settings(settings);
    }
    if toggle(ui, pal, "Start hidden in the system tray", &mut settings.start_in_tray) {
        theme::save_settings(settings);
    }
    if toggle(ui, pal, "Check for updates automatically", &mut settings.update_checks) {
        theme::save_settings(settings);
    }
    ui.add_space(8.0);

    section(ui, pal, "Privacy");
    if toggle(ui, pal, "Ghost mode — hold back read receipts", &mut settings.ghost_mode) {
        theme::save_settings(settings);
    }
    ui.add_space(8.0);

    section(ui, pal, "Window");
    if toggle(
        ui, pal,
        "Close button hides to the tray (quit from the tray menu)",
        &mut settings.close_to_tray,
    ) {
        theme::save_settings(settings);
    }
    ui.add_space(8.0);

    section(ui, pal, "Connection");
    ui.horizontal(|ui| {
        let dot = if info.offline {
            pal.warning
        } else if info.live {
            pal.ok
        } else {
            pal.dim
        };
        let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
        ui.painter().circle_filled(r.center(), 4.0, dot);
        ui.label(if info.offline {
            "Offline — cached content"
        } else if info.live {
            "Connected (live push)"
        } else {
            "Connected"
        });
    });
    ui.label(
        RichText::new("Live push keeps chats, typing and notifications updating in real time.")
            .small()
            .weak(),
    );
    if crate::ui::widgets::btn::secondary(ui, pal, "Refresh chats now").clicked() {
        actions.push(Action::Refresh);
    }
    ui.add_space(8.0);
    section(ui, pal, "Audio");
    if crate::ui::widgets::btn::secondary(ui, pal, "Test call (echo bot)").clicked() {
        actions.push(Action::TestCall);
    }
    ui.label(
        RichText::new("Rings Microsoft's Call Quality Tester; speak after the beep and hear yourself back.")
            .small()
            .weak(),
    );
}

fn appearance(
    ui: &mut Ui,
    settings: &mut Settings,
    info: &SettingsInfo<'_>,
    pal: &crate::theme::Palette,
    actions: &mut Vec<Action>,
) {
    section(ui, pal, "Theme");
    ui.label(
        RichText::new("Built-in palettes, the shared fastframe presets, and any JSON files in ~/.config/teamsfast/themes/.")
            .small()
            .weak(),
    );
    ui.add_space(4.0);

    egui::Grid::new("theme-grid")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            let dark_sel = settings.theme.is_none() && settings.builtin != "light";
            if ui
                .selectable_label(dark_sel, "🔭 TeamsFast Dark (built-in)")
                .clicked()
            {
                settings.theme = None;
                settings.builtin = "dark".into();
                theme::save_settings(settings);
                actions.push(Action::SetTheme(None));
            }
            ui.label(RichText::new("").small());
            ui.end_row();

            let light_sel = settings.theme.is_none() && settings.builtin == "light";
            if ui
                .selectable_label(light_sel, "☀️ TeamsFast Light (built-in)")
                .clicked()
            {
                settings.theme = None;
                settings.builtin = "light".into();
                theme::save_settings(settings);
                actions.push(Action::SetBuiltinLight(true));
            }
            ui.label(RichText::new("").small());
            ui.end_row();

            for (filename, display) in info.themes {
                let sel = settings.theme.as_deref() == Some(filename.as_str());
                if ui.selectable_label(sel, display).clicked() {
                    settings.theme = Some(filename.clone());
                    theme::save_settings(settings);
                    actions.push(Action::SetTheme(Some(filename.clone())));
                }
                // Swatch strip from the palette behind this theme.
                ui.horizontal(|ui| {
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 2.0, pal.accent);
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 2.0, pal.surface);
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 2.0, pal.bubble_out);
                });
                ui.end_row();
            }
        });
    if ui
        .button("Open themes folder")
        .on_hover_text("Create and edit JSON palettes; changes apply live")
        .clicked()
    {
        actions.push(Action::OpenThemeFolder);
    }
    ui.add_space(10.0);

    section(ui, pal, "Text size");
    ui.horizontal(|ui| {
        let mut z = settings.zoom;
        let slider = ui.add(
            egui::Slider::new(&mut z, 0.85..=1.4)
                .text("zoom")
                .fixed_decimals(2),
        );
        ui.label(format!("{:.0}%", z * 100.0));
        if slider.changed() {
            settings.zoom = z;
            theme::save_settings(settings);
            ui.ctx().set_zoom_factor(z);
        }
        if crate::ui::widgets::btn::ghost(ui, pal, "Reset").clicked() {
            settings.zoom = 1.0;
            theme::save_settings(settings);
            ui.ctx().set_zoom_factor(1.0);
        }
    });
    ui.add_space(10.0);
    if toggle(
        ui, pal,
        "Show unread count badges on chats",
        &mut settings.unread_badges,
    ) {
        theme::save_settings(settings);
    }
}

fn notifications(ui: &mut Ui, settings: &mut Settings, pal: &crate::theme::Palette) {
    section(ui, pal, "Desktop notifications");
    if toggle(ui, pal, "Notify for incoming messages", &mut settings.notify) {
        theme::save_settings(settings);
    }
    if toggle(
        ui, pal,
        "Show the message text in the notification",
        &mut settings.notify_preview,
    ) {
        theme::save_settings(settings);
    }
    if toggle(
        ui, pal,
        "Skip notifications while the window is focused",
        &mut settings.skip_focused,
    ) {
        theme::save_settings(settings);
    }
    ui.add_space(6.0);
    ui.label(
        RichText::new("Clicking a notification opens that conversation and raises the window.")
            .small()
            .weak(),
    );
    ui.add_space(10.0);
    section(ui, pal, "Quiet hours");
    let mut q = settings.quiet_hours.clone().unwrap_or_default();
    let mut enabled = settings.quiet_hours.is_some();
    if toggle(ui, pal, "Suppress notifications between", &mut enabled) {
        settings.quiet_hours = enabled.then(|| {
            if q.0.is_empty() { "22:00".into() } else { q.0.clone() }
        }).map(|f| (f, q.1.clone().is_empty().then(|| "07:00".to_string()).unwrap_or(q.1.clone())));
        theme::save_settings(settings);
        q = settings.quiet_hours.clone().unwrap_or_default();
    }
    if enabled {
        ui.horizontal(|ui| {
            let changed = ui
                .add(
                    egui::TextEdit::singleline(&mut q.0)
                        .hint_text("22:00")
                        .desired_width(64.0),
                )
                .changed()
                | ui
                    .add(
                        egui::TextEdit::singleline(&mut q.1)
                            .hint_text("07:00")
                            .desired_width(64.0),
                    )
                    .changed();
            ui.label(RichText::new("and (local time)").small().weak());
            if changed {
                settings.quiet_hours = Some(q);
                theme::save_settings(settings);
            }
        });
    }
}

fn account(
    ui: &mut Ui,
    st: &mut SettingsUi,
    info: &SettingsInfo<'_>,
    pal: &crate::theme::Palette,
    actions: &mut Vec<Action>,
) {
    section(ui, pal, "Signed in");
    if info.signed_in {
        ui.label(if info.self_name.is_empty() {
            "Microsoft account"
        } else {
            info.self_name
        });
        ui.label(
            RichText::new("Tokens live in the OS keyring (libsecret / kwallet).")
                .small()
                .weak(),
        );
        ui.add_space(8.0);
        if !st.confirm_signout {
            if crate::ui::widgets::btn::danger(ui, pal, "Sign out").clicked() {
                st.confirm_signout = true;
            }
        } else {
            ui.horizontal(|ui| {
                ui.label("Remove tokens on this machine?");
                if crate::ui::widgets::btn::danger(ui, pal, "Yes, sign out").clicked() {
                    st.confirm_signout = false;
                    actions.push(Action::SignOut);
                }
                if crate::ui::widgets::btn::secondary(ui, pal, "Cancel").clicked() {
                    st.confirm_signout = false;
                }
            });
        }
    } else {
        ui.label(RichText::new("Not signed in.").weak());
        if crate::ui::widgets::btn::primary(ui, pal, "Sign in…").clicked() {
            actions.push(Action::SignIn);
        }
    }
}

fn storage(
    ui: &mut Ui,
    st: &mut SettingsUi,
    info: &SettingsInfo<'_>,
    pal: &crate::theme::Palette,
    actions: &mut Vec<Action>,
) {
    section(ui, pal, "Local archive");
    ui.label(format!(
        "{} chats and {} messages cached (SQLCipher-encrypted, ~/.local/state/teamsfast/archive.db).",
        info.chats_cached, info.messages_cached
    ));
    ui.label(
        RichText::new("The archive is what makes chats open instantly and offline mode possible.")
            .small()
            .weak(),
    );
    ui.add_space(6.0);
    if !st.confirm_clear {
        if crate::ui::widgets::btn::danger(ui, pal, "Clear local archive…").clicked() {
            st.confirm_clear = true;
        }
    } else {
        ui.horizontal(|ui| {
            ui.label("Delete all cached chats and messages?");
            if crate::ui::widgets::btn::danger(ui, pal, "Yes, clear").clicked() {
                st.confirm_clear = false;
                actions.push(Action::ClearArchive);
            }
            if crate::ui::widgets::btn::secondary(ui, pal, "Cancel").clicked() {
                st.confirm_clear = false;
            }
        });
    }
    ui.add_space(10.0);
    section(ui, pal, "Files");
    if ui.button("Open data folder (logs, archive)").clicked() {
        actions.push(Action::OpenStateFolder);
    }
}

fn about(ui: &mut Ui, pal: &crate::theme::Palette) {
    ui.strong(format!("TeamsFast {VERSION}"));
    ui.label("A native Microsoft Teams client for Linux.");
    ui.label(RichText::new("Rust + egui on the fastframe stack and the teams-core protocol library — no browser engine anywhere.").small().weak());
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.hyperlink_to("Source", "https://github.com/YacineSahli/teamsfast");
        ui.separator();
        ui.hyperlink_to("teams-core", "https://github.com/YacineSahli/teams-core");
    });
    ui.add_space(6.0);
    ui.label(
        RichText::new("MIT licensed. Icons: Lucide (ISC). Emoji: Noto Color Emoji (OFL).")
            .small()
            .weak(),
    );
}
