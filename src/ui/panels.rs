//! Secondary panels: search results, new-chat dialog.
//! (AGENT 1 EXTENDS THIS FILE for search refinement + people picking.)

use crate::backend::Command;
use crate::theme::Palette;
use crate::ui::conversation::Action;
use egui::{RichText, ScrollArea};
use ost::api::SearchHitInfo;
use tokio::sync::mpsc::UnboundedSender;
use crate::theme::RichTextBold;

/// Search results overlay panel. Returns true when closed.
pub fn search_panel(
    ui: &mut egui::Ui,
    hits: &[SearchHitInfo],
    more: bool,
    searching: bool,
    cmd: &UnboundedSender<Command>,
    running_query: &str,
    note: Option<&str>,
    pal: &Palette,
    actions: &mut Vec<Action>,
) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.strong("Search results");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("✕").clicked() {
                close = true;
            }
            if searching {
                ui.spinner();
            }
        });
    });
    if let Some(note) = note {
        ui.colored_label(pal.warning, RichText::new(note).small());
    }
    ui.separator();
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        if hits.is_empty() && !searching && note.is_none() {
            ui.label(RichText::new("No results.").weak());
        }
        for h in hits {
            let target = if h.chat_id.is_empty() {
                h.channel_id.clone().unwrap_or_default()
            } else {
                h.chat_id.clone()
            };
            // Allocate the clickable row FIRST, paint content inside —
            // late `.interact()` gets shadowed by hover-sense labels.
            let avail = ui.available_width() - 4.0;
            let (rect, mut response) =
                ui.allocate_exact_size(egui::vec2(avail, 36.0), egui::Sense::click());
            response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.hovered() {
                ui.painter().rect_filled(rect, 4.0, pal.surface_hover);
            }
            let mut child =
                ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(3.0)));
            {
                let ui = &mut child;
                ui.set_width(ui.available_width());
                ui.add(
                    egui::Label::new(
                        RichText::new(&h.sender)
                            .bold()
                            .size(12.5)
                            .color(pal.link),
                    )
                    .selectable(false),
                );
                ui.add(
                    egui::Label::new(RichText::new(&h.preview).small())
                        .truncate()
                        .selectable(false),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(&h.timestamp)
                            .small()
                            .weak()
                            .color(pal.secondary),
                    )
                    .selectable(false),
                );
            }

            if response.clicked() && !target.is_empty() {
                // ACTION, not raw command: App must update `selected` or the
                // incoming history is dropped on the selected-mismatch guard.
                actions.push(Action::OpenChat(target));
            }
            ui.separator();
        }
        if more && !running_query.is_empty() {
            ui.vertical_centered(|ui| {
                if ui.button("More results").clicked() {
                    cmd.send(Command::Search {
                        query: running_query.to_string(),
                        from: hits.len(),
                    })
                    .ok();
                }
            });
        }
    });
    close
}

/// New-conversation dialog state.
pub struct NewChatState {
    pub open: bool,
    pub peer: String,
    pub topic: String,
    pub members: String,
}

impl Default for NewChatState {
    fn default() -> Self {
        Self {
            open: false,
            peer: String::new(),
            topic: String::new(),
            members: String::new(),
        }
    }
}

/// Uppercase small section label (same rhythm as the settings window).
fn dialog_section(ui: &mut egui::Ui, pal: &Palette, title: &str) {
    ui.add_space(2.0);
    ui.label(
        RichText::new(title.to_uppercase())
            .small()
            .bold()
            .color(pal.secondary),
    );
    ui.add_space(3.0);
}

/// Accent primary that reads disabled until `enabled` (the stock disabled
/// visuals can't dim an explicit accent fill — swap fill/stroke/text).
fn modal_primary(
    ui: &mut egui::Ui,
    pal: &Palette,
    text: &str,
    enabled: bool,
    width: f32,
) -> egui::Response {
    let b = egui::Button::new(
        RichText::new(text)
            .size(12.5)
            .bold()
            .color(if enabled { pal.on_accent } else { pal.secondary }),
    )
    .min_size(egui::vec2(width, 26.0))
    .corner_radius(egui::CornerRadius::same(6))
    .fill(if enabled { pal.accent } else { pal.surface })
    .stroke(if enabled {
        egui::Stroke::NONE
    } else {
        egui::Stroke::new(1.0, pal.outline)
    });
    ui.add_enabled(enabled, b)
}

/// New-conversation dialog CONTENT — App hosts it in an egui::Modal
/// (centered + scrim + Esc/backdrop close, same frame as the join dialog).
/// Fields live on App; this draws and reports actions.
pub fn new_chat_dialog(
    ui: &mut egui::Ui,
    st: &mut NewChatState,
    pal: &Palette,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("New chat").bold().size(15.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(
                    egui::Button::new(crate::theme::Icon::X.image(pal.secondary, 13.0))
                        .fill(egui::Color32::TRANSPARENT)
                        .stroke(egui::Stroke::NONE)
                        .min_size(egui::vec2(22.0, 22.0)),
                )
                .clicked()
            {
                st.open = false;
            }
        });
    });

    ui.add_space(8.0);

    // ---- Direct message ----
    dialog_section(ui, pal, "Direct message");
    let peer_field = ui.add(
        egui::TextEdit::singleline(&mut st.peer)
            .hint_text("Email or UPN — e.g. ada@example.com")
            .desired_width(ui.available_width())
            .min_size(egui::vec2(0.0, 30.0))
            .id_salt("new_chat_peer"),
    );
    let peer = st.peer.trim().to_string();
    ui.add_space(6.0);
    // horizontal wrapper REQUIRED: a bare right_to_left with_layout in this
    // vertical ui corrupts the cursor for everything after it — the modal
    // then re-measures ~150px taller EVERY frame and grows off-screen
    // (probe-verified: delta=0 with the wrapper, +152/frame without). The
    // join dialog got away with a bare r2l row only because it is the LAST
    // element.
    ui.horizontal(|ui| {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let start = modal_primary(ui, pal, "Chat", !peer.is_empty(), 84.0).clicked()
            || (peer_field.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && !peer.is_empty());
        if start {
            actions.push(Action::CreateOneToOne(peer));
            st.open = false;
            st.peer.clear();
        }
    });
    });
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(8.0);

    // ---- Group chat ----
    dialog_section(ui, pal, "Group chat");
    ui.add(
        egui::TextEdit::singleline(&mut st.topic)
            .hint_text("Topic — what is this chat about?")
            .desired_width(ui.available_width())
            .min_size(egui::vec2(0.0, 30.0))
            .id_salt("new_chat_topic"),
    );
    ui.add_space(6.0);
    let members_field = ui.add(
        egui::TextEdit::singleline(&mut st.members)
            .hint_text("Members — comma-separated emails")
            .desired_width(ui.available_width())
            .min_size(egui::vec2(0.0, 30.0))
            .id_salt("new_chat_members"),
    );
    let members: Vec<String> = st
        .members
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let n = members.len();
    ui.add_space(6.0);
    // Same horizontal wrapper as the Chat row above.
    ui.horizontal(|ui| {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let create = modal_primary(
            ui,
            pal,
            "Create group",
            !members.is_empty(),
            110.0,
        )
        .clicked()
            || (members_field.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && !members.is_empty());
        ui.label(
            RichText::new(match n {
                0 => String::new(),
                1 => "1 member".into(),
                _ => format!("{n} members"),
            })
            .small()
            .weak(),
        );
        if create {
            let topic = st.topic.trim().to_string();
            actions.push(Action::CreateGroup { topic, members });
            st.open = false;
            st.topic.clear();
            st.members.clear();
        }
    });
    });
}
