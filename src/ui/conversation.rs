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
use crate::theme::Palette;
use crate::ui::widgets::{avatar, parse_html, render_segments};
use egui::{Color32, CornerRadius, Frame, RichText, ScrollArea, Sense, Stroke};
use ost::api::MessageInfo;
use std::collections::HashSet;
use std::sync::mpsc::Sender;

const QUICK_REACTIONS: [&str; 6] = ["👍", "❤️", "😂", "😮", "😢", "🎉"];

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
    pub pal: &'a Palette,
    pub actions: &'a mut Vec<Action>,
}

/// Conversation header + message list (the center panel).
pub fn conversation_messages(ui: &mut egui::Ui, ctx: &mut ConvCtx<'_>) {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        avatar(ui, &ctx.chat_name, 32.0);
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

    // Oldest at the top, newest pinned to the bottom — Teams style.
    // (History note: a non-zero ScrollArea offset once painted blank here;
    // that turned out to be our panel-order bug — composer rendered AFTER
    // CentralPanel consumed the space — not an egui defect. stick_to_bottom
    // is verified rendering again, see bisect in git history.)
    let mut area = ScrollArea::vertical().auto_shrink(false);
    area = area.stick_to_bottom(true);
    area.show(ui, |ui| {
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
        let n = ctx.messages.len();
        let mut last_day = String::new();
        for (i, m) in ctx.messages.iter().enumerate() {
            let day = format_day_label(&m.timestamp);
            let is_new_day = !day.is_empty() && day != last_day;
            if is_new_day {
                let label = day.clone();
                ui.add_space(6.0);
                crate::ui::widgets::day_separator(ui, &label);
                ui.add_space(6.0);
                last_day = day;
            }
            let own = is_own(m, ctx);
            let grouped = i > 0
                && ctx.messages[i - 1].sender_mri == m.sender_mri
                && m.timestamp.get(0..16) == ctx.messages[i - 1].timestamp.get(0..16);
            message_row(ui, ctx, m, own, grouped);
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

    let compose_fill = if ctx.edit.is_some() || ctx.reply.is_some() {
        Color32::from_rgb(0x2b, 0x2d, 0x3a)
    } else {
        ctx.pal.surface
    };
    Frame::default()
        .fill(compose_fill)
        .stroke(egui::Stroke::new(1.0, ctx.pal.outline))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
        ui.horizontal(|ui| {
        if ui
            .add(egui::Button::new(RichText::new("📎").size(16.0)))
            .on_hover_text("Attach a file")
            .clicked()
        {
            ctx.actions.push(Action::Attach);
        }
        let response = ui.add(
            egui::TextEdit::singleline(draft)
                .hint_text(if ctx.edit.is_some() {
                    "Edit message…"
                } else {
                    "Type a message"
                })
                .desired_width(ui.available_width() - 96.0),
        );
        let ready = !draft.trim().is_empty();
        let send_btn = if ready {
            egui::Button::new(
                RichText::new(format!("Send  "))
                    .strong()
                    .color(ctx.pal.on_accent),
            )
            .fill(ctx.pal.accent)
        } else {
            egui::Button::new(RichText::new("Send").weak())
        };
        let btn = ui.add(send_btn);
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
    if own {
        // Right-aligned row: RTL places the avatar at the far right; the
        // bubble column sits to its left. Every part inside the column is
        // laid out right-anchored (see bubble_parts).
        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
            // In right_to_left order the FIRST item sits at the far right:
            // the avatar mirrors the incoming layout. Grouped rows reserve
            // the identical slot (invisible) so right edges always line up.
            if !grouped {
                avatar(ui, ctx.self_name, 32.0);
            } else {
                let (_slot_rect, _slot_resp) = ui
                    .allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
            }
            ui.vertical(|ui| {
                bubble_parts(ui, ctx, m, own, grouped);
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
                bubble_parts(ui, ctx, m, own, grouped);
            });
        });
    }
    ui.add_space(if grouped { 2.0 } else { 9.0 });
}

/// Reactions, reply quote, bubble, timestamp, hover toolbar — one message's
/// full body. For own messages the parent vertical lives inside an RTL row,
/// so every part is wrapped in an RTL row to anchor at the column's RIGHT
/// edge (mirror of the incoming side).
fn bubble_parts(
    ui: &mut egui::Ui,
    ctx: &mut ConvCtx<'_>,
    m: &MessageInfo,
    own: bool,
    grouped: bool,
) {

    // Reactions chips
    if !m.reactions.is_empty() {
        if own {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                reaction_chips(ui, ctx, m);
            });
        } else {
            ui.horizontal(|ui| {
                reaction_chips(ui, ctx, m);
            });
        }
    }
    // Reply quote preview
    if m.reply_to.is_some() {
        if own {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                quote_preview(ui, ctx, m);
            });
        } else {
            quote_preview(ui, ctx, m);
        }
    }
    // Unsupported connector-card fallback: MS returns a plain-text stub for
    // cards our client can't render — show a muted card instead of the raw
    // "Card - access it on ..." line.
    let unsupported_card = m.raw.contains("cards.unsupported");

    // The bubble itself
    let row_id = egui::Id::new(("msg", &m.id));
    let fill = if own {
        ctx.pal.bubble_out
    } else {
        ctx.pal.bubble_in
    };
    let bubble_rect = if own {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
            Frame::default()
                .fill(fill)
                .corner_radius(egui::CornerRadius {
                    nw: 10,
                    ne: 10,
                    sw: 10,
                    se: 2,
                })
                .inner_margin(egui::Margin::symmetric(10, 5))
                .show(ui, |ui| {
                    if unsupported_card {
                        connector_card(ui);
                        return;
                    }
                    // Own bubbles live in an RTL row — reset to LTR inside
                    // the frame so text is left-aligned, not centered.
                    ui.with_layout(
                        egui::Layout::left_to_right(egui::Align::TOP),
                        |ui| {
                            let segments = parse_html(&m.raw);
                            let file_hits: std::rc::Rc<
                                std::cell::RefCell<Vec<(String, String)>>,
                            > = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
                            let hits2 = file_hits.clone();
                            render_segments(
                                ui,
                                &segments,
                                |ui, url| show_image(ui, ctx, url),
                                |ui, name, url| {
                                    if ui
                                        .button(
                                            RichText::new(format!("📎 {name}"))
                                                .small(),
                                        )
                                        .on_hover_text("Download and open")
                                        .clicked()
                                    {
                                        hits2.borrow_mut().push((
                                            name.to_string(),
                                            url.to_string(),
                                        ));
                                    }
                                },
                            );
                            for (name, url) in file_hits.borrow().iter() {
                                ctx.actions.push(Action::DownloadFile {
                                    name: name.clone(),
                                    url: url.clone(),
                                });
                            }
                        },
                    );
                })
                .response
                .rect
        })
        .inner
    } else {
        Frame::default()
            .fill(fill)
            .corner_radius(egui::CornerRadius {
                nw: 10,
                ne: 10,
                sw: 2,
                se: 10,
            })
            .inner_margin(egui::Margin::symmetric(10, 5))
            .show(ui, |ui| {
                if unsupported_card {
                    connector_card(ui);
                    return;
                }
                ui.with_layout(
                    egui::Layout::left_to_right(egui::Align::TOP),
                    |ui| {
                        let segments = parse_html(&m.raw);
                        let file_hits: std::rc::Rc<
                            std::cell::RefCell<Vec<(String, String)>>,
                        > = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
                        let hits2 = file_hits.clone();
                        render_segments(
                            ui,
                            &segments,
                            |ui, url| show_image(ui, ctx, url),
                            |ui, name, url| {
                                if ui
                                    .button(
                                        RichText::new(format!("📎 {name}")).small(),
                                    )
                                    .on_hover_text("Download and open")
                                    .clicked()
                                {
                                    hits2
                                        .borrow_mut()
                                        .push((name.to_string(), url.to_string()));
                                }
                            },
                        );
                        for (name, url) in file_hits.borrow().iter() {
                            ctx.actions.push(Action::DownloadFile {
                                name: name.clone(),
                                url: url.clone(),
                            });
                        }
                    },
                );
            })
            .response
            .rect
    };

    if std::env::var_os("TEAMSFAST_LAYOUT_DEBUG").is_some() && own {
        eprintln!("DBG bubble own rect={bubble_rect:?}");
    }

    // Hover state: POINTER-POSITION based. egui's per-widget hover is not
    // reliable across an Area boundary (the bar vanishes when the pointer
    // moves into the gap), so track the bar's rect and test containment
    // directly, with a 6px grow to bridge the bubble→bar gap.
    let hover_id = egui::Id::new(("msg-hover", &m.id));
    let pointer = ui.input(|i| i.pointer.latest_pos());
    let bar_rect_prev: Option<egui::Rect> = ui.ctx().data(|d| d.get_temp(hover_id));
    let expand = |r: egui::Rect, by: f32| {
        egui::Rect::from_min_max(
            egui::pos2(r.left() - by, r.top() - by),
            egui::pos2(r.right() + by, r.bottom() + by),
        )
    };
    let in_bubble = pointer
        .map(|p| expand(bubble_rect, 2.0).contains(p))
        .unwrap_or(false);
    let in_bar = pointer
        .zip(bar_rect_prev)
        .map(|(p, r)| expand(r, 6.0).contains(p))
        .unwrap_or(false);
    let open = in_bubble || in_bar;

    if !grouped {
        if own {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format_message_time(&m.timestamp)).small().weak(),
                );
            });
        } else {
            ui.label(RichText::new(format_message_time(&m.timestamp)).small().weak());
        }
    }

    if open {
        let anchor = egui::pos2(
            (bubble_rect.right() - 8.0).max(ui.clip_rect().left() + 8.0),
            (bubble_rect.top() - 40.0).max(ui.clip_rect().top() + 2.0),
        );
        let actions_sink: std::rc::Rc<std::cell::RefCell<Vec<Action>>> =
            std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let sink2 = actions_sink.clone();
        let bar = egui::Area::new(row_id.with("toolbar"))
            .order(egui::Order::Tooltip)
            .fixed_pos(anchor)
            .show(ui.ctx(), |ui| {
                let sink = sink2.clone();
                Frame::default()
                    .fill(Color32::from_rgb(0x26, 0x28, 0x33))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(0x3a, 0x3d, 0x47)))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(5, 3))
                    .show(ui, |ui| {
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                for e in QUICK_REACTIONS {
                                    if ui
                                        .button(RichText::new(e).size(18.0))
                                        .on_hover_text("React")
                                        .clicked()
                                    {
                                        sink.borrow_mut().push(Action::React {
                                            message_id: m.id.clone(),
                                            emoji: (*e).to_string(),
                                            remove: false,
                                        });
                                    }
                                }
                                if ui.button(RichText::new("Reply").size(13.0)).clicked() {
                                    sink.borrow_mut().push(Action::Reply {
                                        message_id: m.id.clone(),
                                        sender: sender_label(m, ctx),
                                        snippet: crate::ui::widgets::segs_to_plain(
                                            &parse_html(&m.raw),
                                        )
                                        .lines()
                                        .next()
                                        .unwrap_or("")
                                        .to_string(),
                                    });
                                }
                                if own {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                crate::theme::Icon::Pencil
                                                    .image(Color32::WHITE, 16.0),
                                            ),
                                        )
                                        .on_hover_text("Edit")
                                        .clicked()
                                    {
                                        let current = crate::ui::widgets::segs_to_plain(
                                            &parse_html(&m.raw),
                                        );
                                        sink.borrow_mut().push(Action::StartEdit {
                                            message_id: m.id.clone(),
                                            current,
                                        });
                                    }
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                crate::theme::Icon::Trash
                                                    .image(Color32::WHITE, 16.0),
                                            ),
                                        )
                                        .on_hover_text("Delete")
                                        .clicked()
                                    {
                                        sink.borrow_mut().push(Action::DeleteMessage(
                                            m.id.clone(),
                                        ));
                                    }
                                }
                                if ui
                                    .add(
                                        egui::Button::new(
                                            crate::theme::Icon::Copy
                                                .image(Color32::WHITE, 16.0),
                                        ),
                                    )
                                    .on_hover_text("Copy text")
                                    .clicked()
                                {
                                    ui.ctx().copy_text(crate::ui::widgets::segs_to_plain(
                                        &parse_html(&m.raw),
                                    ));
                                }
                            },
                        );
                    });
            })
            .response;
        ui.ctx().data_mut(|d| d.insert_temp::<egui::Rect>(hover_id, bar.rect));
    } else if bar_rect_prev.is_some() {
        ui.ctx().data_mut(|d| d.remove::<egui::Rect>(hover_id));
    }
}

/// Muted rendering for connector/bot cards the client can't display
/// (Teams returns the "Card - access it on cards.unsupported" stub).
fn connector_card(ui: &mut egui::Ui) {
    Frame::default()
        .fill(Color32::from_rgb(0x23, 0x25, 0x2b))
        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(0x3a, 0x3d, 0x47)))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("🧩").size(16.0));
                ui.vertical(|ui| {
                    ui.label(RichText::new("Connector card").small().strong());
                    ui.label(
                        RichText::new("Open in Teams to view the card content.")
                            .small()
                            .weak(),
                    );
                });
            });
        });
}

/// Reaction count chips (click = remove your reaction).
fn reaction_chips(ui: &mut egui::Ui, ctx: &mut ConvCtx<'_>, m: &MessageInfo) {
    for r in &m.reactions {
        if Frame::default()
            .fill(Color32::from_rgb(0x38, 0x3a, 0x45))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(5, 1))
            .show(ui, |ui| {
                ui.label(RichText::new(format!("{} {}", r.emoji, r.count)).small())
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
}

/// Quoted-parent preview above a reply.
fn quote_preview(ui: &mut egui::Ui, ctx: &mut ConvCtx<'_>, m: &MessageInfo) {
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
