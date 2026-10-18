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
use crate::theme::RichTextBold;

const QUICK_REACTIONS: [&str; 6] = ["👍", "❤️", "😂", "😮", "😢", "🎉"];

/// The hover bar's quick set: the user's favorites (settings), or the
/// shipped default six while unset. Constrained to single-grapheme
/// emoji the reaction wire knows (workstream F).
fn quick_set(favorites: &[String]) -> Vec<String> {
    if favorites.is_empty() {
        QUICK_REACTIONS.iter().map(|s| s.to_string()).collect()
    } else {
        favorites.to_vec()
    }
}

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
    /// Mark-read rides with OpenChat today; kept for explicit calls.
    #[allow(dead_code)]
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
    /// Settings opens through the gear/top-bar action; kept for parity.
    #[allow(dead_code)]
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
    /// Open the pre-join dialog (preview + modality toggles) for a
    /// meeting — calendar/meet-now joins go through it, like native.
    PreJoin { source: String, label: Option<String> },
    JoinMeeting {
        source: String,
        label: Option<String>,
        camera: bool,
        mic: bool,
    },
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
    /// Open the create-channel dialog with this team preselected (the
    /// per-team affordance from a team row's "+" / context menu).
    NewChannelInTeam(String),
    DeleteChannel {
        team_id: String,
        channel_id: String,
    },
    /// Show this chat's shared files in the Files section.
    ShowChatFiles(String),
    /// Leave the per-chat files scope in the Files section.
    CloseChatFiles,
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
    /// The conversation view applied the prepend scroll adjustment —
    /// clears App's one-shot flag.
    PrependAnchored,
    /// Jump to (scroll to) a message in this conversation; loads older
    /// pages until it is in view (bounded).
    JumpToMessage(String),
    /// The conversation view performed the jump scroll — clears the
    /// app-side target.
    JumpPerformed,
    /// Unpin a message from this chat (the pins bar's per-entry ✕).
    UnpinMessage(String),
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
    /// Message id the view should scroll to (jump-to-message); the row
    /// loop consumes it when the target row is laid out.
    pub jump_target: Option<&'a str>,
    /// User's favorite reaction emoji (settings; empty = default six).
    pub reaction_favorites: &'a [String],
    /// STABLE 1:1-callability facts only (chat id shape — spaces 1:1 that
    /// is not a meeting thread). Transient data (the roster) must not
    /// gate button PRESENCE: while the roster is pending the buttons
    /// render DISABLED instead of vanishing (open/refresh blink report).
    pub can_call: bool,
    /// First page of this conversation is still in flight (no cached
    /// content painted) — the pane shows a centered loader.
    pub conv_loading: bool,
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
    /// Any modal overlay (settings / new chat / join / teams / forward) is
    /// open: message hover bars must not render — they live in the same
    /// Foreground order as egui::Modal and otherwise z-fight above it
    /// (reachable for real via hover-message + Ctrl+N).
    pub overlays_open: bool,
    /// App set this when a load-older prepend just applied (chat id):
    /// after this render, shift the scroll offset by the content-height
    /// delta so the viewport stays on the same messages. Stale flags for
    /// OTHER chats are dropped here (never shift the wrong conversation).
    pub prepend_adjust: Option<String>,
    pub actions: &'a mut Vec<Action>,
}

/// Conversation header + message list (the center panel).
pub fn conversation_messages(ui: &mut egui::Ui, ctx: &mut ConvCtx<'_>) {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        avatar(ui, &ctx.chat_name, 32.0);
        ui.add_space(2.0);
        // Truncating, width-budgeted title: a long name (or the raw
        // thread id before the roster resolves it) previously overflowed
        // the header row, growing the central panel's min_rect past the
        // screen — the wrap width then differed between paints and rows
        // re-wrapped (run-5 layout-shift forensics). The budget keeps
        // room for the trailing header controls.
        let name_w = (ui.available_width() - 250.0)
            .max(24.0)
            .min(ui.available_width());
        ui.add_sized(
            [name_w, 20.0],
            egui::Label::new(RichText::new(&ctx.chat_name).bold().size(17.0))
                .selectable(false)
                .truncate(),
        );
        // Pinned messages: the compact BAR lives under the header (see
        // below); this chip opens the per-pin list (jump + unpin).
        if !ctx.pinned.is_empty() {
            ui.menu_button(
                RichText::new(format!("📌 {}", ctx.pinned.len())).small(),
                |ui| {
                    ui.set_min_width(280.0);
                    ui.strong("Pinned messages");
                    ui.separator();
                    for id in ctx.pinned.to_vec() {
                        let preview: String = ctx
                            .messages
                            .iter()
                            .find(|m| m.id == *id)
                            .map(|m| {
                                format!(
                                    "{}: {}",
                                    sender_label(m, ctx),
                                    crate::ui::widgets::segs_to_plain(&parse_html(&m.raw))
                                        .lines()
                                        .next()
                                        .unwrap_or("")
                                )
                            })
                            .unwrap_or_else(|| "Pinned message (outside the loaded page)".into());
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Label::new(
                                        RichText::new(preview).small().color(ctx.pal.secondary),
                                    )
                                    .selectable(false)
                                    .truncate(),
                                )
                                .on_hover_text("Jump")
                                .clicked()
                            {
                                ctx.actions.push(Action::JumpToMessage(id.clone()));
                            }
                            if ui
                                .add(
                                    egui::Button::new(RichText::new("✕").small())
                                        .fill(egui::Color32::TRANSPARENT)
                                        .stroke(egui::Stroke::NONE),
                                )
                                .on_hover_text("Unpin")
                                .clicked()
                            {
                                ctx.actions.push(Action::UnpinMessage(id.clone()));
                            }
                        });
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
            // Button PRESENCE keys on stable facts only (ctx.can_call =
            // chat id shape). The roster decides enabled-ness: while it
            // is still loading the buttons render grayed-out instead of
            // vanishing (they used to blink out on every open/refresh —
            // empty members read as "bot-only chat" and hid them).
            // Once the roster lands, a bot-only (connector DM) thread
            // hides them for good — there is no human to call.
            let bot_only = !ctx.members.is_empty()
                && ctx.members.keys().all(|m| m.starts_with("28:"));
            if !bot_only {
                let ready = !ctx.members.is_empty();
                ui.add_enabled_ui(ready, |ui| {
                    let call = ui.add(
                        egui::Button::new(crate::theme::Icon::Phone.image(ctx.pal.secondary, 15.0))
                            .fill(egui::Color32::TRANSPARENT),
                    );
                    let video = ui.add(
                        egui::Button::new(crate::theme::Icon::Video.image(ctx.pal.secondary, 15.0))
                            .fill(egui::Color32::TRANSPARENT),
                    );
                    if ready {
                        if call
                            .on_hover_text("Call (audio)")
                            .clicked()
                        {
                            ctx.actions.push(Action::StartCall(ctx.chat_id.clone()));
                        }
                        if video
                            .on_hover_text("Call with camera")
                            .clicked()
                        {
                            ctx.actions.push(Action::StartVideoCall(ctx.chat_id.clone()));
                        }
                    } else {
                        let _ = call.on_disabled_hover_text("Loading chat…");
                        let _ = video.on_disabled_hover_text("Loading chat…");
                    }
                });
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
        // Native Teams only offers the jump when the view is actually
        // scrolled up — an always-on pill is dead chrome that also steals
        // the header width the typing indicator needs. The at-bottom flag
        // is computed after the message ScrollArea each frame (one-frame
        // lag, refreshed beside the scroll state it derives from); while
        // someone is typing the pill yields its slot entirely.
        let at_bottom: bool = ui
            .ctx()
            .data_mut(|d| d.get_temp(egui::Id::new("conv_at_bottom").with(ctx.chat_id.as_str())))
            .unwrap_or(true);
        if ctx.older_link.is_some() && !at_bottom && ctx.typing_user.is_none() {
            if ui
                .small_button(RichText::new("⇩ Latest").small())
                .on_hover_text("Reload the newest messages")
                .clicked()
            {
                ctx.actions.push(Action::JumpLatest);
            }
        }
        if let Some(user) = ctx.typing_user {
            // Truncate to what's left of the header row: with the call/
            // files controls present, a long name overflowed ON TOP of
            // them and clipped at the pane edge (battery P1). Prefer the
            // first name over a mid-word ellipsis when that fits.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let full = format!("{user} is typing…");
                let first = format!(
                    "{} is typing…",
                    user.split_whitespace().next().unwrap_or(user)
                );
                let small = egui::FontId::proportional(12.0);
                let w = |t: &str| {
                    ui.painter()
                        .layout_no_wrap(t.to_owned(), small.clone(), ctx.pal.link)
                        .mesh_bounds
                        .width()
                };
                let avail = ui.available_width();
                let bare = "typing…";
                let label = if w(&full) <= avail {
                    Some(full)
                } else if w(&first) <= avail {
                    Some(first)
                } else if w(bare) <= avail {
                    // Ultra-narrow header: the first-name form truncates
                    // to a meaningless fragment ("Souh…" — narrow-window
                    // battery). A bare "typing…" keeps the signal while
                    // it fits; below that, nothing (blank beats a
                    // hard-clipped "typin…").
                    Some(bare.to_string())
                } else {
                    None
                };
                if let Some(label) = label {
                    ui.add(
                        egui::Label::new(RichText::new(label).small().color(ctx.pal.link))
                            .truncate(),
                    );
                }
            });
        }
    });
    // Pinned-messages bar (workstream E): compact row between the header
    // and the separator — preview of the newest pin, click to jump (the
    // jump loads older pages until the row is in view). The bar appears
    // only while pins exist (user-initiated height change, like the
    // format toolbar).
    if !ctx.pinned.is_empty() {
        let newest = ctx.pinned.last().cloned().unwrap_or_default();
        let preview: String = ctx
            .messages
            .iter()
            .find(|m| m.id == newest)
            .map(|m| {
                format!(
                    "{}: {}",
                    sender_label(m, ctx),
                    crate::ui::widgets::segs_to_plain(&parse_html(&m.raw))
                        .lines()
                        .next()
                        .unwrap_or("")
                )
            })
            .unwrap_or_else(|| "Pinned message".into());
        let bar = Frame::default()
            .fill(crate::theme::tint_over(ctx.pal.panel, ctx.pal.text, 0.04))
            .stroke(Stroke::new(1.0, ctx.pal.outline))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(8, 3))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Image::from_bytes(
                            crate::theme::Icon::Pin.uri(),
                            crate::theme::Icon::Pin.bytes(),
                        )
                        .tint(ctx.pal.secondary)
                        .fit_to_exact_size(egui::Vec2::splat(11.0)),
                    );
                    ui.add_space(4.0);
                    if ui
                        .add(
                            egui::Label::new(
                                RichText::new(preview).small().color(ctx.pal.secondary),
                            )
                            .selectable(false)
                            .truncate(),
                        )
                        .on_hover_text("Jump to pinned message")
                        .clicked()
                    {
                        ctx.actions.push(Action::JumpToMessage(newest.clone()));
                    }
                });
            })
            .response
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let _ = bar;
    }
    // The separator doubles as the anchor line for the sticky-day chip
    // (native Teams pins the visible day in the header region while the
    // view is scrolled up). The chip itself paints after the ScrollArea —
    // later paint wins over the topmost message's clipped sliver.
    let sep_line = ui.separator().rect;
    let sticky_day: Option<String> = {
        // One-frame lag like the "⇩ Latest" pill: the at-bottom flag and
        // the topmost visible day are computed AFTER the scroll area.
        let at_bottom_prev: bool = ui
            .ctx()
            .data_mut(|d| d.get_temp(egui::Id::new("conv_at_bottom").with(ctx.chat_id.as_str())))
            .unwrap_or(true);
        if at_bottom_prev {
            None
        } else {
            ui.ctx()
                .data_mut(|d| d.get_temp(egui::Id::new("conv_top_day").with(ctx.chat_id.as_str())))
                .flatten()
        }
    };

    // Oldest at the top, newest pinned to the bottom — Teams style.
    // (History note: a non-zero ScrollArea offset once painted blank here;
    // that turned out to be our panel-order bug — composer rendered AFTER
    // CentralPanel consumed the space — not an egui defect. stick_to_bottom
    // is verified rendering again, see bisect in git history.)
    if crate::app::qa_flag("TEAMSFAST_RECTLOG") {
        eprintln!("PRESCROLL aw={:.0} maxrect={:?}", ui.available_width(), ui.max_rect());
    }
    // Stable id: the prepend offset-adjust below needs to address the
    // ScrollArea's persistent state directly.
    const CONV_SCROLL_SALT: &str = "conv_msgs_scroll";
    let mut area = ScrollArea::vertical()
        .auto_shrink(false)
        .id_salt(CONV_SCROLL_SALT);
    // NB: this egui style runs FLOATING scroll bars (scroll.floating=true,
    // verified by probe) — they reserve ZERO content width, so bar
    // visibility never affects wrap width here. Do not "fix" width
    // breathing via scroll_bar_visibility; it does nothing under floating.
    area = area.stick_to_bottom(true);
    // QA hook: drive a synthetic scroll through the loaded history (a few
    // thousand px per frame downward from the bottom) and let the caller
    // time the frame — TEAMSFAST_SCROLLBENCH=1 + the SCROLLBENCH log lines.
    if std::env::var("TEAMSFAST_SCROLLBENCH").as_deref() == Ok("1") {
        static BENCH_FRAME: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(0);
        let f = BENCH_FRAME
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            as f32;
        // stick_to_bottom re-pins the offset on EVERY content change —
        // under live traffic the bench's offset drive never moved the
        // view at all (probe-verified run 6: state offset pinned at
        // bottom through the whole "sweep"). Disable the stick while
        // benching so the drive actually sweeps.
        area = area.stick_to_bottom(false);
        // 300 settle frames at the bottom, then a steady upward scroll
        // through the loaded history (~150px/frame; the offset clamps at
        // the content top and stays there).
        let off = (f - 300.0).max(0.0) * 150.0;
        if f > 300.0 {
            area = area.vertical_scroll_offset((30_000.0 - off).max(0.0));
        }
        // The event-driven loop only paints on events — the bench must
        // keep the frames coming (QA hook only).
        ui.ctx().request_repaint();
    }
    // Same-frame bottom pin after a full page replace: DROPPED (adversarial
    // review, empirically verified vs egui 0.36.2). Writing the state
    // offset BEFORE begin cannot work: begin does NOT clamp (content laid
    // at −offset; f32::MAX overflows round_ui to −inf), every row culls,
    // and the stick re-pin then computes NaN — which f32::max collapses
    // to 0, parking the view at the TOP of history with the stick lost
    // (unrecoverable except by hand). The new content's max offset is
    // unknowable before layout, so the replace frame keeps the old
    // offset and the STICK re-pins one frame later (a single mid-content
    // straddle frame — acceptable, self-correcting).
    // Screen-space bottom of the FINAL row — the observable at-bottom
    // signal for the header's "⇩ Latest" pill (egui's State.offset tracks
    // the committed offset, not the rendered offset_target; see the
    // prepend-adjust note for the same trap).
    let mut last_row_bottom = f32::NEG_INFINITY;
    // Day of the topmost VISIBLE row (feeds the header's sticky-day chip).
    let mut top_day: Option<String> = None;
    // Jump-to-message: the row loop records the target row's rect; the
    // state-write below centers it (scroll_to_rect alone loses to the
    // stick's content-change re-pin — the SCROLLUP lesson).
    let jump_pending = ctx.jump_target.map(str::to_string);
    let mut jump_row: Option<egui::Rect> = None;
    let scroll_out = area.show(ui, |ui| {
        if crate::app::qa_flag("TEAMSFAST_RECTLOG") {
            let mr = ui.max_rect();
            eprintln!(
                "RECTFRAME panel_aw={:.0} maxrect_w={:.0} screen_w={:.0} left={:.0} right={:.0} name_len={} name={:?}",
                ui.available_width(),
                mr.width(),
                ui.input(|i| i.content_rect().width()),
                mr.left(),
                mr.right(),
                ctx.chat_name.chars().count(),
                ctx.chat_name.chars().take(30).collect::<String>()
            );
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
        // Row culling: egui's ScrollArea clips PAINTING but still allocates
        // and lays out every row. With a few hundred messages loaded that is
        // hundreds of widget trees per frame. Rows whose (previous-frame)
        // height places them fully outside the view ± margin get a cheap
        // placeholder allocation instead — same flow, same scroll extent
        // (sizes self-heal when a row scrolls into view and renders).
        // 600 (SCROLLBENCH, 50-msg page): p95 1.84ms vs 2.82ms at 240 —
        // more rows pre-rendered = fewer in-view height corrections, and
        // the per-frame cost stays ~1ms median.
        let clip = ui.clip_rect();
        const CULL_MARGIN: f32 = 600.0;
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
            last_row_bottom = last_row_bottom.max(top + known.map(|k| k.y).unwrap_or(40.0));
            // Topmost visible row's day: the first row whose extent reaches
            // past the viewport's top edge (rows above it end above it).
            // Runs before the cull so culled rows above the view are
            // skipped by the geometry test, not by the branch. `last_day`
            // is already this row's day — separators paint ahead of the
            // first row of each day. Empty label = unparseable timestamp
            // (format_day_label's failure shape): capture nothing — the
            // chip would paint an empty pill.
            if top_day.is_none() && !last_day.is_empty() {
                let h_est = known.map(|k| k.y).unwrap_or(40.0);
                if top + h_est > clip.top() {
                    top_day = Some(last_day.clone());
                }
            }
            if let Some(sz) = known
                && (top > clip.bottom() + CULL_MARGIN
                    || top + sz.y < clip.top() - CULL_MARGIN)
            {
                if jump_pending.as_deref() == Some(m.id.as_str()) {
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(clip.left(), top),
                        egui::vec2(clip.width(), sz.y),
                    );
                    // Repeated EXPLICIT scroll requests are the mechanism
                    // that escapes stick_to_bottom: an explicit adjustment
                    // skips the end-of-frame re-pin AND suppresses the
                    // stuck-flag recompute (a raw state write fights the
                    // re-pin forever — probe-verified oscillation).
                    ui.scroll_to_rect(rect, Some(egui::Align::Center));
                    jump_row = Some(rect);
                }
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
            let row_avail = ui.available_width();
            // Teams attributes the timestamp (and seen-by) to the LAST
            // bubble of a run, not the first.
            let last_of_group = ctx.messages.get(i + 1).is_none_or(|n| {
                !groups_with(n, m)
            });
            if jump_pending.as_deref() == Some(m.id.as_str()) {
                let h0 = ui.cursor().top() - top;
                let rect = egui::Rect::from_min_size(
                    egui::pos2(clip.left(), top),
                    egui::vec2(clip.width(), h0.max(40.0)),
                );
                ui.scroll_to_rect(rect, Some(egui::Align::Center));
                jump_row = Some(rect);
            }
            message_row(ui, ctx, m, own, grouped, last_of_group);
            // Record the rendered height for culling. The flow adds item
            // spacing on top of each allocation, so strip it back out — the
            // placeholder gets its own spacing when allocated.
            let h = (ui.cursor().top() - top - ui.spacing().item_spacing.y).max(0.0);
            last_row_bottom = last_row_bottom.max(top + h);
            if h > 0.0 {
                ui.ctx()
                    .data_mut(|d| d.insert_temp(row_id, egui::vec2(0.0, h)));
            }
            // QA: per-row rect stability log (TEAMSFAST_RECTLOG=1). One
            // line per rendered row per frame — diffing the cached-paint
            // frame against the network-settled frame proves whether rows
            // moved (layout shift) without pixel-guessing.
            if crate::app::qa_flag("TEAMSFAST_RECTLOG") {
                let tail: String = m.id.chars().rev().take(6).collect();
                eprintln!(
                    "RECT {tail} top={top:.1} h={h:.1} rt={} rx={} rawlen={} g={} last={} aw={:.0}",
                    m.reply_to.is_some(),
                    m.reactions.len(),
                    m.raw.len(),
                    grouped,
                    last_of_group,
                    row_avail
                );
            }
        }
        // Own sends in flight / failed: translucent bubble, Retry on error.
        for p in ctx.pending {
            pending_bubble(ui, ctx, p);
        }
    });
    // QA hook: auto-jump to the newest pinned message a few seconds after
    // open (TEAMSFAST_JUMP=1) — exercises the full pins-bar path incl.
    // load-until-found when the pin is outside the first page.
    if crate::app::qa_flag("TEAMSFAST_JUMP") {
        static SEEN_AT: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        static FIRED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        let ready = matches!(SEEN_AT.get_or_init(std::time::Instant::now), t if t.elapsed() > std::time::Duration::from_secs(3));
        if ready
            && !FIRED.swap(true, std::sync::atomic::Ordering::Relaxed)
            && let Some(id) = ctx.pinned.last().cloned()
        {
            ctx.actions.push(Action::JumpToMessage(id));
        }
        // The frame loop is event-driven — a quiet chat produces no
        // frames after the 3s mark, so the hook pumps its own repaints
        // until it fires (the SCROLLBENCH pattern).
        if !FIRED.load(std::sync::atomic::Ordering::Relaxed) {
            ui.ctx().request_repaint();
        }
    }
    if crate::app::qa_flag("TEAMSFAST_RECTLOG") {
        eprintln!(
            "SCROLLAW inner_w={:.0} content_h={:.0} floating={} central_min_right={:.0}",
            scroll_out.inner_rect.width(),
            scroll_out.content_size.y,
            ui.spacing().scroll.floating,
            ui.min_rect().right()
        );
    }

    // Prepend anchoring: egui's scroll offset is measured from the
    // content TOP, so history prepended above the view slides everything
    // the user is reading down by the prepended height (a 50-message page
    // is ~20k px — the reading position is lost entirely). After the
    // prepend's first render we know the height delta; shift the offset
    // by it and repaint (the state write is consumed by the NEXT begin —
    // the calendar scroll-to-today lesson).
    let grew = {
        let h_key = egui::Id::new("conv_content_h").with(ctx.chat_id.as_str());
        let prev_h: Option<f32> = ui.ctx().data_mut(|d| d.get_temp(h_key));
        let h2 = scroll_out.content_size.y;
        let grew = prev_h.is_some_and(|p| h2 > p + 0.5);
        let armed = ctx.prepend_adjust.take();
        if let Some(arm_chat) = armed
            && arm_chat == ctx.chat_id
            && let Some(h1) = prev_h
            && h2 > h1
        {
            let state_id = ui.make_persistent_id(egui::IdSalt::new(CONV_SCROLL_SALT));
            if let Some(mut state) = egui::scroll_area::State::load(ui.ctx(), state_id) {
                // NB: an in-flight ANIMATED scroll (JumpLatest/scroll_to)
                // can lerp back to its target next frame and discard this
                // adjustment — egui's State.offset_target is private with
                // no setter, so this race stays open (prepend-during-jump
                // is rare; the buttons sit at opposite ends of the list).
                if crate::app::qa_flag("TEAMSFAST_RECTLOG") {
                    eprintln!(
                        "PREPENDADJUST h1={h1:.0} h2={h2:.0} delta={} offset={:.0}",
                        h2 - h1,
                        state.offset.y
                    );
                }
                state.offset.y += h2 - h1;
                ui.ctx().data_mut(|d| d.insert_temp(state_id, state));
            }
            ui.ctx().request_repaint();
            ctx.actions.push(Action::PrependAnchored);
        }
        ui.ctx().data_mut(|d| d.insert_temp(h_key, h2));
        grew
    };

    // At-bottom flag for the header's "⇩ Latest" pill (native shows the
    // jump affordance only when the view is scrolled up). offset is
    // content-relative from the top, so "at bottom" is offset + viewport
    // reaching the content height.
    let at_bottom = {
        let key = egui::Id::new("conv_at_bottom").with(ctx.chat_id.as_str());
        let prev: Option<bool> = ui.ctx().data_mut(|d| d.get_temp(key));
        // 40px slack: stick_to-bottom parks the last row's bottom AT the
        // viewport bottom; short conversations park it above.
        let at_bottom = effective_at_bottom(
            last_row_bottom.is_finite()
                && last_row_bottom <= scroll_out.inner_rect.bottom() + 40.0,
            grew,
            prev,
        );
        if prev != Some(at_bottom) {
            // The header's "⇩ Latest" pill reads this one frame late; on
            // the event-driven loop nothing repaints when the flag flips,
            // so the pill (and its disappearance) would never paint.
            ui.ctx().request_repaint();
        }
        ui.ctx().data_mut(|d| d.insert_temp(key, at_bottom));
        at_bottom
    };
    // Jump driver: repeat-write the scroll state until the target row
    // is centered on a STABLE frame (content not growing — the stick
    // re-pins on every content change and would eat a one-shot). This is
    // the same mechanism that makes TEAMSFAST_SCROLLUP hold.
    if let Some(rect) = jump_row
        && let Some(state_id) = Some(ui.make_persistent_id(egui::IdSalt::new(CONV_SCROLL_SALT)))
        && let Some(st) = egui::scroll_area::State::load(ui.ctx(), state_id)
    {
        // Landed when the live offset centers the target row on a stable
        // frame. egui's scroll_to_rect centers the rect's CENTER — the
        // expected offset must include half the row height or the check
        // never passes (a permanent repaint loop + the pending scroll
        // hijacking every later scroll; adversarial review P1).
        let content_top = rect.top() - scroll_out.inner_rect.top() + st.offset.y;
        let target =
            (content_top + rect.height() / 2.0 - scroll_out.inner_rect.height() / 2.0).max(0.0);
        if !grew && (st.offset.y - target).abs() < 2.0 {
            ctx.actions.push(Action::JumpPerformed);
        } else {
            ui.ctx().request_repaint();
        }
    }
    // Sticky-day chip state (native Teams shows the visible day in the
    // header region while scrolled). Stored only for the header's NEXT
    // frame — one-frame lag, same contract as conv_at_bottom. Repaint on
    // change so the chip tracks day boundaries on the event-driven loop.
    {
        let day_key = egui::Id::new("conv_top_day").with(ctx.chat_id.as_str());
        let new_day = if at_bottom { None } else { top_day };
        let prev: Option<String> = ui.ctx().data_mut(|d| d.get_temp(day_key));
        if prev != new_day {
            ui.ctx().request_repaint();
        }
        ui.ctx().data_mut(|d| d.insert_temp(day_key, new_day));
    }
    if let Some(day) = sticky_day.as_deref() {
        crate::ui::widgets::sticky_day_chip(ui.painter(), ctx.pal, sep_line, day);
    }

    // First-page loader: opening a chat with no cached content left the
    // pane a pure void until the worker's page arrived (open-blink
    // report). The pane is never cleared on refresh — this only paints
    // while genuinely empty + in flight. Spinner via ui.put at an
    // absolute rect (ScrollArea-adjacent centering must be painter-style;
    // layout centering collapses available rects), animated by the tick
    // gate (conv_loading).
    if ctx.conv_loading && ctx.messages.is_empty() && ctx.pending.is_empty() {
        let rect = scroll_out.inner_rect;
        let center = egui::pos2(rect.center().x, rect.center().y.max(rect.top() + 80.0));
        ui.put(
            egui::Rect::from_center_size(egui::pos2(center.x, center.y - 14.0), egui::vec2(28.0, 28.0)),
            egui::Spinner::new().size(28.0),
        );
        ui.painter().text(
            egui::pos2(center.x, center.y + 20.0),
            egui::Align2::CENTER_CENTER,
            "Loading messages…",
            egui::FontId::proportional(13.0),
            ctx.pal.secondary,
        );
    }

    // QA hook: force the conversation to a scrolled-up position once the
    // history is loaded (TEAMSFAST_SCROLLUP=1). The SCROLLBENCH offset
    // drive is re-pinned by stick_to_bottom every frame — the prepend
    // adjust's direct state write is the mechanism that actually holds.
    // NB under this flag the repeating write overrides the prepend anchor
    // and the Latest pill's scroll-to animation same-frame — don't read
    // SCROLLUP shots as prepend/jump QA.
    if crate::app::qa_flag("TEAMSFAST_SCROLLUP") {
        static SEEN_AT: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        // Arm late: firing on the cached paint lets the arriving network
        // page grow the content and re-pin the view before the shot. The
        // write REPEATS every frame (a one-shot raced the network
        // full-replace: stick_to_bottom re-pins on every content change,
        // so a late reply ate the written offset — run-7 forensics).
        // The height gate must clear offset 2000 + viewport: egui clamps
        // the state offset to max-scroll, so a chat in (1200, ~2700]px
        // would pass the gate yet render AT BOTTOM (false "hook broken").
        let ready = match SEEN_AT.get_or_init(std::time::Instant::now) {
            t => t.elapsed() > std::time::Duration::from_secs(3),
        };
        if ready && scroll_out.content_size.y > 2800.0 {
            let state_id = ui.make_persistent_id(egui::IdSalt::new(CONV_SCROLL_SALT));
            if let Some(mut st) = egui::scroll_area::State::load(ui.ctx(), state_id) {
                st.offset.y = 2000.0;
                ui.ctx().data_mut(|d| d.insert_temp(state_id, st));
            }
            ui.ctx().request_repaint();
        }
    }
}

/// At-bottom with the growth latch: egui re-pins stick_to_bottom at
/// ScrollArea::END, so the one frame where content GREW renders at the
/// stale offset and geometry alone says "not at bottom" (an incoming
/// message taller than the 40px slack flashed the Latest pill and the
/// sticky-day chip for a frame under live traffic). Treat growth while
/// previously at bottom as still-at-bottom; the next frame's geometry
/// confirms.
fn effective_at_bottom(geom: bool, grew_since_prev: bool, prev: Option<bool>) -> bool {
    geom || (grew_since_prev && prev == Some(true))
}

/// Own message bubble for a send that hasn't been confirmed by the server
/// yet (translucent) or that failed (danger border + Retry / Discard).
fn pending_bubble(ui: &mut egui::Ui, ctx: &mut ConvCtx<'_>, p: &PendingBubble) {
    // Own sends: translucent bubble right-aligned in the SAME cluster as
    // confirmed own rows (12px margin + 32px avatar + gap), so the
    // pending→confirmed swap never jumps horizontally; on error the
    // status + Retry/Discard render UNDER it — beside the bubble
    // (right-to-left) a wide bubble pushed them off the pane's left edge
    // entirely (the run-4 error-path QA caught Retry invisible, "not
    // sent" clipped to "nt").
    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
        // Slot math hoisted from the entry width (== message_row's avail
        // basis): inside the cluster the remaining width is smaller, which
        // would re-cap wide drafts below the confirmed slot.
        let avail = ui.available_width().max(40.0) - 12.0;
        let text_w = ui
            .painter()
            .layout(
                p.text.clone(),
                egui::FontId::proportional(14.0),
                Color32::WHITE,
                f32::INFINITY,
            )
            .mesh_bounds
            .width();
        let slot_w = own_slot_width(text_w, avail);
        ui.add_space(12.0);
        avatar(ui, ctx.self_name, 32.0);
        ui.add_space(6.0);
        ui.vertical(|ui| {
            let (fill, stroke) = match &p.error {
                None => (ctx.pal.bubble_out, egui::Stroke::NONE),
                Some(_) => (ctx.pal.bubble_out, egui::Stroke::new(1.0, ctx.pal.danger)),
            };
            let frame = egui::Frame::default()
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
                .inner_margin(egui::Margin::symmetric(12, 7));
            frame.show(ui, |ui| {
                // Same slot math as the confirmed own row so the
                // pending→confirmed swap never changes the bubble's width.
                ui.set_max_width(slot_w.max(24.0));
                ui.set_min_width(24.0);
                // Lite-render the draft's markers (bold/italic/links) the
                // way incoming connector markdown renders — the raw
                // markers showed as literal asterisks for the send's
                // lifetime.
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let pieces = crate::ui::widgets::split_markdown_links(&p.text);
                    for piece in pieces {
                        match piece {
                            crate::ui::widgets::Piece::Plain(t) => {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(t).color(ctx.pal.text),
                                    )
                                    .selectable(false),
                                );
                            }
                            crate::ui::widgets::Piece::Bold(t) => {
                                ui.add(
                                    egui::Label::new(RichText::new(t).bold().color(ctx.pal.text))
                                        .selectable(false),
                                );
                            }
                            crate::ui::widgets::Piece::Italic(t) => {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(t).italics().color(ctx.pal.text),
                                    )
                                    .selectable(false),
                                );
                            }
                            crate::ui::widgets::Piece::MdLink { text, url } => {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(text)
                                            .underline()
                                            .color(ctx.pal.link),
                                    )
                                    .selectable(false),
                                )
                                .on_hover_text(url);
                            }
                        }
                    }
                });
            });
            // Status/actions row under the bubble, right-aligned.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                match &p.error {
                    None => {
                        ui.label(RichText::new("sending…").small().weak());
                    }
                    Some(err) => {
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
                                egui::Button::new(
                                    RichText::new("Discard").small().color(ctx.pal.secondary),
                                )
                                .fill(Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE),
                            )
                            .clicked()
                        {
                            ctx.actions.push(Action::DismissSend(p.cmid.clone()));
                        }
                        ui.label(
                            RichText::new("not sent")
                                .small()
                                .color(ctx.pal.danger),
                        )
                        .on_hover_text(err);
                    }
                }
            });
            ui.add_space(4.0);
        });
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
            .fill(ctx.pal.surface)
            .stroke(Stroke::new(1.0, ctx.pal.outline))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Replying to {who}")).small().bold());
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
                    ui.label(RichText::new("Editing").small().bold());
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
            // Format toolbar (markdown-ish markers in the draft;
            // build_message_html turns them into wire HTML on send).
            // Native-Teams-style: hidden until the format toggle opens it,
            // so the composer's resting height is unchanged.
            let fmt_key = egui::Id::new("composer_fmt_open");
            let mut fmt_open: bool = ui
                .ctx()
                .data_mut(|d| d.get_temp(fmt_key))
                .unwrap_or_else(|| crate::app::qa_flag("TEAMSFAST_FMTBAR"));
            if fmt_open {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    fmt_wrap_btn(ui, draft, RichText::new("B").bold(), "**", "**", "Bold (**, Ctrl+B)");
                    fmt_wrap_btn(ui, draft, RichText::new("I").italics(), "_", "_", "Italic (_, Ctrl+I)");
                    fmt_wrap_btn(
                        ui,
                        draft,
                        RichText::new("S").strikethrough(),
                        "~~",
                        "~~",
                        "Strikethrough (~~)",
                    );
                    fmt_wrap_btn(
                        ui,
                        draft,
                        RichText::new("</>").monospace(),
                        "`",
                        "`",
                        "Inline code (`)",
                    );
                    fmt_wrap_btn(
                        ui,
                        draft,
                        RichText::new("{;}").monospace(),
                        "```\n",
                        "\n```",
                        "Code block (```)",
                    );
                    // Bullet: prefixes the spanned lines (toggle).
                    if ui
                        .add(
                            egui::Button::new(RichText::new("•").size(14.0))
                                .fill(Color32::TRANSPARENT)
                                .stroke(Stroke::NONE)
                                .min_size(egui::vec2(26.0, 22.0)),
                        )
                        .on_hover_text("Bulleted list (-)")
                        .clicked()
                    {
                        composer_apply(draft, ui.ctx(), splice_bullets);
                    }
                    // Link: [selection]() with the caret dropped between
                    // the parens for the URL paste.
                    if ui
                        .add(
                            egui::Button::new(RichText::new("🔗").size(13.0))
                                .fill(Color32::TRANSPARENT)
                                .stroke(Stroke::NONE)
                                .min_size(egui::vec2(26.0, 22.0)),
                        )
                        .on_hover_text("Link [text](url)")
                        .clicked()
                    {
                        composer_apply(draft, ui.ctx(), |t, a, b| {
                            let (out, _na, nb) = splice_wrap(t, a, b, "[", "]()");
                            // nb = inner end; the "(" sits at nb+1, so the
                            // caret lands right after it.
                            (out, nb + 2, nb + 2)
                        });
                    }
                });
                ui.add_space(4.0);
            }
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
                // Format toggle (native Teams' "A" format pane).
                if ui
                    .add(
                        egui::Button::new(RichText::new("A").size(14.0).bold())
                            .fill(Color32::TRANSPARENT)
                            .stroke(if fmt_open {
                                Stroke::new(1.0, ctx.pal.accent)
                            } else {
                                Stroke::NONE
                            })
                            .min_size(egui::vec2(26.0, 24.0)),
                    )
                    .on_hover_text("Formatting")
                    .clicked()
                {
                    fmt_open = !fmt_open;
                }
                ui.ctx().data_mut(|d| d.insert_temp(fmt_key, fmt_open));
                ui.add_space(2.0);

                // Input: the TextEdit's OWN frame does the styling —
                // recessed fill (extreme bg), quiet idle stroke, and the
                // accent focus ring comes from selection.stroke. No wrapper
                // box: the old Frame-inside-TextEdit was three mismatched
                // boxes in one row. Multi-line: Enter sends; Shift+Tab and
                // Shift+Enter insert a newline (Teams parity). The input
                // grows to ~6 lines, then scrolls internally.
                //
                // Keys are consumed BEFORE the TextEdit sees them: a
                // multiline TextEdit turns plain Enter into a newline and
                // Shift+Tab into a de-indent, so interception must happen
                // upstream. Focus is the PREVIOUS frame's widget id (the
                // field lives inside the ScrollArea's child Ui, so its id
                // isn't derivable before add) — first-frame presses are
                // impossible anyway: you can't type into an unfocused field.
                let field_w = ui.available_width() - 44.0;
                // Singleton composer: the field id is remembered under a
                // fixed context key so next frame's interception can find
                // it regardless of which Ui nests the TextEdit.
                let field_key = egui::Id::new("teamsfast_composer_field");
                let prev_field_id: Option<egui::Id> =
                    ui.ctx().data_mut(|d| d.get_temp(field_key));
                let field_focused = prev_field_id
                    .is_some_and(|id| ui.memory(|m| m.focused() == Some(id)));
                let mut send = false;
                let mut newline = false;
                let mut wrap: Option<(&'static str, &'static str)> = None;
                if field_focused {
                    ui.input_mut(|i| {
                        // consume_key matches logically (extra Shift is
                        // ignored), so each combo gets an exact-modifier
                        // gate — plain Enter must not swallow Shift+Enter.
                        if i.key_pressed(egui::Key::Tab)
                            && i.modifiers.matches_exact(egui::Modifiers::SHIFT)
                        {
                            i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab);
                            newline = true;
                        } else if i.key_pressed(egui::Key::Enter) {
                            if i.modifiers.matches_exact(egui::Modifiers::SHIFT) {
                                i.consume_key(egui::Modifiers::SHIFT, egui::Key::Enter);
                                newline = true;
                            } else if i.modifiers.matches_exact(egui::Modifiers::NONE) {
                                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
                                send = true;
                            }
                        } else if i.key_pressed(egui::Key::B)
                            && i.modifiers.matches_exact(egui::Modifiers::CTRL)
                        {
                            i.consume_key(egui::Modifiers::CTRL, egui::Key::B);
                            wrap = Some(("**", "**"));
                        } else if i.key_pressed(egui::Key::I)
                            && i.modifiers.matches_exact(egui::Modifiers::CTRL)
                        {
                            i.consume_key(egui::Modifiers::CTRL, egui::Key::I);
                            wrap = Some(("_", "_"));
                        }
                    });
                }
                if let Some((open, close)) = wrap {
                    composer_apply(draft, ui.ctx(), |t, a, b| {
                        splice_wrap(t, a, b, open, close)
                    });
                }
                ui.vertical(|ui| {
                    ui.set_max_width(field_w);
                    egui::ScrollArea::vertical()
                        .id_salt("composer_input_scroll")
                        .auto_shrink([false, true])
                        .max_height(130.0)
                        .show(ui, |ui| {
                            let field = ui.add(
                                egui::TextEdit::multiline(draft)
                                    .id_salt("composer_field")
                                    .hint_text(if ctx.edit.is_some() {
                                        "Edit message…"
                                    } else {
                                        "Type a message"
                                    })
                                    .desired_rows(1)
                                    .desired_width(ui.available_width())
                                    .min_size(egui::vec2(0.0, 32.0)),
                            );
                            ui.ctx().data_mut(|d| d.insert_temp(field_key, field.id));
                            if newline && field.has_focus() {
                                insert_newline(draft, ui.ctx(), field.id);
                            }
                        });
                });

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
                if (send_response.clicked() || send) && ready {
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

/// Insert a line break at the composer cursor (Shift+Tab / Shift+Enter).
/// The key was consumed before the TextEdit ran, so the insertion is done
/// by hand: splice `\n` over the selection, then point the cursor just
/// after it — otherwise it stays on the wrong side of the break.
fn insert_newline(draft: &mut String, ctx: &egui::Context, field_id: egui::Id) {
    let mut state =
        egui::widgets::text_edit::TextEditState::load(ctx, field_id).unwrap_or_default();
    if let Some(range) = state.cursor.char_range() {
        let chars = range.as_sorted_char_range();
        let (a, b) = (chars.start.0, chars.end.0);
        // CCursor indices are character counts; String ranges are bytes.
        let byte_of = |ci: usize| draft.char_indices().nth(ci).map(|(x, _)| x).unwrap_or(draft.len());
        if a <= b && b <= draft.chars().count() {
            draft.replace_range(byte_of(a)..byte_of(b), "\n");
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(
                    a + 1,
                ))));
            state.store(ctx, field_id);
        }
    } else {
        draft.push('\n');
    }
}

// ---------------------------------------------------------------- composer
// formatting: the draft stays plain text carrying markdown-ish markers;
// teams-core's build_message_html turns them into wire HTML on send.

/// Vertical anchor for the hover action bar (workstream D: dock UNDER the
/// message). Priority: below the bubble + reserved reaction strip (never
/// covers chips/timestamp); not enough room at the viewport bottom →
/// above the bubble (the old resting place); bubble clipped at the
/// viewport top (tall card mid-scroll) → tooltip-style above the
/// pointer, else clamped in-view under the bubble.
fn hover_bar_anchor_y(
    bubble: egui::Rect,
    strip_h: f32,
    bar_h: f32,
    clip: egui::Rect,
    pointer_y: Option<f32>,
) -> f32 {
    let below_y = bubble.bottom() + strip_h + 4.0;
    if below_y + bar_h <= clip.bottom() - 4.0 {
        return below_y;
    }
    if bubble.top() - bar_h - 4.0 >= clip.top() + 2.0 {
        return bubble.top() - bar_h - 4.0;
    }
    if bubble.top() < clip.top() + 2.0 {
        return pointer2_anchor(pointer_y, bar_h, clip, bubble);
    }
    (bubble.bottom() + 6.0).min(clip.bottom() - bar_h - 4.0)
}

fn pointer2_anchor(pointer_y: Option<f32>, bar_h: f32, clip: egui::Rect, bubble: egui::Rect) -> f32 {
    pointer_y
        .map(|p| (p - bar_h - 10.0).max(clip.top() + 4.0))
        .unwrap_or_else(|| (bubble.bottom() + 6.0).min(clip.bottom() - bar_h - 4.0))
}

/// Splice `open`/`close` around chars [a,b) — TOGGLING OFF when the exact
/// markers already wrap that span (native toolbar behaviour: press again
/// to remove). Returns the new text and the char span to select after
/// (the inner text when wrapping, the collapsed point when unwrapping).
/// All indices are CCursor (character) positions.
fn splice_wrap(
    text: &str,
    a: usize,
    b: usize,
    open: &str,
    close: &str,
) -> (String, usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    if a > b || b > n {
        return (text.to_string(), a, b);
    }
    let oc = open.chars().count();
    let cc = close.chars().count();
    // Toggle off: exact markers already surround [a,b).
    if a >= oc && b + cc <= n {
        let before: String = chars[a - oc..a].iter().collect();
        let after: String = chars[b..b + cc].iter().collect();
        if before == open && after == close {
            let mut out: String = chars[..a - oc].iter().collect();
            out.extend(chars[a..b].iter());
            out.extend(chars[b + cc..].iter());
            return (out, a - oc, b - oc);
        }
    }
    let mut out: String = chars[..a].iter().collect();
    out.push_str(open);
    out.extend(chars[a..b].iter());
    out.push_str(close);
    out.extend(chars[b..].iter());
    (out, a + oc, b + oc)
}

/// Prefix every line spanned by chars [a,b) with "- " — TOGGLING OFF
/// when every spanned line already carries the prefix. Returns the new
/// text and the char span covering the same logical content (all CCursor
/// character positions, like the rest of the composer machinery).
fn splice_bullets(text: &str, a: usize, b: usize) -> (String, usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    if a > b || b > n {
        return (text.to_string(), a, b);
    }
    // Line starts touched by the selection: a line [s, end) counts when
    // the selection overlaps it — or CONTAINS the caret (a == b).
    let mut lines: Vec<usize> = Vec::new();
    let mut s = 0usize;
    for i in 0..=n {
        if i == n || chars[i] == '\n' {
            let end = i;
            let overlaps = if a == b {
                s <= a && a <= end
            } else {
                a < end && b > s
            };
            if overlaps && end > s {
                lines.push(s);
            }
            s = i + 1;
        }
    }
    if lines.is_empty() {
        return (text.to_string(), a, b);
    }
    let prefixed = |s: usize| chars.get(s) == Some(&'-') && chars.get(s + 1) == Some(&' ');
    let all_prefixed = lines.iter().all(|&s| prefixed(s));
    let mut result: Vec<char> = Vec::with_capacity(n + 2 * lines.len());
    let (mut da, mut db) = (0i32, 0i32); // signed: +2 per added prefix, -2 per removed
    let mut i = 0usize;
    while i < n {
        let at_touched_line = (i == 0 || chars[i - 1] == '\n') && lines.contains(&i);
        if at_touched_line && all_prefixed && prefixed(i) {
            if i <= a {
                da -= 2;
            }
            if i < b {
                db -= 2;
            }
            i += 2; // drop this line's "- "
            continue;
        }
        if at_touched_line && !all_prefixed && !prefixed(i) {
            if i <= a {
                da += 2;
            }
            if i < b {
                db += 2;
            }
            result.push('-');
            result.push(' ');
        }
        result.push(chars[i]);
        i += 1;
    }
    let out_len = result.len();
    let clamp = |v: i32| v.clamp(0, out_len as i32) as usize;
    (
        result.into_iter().collect(),
        clamp(a as i32 + da),
        clamp(b as i32 + db),
    )
}

/// Apply a pure splice function at the composer's cursor/selection, then
/// restore focus and select the returned span (so a second press toggles
/// the markers off, and typing over a wrap replaces the inner text).
fn composer_apply(
    draft: &mut String,
    ctx: &egui::Context,
    splice: impl Fn(&str, usize, usize) -> (String, usize, usize),
) {
    let Some(field_id) = ctx
        .data_mut(|d| d.get_temp(egui::Id::new("teamsfast_composer_field")))
    else {
        return;
    };
    let mut state =
        egui::widgets::text_edit::TextEditState::load(ctx, field_id).unwrap_or_default();
    let (a, b) = match state.cursor.char_range() {
        Some(r) => {
            let c = r.as_sorted_char_range();
            (c.start.0, c.end.0)
        }
        None => {
            let n = draft.chars().count();
            (n, n)
        }
    };
    let (new_text, na, nb) = splice(draft, a, b);
    *draft = new_text;
    let range = if na == nb {
        egui::text::CCursorRange::one(egui::text::CCursor::new(na))
    } else {
        egui::text::CCursorRange::two(egui::text::CCursor::new(na), egui::text::CCursor::new(nb))
    };
    state.cursor.set_char_range(Some(range));
    state.store(ctx, field_id);
    ctx.memory_mut(|m| m.request_focus(field_id));
}

/// One ghost toolbar button that wraps the selection with a marker pair.
fn fmt_wrap_btn(
    ui: &mut egui::Ui,
    draft: &mut String,
    label: RichText,
    open: &str,
    close: &str,
    tip: &str,
) {
    if ui
        .add(
            egui::Button::new(label.size(12.5))
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::NONE)
                .min_size(egui::vec2(26.0, 22.0)),
        )
        .on_hover_text(tip)
        .clicked()
    {
        let o = open.to_string();
        let c = close.to_string();
        composer_apply(draft, ui.ctx(), move |t, a, b| splice_wrap(t, a, b, &o, &c));
    }
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
        // Own row: measure the text, give the bubble exactly that width
        // (shrink-to-fit, capped), then push the whole cluster — bubble
        // slot + avatar gutter — to the pane's right edge. The push is a
        // computed LEADING space in this LTR row (the 397466e refactor
        // dropped the old full-width RTL wrapper, which left-anchored the
        // cluster and made each avatar trail its own bubble's width — the
        // "avatar wobble"; RTL rebuilds all mis-anchor on this stack).
        // `slot_w` cancels in lead + slot + gutter, so every own row ends
        // at the same x: fixed avatar column, bubble hugging it from the
        // left, exactly like the incoming layout mirrored.
        let avail = ui.available_width() - 12.0;
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
        let slot_w = own_slot_width(own_content_width(m, ctx, text_w), avail);
        let isx = ui.spacing().item_spacing.x;
        let lead = own_row_lead(slot_w, avail, isx);
        let mut row_min = egui::Rect::NOTHING;
        ui.horizontal(|ui| {
            ui.add_space(lead + 6.0);
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
            row_min = ui.min_rect();
        });
        // Rectlog probe for the own-row anchor: the row's painted extent
        // must end at one shared right edge (lead math) and never escape
        // the pane (row_min.right > pane right = escaped content).
        if crate::app::qa_flag("TEAMSFAST_RECTLOG") {
            eprintln!(
                "RECTOWN slot={slot_w:.0} lead={lead:.0} row_min_r={:.0} row_min_l={:.0}",
                row_min.right(),
                row_min.left()
            );
        }
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.add_space(6.0);
            if grouped {
                // 43 = avatar(32) + gap(3) + one inter-widget spacing:
                // 38 shifted every avatar-less group 5px left of the
                // named groups' text column (battery round 6).
                ui.add_space(43.0);
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
                                RichText::new(&name).small().bold().color(ctx.pal.secondary),
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
    // The reserved strip adds ~20px under EVERY bubble, so group-internal
    // spacing and group boundaries converged (light QA measured 40px vs
    // 42px — groups read as broken). Widen the inter-group gap so the
    // hierarchy survives the reservation.
    ui.add_space(if grouped { 2.0 } else { 34.0 });
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
    _grouped: bool,
    last_of_group: bool,
) {

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

    // Own bubbles are indigo/violet — the theme link color fails AA on
    // them in BOTH themes (2.8:1 dark, 3.2:1 light; battery round 6).
    // Body links inside own bubbles render through this bubble-aware
    // palette clone; cards paint their own surface and keep pal.link.
    let link_pal = if own {
        let mut p = ctx.pal.clone();
        p.link = if ctx.pal.is_light() {
            Color32::from_rgb(0x1e, 0x4f, 0xb0)
        } else {
            Color32::from_rgb(0xb3, 0xcc, 0xf7)
        };
        p
    } else {
        ctx.pal.clone()
    };

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
                            let mention_hits: std::rc::Rc<
                                std::cell::RefCell<Vec<(String, Option<String>)>>,
                            > = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
                            let hits3 = mention_hits.clone();
                            render_segments(
                                ui,
                                &segments,
                                &link_pal,
                                |ui, url| show_image(ui, ctx, url),
                                |ui, name, url| {
                                    if ui
                                        .button(
                                            RichText::new(format!(
                                                "📎 {}",
                                                crate::ui::widgets::shorten_link_text(name)
                                            ))
                                            .small(),
                                        )
                                        .on_hover_text(format!("Download {name}"))
                                        .clicked()
                                    {
                                        hits2.borrow_mut().push((
                                            name.to_string(),
                                            url.to_string(),
                                        ));
                                    }
                                },
                                |_, name, mri| {
                                    hits3
                                        .borrow_mut()
                                        .push((name.to_string(), mri.map(str::to_string)));
                                },
                            );
                            for (name, mri) in mention_hits.borrow().iter() {
                                open_mention_contact(ctx, name, mri.as_deref());
                            }
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
                        let mention_hits: std::rc::Rc<
                            std::cell::RefCell<Vec<(String, Option<String>)>>,
                        > = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
                        let hits3 = mention_hits.clone();
                        render_segments(
                            ui,
                            &segments,
                            &link_pal,
                            |ui, url| show_image(ui, ctx, url),
                            |ui, name, url| {
                                if ui
                                    .button(
                                        RichText::new(format!(
                                            "📎 {}",
                                            crate::ui::widgets::shorten_link_text(name)
                                        ))
                                        .small(),
                                    )
                                    .on_hover_text(format!("Download {name}"))
                                    .clicked()
                                {
                                    hits2
                                        .borrow_mut()
                                        .push((name.to_string(), url.to_string()));
                                }
                            },
                            |_, name, mri| {
                                hits3
                                    .borrow_mut()
                                    .push((name.to_string(), mri.map(str::to_string)));
                            },
                        );
                        for (name, mri) in mention_hits.borrow().iter() {
                            open_mention_contact(ctx, name, mri.as_deref());
                        }
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

    if crate::app::qa_flag("TEAMSFAST_LAYOUT_DEBUG") && own {
        eprintln!("DBG bubble own rect={bubble_rect:?}");
    }

    // Reserved reaction strip: fixed-height row attached at the bubble's
    // bottom-right, allocated on EVERY message from first paint.
    // Reactions only ever arrive with network pages (the archive never
    // stores them) and with later refreshes — previously the chips row
    // appeared ABOVE the bubble and grew the row by ~46px, shoving every
    // later message down (the run-5 P0). Reserving the height up front
    // means a reaction appearing changes only the strip's contents. The
    // last-of-group meta (timestamp/seen/pin) shares the reserved row.
    reaction_strip(ui, ctx, m, own, bubble_rect.right(), last_of_group);

    // Hover state: POINTER-POSITION based. egui's per-widget hover is not
    // reliable across an Area boundary (the bar vanishes when the pointer
    // moves into the gap), so track the bar's rect and test containment
    // directly, with a 6px grow to bridge the bubble→bar gap.
    let hover_id = egui::Id::new(("msg-hover", &m.id));


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
    // The hover target includes the reserved reaction strip below the
    // bubble (it is part of the message row): the bar now docks BELOW
    // the strip, and without bridging it the pointer crossing the strip
    // fell into a dead zone and closed the bar mid-move.
    let hover_zone = egui::Rect::from_min_max(
        egui::pos2(bubble_rect.left() - 2.0, bubble_rect.top() - 2.0),
        egui::pos2(bubble_rect.right() + 2.0, bubble_rect.bottom() + REACT_STRIP_H + 6.0),
    );
    let in_bubble = pointer2.map(|p| hover_zone.contains(p)).unwrap_or(false);
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
    let pointer_open = in_bubble || in_bar;
    let open = if hook { forced } else { pointer_open && !ctx.overlays_open };

    if open {
        let clip = ui.clip_rect();
        // Emoji row (6 x 30) + separator + Reply/Forward/Pin/Copy (+ Edit/
        // Delete on own). First-frame sizes are the MEASURED settle sizes
        // (LAYOUT_DEBUG, 1100x760) — the old 396x48 guess snapped the bar
        // ~20px sideways and 12px up one frame after opening.
        const BAR_H: f32 = 36.0;
        const BAR_W_INCOMING: f32 = 417.0;
        const BAR_W_OWN: f32 = 481.0;
        // Measured bar size from the previous frame (content-sized and
        // stable). Anchoring with the real width keeps the bar's right
        // edge aligned to the bubble's.
        let (bar_w, bar_h) = bar_rect_prev
            .map(|r| (r.width(), r.height()))
            .unwrap_or((
                if own { BAR_W_OWN } else { BAR_W_INCOMING },
                BAR_H,
            ));
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
        // Anchor BELOW the message: the reserved reaction strip already
        // lives under every bubble, so the bar docks under the STRIP —
        // it never covers chips or the timestamp (user report: the bar
        // hanging over the header side of a message read as intruding
        // into the chrome while scrolling). Fallbacks keep the bar on
        // screen (see hover_bar_anchor_y).
        let anchor_y = hover_bar_anchor_y(
            bubble_rect,
            REACT_STRIP_H,
            bar_h,
            clip,
            pointer2.map(|p| p.y),
        );
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
                            let quick = quick_set(ctx.reaction_favorites);
                            for e in &quick {
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
                                if crate::app::qa_flag("TEAMSFAST_LAYOUT_DEBUG") {
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
                                // Pin button on an ALREADY-pinned message
                                // jumps to it instead of unpinning (the
                                // user report's ask); unpin lives in the
                                // header's pinned-messages menu.
                                if ctx.pinned.contains(&m.id) {
                                    sink.borrow_mut().push(Action::JumpToMessage(m.id.clone()));
                                } else {
                                    sink.borrow_mut().push(Action::TogglePinMessage {
                                        chat_id: ctx.chat_id.clone(),
                                        message_id: m.id.clone(),
                                    });
                                }
                            }
                            if do_edit {
                                // Marker text, not the plain projection:
                                // saving an untouched edit must preserve
                                // the formatting (the wire builder turns
                                // markers back into HTML).
                                let current = crate::ui::widgets::segs_to_markers(
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
        if crate::app::qa_flag("TEAMSFAST_LAYOUT_DEBUG") {
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
                    ui.label(RichText::new("Connector card").small().bold());
                    ui.label(
                        RichText::new("Open in Teams to view the card content.")
                            .small()
                            .weak(),
                    );
                });
            });
        });
}

/// Height reserved under EVERY bubble for the reaction strip. Chips are
/// sized to fit inside it (`reaction_strip_reserves_constant_height`
/// unit test); the reservation is what keeps a reaction appearing from
/// moving anything.
const REACT_STRIP_H: f32 = 18.0;
/// Own-message slot floor: the bubble column reserves at least this much
/// width so up to ~3 chips fit under a short bubble. The bubble itself
/// stays text-width and right-aligned inside the (possibly wider) slot —
/// the floor is invisible until reactions exist.
const REACT_SLOT_FLOOR: f32 = 150.0;

/// Pure own-slot math (unit-tested): text width + padding, clamped, and
/// floored so the reserved reaction strip fits without touching the
/// bubble's own width.
fn own_slot_width(text_w: f32, avail: f32) -> f32 {
    let cap = (avail * 0.85).min(680.0).min((avail - 44.0).max(56.0));
    // max-then-min, never clamp: on tiny panes cap dips below 56 and
    // clamp(56, cap<56) PANICS (reachable by resizing — no min window
    // size is set). Degradation: a sub-66px pane overflows the row
    // instead of crashing.
    (text_w + 26.0).max(56.0).min(cap).max(REACT_SLOT_FLOOR.min(cap))
}

/// Leading space that right-anchors the own-message cluster (bubble slot +
/// 6 gap + 32 avatar / 38 grouped gutter + one inter-widget spacing) so it
/// ends exactly at `avail`. Pure math (unit-tested): `slot_w` cancels, i.e.
/// every own row shares one right edge regardless of bubble width — the
/// avatar column never wobbles. Clamps at 0: panes narrower than the cap
/// degrade to a left-anchored cluster (overflow class, never a panic).
fn own_row_lead(slot_w: f32, avail: f32, isx: f32) -> f32 {
    (avail - slot_w - 44.0 - isx).max(0.0)
}

/// Chat-image display box (width-capped 340, height-capped 460). Shared by
/// the renderer and the own-slot measurement so the reserved slot always
/// contains the painted image (an escaped row grows the parent max_rect and
/// poisons every following row's available_width).
fn image_display_box(w: usize, h: usize) -> egui::Vec2 {
    let scale = (340.0 / w as f32).min(460.0 / h as f32).min(1.0);
    egui::vec2(w as f32 * scale, h as f32 * scale)
}

/// Width the own-row slot must reserve for this message: the widest of the
/// plain text and every deterministic non-text part. Cards render at ≤560
/// (cards.rs clamps to that), images at image_display_box (from persisted
/// decoded sizes; unknown sizes reserve the 340 placeholder box — the same
/// box the renderer shows, so no reflow once decoded).
fn own_content_width(m: &MessageInfo, ctx: &ConvCtx<'_>, text_w: f32) -> f32 {
    let mut w = text_w;
    if crate::ui::cards::extract_card_cached(&m.raw).0.is_some() {
        w = w.max(560.0);
    }
    for seg in crate::ui::widgets::parse_html_cached(&m.raw).iter() {
        if let crate::ui::widgets::Seg::Image { url } = seg {
            let disp = ctx
                .image_sizes
                .get(url)
                .map(|&[iw, ih]| image_display_box(iw, ih))
                .unwrap_or_else(|| egui::vec2(340.0, 200.0));
            w = w.max(disp.x);
        }
    }
    w
}

/// The always-reserved under-bubble row: fixed height, right edge
/// anchored to the BUBBLE's right edge (narrow card bubbles keep their
/// chips attached instead of hanging to the column edge), chips
/// right-aligned — and for the last message of a group the timestamp /
/// seen-by / pin meta joins the SAME row (an empty reserved strip between
/// a bubble and its timestamp read as detached; merging keeps the
/// hierarchy tight at zero extra height).
fn reaction_strip(
    ui: &mut egui::Ui,
    ctx: &mut ConvCtx<'_>,
    m: &MessageInfo,
    own: bool,
    bubble_right: f32,
    last_of_group: bool,
) {
    let w = ui.available_width().max(0.0);
    // allocate_ui does NOT honor the desired height as a floor (an empty
    // child shrinks it) — the reservation must be an exact allocation
    // with the chips painted into a child of that rect.
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, REACT_STRIP_H), egui::Sense::hover());
    let right = bubble_right.min(rect.right()).max(rect.left());
    let anchor = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.top()),
        egui::pos2(right, rect.bottom()),
    );
    let mut strip = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(anchor)
            .layout(if own {
                egui::Layout::right_to_left(egui::Align::Center)
            } else {
                egui::Layout::left_to_right(egui::Align::Center)
            }),
    );
    // Chips beyond what fits must never paint over the neighboring
    // column — clip the strip to its reserved rect (width-capped chips +
    // this clip are the two guards; red-team P1-1).
    strip.set_clip_rect(anchor);
    // 8px: 3px read as one glued clump between timestamp and chips.
    strip.spacing_mut().item_spacing.x = 8.0;
    if own {
        // Right-to-left: chips rightmost against the bubble edge, then
        // seen-by / timestamp / pin flowing left (the old meta order).
        if !m.reactions.is_empty() {
            reaction_chips(&mut strip, ctx, m, own);
        }
        if last_of_group {
            meta_line(&mut strip, ctx, m);
        }
    } else {
        // Left-to-right: timestamp first at the column's left edge, then
        // the chips right-aligned to the bubble's right edge.
        if last_of_group {
            meta_line(&mut strip, ctx, m);
        }
        if !m.reactions.is_empty() {
            strip.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    reaction_chips(ui, ctx, m, own);
                },
            );
        }
    }
}

/// Timestamp + seen-by + pin, as it appeared under the last bubble of a
/// group (now inside the reserved strip row).
fn meta_line(ui: &mut egui::Ui, ctx: &mut ConvCtx<'_>, m: &MessageInfo) {
    let pinned = ctx.pinned.contains(&m.id);
    let seen_by: Option<String> = ctx.receipts.get(&m.id).map(|users| {
        let names: Vec<String> = users
            .iter()
            .map(|u| ctx.members.get(u).cloned().unwrap_or_else(|| "Someone".into()))
            .collect();
        names.join(", ")
    });
    if ui.layout().main_dir() == egui::Direction::RightToLeft {
        if let Some(names) = seen_by {
            ui.label(RichText::new("Seen").small().weak().color(ctx.pal.ok))
                .on_hover_text(format!("Seen by {names}"));
        }
        ui.label(RichText::new(format_message_time(&m.timestamp)).small().weak());
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
    } else {
        ui.label(RichText::new(format_message_time(&m.timestamp)).small().weak());
    }
}

/// Reaction count chips: colour emoji + names on hover; click opens the
/// full reactor list (and removes your own reaction when you were in it).
fn reaction_chips(ui: &mut egui::Ui, ctx: &mut ConvCtx<'_>, m: &MessageInfo, own: bool) {
    // Cap chips to the strip's width: a 6-reaction announcement message
    // would otherwise paint past the column (clipped) and hide the meta
    // line. Overflow collapses into one "+K" chip; the full list is one
    // click away (the popup shows every reactor).
    let budget = ui.available_width();
    let est = |r: &ost::api::ReactionCount| -> f32 {
        let count_w = ui
            .painter()
            .layout(
                r.count.to_string(),
                egui::FontId::proportional(11.0),
                Color32::WHITE,
                f32::INFINITY,
            )
            .mesh_bounds
            .width();
        13.0 + count_w + 12.0 + 3.0
    };
    let mut used = 0.0f32;
    let mut shown = 0usize;
    for (i, r) in m.reactions.iter().enumerate() {
        // Reserve room for the "+K" chip ONLY when more reactions follow —
        // a flat reserve collapsed one chip that actually fit.
        let reserve = if i + 1 < m.reactions.len() { 34.0 } else { 0.0 };
        let w = est(r);
        if used + w + reserve > budget {
            break;
        }
        used += w;
        shown += 1;
    }
    let hidden = m.reactions.len() - shown;
    // Chip tint derives from the HOST bubble — a fixed bubble_in fill sat
    // invisible on the light incoming bubble (both white) and mismatched
    // the own indigo bubble.
    let base = if own {
        ctx.pal.bubble_out
    } else {
        ctx.pal.bubble_in
    };
    // Light theme: the 0.09 tint over white was a ~3 RGB delta — chips
    // read as smudges. Strengthen fill+stroke there.
    let (fill_a, stroke_a) = if ctx.pal.is_light() {
        (0.14, 0.75)
    } else {
        (0.09, 0.5)
    };
    for (i, r) in m.reactions.iter().enumerate() {
        if i >= shown {
            if hidden > 0 {
                let names: Vec<String> = m
                    .reactions
                    .iter()
                    .skip(shown)
                    .flat_map(|r| r.reactors.iter().map(|x| x.name.clone()))
                    .collect();
                let tip = format!("{} more — click for all", names.join(", "));
                let chip = Frame::default()
                    .fill(crate::theme::tint_over(base, ctx.pal.text, 0.09))
                    .stroke(Stroke::new(1.0, ctx.pal.outline.gamma_multiply(0.5)))
                    .corner_radius(CornerRadius::same(9))
                    .inner_margin(egui::Margin::symmetric(5, 1))
                    .show(ui, |ui| {
                        ui.label(RichText::new(format!("+{hidden}")).small());
                    })
                    .response;
                if chip.clicked() {
                    ctx.actions.push(Action::ShowReactions {
                        emoji: m.reactions[shown].emoji.clone(),
                        names,
                    });
                }
                let _ = tip;
                let _ = chip.on_hover_text(tip);
            }
            break;
        }
        let names: Vec<String> = r.reactors.iter().map(|x| x.name.clone()).collect();
        let tip = if names.is_empty() {
            format!("{} reaction{}", r.count, if r.count == 1 { "" } else { "s" })
        } else {
            format!("{} by {}", r.emoji, names.join(", "))
        };
        let chip = Frame::default()
            .fill(crate::theme::tint_over(base, ctx.pal.text, fill_a))
            .stroke(Stroke::new(1.0, ctx.pal.outline.gamma_multiply(stroke_a)))
            .corner_radius(CornerRadius::same(9))
            // Sized to fit inside REACT_STRIP_H (unit-tested) — the strip's
            // reserved height must never grow when chips render.
            .inner_margin(egui::Margin::symmetric(5, 1))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if own {
                        // The strip is right-to-left on own rows; adding
                        // the count first keeps the visual order
                        // emoji-then-count on both sides.
                        ui.label(RichText::new(r.count.to_string()).small());
                        emoji_widget(ui, ctx, &r.emoji, 13.0);
                    } else {
                        emoji_widget(ui, ctx, &r.emoji, 13.0);
                        ui.label(RichText::new(r.count.to_string()).small());
                    }
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
                .inner_margin(egui::Margin::symmetric(8, 3))
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
                        .size(13.0)
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

/// A mention chip was clicked: open the person's contact card. The wire
/// MRI wins; without one (the span-index mention shape), resolve the name
/// against this chat's roster. Unresolvable names surface a status note
/// instead of a dead click.
fn open_mention_contact(ctx: &mut ConvCtx<'_>, name: &str, mri: Option<&str>) {
    let resolved = mri
        .map(str::to_string)
        .or_else(|| {
            ctx.members
                .iter()
                .find(|(_, n)| n.as_str() == name)
                .map(|(m, _)| m.clone())
        });
    if let Some(mri) = resolved {
        ctx.actions.push(Action::ShowContact {
            mri,
            name: name.to_string(),
        });
    } else {
        ctx.actions
            .push(Action::Status(format!("No member info for {name} in this chat")));
    }
}

/// Inline image: textured when loaded, fetch requested once, placeholder
/// otherwise (failed URLs stay in `pending_images` so they never retry).
fn show_image(ui: &mut egui::Ui, ctx: &mut ConvCtx<'_>, url: &str) -> bool {
    // Display box: width-capped at 340, height-capped at 460. Placeholder
    // reserves the SAME box (from the archive's decoded sizes) so an image
    // arriving mid-session doesn't reflow the conversation.
    let display_box = image_display_box;
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

    /// The growth latch: the frame where an incoming message grew the
    /// content renders at the stale offset (stick re-pins at ScrollArea
    /// end) — geometry alone reads "scrolled up" and flashed the Latest
    /// pill + sticky-day chip for one frame. Growth while previously at
    /// bottom must count as at-bottom; genuine scrolls must not.
    #[test]
    fn at_bottom_growth_latch() {
        use super::effective_at_bottom;
        // Settled at bottom, nothing grew: bottom.
        assert!(effective_at_bottom(true, false, Some(true)));
        // Grew while at bottom (the incoming-message frame): STILL bottom.
        assert!(effective_at_bottom(false, true, Some(true)));
        // Grew while NOT at bottom (prepend/load-older): stays not-bottom.
        assert!(!effective_at_bottom(false, true, Some(false)));
        // First frame ever (no prev): geometry only.
        assert!(!effective_at_bottom(false, true, None));
        assert!(effective_at_bottom(true, true, None));
        // Plain scroll: geometry rules.
        assert!(!effective_at_bottom(false, false, Some(true)));
    }

    /// The no-shift invariant, unit level: the reserved reaction strip
    /// allocates EXACTLY REACT_STRIP_H whether empty or carrying chips —
    /// reactions appearing must never change any row height (run-5 P0).
    #[test]
    fn reaction_strip_reserves_constant_height() {
        crate::emoji::setup();
        let empty: HashMap<String, String> = HashMap::new();
        let textures: HashMap<String, (egui::TextureHandle, [usize; 2])> = HashMap::new();
        let image_sizes: HashMap<String, [usize; 2]> = HashMap::new();
        let mut pending_images: HashSet<String> = HashSet::new();
        let mut emoji_textures: HashMap<String, egui::TextureHandle> = HashMap::new();
        let egui_ctx = crate::theme::test_ctx();

        let react = |emoji: &str, count: usize| ost::api::ReactionCount {
            emoji: emoji.to_string(),
            count,
            reactors: (1..=count)
                .map(|i| ost::api::Reactor {
                    id: format!("8:orgid:t{i}"),
                    name: format!("Tester {i}"),
                })
                .collect(),
        };
        let mut msg_empty = test_message();
        let mut msg_full = test_message();
        msg_full.reactions = vec![react("\u{1f44d}", 2), react("\u{2764}", 1), react("\u{1f389}", 3)];

        let mut heights: Vec<f32> = Vec::new();
        for m in [&msg_empty, &msg_full] {
            let mut actions = Vec::new();
            let measured = std::cell::Cell::new(0.0f32);
            let mut out = egui_ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.vertical(|ui| {
                        let top = ui.cursor().top();
                        let mut ctx = ConvCtx {
                            chat_name: "Chat".into(),
                            chat_id: "19:chat@thread.v2".into(),
                            messages: std::slice::from_ref(m),
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
                            jump_target: None,
                            reaction_favorites: &[],
                            can_call: false,
                            call_label: None,
                            textures: &textures,
                            image_sizes: &image_sizes,
                            pending_images: &mut pending_images,
                            emoji_textures: &mut emoji_textures,
                            pal: &Palette::dark(),
                            overlays_open: false,
                            prepend_adjust: None,
                            conv_loading: false,
                            actions: &mut actions,
                        };
                        reaction_strip(ui, &mut ctx, m, false, 400.0, true);
                        let h = ui.cursor().top() - top;
                        assert!(h > 0.0, "strip allocated nothing");
                        measured.set(h - ui.spacing().item_spacing.y);
                    });
                });
            // The emoji raster queues a texture delta; a test Context has
            // no renderer to apply it — clear before drop (its own advice)
            // or the Context destructor panics.
            out.textures_delta.clear();
            heights.push(measured.get());
        }
        assert_eq!(heights[0], REACT_STRIP_H, "empty strip must reserve exactly REACT_STRIP_H");
        assert_eq!(
            heights[1], REACT_STRIP_H,
            "chips must fit inside the reservation (got {})",
            heights[1]
        );
    }

    /// Own-slot math: the slot reserves room for the reaction strip floor
    /// without changing the bubble cap behavior.
    #[test]
    fn own_slot_width_floor_and_cap() {
        // Short text: floored so chips fit under it.
        assert_eq!(own_slot_width(10.0, 800.0), REACT_SLOT_FLOOR);
        // Long text: text width + padding, capped at 85%/680.
        assert_eq!(own_slot_width(1000.0, 800.0), 680.0);
        assert_eq!(own_slot_width(300.0, 800.0), 326.0);
        // Tiny window: the avatar row (44px) must still fit beside the
        // slot — the slot caps at avail-44, never overflowing the pane
        // (red-team P2-1).
        let slot = own_slot_width(10.0, 100.0);
        assert_eq!(slot, 56.0);
        assert!(slot + 44.0 <= 100.0);
        // Panic zone: sub-66px panes used to hit clamp(56, cap<56) and
        // abort (reachable by window resize — no min size is set).
        let slot = own_slot_width(10.0, 60.0);
        assert!(slot <= 51.0, "cap must stay the binding constraint");
        // Narrow-but-sane window still floors when it can.
        let slot = own_slot_width(10.0, 400.0);
        assert_eq!(slot, REACT_SLOT_FLOOR);
        assert!(slot + 44.0 <= 400.0);
    }

    /// The own-row right anchor: lead + slot + gutter + one inter-widget
    /// spacing must consume exactly `avail` for EVERY slot width — that
    /// cancellation is what pins the avatar column to one x (397466e
    /// left-anchored the cluster; the wobble was the avatar trailing each
    /// bubble's measured width).
    #[test]
    fn own_row_lead_pins_right_edge() {
        let isx = 8.0;
        for slot in [150.0, 236.5, 380.0, 607.75] {
            let avail = 715.0;
            let lead = own_row_lead(slot, avail, isx);
            assert!(lead > 0.0);
            assert!((lead + slot + 44.0 + isx - avail).abs() < 1e-4);
        }
        // Same right edge for different slot widths: the sum is invariant.
        let a = own_row_lead(150.0, 715.0, isx) + 150.0;
        let b = own_row_lead(607.75, 715.0, isx) + 607.75;
        assert!((a - b).abs() < 1e-4);
        // Panic/degradation zone: never negative.
        assert_eq!(own_row_lead(800.0, 715.0, isx), 0.0);
        assert_eq!(own_row_lead(150.0, 100.0, isx), 0.0);
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

        let egui_ctx = crate::theme::test_ctx();
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
                    jump_target: None,
                    reaction_favorites: &[],
                    can_call: false,
                    call_label: None,
                    textures: &textures,
                    image_sizes: &image_sizes,
                    pending_images,
                    emoji_textures,
                    pal: &Palette::dark(),
                    overlays_open: false,
                    prepend_adjust: None,
                    conv_loading: false,
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

    /// Draw only the composer; optionally focus `focus` first. Returns the
    /// field's widget id (the composer records it in temp storage) and the
    /// pushed actions.
    fn draw_composer_frame(
        egui_ctx: &egui::Context,
        input: egui::RawInput,
        draft: &mut String,
        focus: Option<egui::Id>,
    ) -> (Option<egui::Id>, Vec<Action>) {
        let messages: Vec<MessageInfo> = Vec::new();
        let empty: std::collections::HashMap<String, String> = HashMap::new();
        let textures: HashMap<String, (egui::TextureHandle, [usize; 2])> = HashMap::new();
        let image_sizes: HashMap<String, [usize; 2]> = HashMap::new();
        let mut pending_images: HashSet<String> = HashSet::new();
        let mut emoji_textures: HashMap<String, egui::TextureHandle> = HashMap::new();
        let mut actions: Vec<Action> = Vec::new();
        let mut field_id: Option<egui::Id> = None;
        let mut out = egui_ctx.run_ui(input, |ui| {
            if let Some(id) = focus {
                ui.memory_mut(|m| m.request_focus(id));
            }
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
                jump_target: None,
                reaction_favorites: &[],
                can_call: false,
                call_label: None,
                textures: &textures,
                image_sizes: &image_sizes,
                pending_images: &mut pending_images,
                emoji_textures: &mut emoji_textures,
                pal: &Palette::dark(),
                overlays_open: false,
                prepend_adjust: None,
                conv_loading: false,
                actions: &mut actions,
            };
            conversation_composer(ui, &mut ctx, draft);
            field_id = ui
                .ctx()
                .data_mut(|d| d.get_temp::<egui::Id>(egui::Id::new("teamsfast_composer_field")));
        });
        out.textures_delta.clear();
        (field_id, actions)
    }

    fn key_input(
        screen: egui::Rect,
        key: egui::Key,
        modifiers: egui::Modifiers,
    ) -> egui::RawInput {
        // InputState.modifiers only advances on ModifiersChanged (winit
        // always sends it first); a bare Key event does not carry it over.
        let mut events = Vec::with_capacity(2);
        if modifiers != egui::Modifiers::default() {
            events.push(egui::Event::ModifiersChanged(modifiers));
        }
        events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        });
        egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        }
    }

    /// Composer formatting splices: wrap/toggle-off, char-index safety on
    /// multi-byte text, and bullet line prefixing (run-7 workstream C).
    #[test]
    fn composer_splice_wrap_and_bullets() {
        use super::{splice_bullets, splice_wrap};
        // Wrap a selection.
        let (t, a, b) = splice_wrap("hello world", 0, 5, "**", "**");
        assert_eq!(t, "**hello** world");
        assert_eq!((a, b), (2, 7)); // inner span selected for toggling
        // Toggle off the same span.
        let (t2, a2, b2) = splice_wrap(&t, a, b, "**", "**");
        assert_eq!(t2, "hello world");
        assert_eq!((a2, b2), (0, 5));
        // Empty selection inserts an empty pair with the caret between
        // the markers ("a``b" → caret at char 2, between the backticks).
        let (t, a, b) = splice_wrap("ab", 1, 1, "`", "`");
        assert_eq!(t, "a``b");
        assert_eq!((a, b), (2, 2));
        // Multi-byte safety: indices are CHAR positions.
        let (t, a, b) = splice_wrap("héllo wörld", 1, 5, "_", "_");
        assert_eq!(t, "h_éllo_ wörld");
        assert_eq!((a, b), (2, 6));
        // Bullets: prefix each spanned line (toggle).
        let (t, a, b) = splice_bullets("one\ntwo\nthree", 0, 7);
        assert_eq!(t, "- one\n- two\nthree");
        // Span still covers the words (prefixes excluded), native-style.
        assert_eq!((a, b), (2, 11));
        // Toggle off selects the lines again, markers gone.
        let (t2, _, _) = splice_bullets(&t, 0, 11);
        assert_eq!(t2, "one\ntwo\nthree");
        // Caret on a line prefixes just that line.
        let (t, _, _) = splice_bullets("a\nb", 2, 2);
        assert_eq!(t, "a\n- b");
        // Mixed: only lines lacking the prefix get one.
        let (t, _, _) = splice_bullets("- a\nb", 0, 5);
        assert_eq!(t, "- a\n- b");
    }

    /// Favorites plumbing: empty = default six; explicit list wins
    /// verbatim (the picker that edits it is capture-gated — the wire
    /// probe 404'd even for known types).
    #[test]
    fn quick_set_favorites_override() {
        use super::quick_set;
        assert_eq!(quick_set(&[]), ["👍", "❤️", "😂", "😮", "😢", "🎉"]);
        assert_eq!(quick_set(&["👍".into(), "🎉".into()]), ["👍", "🎉"]);
    }

    /// Hover-bar anchoring (workstream D): below the strip when it fits,
    /// above the bubble at the viewport bottom, tooltip at the pointer
    /// when the bubble is clipped at the top.
    #[test]
    fn hover_bar_anchor_priority() {
        use super::hover_bar_anchor_y;
        let clip = egui::Rect::from_min_max(egui::pos2(0.0, 100.0), egui::pos2(700.0, 700.0));
        let bubble = egui::Rect::from_min_max(egui::pos2(50.0, 300.0), egui::pos2(400.0, 380.0));
        // Room below: dock under bubble + strip.
        let y = hover_bar_anchor_y(bubble, 18.0, 36.0, clip, None);
        assert_eq!(y, 380.0 + 18.0 + 4.0);
        // Near the viewport bottom: fall back above the bubble.
        let low = egui::Rect::from_min_max(egui::pos2(50.0, 600.0), egui::pos2(400.0, 660.0));
        let y = hover_bar_anchor_y(low, 18.0, 36.0, clip, None);
        assert_eq!(y, 600.0 - 36.0 - 4.0);
        // Bubble clipped at the viewport top: tooltip above the pointer.
        let tall = egui::Rect::from_min_max(egui::pos2(50.0, 0.0), egui::pos2(400.0, 660.0));
        let y = hover_bar_anchor_y(tall, 18.0, 36.0, clip, Some(200.0));
        assert_eq!(y, 200.0 - 36.0 - 10.0);
        // Same shape without a pointer: clamp in view under the bubble.
        let y = hover_bar_anchor_y(tall, 18.0, 36.0, clip, None);
        assert_eq!(y, (660.0f32 + 6.0).min(700.0 - 36.0 - 4.0));
    }

    /// Regression: the multiline composer. Enter sends; Shift+Tab and
    /// Shift+Enter insert a line break at the cursor (egui would otherwise
    /// de-indent on Shift+Tab and newline on every Enter); none of the
    /// combos leak through when the field is not focused.
    #[test]
    fn composer_enter_sends_shift_tab_newline() {
        crate::emoji::setup();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 220.0));
        let blank = egui::RawInput { screen_rect: Some(screen), ..Default::default() };

        // Frame 1: paint once so the composer records its field id.
        let egui_ctx = crate::theme::test_ctx();
        let mut draft = "hello".to_string();
        let (field_id, _) =
            draw_composer_frame(&egui_ctx, blank.clone(), &mut draft, None);
        let field_id = field_id.expect("composer must record its field id");

        // Enter (focused) sends and clears.
        let (_, actions) = draw_composer_frame(
            &egui_ctx,
            key_input(screen, egui::Key::Enter, egui::Modifiers::default()),
            &mut draft,
            Some(field_id),
        );
        assert!(
            matches!(actions.as_slice(), [Action::Send(t)] if t == "hello"),
            "{actions:?}"
        );
        assert!(draft.is_empty());

        // Fresh context: Shift+Tab breaks the line, cursor lands after it.
        let egui_ctx = crate::theme::test_ctx();
        let mut draft = "hello".to_string();
        let (field_id, _) =
            draw_composer_frame(&egui_ctx, blank.clone(), &mut draft, None);
        let field_id = field_id.unwrap();
        let (_, actions) = draw_composer_frame(
            &egui_ctx,
            key_input(screen, egui::Key::Tab, egui::Modifiers::SHIFT),
            &mut draft,
            Some(field_id),
        );
        assert_eq!(draft, "hello\n");
        assert!(actions.is_empty(), "{actions:?}");
        let type_x = egui::RawInput {
            screen_rect: Some(screen),
            events: vec![egui::Event::Text("x".into())],
            ..Default::default()
        };
        draw_composer_frame(&egui_ctx, type_x, &mut draft, Some(field_id));
        assert_eq!(draft, "hello\nx", "cursor must sit after the break");

        // Shift+Enter breaks too.
        let egui_ctx = crate::theme::test_ctx();
        let mut draft = "hello".to_string();
        let (field_id, _) =
            draw_composer_frame(&egui_ctx, blank.clone(), &mut draft, None);
        let field_id = field_id.unwrap();
        let (_, actions) = draw_composer_frame(
            &egui_ctx,
            key_input(screen, egui::Key::Enter, egui::Modifiers::SHIFT),
            &mut draft,
            Some(field_id),
        );
        assert_eq!(draft, "hello\n");
        assert!(actions.is_empty(), "{actions:?}");

        // Unfocused combos leak nowhere: no send, no break.
        let egui_ctx = crate::theme::test_ctx();
        let mut draft = "hello".to_string();
        let _ = draw_composer_frame(&egui_ctx, blank.clone(), &mut draft, None);
        let (_, actions) = draw_composer_frame(
            &egui_ctx,
            key_input(screen, egui::Key::Enter, egui::Modifiers::default()),
            &mut draft,
            None,
        );
        let (_, actions2) = draw_composer_frame(
            &egui_ctx,
            key_input(screen, egui::Key::Tab, egui::Modifiers::SHIFT),
            &mut draft,
            None,
        );
        assert!(actions.is_empty() && actions2.is_empty());
        assert_eq!(draft, "hello");
    }
}
