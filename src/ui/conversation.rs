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

use crate::model::format_message_time;
use crate::theme::Palette;
use crate::ui::widgets::{avatar, parse_html, parse_html_cached, render_segments};
use egui::{Color32, CornerRadius, Frame, RichText, ScrollArea, Sense, Stroke};
use ost::api::MessageInfo;
use std::collections::{HashMap, HashSet};

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
    ShowReactions {
        emoji: String,
        names: Vec<String>,
    },
    OpenSearch,
    ShowNewChat,
    CreateOneToOne(String),
    CreateGroup {
        topic: String,
        members: Vec<String>,
    },
    Refresh,
    // ---- app-level actions (settings, account, storage) ----
    OpenSettings,
    /// Theme file selection: Some(filename) or None for the built-in.
    SetTheme(Option<String>),
    SetBuiltinLight(bool),
    SignIn,
    SignOut,
    ClearArchive,
    OpenThemeFolder,
    OpenStateFolder,
    // ---- chat list row ops ----
    TogglePin(String),
    ToggleMute { chat_id: String, muted: bool },
    MarkUnread(String),
    HideChat(String),
    LeaveChat(String),
    // ---- message ops ----
    /// Forward text to another chat (opens the picker).
    Forward(String),
    /// Locally pin/unpin a message in this chat.
    TogglePinMessage { chat_id: String, message_id: String },
    /// Retry a failed send by its clientmessageid.
    RetrySend(String),
    /// Discard a failed/pending send bubble.
    DismissSend(String),
    /// Refetch the newest page (scrolls home / jump to latest).
    JumpLatest,
    // ---- sections ----
    /// Re-fetch the data of the visible section.
    ReloadSection,
    /// Calendar week/month navigation: fetch an arbitrary window.
    LoadCalendarRange { start_unix: u64, days: u64 },
    /// Open a URL with the system handler.
    OpenLink(String),
    /// Copy text to the clipboard.
    CopyText(String),
    /// Download a OneDrive item and open it.
    DownloadDriveFile {
        drive_id: String,
        item_id: String,
        name: String,
    },
    OpenTodoList(String),
    AddTodoTask {
        list_id: String,
        title: String,
    },
    SetTodoDone {
        list_id: String,
        task_id: String,
        done: bool,
    },
    ClearActivity,
    // ---- calls ----
    /// Place a 1:1 audio call to this chat's peer.
    StartCall(String),
    /// Place a 1:1 call with the camera on (video modality, H.264 send).
    StartVideoCall(String),
    /// Ring the echo/test bot.
    TestCall,
    /// Accept the ringing incoming call.
    AcceptIncoming,
    /// Decline the ringing incoming call.
    DeclineIncoming,
    /// Join a meeting (join URL / thread id / meet ID) with a display label.
    JoinMeeting { source: String, label: Option<String> },
    /// Open the "join with link" input.
    ShowJoinDialog,
    /// Per-chat notification level ("all" | "mentions" | "off").
    SetNotifyLevel { chat_id: String, level: String },
    // ---- teams management ----
    ShowTeamDialog,
    CreateChannel { team_id: String, name: String },
    SearchPublicTeams(String),
    JoinTeam { team_id: String, name: String },
    CreateTeam(String),
    /// Open the contact card for a member (mri + display name).
    ShowContact { mri: String, name: String },
    /// Tick/untick a planner task.
    SetPlannerDone {
        task_id: String,
        etag: String,
        done: bool,
    },
    RenameChannel {
        team_id: String,
        channel_id: String,
        name: String,
    },
    DeleteChannel {
        team_id: String,
        channel_id: String,
    },
    /// Show this chat's shared files in the Files section.
    ShowChatFiles(String),
    ReadNotePage(String),
    AppendNote { page_id: String, text: String },
    /// Create an instant meeting and join it.
    MeetNow,
    /// Start/open the 1:1 with this member's mri.
    ChatWith { mri: String, name: String },
    HangUp,
    /// Surface a transient note in the top bar (e.g. why a calendar click
    /// had no join action).
    Status(String),
}

/// An own message on its way out (or failed, awaiting Retry) — the view
/// mirrors App's PendingSend without importing App.
pub struct PendingBubble {
    pub cmid: String,
    pub text: String,
    pub error: Option<String>,
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
    /// Own sends in flight / failed for this chat.
    pub pending: &'a [PendingBubble],
    /// message id → user mris that have read it ("Seen by").
    pub receipts: &'a std::collections::HashMap<String, Vec<String>>,
    /// Locally pinned message ids in this chat.
    pub pinned: &'a [String],
    /// True when this chat is a 1:1 thread a call can be placed to.
    pub can_call: bool,
    /// Active-call banner text (None when idle).
    pub call_label: Option<&'a str>,
    pub textures: &'a std::collections::HashMap<String, (egui::TextureHandle, [usize; 2])>,
    /// Known image sizes (url -> [w,h]) — placeholders reserve the final
    /// box so an arriving image doesn't reflow the conversation.
    pub image_sizes: &'a std::collections::HashMap<String, [usize; 2]>,
    pub pending_images: &'a mut HashSet<String>,
    /// Emoji raster cache: cluster -> texture (colour, from bundled Noto).
    pub emoji_textures: &'a mut HashMap<String, egui::TextureHandle>,
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
        // Pinned messages menu.
        if !ctx.pinned.is_empty() {
            ui.menu_button(
                crate::theme::Icon::Pin.image(ctx.pal.secondary, 14.0),
                |ui| {
                    ui.set_min_width(260.0);
                    ui.strong("Pinned");
                    ui.separator();
                    let mut shown = 0;
                    for id in ctx.pinned {
                        if let Some(m) = ctx.messages.iter().find(|m| &m.id == id) {
                            ui.label(
                                RichText::new(format!(
                                    "📌 {}: {}",
                                    sender_label(m, ctx),
                                    crate::ui::widgets::segs_to_plain(&parse_html(&m.raw))
                                        .lines()
                                        .next()
                                        .unwrap_or("")
                                ))
                                .small()
                                .color(ctx.pal.secondary),
                            );
                            shown += 1;
                        }
                    }
                    if shown == 0 {
                        ui.label(
                            RichText::new("Pinned messages are outside the loaded page.")
                                .small()
                                .weak(),
                        );
                    }
                },
            );
        }
        // Call controls for 1:1 chats.
        if let Some(label) = ctx.call_label {
            ui.label(
                egui::RichText::new(format!("● {label}"))
                    .small()
                    .color(ctx.pal.ok),
            );
            if ui
                .add(
                    egui::Button::new(egui::RichText::new("Hang up").small().color(ctx.pal.on_accent))
                        .fill(ctx.pal.danger)
                        .min_size(egui::vec2(64.0, 22.0)),
                )
                .clicked()
            {
                ctx.actions.push(Action::HangUp);
            }
        } else if ctx.can_call {
            let call = ui.add(
                egui::Button::new(crate::theme::Icon::Phone.image(ctx.pal.secondary, 15.0))
                    .fill(egui::Color32::TRANSPARENT),
            );
            if call
                .on_hover_text("Call (audio)")
                .clicked()
            {
                ctx.actions.push(Action::StartCall(ctx.chat_id.clone()));
            }
            let video = ui.add(
                egui::Button::new(crate::theme::Icon::Video.image(ctx.pal.secondary, 15.0))
                    .fill(egui::Color32::TRANSPARENT),
            );
            if video
                .on_hover_text("Call with camera")
                .clicked()
            {
                ctx.actions.push(Action::StartVideoCall(ctx.chat_id.clone()));
            }
        }
        // Shared files of this conversation (Files section).
        if ui
            .add(
                egui::Button::new(crate::theme::Icon::FileText.image(ctx.pal.secondary, 14.0))
                    .fill(egui::Color32::TRANSPARENT),
            )
            .on_hover_text("Files shared in this chat")
            .clicked()
        {
            ctx.actions.push(Action::ShowChatFiles(ctx.chat_id.clone()));
        }
        // Jump to latest: history is paged above, so re-anchor to the tail.
        if ctx.older_link.is_some() {
            if ui
                .small_button(RichText::new("⇩ Latest").small())
                .on_hover_text("Reload the newest messages")
                .clicked()
            {
                ctx.actions.push(Action::JumpLatest);
            }
        }
        if let Some(user) = ctx.typing_user {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{user} is typing…"))
                        .small()
                        .color(ctx.pal.link),
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
        // Row culling: egui's ScrollArea clips PAINTING but still allocates
        // and lays out every row. With a few hundred messages loaded that is
        // hundreds of widget trees per frame. Rows whose (previous-frame)
        // height places them fully outside the view ± margin get a cheap
        // placeholder allocation instead — same flow, same scroll extent
        // (sizes self-heal when a row scrolls into view and renders).
        let clip = ui.clip_rect();
        const CULL_MARGIN: f32 = 240.0;
        let mut last_day = String::new();
        for (i, m) in ctx.messages.iter().enumerate() {
            // Day label is content-cached (a jiff parse per row per frame
            // otherwise); it must advance for every row, culled or not, or
            // separators after a culled run would repeat.
            let day = crate::model::format_day_label_cached(&m.timestamp);
            let is_new_day = !day.is_empty() && day.as_str() != last_day;
            if is_new_day {
                let label = day.to_string();
                ui.add_space(6.0);
                crate::ui::widgets::day_separator(ui, ctx.pal, &label);
                ui.add_space(6.0);
                last_day = day.to_string();
            }
            let row_id = ui.id().with(("msg-row-h", &m.id));
            let known: Option<egui::Vec2> = ui.ctx().data_mut(|d| d.get_temp(row_id));
            let top = ui.cursor().top();
            if let Some(sz) = known
                && (top > clip.bottom() + CULL_MARGIN
                    || top + sz.y < clip.top() - CULL_MARGIN)
            {
                ui.allocate_exact_size(sz, egui::Sense::hover());
                continue;
            }
            let own = is_own(m, ctx);
            // Group consecutive messages from the same sender within a
            // 5-minute window (Teams behaviour; the old same-minute rule
            // split runs that pause for a moment).
            let groups_with =
                |a: &MessageInfo, b: &MessageInfo| -> bool {
                    if a.sender_mri != b.sender_mri {
                        return false;
                    }
                    match (
                        crate::model::to_epoch_ms(a.timestamp.as_str()),
                        crate::model::to_epoch_ms(b.timestamp.as_str()),
                    ) {
                        (Some(t), Some(pt)) => t.saturating_sub(pt) <= 300_000,
                        _ => a.timestamp.get(0..16) == b.timestamp.get(0..16),
                    }
                };
            let grouped = i > 0 && groups_with(m, &ctx.messages[i - 1]);
            // Teams attributes the timestamp (and seen-by) to the LAST
            // bubble of a run, not the first.
            let last_of_group = ctx.messages.get(i + 1).is_none_or(|n| {
                !groups_with(n, m)
            });
            message_row(ui, ctx, m, own, grouped, last_of_group);
            // Record the rendered height for culling. The flow adds item
            // spacing on top of each allocation, so strip it back out — the
            // placeholder gets its own spacing when allocated.
            let h = (ui.cursor().top() - top - ui.spacing().item_spacing.y).max(0.0);
            if h > 0.0 {
                ui.ctx()
                    .data_mut(|d| d.insert_temp(row_id, egui::vec2(0.0, h)));
            }
        }
        // Own sends in flight / failed: translucent bubble, Retry on error.
        for p in ctx.pending {
            pending_bubble(ui, ctx, p);
        }
    });
}

/// Own message bubble for a send that hasn't been confirmed by the server
/// yet (translucent) or that failed (danger border + Retry / Discard).
fn pending_bubble(ui: &mut egui::Ui, ctx: &mut ConvCtx<'_>, p: &PendingBubble) {
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
            ui.vertical(|ui| {
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::TOP),
                    |ui| {
                        let (fill, stroke) = match &p.error {
                            None => (
                                ctx.pal.bubble_out,
                                egui::Stroke::NONE,
                            ),
                            Some(_) => (
                                ctx.pal.bubble_out,
                                egui::Stroke::new(1.0, ctx.pal.danger),
                            ),
                        };
                        egui::Frame::default()
                            .fill({
                                // 55% alpha while pending.
                                let [r, g, b, _] = fill.to_srgba_unmultiplied();
                                Color32::from_rgba_unmultiplied(r, g, b, 140)
                            })
                            .stroke(stroke)
                            .corner_radius(egui::CornerRadius {
                                nw: 12,
                                ne: 12,
                                sw: 12,
                                se: 3,
                            })
                            .inner_margin(egui::Margin::symmetric(12, 7))
                            .show(ui, |ui| {
                                ui.with_layout(
                                    egui::Layout::left_to_right(egui::Align::TOP),
                                    |ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&p.text).color(ctx.pal.text),
                                            )
                                            .selectable(false),
                                        );
                                    },
                                );
                            });
                        match &p.error {
                            None => {
                                ui.label(
                                    RichText::new("sending…")
                                        .small()
                                        .weak(),
                                );
                            }
                            Some(err) => {
                                ui.label(
                                    RichText::new("not sent")
                                        .small()
                                        .color(ctx.pal.danger),
                                )
                                .on_hover_text(err);
                                if ui
                                    .add(
                                        egui::Button::new(RichText::new("Retry").small())
                                            .fill(ctx.pal.accent)
                                            .corner_radius(egui::CornerRadius::same(8)),
                                    )
                                    .clicked()
                                {
                                    ctx.actions.push(Action::RetrySend(p.cmid.clone()));
                                }
                                if ui
                                    .add(
                                        egui::Button::new(RichText::new("Discard").small())
                                            .fill(Color32::TRANSPARENT),
                                    )
                                    .clicked()
                                {
                                    ctx.actions
                                        .push(Action::DismissSend(p.cmid.clone()));
                                }
                            }
                        }
                    },
                );
            });
            ui.add_space(6.0);
            avatar(ui, ctx.self_name, 32.0);
        });
    });
    ui.add_space(9.0);
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
            .fill(ctx.pal.surface)
            .stroke(Stroke::new(1.0, ctx.pal.outline))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(10, 6))
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
            .fill(ctx.pal.surface)
            .stroke(Stroke::new(1.0, ctx.pal.warning))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(10, 6))
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
        ctx.pal.surface_hover
    } else {
        ctx.pal.surface
    };

    Frame::default()
        .fill(compose_fill)
        // One quiet surface — the old hairline border made the composer
        // read as a fourth box next to the input and send tiles.
        .stroke(Stroke::NONE)
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(8, 7))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(2.0);
                // Attach (Lucide paperclip, ghost style, animated hover).
                let attach = crate::ui::widgets::icon_button(
                    ui,
                    crate::theme::Icon::Paperclip,
                    "Attach a file",
                );
                if attach.clicked() {
                    ctx.actions.push(Action::Attach);
                }
                ui.add_space(2.0);

                // Input: the TextEdit's OWN frame does the styling —
                // recessed fill (extreme bg), quiet idle stroke, and the
                // accent focus ring comes from selection.stroke. No wrapper
                // box: the old Frame-inside-TextEdit was three mismatched
                // boxes in one row.
                let field = ui.add(
                    egui::TextEdit::singleline(draft)
                        .hint_text(if ctx.edit.is_some() {
                            "Edit message…"
                        } else {
                            "Type a message"
                        })
                        .desired_width(ui.available_width() - 44.0)
                        .min_size(egui::vec2(0.0, 32.0)),
                );
                let _ = field;

                ui.add_space(6.0);
                // Send: accent tile when armed, recessed when empty —
                // animated hover/press via the shared widget.
                let ready = !draft.trim().is_empty();
                let send_response = crate::ui::widgets::accent_icon_button(
                    ui,
                    crate::theme::Icon::Send,
                    "Send",
                    ctx.pal.accent,
                    ctx.pal.on_accent,
                    ready,
                );
                let enter =
                    field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (send_response.clicked() || enter) && ready {
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
    last_of_group: bool,
) {
    if own {
        // Right-aligned row: RTL places the avatar at the far right; the
        // bubble column sits to its left. Every part inside the column is
        // laid out right-anchored (see bubble_parts).
        // Own row: measure the text, give the bubble exactly that width
        // (shrink-to-fit, capped), right-aligned; avatar at the far right.
        let avail = ui.available_width() - 12.0;
        let cap = (avail * 0.85).min(680.0);
        let plain = crate::ui::widgets::plain_cached(&m.raw);
        let text_w = ui
            .painter()
            .layout(
                plain.as_str().to_owned(),
                egui::FontId::proportional(14.0),
                Color32::WHITE,
                f32::INFINITY,
            )
            .mesh_bounds
            .width();
        let slot_w = (text_w + 26.0).clamp(56.0, cap);
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            ui.allocate_ui(egui::vec2(slot_w, 10.0), |ui| {
                ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                    bubble_parts(ui, ctx, m, own, grouped, last_of_group);
                });
            });
            if !grouped {
                ui.add_space(6.0);
                avatar(ui, ctx.self_name, 32.0);
            } else {
                ui.add_space(38.0);
            }
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
                    let name = sender_label(m, ctx);
                    // Native Teams: sender names are secondary gray —
                    // accent blue is reserved for mentions and links.
                    if ui
                        .add(
                            egui::Label::new(
                                RichText::new(&name).small().strong().color(ctx.pal.secondary),
                            )
                            .selectable(false),
                        )
                        .on_hover_text("Contact card")
                        .clicked()
                    {
                        ctx.actions.push(Action::ShowContact {
                            mri: m.sender_mri.clone(),
                            name,
                        });
                    }
                }
                bubble_parts(ui, ctx, m, own, grouped, last_of_group);
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
    last_of_group: bool,
) {

    // Reactions chips (hover = who reacted; click = full list popup)
    if !m.reactions.is_empty() {
        if own {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                reaction_chips(ui, ctx, m, own);
            });
        } else {
            ui.horizontal(|ui| {
                reaction_chips(ui, ctx, m, own);
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
    // Connector/adaptive card: a Swift-b64 payload renders as a card;
    // the "cards.unsupported" stub (MS sends it when the client can't
    // display cards) shows a muted placeholder. Decode + id salt are
    // content-cached (b64 scan + JSON parse per frame is expensive).
    let (decoded_card, card_salt) = crate::ui::cards::extract_card_cached(&m.raw);

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
                // 12px body with a 3px corner toward the avatar — the
                // iMessage "tail hint": direction without a drawn tail.
                .corner_radius(egui::CornerRadius {
                    nw: 12,
                    ne: 12,
                    sw: 12,
                    se: 3,
                })
                .inner_margin(egui::Margin::symmetric(12, 7))
                .show(ui, |ui| {
                    if let Some(card) = decoded_card.as_ref() {
                        crate::ui::cards::render_card(ui, card, ctx.pal, ctx.actions, card_salt);
                        return;
                    }
                    if m.raw.contains("cards.unsupported") {
                        connector_card(ui, ctx.pal);
                        return;
                    }
                    // Own bubbles live in an RTL row — reset to LTR inside
                    // the frame so text is left-aligned, not centered.
                    ui.with_layout(
                        egui::Layout::left_to_right(egui::Align::TOP),
                        |ui| {
                            let segments = parse_html_cached(&m.raw);
                            let file_hits: std::rc::Rc<
                                std::cell::RefCell<Vec<(String, String)>>,
                            > = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
                            let hits2 = file_hits.clone();
                            render_segments(
                                ui,
                                &segments,
                                ctx.pal,
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
                nw: 12,
                ne: 12,
                sw: 3,
                se: 12,
            })
            .inner_margin(egui::Margin::symmetric(12, 7))
            .show(ui, |ui| {
                if let Some(card) = decoded_card.as_ref() {
                    crate::ui::cards::render_card(ui, card, ctx.pal, ctx.actions, card_salt);
                    return;
                }
                if m.raw.contains("cards.unsupported") {
                    connector_card(ui, ctx.pal);
                    return;
                }
                ui.with_layout(
                    egui::Layout::left_to_right(egui::Align::TOP),
                    |ui| {
                        let segments = parse_html_cached(&m.raw);
                        let file_hits: std::rc::Rc<
                            std::cell::RefCell<Vec<(String, String)>>,
                        > = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
                        let hits2 = file_hits.clone();
                        render_segments(
                            ui,
                            &segments,
                            ctx.pal,
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

    if last_of_group {
        let pinned = ctx.pinned.contains(&m.id);
        let seen_by: Option<String> = ctx.receipts.get(&m.id).map(|users| {
            let names: Vec<String> = users
                .iter()
                .map(|u| ctx.members.get(u).cloned().unwrap_or_else(|| "Someone".into()))
                .collect();
            names.join(", ")
        });
        if own {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(names) = seen_by {
                    ui.label(
                        RichText::new("Seen")
                            .small()
                            .weak()
                            .color(ctx.pal.ok),
                    )
                    .on_hover_text(format!("Seen by {names}"));
                }
                ui.label(
                    RichText::new(format_message_time(&m.timestamp)).small().weak(),
                );
                if pinned {
                    ui.add(
                        egui::Image::from_bytes(
                            crate::theme::Icon::Pin.uri(),
                            crate::theme::Icon::Pin.bytes(),
                        )
                        .tint(ctx.pal.secondary)
                        .fit_to_exact_size(egui::Vec2::splat(10.0)),
                    );
                }
            });
        } else {
            ui.label(RichText::new(format_message_time(&m.timestamp)).small().weak());
        }
    }

    // Floating action bar: anchored ABOVE the bubble, right-aligned to its
    // right edge, clamped inside the panel. Content-sized (no with_layout
    // stretch), colour emoji images, always fully clickable.
    let pointer2 = ui.input(|i| i.pointer.latest_pos());
    let bar_rect_prev: Option<egui::Rect> = ui.ctx().data(|d| d.get_temp(hover_id));
    let expand = |r: egui::Rect, by: f32| {
        egui::Rect::from_min_max(
            egui::pos2(r.left() - by, r.top() - by),
            egui::pos2(r.right() + by, r.bottom() + by),
        )
    };
    let in_bubble = pointer2
        .map(|p| expand(bubble_rect, 2.0).contains(p))
        .unwrap_or(false);
    let in_bar = pointer2
        .zip(bar_rect_prev)
        .map(|(p, r)| expand(r, 8.0).contains(p))
        .unwrap_or(false);
    // QA hook: force the bar open on the LAST message for screenshots
    // (headless runs have no reliable pointer position). Exclusive with
    // pointer hover — Xvfb's phantom pointer otherwise opens a second
    // bar on whatever it lands on and the shot shows both.
    let hook = std::env::var("TEAMSFAST_HOVER").as_deref() == Ok("1");
    let forced = hook
        && std::ptr::eq(m as *const _, ctx.messages.last().unwrap_or(m) as *const _);
    let open = if hook { forced } else { in_bubble || in_bar };

    if open {
        let clip = ui.clip_rect();
        // Emoji row (6 x 32) + separator + Reply/Edit/Delete/Copy.
        const BAR_W: f32 = 396.0;
        // Measured bar size from the previous frame (content-sized and
        // stable); BAR_W/48 cover the first frame. Anchoring with the real
        // width keeps the bar's right edge aligned to the bubble's.
        let (bar_w, bar_h) = bar_rect_prev
            .map(|r| (r.width(), r.height()))
            .unwrap_or((BAR_W, 48.0));
        // Right edge aligned to the bubble's; for INCOMING bubbles the bar
        // must not extend left into the avatar gutter (it read as
        // misaligned with the message).
        let min_x = if own {
            clip.left() + 8.0
        } else {
            (bubble_rect.left() - 8.0).max(clip.left() + 8.0)
        };
        let anchor_x = (bubble_rect.right() - bar_w)
            .max(min_x)
            .min(clip.right() - bar_w - 8.0);
        // Prefer above the bubble with a 4px gap. The old hardcoded 44px
        // offset was smaller than the real bar height, so the bar dug into
        // the bubble's first text line. When the bubble is clipped at the
        // viewport top (tall card), anchor tooltip-style just above the
        // pointer — the below-fallback covered the NEXT message's first
        // line there.
        let anchor_y = if bubble_rect.top() < clip.top() + 2.0 {
            pointer2
                .map(|p| (p.y - bar_h - 10.0).max(clip.top() + 4.0))
                .unwrap_or_else(|| (bubble_rect.bottom() + 6.0).min(clip.bottom() - bar_h - 4.0))
        } else if bubble_rect.top() - bar_h - 4.0 >= clip.top() + 2.0 {
            bubble_rect.top() - bar_h - 4.0
        } else {
            (bubble_rect.bottom() + 6.0).min(clip.bottom() - bar_h - 4.0)
        };
        let anchor = egui::pos2(anchor_x, anchor_y);
        let actions_sink: std::rc::Rc<std::cell::RefCell<Vec<Action>>> =
            std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let sink = actions_sink.clone();
        let bar = egui::Area::new(row_id.with("toolbar"))
            // Foreground, not Tooltip: tooltip-order layers are
            // click-through on this egui — every button in the bar was
            // dead (menus/popups use Foreground for the same reason).
            .order(egui::Order::Foreground)
            .fixed_pos(anchor)
            .show(ui.ctx(), |ui| {
                // ONE continuous pill: the old per-button bordered tiles
                // read as a debug toolbar stamped over the message, with
                // the text showing through the gaps. Stadium shape, soft
                // shadow, one hairline edge.
                Frame::default()
                    .fill(ctx.pal.surface)
                    // Full-strength outline hairline (gamma-multiplied it
                    // vanished on the light pill's white fill) + the soft
                    // shadow carry the separation in both themes.
                    .stroke(Stroke::new(1.0, ctx.pal.outline))
                    .corner_radius(CornerRadius::same(17))
                    .inner_margin(egui::Margin::symmetric(6, 3))
                    .shadow(egui::Shadow {
                        offset: [0, 4],
                        blur: 16,
                        spread: 0,
                        color: Color32::from_black_alpha(90),
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            for e in QUICK_REACTIONS {
                                // The emoji must live INSIDE the button:
                                // Button::image. (Button::new of a
                                // pre-added widget paints the emoji on
                                // the bar and leaves an empty chip.)
                                let btn = match emoji_image(ui, ctx, e, 20.0) {
                                    Some(img) => egui::Button::image(img),
                                    None => egui::Button::new(
                                        RichText::new((*e).to_string()).size(20.0),
                                    ),
                                }
                                .fill(Color32::TRANSPARENT)
                                // Transparent fill alone still draws the
                                // inactive bg_stroke — six ghost tiles
                                // inside the pill (same trap as the modal
                                // ✕ buttons).
                                .stroke(Stroke::NONE)
                                .min_size(egui::vec2(30.0, 28.0));
                                let r = ui.add(btn).on_hover_text("React");
                                if std::env::var_os("TEAMSFAST_LAYOUT_DEBUG").is_some() {
                                    eprintln!(
                                        "react btn rect={:?} hovered={} clicked={} contains_ptr={}",
                                        r.rect,
                                        r.hovered(),
                                        r.clicked(),
                                        r.contains_pointer()
                                    );
                                }
                                if r.clicked() {
                                    sink.borrow_mut().push(Action::React {
                                        message_id: m.id.clone(),
                                        emoji: (*e).to_string(),
                                        remove: false,
                                    });
                                }
                            }
                            // Divider between reactions and message actions.
                            let (div, _) = ui.allocate_exact_size(
                                egui::vec2(1.0, 22.0),
                                egui::Sense::hover(),
                            );
                            ui.painter().line_segment(
                                [div.center_top(), div.center_bottom()],
                                Stroke::new(
                                    1.0,
                                    crate::theme::tint_over(ctx.pal.surface, ctx.pal.secondary, 0.55),
                                ),
                            );
                            ui.add_space(2.0);
                            let icon_act = |ui: &mut egui::Ui,
                                            icon: crate::theme::Icon,
                                            tip: &str,
                                            on: &mut bool| {
                                let r = crate::ui::widgets::icon_button(ui, icon, tip);
                                if r.clicked() {
                                    *on = true;
                                }
                            };
                            let mut do_reply = false;
                            let mut do_forward = false;
                            let mut do_pin = false;
                            let mut do_edit = false;
                            let mut do_delete = false;
                            let mut do_copy = false;
                            icon_act(
                                ui,
                                crate::theme::Icon::Reply,
                                "Quote and reply",
                                &mut do_reply,
                            );
                            icon_act(ui, crate::theme::Icon::Forward, "Forward", &mut do_forward);
                            let pinned_here = ctx.pinned.contains(&m.id);
                            icon_act(
                                ui,
                                if pinned_here {
                                    crate::theme::Icon::PinOff
                                } else {
                                    crate::theme::Icon::Pin
                                },
                                if pinned_here {
                                    "Unpin message"
                                } else {
                                    "Pin message"
                                },
                                &mut do_pin,
                            );
                            if own {
                                icon_act(ui, crate::theme::Icon::Pencil, "Edit", &mut do_edit);
                                icon_act(ui, crate::theme::Icon::Trash, "Delete", &mut do_delete);
                            }
                            icon_act(ui, crate::theme::Icon::Copy, "Copy text", &mut do_copy);
                            if do_reply {
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
                            if do_forward {
                                sink.borrow_mut().push(Action::Forward(
                                    crate::ui::widgets::segs_to_plain(&parse_html(&m.raw)),
                                ));
                            }
                            if do_pin {
                                sink.borrow_mut().push(Action::TogglePinMessage {
                                    chat_id: ctx.chat_id.clone(),
                                    message_id: m.id.clone(),
                                });
                            }
                            if do_edit {
                                let current = crate::ui::widgets::segs_to_plain(
                                    &parse_html(&m.raw),
                                );
                                sink.borrow_mut().push(Action::StartEdit {
                                    message_id: m.id.clone(),
                                    current,
                                });
                            }
                            if do_delete {
                                sink.borrow_mut().push(Action::DeleteMessage(m.id.clone()));
                            }
                            if do_copy {
                                ui.ctx().copy_text(crate::ui::widgets::segs_to_plain(
                                    &parse_html(&m.raw),
                                ));
                            }
                        });
                    });
            })
            .response;
        // The bar's buttons collect into `sink` because they live in a
        // nested closure over `ui`; drain it here or the clicks vanish
        // (reply/forward/react/pin/edit/delete were silently dead).
        for a in actions_sink.borrow().iter() {
            ctx.actions.push(a.clone());
        }
        actions_sink.borrow_mut().clear();
        if std::env::var_os("TEAMSFAST_LAYOUT_DEBUG").is_some() {
            eprintln!(
                "hover bar msg={} rect={:?} bubble={:?} clip={clip:?}",
                &m.id
                    .chars()
                    .rev()
                    .take(6)
                    .collect::<String>(),
                bar.rect,
                bubble_rect
            );
        }
        ui.ctx()
            .data_mut(|d| d.insert_temp::<egui::Rect>(hover_id, bar.rect));
    } else if bar_rect_prev.is_some() {
        ui.ctx().data_mut(|d| d.remove::<egui::Rect>(hover_id));
    }
}

/// Muted rendering for connector/bot cards the client can't display
/// (Teams returns the "Card - access it on cards.unsupported" stub).
fn connector_card(ui: &mut egui::Ui, pal: &Palette) {
    Frame::default()
        .fill(pal.panel)
        .stroke(egui::Stroke::new(1.0, pal.outline))
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

/// Reaction count chips: colour emoji + names on hover; click opens the
/// full reactor list (and removes your own reaction when you were in it).
fn reaction_chips(ui: &mut egui::Ui, ctx: &mut ConvCtx<'_>, m: &MessageInfo, own: bool) {
    // Chip tint derives from the HOST bubble — a fixed bubble_in fill sat
    // invisible on the light incoming bubble (both white) and mismatched
    // the own indigo bubble.
    let base = if own {
        ctx.pal.bubble_out
    } else {
        ctx.pal.bubble_in
    };
    for r in &m.reactions {
        let names: Vec<String> = r.reactors.iter().map(|x| x.name.clone()).collect();
        let tip = if names.is_empty() {
            format!("{} reaction{}", r.count, if r.count == 1 { "" } else { "s" })
        } else {
            format!("{} by {}", r.emoji, names.join(", "))
        };
        let chip = Frame::default()
            .fill(crate::theme::tint_over(base, ctx.pal.text, 0.09))
            .stroke(Stroke::new(1.0, ctx.pal.outline.gamma_multiply(0.5)))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(6, 2))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    emoji_widget(ui, ctx, &r.emoji, 15.0);
                    ui.label(RichText::new(r.count.to_string()).small());
                });
            })
            .response;
        let chip = chip.on_hover_text(tip);
        if chip.clicked() {
            ctx.actions.push(Action::ShowReactions { emoji: r.emoji.clone(), names });
        }
    }
}

/// Quoted-parent preview above a reply.
fn quote_preview(ui: &mut egui::Ui, ctx: &mut ConvCtx<'_>, m: &MessageInfo) {
    if let Some(pid) = &m.reply_to {
        if let Some(parent) = ctx.messages.iter().find(|p| &p.id == pid) {
            Frame::default()
                .fill(Color32::from_black_alpha(16))
                .corner_radius(CornerRadius::same(4))
                .inner_margin(egui::Margin::symmetric(6, 2))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!(
                            "{}: {}",
                            sender_label(parent, ctx),
                            crate::ui::widgets::plain_cached(&parent.raw)
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

/// Rasterize (cached) and return a sized image for an emoji cluster.
/// Clusters whose raster failed — retried never (otherwise every frame
/// re-attempted the render and re-logged the warning).
fn emoji_failed(cluster: &str) -> bool {
    static FAILED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    FAILED
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(cluster)
}

fn emoji_mark_failed(cluster: &str) {
    static FAILED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    FAILED
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(cluster.to_string());
}

/// Does NOT add anything to a Ui — callers place it inside their widget
/// (a pre-added image paints on the parent and leaves its button empty).
fn emoji_image(
    ui: &egui::Ui,
    ctx: &mut ConvCtx<'_>,
    cluster: &str,
    px: f32,
) -> Option<egui::Image<'static>> {
    if !ctx.emoji_textures.contains_key(cluster) && !emoji_failed(cluster) {
        if let Some((rgba, size)) = crate::theme::raster_emoji(cluster, 64) {
            let img = egui::ColorImage::from_rgba_unmultiplied(
                [size[0], size[1]],
                &rgba,
            );
            let tex = ui
                .ctx()
                .load_texture(format!("emoji:{cluster}"), img, Default::default());
            ctx.emoji_textures.insert(cluster.to_string(), tex);
        } else {
            emoji_mark_failed(cluster);
        }
    }
    let tex = ctx.emoji_textures.get(cluster)?;
    let scale = px / tex.size()[1].max(1) as f32;
    let size = egui::vec2(
        tex.size()[0] as f32 * scale,
        tex.size()[1] as f32 * scale,
    );
    Some(egui::Image::new((tex.id(), size)))
}

/// Add the emoji widget directly to `ui` (reaction chips under messages).
fn emoji_widget(ui: &mut egui::Ui, ctx: &mut ConvCtx<'_>, cluster: &str, px: f32) {
    match emoji_image(ui, ctx, cluster, px) {
        Some(img) => {
            ui.add(img);
        }
        None => {
            ui.label(RichText::new(cluster).size(px));
        }
    }
}

/// Inline image: textured when loaded, fetch requested once, placeholder
/// otherwise (failed URLs stay in `pending_images` so they never retry).
fn show_image(ui: &mut egui::Ui, ctx: &mut ConvCtx<'_>, url: &str) -> bool {
    // Display box: width-capped at 340, height-capped at 460. Placeholder
    // reserves the SAME box (from the archive's decoded sizes) so an image
    // arriving mid-session doesn't reflow the conversation.
    let display_box = |w: usize, h: usize| -> egui::Vec2 {
        let scale = (340.0 / w as f32).min(460.0 / h as f32).min(1.0);
        egui::vec2(w as f32 * scale, h as f32 * scale)
    };
    if let Some((tex, size)) = ctx.textures.get(url) {
        let disp = display_box(size[0], size[1]);
        let resp = ui.add(egui::Image::new((tex.id(), disp)).sense(Sense::click()));
        if resp.clicked() {
            ctx.actions.push(Action::OpenImage(url.to_string()));
        }
        true
    } else {
        if ctx.pending_images.insert(url.to_string()) {
            ctx.actions.push(Action::FetchImage(url.to_string()));
        }
        let box_size = ctx
            .image_sizes
            .get(url)
            .map(|[w, h]| display_box(*w, *h))
            .unwrap_or(egui::vec2(340.0, 200.0));
        let (rect, _) =
            ui.allocate_exact_size(box_size, Sense::hover());
        ui.painter().rect_filled(rect, 6, ctx.pal.surface);
        ui.painter().rect_stroke(
            rect,
            6,
            Stroke::new(1.0, ctx.pal.outline),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "⏳ loading image…",
            egui::FontId::proportional(12.0),
            ctx.pal.secondary,
        );
        false
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Palette;

    fn test_message() -> MessageInfo {
        MessageInfo {
            id: "1111111111111111111".into(),
            sender_mri: "8:orgid:aaaa-bbbb".into(),
            sender: "Alice".into(),
            timestamp: "2026-10-09T10:00:00.000Z".into(),
            content: "hello there".into(),
            raw: "<p>hello there</p>".into(),
            reactions: vec![],
            reply_to: None,
            client_message_id: None,
        }
    }

    /// Regression: the hover bar's buttons collected actions into a RefCell
    /// sink that nothing drained — reply/forward/react/pin/edit/delete were
    /// silently dead. Sweep-click the forced-open bar and require actions.
    #[test]
    fn hover_bar_buttons_push_actions() {
        crate::emoji::setup();
        // Safety: no other test draws conversation rows concurrently.
        unsafe { std::env::set_var("TEAMSFAST_HOVER", "1") };
        unsafe { std::env::set_var("TEAMSFAST_LAYOUT_DEBUG", "1") };

        let messages = vec![test_message()];
        let empty: std::collections::HashMap<String, String> = HashMap::new();
        let textures: HashMap<String, (egui::TextureHandle, [usize; 2])> = HashMap::new();
        let image_sizes: HashMap<String, [usize; 2]> = HashMap::new();
        let mut pending_images: HashSet<String> = HashSet::new();
        let mut emoji_textures: HashMap<String, egui::TextureHandle> = HashMap::new();

        let egui_ctx = egui::Context::default();
        let draw = |egui_ctx: &egui::Context,
                    input: egui::RawInput,
                    emoji_textures: &mut HashMap<String, egui::TextureHandle>,
                    pending_images: &mut HashSet<String>|
         -> Vec<Action> {
            let mut actions: Vec<Action> = Vec::new();
            let mut out = egui_ctx.run_ui(input, |ui| {
                let mut ctx = ConvCtx {
                    chat_name: "Chat".into(),
                    chat_id: "19:chat@thread.v2".into(),
                    messages: &messages,
                    members: &empty,
                    self_name: "Me".into(),
                    self_id: Some("other-id"),
                    older_link: None,
                    loading_older: false,
                    typing_user: None,
                    edit: None,
                    reply: None,
                    uploads: &[],
                    pending: &[],
                    receipts: &empty2(),
                    pinned: &[],
                    can_call: false,
                    call_label: None,
                    textures: &textures,
                    image_sizes: &image_sizes,
                    pending_images,
                    emoji_textures,
                    pal: &Palette::dark(),
                    actions: &mut actions,
                };
                conversation_messages(ui, &mut ctx);
            });
            out.textures_delta.clear();
            actions
        };

        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let blank = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
        // Frame 1: lay out (hook forces the bar open on the last message).
        let _ = draw(&egui_ctx, blank.clone(), &mut emoji_textures, &mut pending_images);
        // Frame 2: the bar re-anchors with the measured height — settle so
        // the persisted rect is the one every later frame will paint.
        let _ = draw(&egui_ctx, blank, &mut emoji_textures, &mut pending_images);

        let bar = egui_ctx.data(|d| {
            d.get_temp::<egui::Rect>(egui::Id::new(("msg-hover", &messages[0].id)))
        })
        .expect("forced hover bar must persist its rect");

        // Frame 2..n: hover a spot, then press+release the next frame;
        // something must arrive.
        let mut got: Vec<Action> = Vec::new();
        for i in 1..20 {
            let x = bar.left() + bar.width() * (i as f32 / 20.0);
            let y = bar.center().y;
            let pos = egui::pos2(x, y);
            let hover = egui::RawInput {
                screen_rect: Some(screen),
                events: vec![egui::Event::PointerMoved(pos)],
                ..Default::default()
            };
            let _ = draw(&egui_ctx, hover, &mut emoji_textures, &mut pending_images);
            let press = egui::RawInput {
                screen_rect: Some(screen),
                events: vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                }],
                ..Default::default()
            };
            let _ = draw(&egui_ctx, press, &mut emoji_textures, &mut pending_images);
            let release = egui::RawInput {
                screen_rect: Some(screen),
                events: vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }],
                ..Default::default()
            };
            got.extend(draw(&egui_ctx, release, &mut emoji_textures, &mut pending_images));
        }
        unsafe { std::env::remove_var("TEAMSFAST_HOVER") };
        unsafe { std::env::remove_var("TEAMSFAST_LAYOUT_DEBUG") };
        assert!(
            !got.is_empty(),
            "clicks across the hover bar must produce actions (sink drained)"
        );
    }

    fn empty2() -> std::collections::HashMap<String, Vec<String>> {
        HashMap::new()
    }
}
