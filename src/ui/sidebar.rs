//! Left sidebar: view switch (Chats / Teams), search field, chat list.

use crate::backend::Command;
use crate::model::{clean_preview, format_chat_time};
use crate::ui::conversation::Action;
use crate::ui::widgets::avatar;
use egui::{RichText, ScrollArea, Ui};
use ost::api::{ChatInfo, TeamInfo};

#[derive(PartialEq, Clone, Copy)]
pub enum SideView {
    Chats,
    Teams,
}

/// Chat-list row payload the sidebar needs.
pub struct SidebarCtx<'a> {
    pub chats: &'a [ChatInfo],
    pub selected: Option<&'a String>,
    pub teams: &'a [TeamInfo],
    pub view: SideView,
    pub cmd: &'a tokio::sync::mpsc::UnboundedSender<Command>,
}

pub fn sidebar(
    ui: &mut Ui,
    ctx: &mut SidebarCtx<'_>,
    search: &mut String,
    loading_teams: &mut bool,
    actions: &mut Vec<Action>,
) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.selectable_value(&mut ctx.view, SideView::Chats, RichText::new("Chats").strong());
        ui.selectable_value(&mut ctx.view, SideView::Teams, RichText::new("Teams").strong());
    });
    ui.add_space(2.0);

    match ctx.view {
        SideView::Chats => {
            // Search + new chat, constrained to the panel width.
            let avail = ui.available_width();
            ui.allocate_ui(egui::vec2(avail, 26.0), |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("+ New")
                        .on_hover_text("New chat (Ctrl+N)")
                        .clicked()
                    {
                        actions.push(Action::ShowNewChat);
                    }
                    let field = ui.add_sized(
                        [ui.available_width(), 24.0],
                        egui::TextEdit::singleline(search)
                            .hint_text("Search messages (Enter)")
                            .font(egui::TextStyle::Small),
                    );
                    if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let q = search.trim().to_string();
                        if !q.is_empty() {
                            ctx.cmd.send(Command::Search { query: q, from: 0 }).ok();
                            actions.push(Action::OpenSearch);
                        }
                    }
                });
            });
            ui.add_space(2.0);
            ui.separator();

            ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                for chat in ctx.chats {
                    chat_row(ui, ctx, chat);
                }
            });
        }
        SideView::Teams => {
            ui.separator();
            if ctx.teams.is_empty() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("loading teams…").weak());
                });
                if !*loading_teams {
                    *loading_teams = true;
                    ctx.cmd.send(Command::LoadTeams).ok();
                }
            }
            ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                for team in ctx.teams {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        avatar(ui, &team.name, 22.0);
                        ui.strong(&team.name);
                    });
                    for ch in &team.channels {
                        let sel = ctx.selected == Some(&ch.id);
                        if ui.selectable_label(sel, format!("# {}", ch.name)).clicked() {
                            ctx.cmd.send(Command::OpenChat(ch.id.clone())).ok();
                        }
                    }
                }
            });
        }
    }
}

fn chat_row(ui: &mut Ui, ctx: &SidebarCtx<'_>, chat: &ChatInfo) {
    let selected = ctx.selected == Some(&chat.id);
    let label = match chat.name.as_str() {
        "" => {
            let mut s: String = chat.id.chars().take(24).collect();
            s.push('…');
            s
        }
        "[Direct message]" => "Direct message".into(),
        other => other.to_string(),
    };
    let time = format_chat_time(&chat.last_message_time);

    // Explicit-size row so the right column can never overflow the panel.
    let avail = ui.available_width();
    let response = ui
        .allocate_ui(egui::vec2(avail, 40.0), |ui| {
            ui.horizontal(|ui| {
                ui.add_space(2.0);
                avatar(ui, &label, 30.0);
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    // Name … time, both constrained.
                    ui.allocate_ui(egui::vec2(ui.available_width(), 17.0), |ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !time.is_empty() {
                                ui.label(RichText::new(time).small().weak());
                            }
                            if chat.is_group {
                                ui.label(RichText::new("👥").size(13.0).weak());
                            }
                            ui.add(
                                egui::Label::new(RichText::new(&label).strong()).truncate(),
                            );
                        });
                    });
                    if let Some(p) = &chat.last_message_preview {
                        ui.add(
                            egui::Label::new(RichText::new(clean_preview(p)).small().weak())
                                .truncate(),
                        );
                    }
                });
            });
        })
        .response
        .interact(egui::Sense::click());

    if response.clicked() {
        ctx.cmd.send(Command::OpenChat(chat.id.clone())).ok();
    }
    if selected {
        ui.painter().rect_filled(
            response.rect,
            4.0,
            egui::Color32::from_rgba_unmultiplied(0x5b, 0x5f, 0xc7, 0x40),
        );
    }
    ui.separator();
}
