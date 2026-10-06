//! Conversation view: message bubbles, hover toolbar, reactions, reply/edit
//! composer, typing indicator, older-page loading.
//!
//! Views never touch protocol state: they push [`Action`]s; `App::apply`
//! turns them into Commands / local state changes.

use crate::model::{format_day_label, format_message_time};
use crate::ui::widgets::{avatar, parse_html, render_segments};
use egui::{Color32, CornerRadius, Frame, RichText, ScrollArea, Sense};
use ost::api::MessageInfo;
use std::collections::HashSet;
use std::sync::mpsc::Sender;

/// UI-intent actions produced by views, applied by `App`.
#[derive(Debug, Clone)]
pub enum Action {
    OpenChat(String),
    Send(String),
    SendReply {
        parent_id: String,
        sender: String,
        snippet: String,
        text: String,
    },
    Reply {
        message_id: String,
        sender: String,
        snippet: String,
    },
    ApplyEdit {
        message_id: String,
        text: String,
    },
    StartEdit {
        message_id: String,
        current: String,
    },
    CancelEditReply,
    DeleteMessage(String),
    React {
        message_id: String,
        emoji: String,
        remove: bool,
    },
    LoadOlder,
    MarkRead(String),
    Attach,
    FetchImage(String),
    OpenSearch,
    ShowNewChat,
    CreateOneToOne(String),
    CreateGroup {
        topic: String,
        members: Vec<String>,
    },
    Refresh,
}

/// Everything the conversation view reads from the App.
pub struct ConvCtx<'a> {
    pub chat_name: String,
    pub chat_id: String,
    pub messages: &'a [MessageInfo],
    pub members: &'a std::collections::HashMap<String, String>,
    pub self_name: &'a str,
    pub self_id: Option<&'a str>,
    pub older_link: Option<&'a str>,
    pub loading_older: bool,
    pub typing_user: Option<&'a str>,
    pub edit: Option<(String, String)>,
    pub reply: Option<(String, String, String)>,
    pub uploads: &'a [(String, u64, u64)],
    pub textures: &'a std::collections::HashMap<String, (egui::TextureHandle, [usize; 2])>,
    pub pending_images: &'a mut HashSet<String>,
    pub actions: &'a mut Vec<Action>,
}

/// Returns `true` when the composer should keep focus.
pub fn conversation(ui: &mut egui::Ui, ctx: &mut ConvCtx<'_>, draft: &mut String) -> bool {
    let mut keep_focus = false;

    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.heading(RichText::new(&ctx.chat_name).strong().size(17.0));
    });
    ui.add_space(2.0);
    if let Some(user) = ctx.typing_user {
        ui.label(
            RichText::new(format!("{user} is typing…"))
                .small()
                .color(Color32::from_rgb(0x8a, 0x88, 0xff)),
        );
    }
    ui.separator();

    ScrollArea::vertical()
        .auto_shrink(false)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            if ctx.older_link.is_some() {
                ui.vertical_centered(|ui| {
                    if ctx.loading_older {
                        ui.spinner();
                    } else if ui
                        .button(RichText::new("Load earlier messages").small().weak())
                        .clicked()
                    {
                        ctx.actions.push(Action::LoadOlder);
                    }
                });
            }
            let mut last_day = String::new();
            for (i, m) in ctx.messages.iter().enumerate() {
                let day = format_day_label(&m.timestamp);
                let is_new_day = !day.is_empty() && day != last_day;
                if is_new_day {
                    let label = day.clone();
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(label).small().weak());
                    });
                    last_day = day;
                }
                let own = is_own(m, ctx);
                let grouped = i > 0
                    && ctx.messages[i - 1].sender_mri == m.sender_mri
                    && m.timestamp.get(0..16) == ctx.messages[i - 1].timestamp.get(0..16);
                message_row(ui, ctx, m, own, grouped);
            }
        });

    ui.separator();

    if let Some((_, who, snippet)) = &ctx.reply {
        Frame::default()
            .fill(Color32::from_rgb(0x2b, 0x2d, 0x3a))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Replying to {who}")).small().strong());
                    ui.add(egui::Label::new(RichText::new(snippet).small().weak()).truncate());
                    if ui.small_button("✕").clicked() {
                        ctx.actions.push(Action::CancelEditReply);
                    }
                });
            });
    }
    if let Some((_, current)) = &ctx.edit {
        Frame::default()
            .fill(Color32::from_rgb(0x3a, 0x33, 0x2b))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Editing").small().strong());
                    ui.add(egui::Label::new(RichText::new(current).small().weak()).truncate());
                    if ui.small_button("✕").clicked() {
                        ctx.actions.push(Action::CancelEditReply);
                    }
                });
            });
    }

    for (name, sent, total) in ctx.uploads {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("⬆ {name}")).small());
            let frac = if *total > 0 { *sent as f32 / *total as f32 } else { 0.0 };
            ui.add(egui::ProgressBar::new(frac).desired_height(8.0));
        });
    }

    ui.horizontal(|ui| {
        if ui.button("📎").on_hover_text("Attach a file").clicked() {
            ctx.actions.push(Action::Attach);
        }
        let ready = !draft.trim().is_empty();
        let btn = ui.add_enabled(ready, egui::Button::new("Send"));
        let response = ui.add(
            egui::TextEdit::singleline(draft)
                .hint_text(if ctx.edit.is_some() { "Edit message…" } else { "Type a message" })
                .desired_width(ui.available_width() - 64.0),
        );
        let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (btn.clicked() || enter) && ready {
            let text = draft.trim().to_string();
            draft.clear();
            keep_focus = true;
            if let Some((mid, _)) = ctx.edit.clone() {
                ctx.actions.push(Action::ApplyEdit { message_id: mid, text });
            } else if let Some((pid, who, snip)) = ctx.reply.clone() {
                ctx.actions.push(Action::SendReply {
                    parent_id: pid,
                    sender: who,
                    snippet: snip,
                    text,
                });
            } else {
                ctx.actions.push(Action::Send(text));
            }
        }
    });

    keep_focus
}

fn is_own(m: &MessageInfo, ctx: &ConvCtx<'_>) -> bool {
    if let Some(me) = ctx.self_id {
        if m.sender_mri.contains(me) {
            return true;
        }
    }
    !ctx.self_name.is_empty() && m.sender == ctx.self_name
}

fn sender_label(m: &MessageInfo, ctx: &ConvCtx<'_>) -> String {
    if !m.sender.is_empty() && m.sender != "?" {
        return m.sender.clone();
    }
    if let Some(name) = ctx.members.get(&m.sender_mri) {
        return name.clone();
    }
    "Unknown".into()
}

fn message_row(ui: &mut egui::Ui, ctx: &mut ConvCtx<'_>, m: &MessageInfo, own: bool, grouped: bool) {
    if !own {
        ui.horizontal_wrapped(|ui| {
            ui.add_space(6.0);
            if grouped {
                ui.add_space(36.0);
            } else {
                avatar(ui, &sender_label(m, ctx), 32.0);
                ui.add_space(3.0);
            }
            ui.vertical(|ui| {
                if !grouped {
                    ui.label(RichText::new(sender_label(m, ctx)).small().strong());
                }
                bubble(ui, ctx, m, false, grouped);
            });
        });
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.add_space(6.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                ui.vertical(|ui| {
                    bubble(ui, ctx, m, true, grouped);
                });
                if !grouped {
                    avatar(ui, ctx.self_name, 32.0);
                } else {
                    ui.add_space(36.0);
                }
            });
        });
    }
    ui.add_space(3.0);
}

fn bubble(ui: &mut egui::Ui, ctx: &mut ConvCtx<'_>, m: &MessageInfo, own: bool, grouped: bool) {
    let fill = if own {
        Color32::from_rgb(0x3b, 0x3e, 0xcf)
    } else {
        Color32::from_rgb(0x2b, 0x2d, 0x31)
    };
    let row_id = egui::Id::new(("msg", &m.id));

    ui.vertical(|ui| {
        if !m.reactions.is_empty() {
            ui.horizontal(|ui| {
                for r in &m.reactions {
                    let mine = false; // precise "did I react" needs reactor list; toggle-off via remove
                    let _ = mine;
                    if Frame::default()
                        .fill(if r.count > 0 {
                            Color32::from_rgb(0x38, 0x3a, 0x45)
                        } else {
                            Color32::TRANSPARENT
                        })
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(egui::Margin::symmetric(5, 1))
                        .show(ui, |ui| ui.label(RichText::new(format!("{} {}", r.emoji, r.count)).small()))
                        .response
                        .clicked()
                    {
                        ctx.actions.push(Action::React {
                            message_id: m.id.clone(),
                            emoji: r.emoji.clone(),
                            remove: true,
                        });
                    }
                }
            });
        }

        if let Some(pid) = &m.reply_to {
            if let Some(parent) = ctx.messages.iter().find(|p| &p.id == pid) {
                Frame::default()
                    .fill(Color32::from_rgb(0x25, 0x27, 0x30))
                    .corner_radius(CornerRadius::same(4))
                    .inner_margin(egui::Margin::symmetric(6, 2))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!(
                                "{}: {}",
                                sender_label(parent, ctx),
                                crate::ui::widgets::segs_to_plain(&parse_html(&parent.raw))
                                    .lines()
                                    .next()
                                    .unwrap_or("")
                            ))
                            .small()
                            .weak(),
                        );
                    });
            }
        }

        let bubble = Frame::default()
            .fill(fill)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(10, 5))
            .show(ui, |ui| {
                render_segments(ui, &parse_html(&m.raw), |ui, url| {
                    show_image(ui, ctx, url)
                });
            });

        let hov = ui
            .interact(bubble.response.rect, row_id, Sense::hover())
            .hovered();
        if !grouped {
            ui.label(RichText::new(format_message_time(&m.timestamp)).small().weak());
        }
        if hov {
            hover_toolbar(ui, ctx, m, own);
        }
    });
}

fn show_image(ui: &mut egui::Ui, ctx: &mut ConvCtx<'_>, url: &str) -> bool {
    if let Some((tex, size)) = ctx.textures.get(url) {
        let max_w = 340.0f32;
        let scale = (max_w / size[0] as f32).min(1.0);
        let size = egui::vec2(size[0] as f32 * scale, size[1] as f32 * scale);
        ui.add(egui::Image::new((tex.id(), size)));
        true
    } else {
        if ctx.pending_images.insert(url.to_string()) {
            ctx.actions.push(Action::FetchImage(url.to_string()));
        }
        Frame::default()
            .fill(Color32::from_rgb(0x22, 0x24, 0x2a))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.label(RichText::new("🖼 loading image…").weak().small());
            });
        false
    }
}

fn hover_toolbar(ui: &mut egui::Ui, ctx: &mut ConvCtx<'_>, m: &MessageInfo, own: bool) {
    const QUICK: [&str; 6] = ["👍", "❤️", "😂", "😮", "😢", "🎉"];
    Frame::default()
        .fill(Color32::from_rgb(0x26, 0x28, 0x33))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(4, 2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for e in QUICK {
                    if ui
                        .button(RichText::new(e).small())
                        .on_hover_text("React")
                        .clicked()
                    {
                        ctx.actions.push(Action::React {
                            message_id: m.id.clone(),
                            emoji: e.into(),
                            remove: false,
                        });
                    }
                }
                if ui
                    .button(RichText::new("↩").small())
                    .on_hover_text("Reply")
                    .clicked()
                {
                    ctx.actions.push(Action::Reply {
                        message_id: m.id.clone(),
                        sender: sender_label(m, ctx),
                        snippet: crate::ui::widgets::segs_to_plain(&parse_html(&m.raw))
                            .lines()
                            .next()
                            .unwrap_or("")
                            .to_string(),
                    });
                }
                if own {
                    if ui
                        .button(RichText::new("✎").small())
                        .on_hover_text("Edit")
                        .clicked()
                    {
                        let current = crate::ui::widgets::segs_to_plain(&parse_html(&m.raw));
                        ctx.actions.push(Action::StartEdit {
                            message_id: m.id.clone(),
                            current,
                        });
                    }
                    if ui
                        .button(RichText::new("🗑").small())
                        .on_hover_text("Delete")
                        .clicked()
                    {
                        ctx.actions.push(Action::DeleteMessage(m.id.clone()));
                    }
                }
                if ui
                    .button(RichText::new("⧉").small())
                    .on_hover_text("Copy")
                    .clicked()
                {
                    ui.ctx()
                        .copy_text(crate::ui::widgets::segs_to_plain(&parse_html(&m.raw)));
                }
            });
        });
}
