//! Conversation view: message bubbles, hover toolbar, reactions, reply/edit
//! composer, typing indicator, older-page loading.
//!
//! Views never touch protocol state: they push [`Action`]s; `App::apply`
//! turns them into Commands / local state changes.
//!
//! Ordering note: the list renders NEWEST FIRST. egui 0.36.2 on this stack
//! paints nothing for any non-zero ScrollArea offset (stick_to_bottom,
//! scroll_to_cursor and vertical_scroll_offset all end up clipped away), so
//! the list opens at offset 0 — which we make the newest message. Older
//! history pages downward via "Load earlier messages".

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
    OpenImage(String),
    DownloadFile { name: String, url: String },
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

/// Conversation header + message list (the center panel).
pub fn conversation_messages(ui: &mut egui::Ui, ctx: &mut ConvCtx<'_>) {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        avatar(ui, &ctx.chat_name, 30.0);
        ui.add_space(2.0);
        ui.heading(RichText::new(&ctx.chat_name).strong().size(17.0));
        if let Some(user) = ctx.typing_user {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{user} is typing…"))
                        .small()
                        .color(Color32::from_rgb(0x8a, 0x88, 0xff)),
                );
            });
        }
    });
    ui.separator();

    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let n = ctx.messages.len();
        let mut last_day = String::new();
        // Newest first: messages[0] is the oldest, messages[n-1] the newest.
        for (ri, m) in ctx.messages.iter().rev().enumerate() {
            let i = n - 1 - ri; // original index; i+1 is the message NEWER than this one
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
            // Grouped with the message directly NEWER (rendered above it).
            let grouped = i + 1 < n
                && ctx.messages[i + 1].sender_mri == m.sender_mri
                && m.timestamp.get(0..16) == ctx.messages[i + 1].timestamp.get(0..16);
            message_row(ui, ctx, m, own, grouped);
        }
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
    });
}

/// Composer + banners (the bottom panel).
pub fn conversation_composer(
    ui: &mut egui::Ui,
    ctx: &mut ConvCtx<'_>,
    draft: &mut String,
) -> bool {
    let mut keep_focus = false;

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
            let frac = if *total > 0 {
                *sent as f32 / *total as f32
            } else {
                0.0
            };
            ui.add(egui::ProgressBar::new(frac).desired_height(8.0));
        });
    }

    ui.add_space(2.0);
    ui.horizontal(|ui| {
        if ui.button("📎").on_hover_text("Attach a file").clicked() {
            ctx.actions.push(Action::Attach);
        }
        let response = ui.add(
            egui::TextEdit::singleline(draft)
                .hint_text(if ctx.edit.is_some() {
                    "Edit message…"
                } else {
                    "Type a message"
                })
                .desired_width(ui.available_width() - 64.0),
        );
        let ready = !draft.trim().is_empty();
        let btn = ui.add_enabled(ready, egui::Button::new("Send"));
        let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (btn.clicked() || enter) && ready {
            let text = draft.trim().to_string();
            draft.clear();
            keep_focus = true;
            if let Some((mid, _)) = ctx.edit.clone() {
                ctx.actions
                    .push(Action::ApplyEdit { message_id: mid, text });
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

fn message_row(
    ui: &mut egui::Ui,
    ctx: &mut ConvCtx<'_>,
    m: &MessageInfo,
    own: bool,
    grouped: bool,
) {
    let avail = ui.available_width() - 12.0;
    if own {
        // Right-aligned row: avatar pinned at the right edge, bubble in an
        // explicitly-sized slot to its left. (set_max_width inside a
        // right_to_left layout SHIFTS content left instead of capping it.)
        let cap = ((avail - 44.0) * 0.8).min(620.0);
        ui.allocate_ui(egui::vec2(avail, 10.0), |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                // In right_to_left order the FIRST item sits at the far
                // right: the avatar mirrors the incoming layout.
                if !grouped {
                    avatar(ui, ctx.self_name, 32.0);
                } else {
                    ui.add_space(36.0);
                }
                ui.allocate_ui(egui::vec2(cap, 10.0), |ui| {
                    ui.vertical(|ui| {
                        bubble(ui, ctx, m, true, grouped);
                    });
                });
            });
        });
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.add_space(6.0);
            if grouped {
                ui.add_space(38.0);
            } else {
                avatar(ui, &sender_label(m, ctx), 32.0);
                ui.add_space(3.0);
            }
            ui.vertical(|ui| {
                ui.set_max_width((ui.available_width() * 0.8).min(620.0));
                if !grouped {
                    ui.label(RichText::new(sender_label(m, ctx)).small().strong());
                }
                bubble(ui, ctx, m, false, grouped);
            });
        });
    }
    ui.add_space(3.0);
}

fn bubble(
    ui: &mut egui::Ui,
    ctx: &mut ConvCtx<'_>,
    m: &MessageInfo,
    own: bool,
    grouped: bool,
) {
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
                    if Frame::default()
                        .fill(Color32::from_rgb(0x38, 0x3a, 0x45))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(egui::Margin::symmetric(5, 1))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("{} {}", r.emoji, r.count)).small(),
                            )
                        })
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

        let file_hits: std::rc::Rc<std::cell::RefCell<Vec<(String, String)>>> =
            std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let hits2 = file_hits.clone();
        let segments = parse_html(&m.raw);
        let bubble = Frame::default()
            .fill(fill)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(10, 5))
            .show(ui, |ui| {
                let hits2 = hits2.clone();
                render_segments(
                    ui,
                    &segments,
                    |ui, url| show_image(ui, ctx, url),
                    |ui, name, url| {
                        if ui
                            .button(RichText::new(format!("📎 {name}")).small())
                            .on_hover_text("Download and open")
                            .clicked()
                        {
                            hits2.borrow_mut().push((name.to_string(), url.to_string()));
                        }
                    },
                );
            });
        for (name, url) in file_hits.borrow().iter() {
            ctx.actions.push(Action::DownloadFile {
                name: name.clone(),
                url: url.clone(),
            });
        }

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

/// Inline image: textured when loaded, fetch requested once, placeholder
/// otherwise (failed URLs stay in `pending_images` so they never retry).
fn show_image(ui: &mut egui::Ui, ctx: &mut ConvCtx<'_>, url: &str) -> bool {
    if let Some((tex, size)) = ctx.textures.get(url) {
        let max_w = 340.0f32;
        let scale = (max_w / size[0] as f32).min(1.0);
        let disp = egui::vec2(size[0] as f32 * scale, size[1] as f32 * scale);
        let resp = ui.add(egui::Image::new((tex.id(), disp)).sense(Sense::click()));
        if resp.clicked() {
            ctx.actions.push(Action::OpenImage(url.to_string()));
        }
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
                ui.label(RichText::new("🖼 image unavailable").weak().small());
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
                    .small_button("Reply")
                    .on_hover_text("Quote and reply")
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
                    if ui.small_button("Edit").on_hover_text("Edit").clicked() {
                        let current = crate::ui::widgets::segs_to_plain(&parse_html(&m.raw));
                        ctx.actions.push(Action::StartEdit {
                            message_id: m.id.clone(),
                            current,
                        });
                    }
                    if ui.small_button("Delete").on_hover_text("Delete").clicked() {
                        ctx.actions.push(Action::DeleteMessage(m.id.clone()));
                    }
                }
                if ui.small_button("Copy").on_hover_text("Copy text").clicked() {
                    ui.ctx()
                        .copy_text(crate::ui::widgets::segs_to_plain(&parse_html(&m.raw)));
                }
            });
        });
}
