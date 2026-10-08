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

    let modal = egui::Modal::new(egui::Id::new("settings_modal"));
    let resp = modal.show(ui.ctx(), |ui| {
        ui.set_width(520.0);
        // Header row: title + close affordance (the modal itself also
        // closes on Esc / backdrop click).
        ui.horizontal(|ui| {
            ui.label(RichText::new("Settings").strong().size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(crate::theme::Icon::X.image(pal.secondary, 13.0))
                            .fill(egui::Color32::TRANSPARENT),
                    )
                    .clicked()
                {
                    open = false;
                }
            });
        });
        ui.add_space(4.0);
        {
            // Tab strip.
            ui.horizontal(|ui| {
                let mut tab = st.tab;
                for t in [
                    SettingsTab::General,
                    SettingsTab::Appearance,
                    SettingsTab::Notifications,
                    SettingsTab::Account,
                    SettingsTab::Storage,
                    SettingsTab::About,
                ] {
                    let sel = tab == Some(t);
                    let resp = ui.selectable_label(
                        sel,
                        // Selected = accent pill; accent-on-accent text is
                        // invisible, so the selected tab uses on_accent.
                        RichText::new(t.title()).strong().small().color(if sel {
                            pal.on_accent
                        } else {
                            pal.text
                        }),
                    );
                    if sel {
                        // Underline the active tab — colour alone is subtle.
                        let y = resp.rect.bottom() + 1.5;
                        ui.painter().line_segment(
                            [resp.rect.left_top() + egui::vec2(2.0, y - resp.rect.top()),
                             resp.rect.right_top() + egui::vec2(-2.0, y - resp.rect.top())],
                            egui::Stroke::new(2.0, pal.accent),
                        );
                    }
                    if resp.clicked() {
                        tab = Some(t);
                        st.confirm_clear = false;
                        st.confirm_signout = false;
                    }
                }
                st.tab = tab;
            });
            ui.separator();

            let tab = st.tab;
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| match tab {
                    Some(SettingsTab::General) => general(ui, settings, info, actions),
                    Some(SettingsTab::Appearance) => appearance(ui, settings, info, pal, actions),
                    Some(SettingsTab::Notifications) => notifications(ui, settings),
                    Some(SettingsTab::Account) => account(ui, st, info, actions),
                    Some(SettingsTab::Storage) => storage(ui, st, info, actions),
                    _ => about(ui),
                });
        }
    });
    if resp.should_close() {
        open = false;
    }
    st.open = open;
}

/// Checkbox that reports whether it changed (caller persists settings).
fn toggle(ui: &mut Ui, label: &str, value: &mut bool) -> bool {
    ui.checkbox(value, label).changed()
}

fn general(ui: &mut Ui, settings: &mut Settings, info: &SettingsInfo<'_>, actions: &mut Vec<Action>) {
    ui.strong("Startup");
    if toggle(
        ui,
        "Connect live (push updates) automatically",
        &mut settings.auto_live,
    ) {
        theme::save_settings(settings);
    }
    if toggle(ui, "Start hidden in the system tray", &mut settings.start_in_tray) {
        theme::save_settings(settings);
    }
    if toggle(ui, "Check for updates automatically", &mut settings.update_checks) {
        theme::save_settings(settings);
    }
    ui.add_space(8.0);

    ui.strong("Privacy");
    if toggle(ui, "Ghost mode — hold back read receipts", &mut settings.ghost_mode) {
        theme::save_settings(settings);
    }
    ui.add_space(8.0);

    ui.strong("Window");
    if toggle(
        ui,
        "Close button hides to the tray (quit from the tray menu)",
        &mut settings.close_to_tray,
    ) {
        theme::save_settings(settings);
    }
    ui.add_space(8.0);

    ui.strong("Connection");
    ui.horizontal(|ui| {
        let dot = if info.offline {
            egui::Color32::from_rgb(0xd1, 0xa5, 0x4a)
        } else if info.live {
            egui::Color32::from_rgb(0x6f, 0xd1, 0x94)
        } else {
            egui::Color32::from_rgb(0x9a, 0x9d, 0xa3)
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
    if ui.button("Refresh chats now").clicked() {
        actions.push(Action::Refresh);
    }
    ui.add_space(8.0);
    ui.strong("Audio");
    if ui.button("Test call (echo bot)").clicked() {
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
    ui.strong("Theme");
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

    ui.strong("Text size");
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
        if ui.small_button("Reset").clicked() {
            settings.zoom = 1.0;
            theme::save_settings(settings);
            ui.ctx().set_zoom_factor(1.0);
        }
    });
    ui.add_space(10.0);
    if toggle(
        ui,
        "Show unread count badges on chats",
        &mut settings.unread_badges,
    ) {
        theme::save_settings(settings);
    }
}

fn notifications(ui: &mut Ui, settings: &mut Settings) {
    ui.strong("Desktop notifications");
    if toggle(ui, "Notify for incoming messages", &mut settings.notify) {
        theme::save_settings(settings);
    }
    if toggle(
        ui,
        "Show the message text in the notification",
        &mut settings.notify_preview,
    ) {
        theme::save_settings(settings);
    }
    if toggle(
        ui,
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
    ui.strong("Quiet hours");
    let mut q = settings.quiet_hours.clone().unwrap_or_default();
    let mut enabled = settings.quiet_hours.is_some();
    if ui.checkbox(&mut enabled, "Suppress notifications between").changed() {
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

fn account(ui: &mut Ui, st: &mut SettingsUi, info: &SettingsInfo<'_>, actions: &mut Vec<Action>) {
    ui.strong("Signed in");
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
            if ui.button("Sign out").clicked() {
                st.confirm_signout = true;
            }
        } else {
            ui.horizontal(|ui| {
                ui.label("Remove tokens on this machine?");
                if ui.button("Yes, sign out").clicked() {
                    st.confirm_signout = false;
                    actions.push(Action::SignOut);
                }
                if ui.button("Cancel").clicked() {
                    st.confirm_signout = false;
                }
            });
        }
    } else {
        ui.label(RichText::new("Not signed in.").weak());
        if ui.button("Sign in…").clicked() {
            actions.push(Action::SignIn);
        }
    }
}

fn storage(ui: &mut Ui, st: &mut SettingsUi, info: &SettingsInfo<'_>, actions: &mut Vec<Action>) {
    ui.strong("Local archive");
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
        if ui.button("Clear local archive…").clicked() {
            st.confirm_clear = true;
        }
    } else {
        ui.horizontal(|ui| {
            ui.label("Delete all cached chats and messages?");
            if ui.button("Yes, clear").clicked() {
                st.confirm_clear = false;
                actions.push(Action::ClearArchive);
            }
            if ui.button("Cancel").clicked() {
                st.confirm_clear = false;
            }
        });
    }
    ui.add_space(10.0);
    ui.strong("Files");
    if ui.button("Open data folder (logs, archive)").clicked() {
        actions.push(Action::OpenStateFolder);
    }
}

fn about(ui: &mut Ui) {
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
