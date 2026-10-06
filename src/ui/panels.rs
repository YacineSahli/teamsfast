//! Secondary panels: search results, new-chat dialog.
//! (AGENT 1 EXTENDS THIS FILE for search refinement + people picking.)

use crate::backend::Command;
use crate::ui::conversation::Action;
use egui::{RichText, ScrollArea};
use ost::api::SearchHitInfo;
use tokio::sync::mpsc::UnboundedSender;

/// Search results overlay panel. Returns true when closed.
pub fn search_panel(
    ui: &mut egui::Ui,
    hits: &[SearchHitInfo],
    more: bool,
    searching: bool,
    cmd: &UnboundedSender<Command>,
    running_query: &str,
    note: Option<&str>,
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
        ui.colored_label(
            egui::Color32::from_rgb(0xd1, 0xa5, 0x4a),
            RichText::new(note).small(),
        );
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
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(avail, 36.0), egui::Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(
                    rect,
                    4.0,
                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 12),
                );
            }
            let mut child =
                ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(3.0)));
            {
                let ui = &mut child;
                ui.set_width(ui.available_width());
                ui.label(RichText::new(&h.sender).strong().size(12.5));
                ui.add(egui::Label::new(RichText::new(&h.preview).small()).truncate());
                ui.label(RichText::new(&h.timestamp).small().weak());
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

/// New-chat dialog. Fields live on App; this draws and reports actions.
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

pub fn new_chat_dialog(
    ui: &mut egui::Ui,
    st: &mut NewChatState,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.strong("New conversation");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("✕").clicked() {
                st.open = false;
            }
        });
    });
    ui.separator();
    ui.label(RichText::new("Start 1:1 — email or UPN:").small());
    ui.horizontal(|ui| {
        let f = ui.text_edit_singleline(&mut st.peer);
        if ui.button("Chat").clicked()
            || (f.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
        {
            let peer = st.peer.trim().to_string();
            if !peer.is_empty() {
                actions.push(Action::CreateOneToOne(peer));
                st.open = false;
                st.peer.clear();
            }
        }
    });
    ui.add_space(6.0);
    ui.label(RichText::new("Group — topic:").small());
    ui.text_edit_singleline(&mut st.topic);
    ui.label(RichText::new("Members (comma-separated emails):").small());
    ui.text_edit_singleline(&mut st.members);
    if ui.button("Create group").clicked() {
        let members: Vec<String> = st
            .members
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let topic = st.topic.trim().to_string();
        if !members.is_empty() {
            actions.push(Action::CreateGroup { topic, members });
            st.open = false;
            st.topic.clear();
            st.members.clear();
        }
    }
}
