//! Left sidebar: view switch (Chats / Teams), search field, chat list.

use crate::backend::Command;
use crate::model::{clean_preview, format_chat_time};
use crate::theme::Palette;
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
    pub pal: &'a Palette,
    /// Unread state per chat id: (is-unread, approximate count).
    pub unread: &'a std::collections::HashMap<String, (bool, u32)>,
    /// Whether count badges are enabled (settings).
    pub show_badges: bool,
    /// Locally pinned chat ids (icon hint; App owns ordering).
    pub pinned: &'a std::collections::HashSet<String>,
    /// Known muted chat ids (bell-off hint).
    pub muted: &'a std::collections::HashSet<String>,
    /// Per-chat notification level map ("all" | "mentions" | "off").
    pub notify_levels: &'a std::collections::HashMap<String, String>,
}

impl SidebarCtx<'_> {
    pub fn notify_level(&self, chat_id: &str) -> &str {
        self.notify_levels
            .get(chat_id)
            .map(|s| s.as_str())
            .unwrap_or("all")
    }
}

pub fn sidebar(
    ui: &mut Ui,
    ctx: &mut SidebarCtx<'_>,
    search: &mut String,
    loading_teams: &mut bool,
    actions: &mut Vec<Action>,
) {
    ui.add_space(4.0);

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
                    chat_row(ui, ctx, chat, actions);
                }
            });
        }
        SideView::Teams => {
            ui.horizontal(|ui| {
                if ui
                    .small_button(RichText::new("+ Channel").small())
                    .on_hover_text("Create a channel / join or create a team")
                    .clicked()
                {
                    actions.push(Action::ShowTeamDialog);
                }
            });
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
                            actions.push(Action::OpenChat(ch.id.clone()));
                        }
                    }
                }
            });
        }
    }
}

fn display_name(chat: &ChatInfo) -> String {
    match chat.name.as_str() {
        "" => {
            let mut s: String = chat.id.chars().take(24).collect();
            s.push('…');
            s
        }
        "[Direct message]" => "Direct message".into(),
        other => other.to_string(),
    }
}

/// One chat row. Returns the row rect (for tests).
///
/// egui pattern note: the row's clickable Response is allocated FIRST and
/// the content painted inside a child Ui afterwards. Registering the click
/// AFTER painting (`.allocate_ui(..).response.interact(..)`) gets shadowed
/// by the hover-sense labels inside, and clicks silently do nothing.
fn chat_row(
    ui: &mut Ui,
    ctx: &SidebarCtx<'_>,
    chat: &ChatInfo,
    actions: &mut Vec<Action>,
) -> egui::Rect {
    let selected = ctx.selected == Some(&chat.id);
    let label = display_name(chat);
    let time = format_chat_time(&chat.last_message_time);
    let unread = ctx.unread.get(&chat.id).copied().unwrap_or((false, 0));

    let avail = ui.available_width() - 6.0;
    let (rect, mut response) =
        ui.allocate_exact_size(egui::vec2(avail, 44.0), egui::Sense::click());
    response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    // Row background: selection / hover.
    let bg = if selected {
        egui::Color32::from_rgba_unmultiplied(
            ctx.pal.accent.r(),
            ctx.pal.accent.g(),
            ctx.pal.accent.b(),
            0x50,
        )
    } else if response.hovered() {
        ctx.pal.surface_hover
    } else {
        egui::Color32::TRANSPARENT
    };
    if bg != egui::Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 6.0, bg);
    }
    if selected {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(rect.left_top(), egui::vec2(3.0, rect.height())),
            2.0,
            ctx.pal.accent,
        );
    }

    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(
        rect.shrink2(egui::vec2(6.0, 3.0)),
    ));
    {
        let ui = &mut child;
        ui.horizontal(|ui| {
                ui.add_space(2.0);
                avatar(ui, &label, 30.0);
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        // Reserve the right column so long names can never
                        // collide with the timestamp/group tag.
                        let name_w = (ui.available_width() - 78.0).max(60.0);
                        ui.allocate_ui(egui::vec2(name_w, 16.0), |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&label)
                                        .strong()
                                        .color(if unread.0 {
                                            ctx.pal.text
                                        } else {
                                            ctx.pal.secondary
                                        }),
                                )
                                .truncate()
                                .selectable(false),
                            );
                        });
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                // Unread badge: count pill when we have one,
                                // plain dot when the messages aren't cached.
                                if unread.0 && ctx.show_badges {
                                    let count = unread.1;
                                    if count > 0 {
                                        let text = if count > 99 {
                                            "99+".to_string()
                                        } else {
                                            count.to_string()
                                        };
                                        let pad = 7.0;
                                        let w = text.len() as f32 * 6.5 + pad * 2.0;
                                        let (r, _) = ui.allocate_exact_size(
                                            egui::vec2(w, 15.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().rect_filled(
                                            r,
                                            7.5,
                                            ctx.pal.accent,
                                        );
                                        ui.painter().text(
                                            r.center(),
                                            egui::Align2::CENTER_CENTER,
                                            text,
                                            egui::FontId::proportional(10.0),
                                            ctx.pal.on_accent,
                                        );
                                    } else {
                                        let (r, _) = ui.allocate_exact_size(
                                            egui::vec2(10.0, 10.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().circle_filled(
                                            r.center(),
                                            3.5,
                                            ctx.pal.accent,
                                        );
                                    }
                                }
                                if !time.is_empty() {
                                    ui.label(
                                        RichText::new(time)
                                            .small()
                                            .color(ctx.pal.secondary),
                                    );
                                }
                                if ctx.pinned.contains(&chat.id) {
                                    ui.add(
                                        egui::Image::from_bytes(
                                            crate::theme::Icon::Pin.uri(),
                                            crate::theme::Icon::Pin.bytes(),
                                        )
                                        .tint(ctx.pal.secondary)
                                        .fit_to_exact_size(egui::Vec2::splat(11.0)),
                                    );
                                }
                                if ctx.muted.contains(&chat.id) {
                                    ui.add(
                                        egui::Image::from_bytes(
                                            crate::theme::Icon::BellOff.uri(),
                                            crate::theme::Icon::BellOff.bytes(),
                                        )
                                        .tint(ctx.pal.secondary)
                                        .fit_to_exact_size(egui::Vec2::splat(11.0)),
                                    );
                                }
                                if chat.is_group {
                                    ui.label(
                                        RichText::new("👥")
                                            .size(13.0)
                                            .color(ctx.pal.secondary),
                                    );
                                }
                            },
                        );
                    });
                    if let Some(p) = &chat.last_message_preview {
                        ui.add(
                            egui::Label::new(
                                RichText::new(clean_preview(p))
                                    .small()
                                    .color(if unread.0 {
                                        ctx.pal.text
                                    } else {
                                        ctx.pal.secondary
                                    }),
                            )
                            .truncate()
                            .selectable(false),
                        );
                    }
                });
            });
    }

    // Rows act as whole buttons: non-selectable labels (egui text selection
    // would otherwise swallow clicks on the name/preview).
    if response.clicked() {
        // Push the ACTION (App::apply updates `selected` first). Sending the
        // raw Command here bypasses App state and the history gets dropped
        // on a selected-mismatch guard.
        actions.push(Action::OpenChat(chat.id.clone()));
    }
    // Right-click menu: pin / mute / mark-unread / hide / leave.
    response.context_menu(|ui| {
        let pinned = ctx.pinned.contains(&chat.id);
        if ui
            .button(if pinned { "Unpin from top" } else { "Pin to top" })
            .clicked()
        {
            actions.push(Action::TogglePin(chat.id.clone()));
            ui.close();
        }
        let muted = ctx.muted.contains(&chat.id) || ctx.notify_level(&chat.id) == "off";
        if ui
            .button(if muted { "Unmute" } else { "Mute notifications" })
            .clicked()
        {
            actions.push(Action::ToggleMute {
                chat_id: chat.id.clone(),
                muted: !muted,
            });
            ui.close();
        }
        if ui.button("Mark as unread").clicked() {
            actions.push(Action::MarkUnread(chat.id.clone()));
            ui.close();
        }
        ui.separator();
        let level = ctx.notify_level(&chat.id);
        ui.label(RichText::new("Notifications").small().weak());
        for (key, text) in [("all", "All messages"), ("mentions", "Mentions only"), ("off", "Off")] {
            if ui
                .selectable_label(
                    level == key,
                    RichText::new(format!("{}  {text}", if level == key { "●" } else { "○" }))
                        .small(),
                )
                .clicked()
            {
                actions.push(Action::SetNotifyLevel {
                    chat_id: chat.id.clone(),
                    level: key.to_string(),
                });
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Hide chat").clicked() {
            actions.push(Action::HideChat(chat.id.clone()));
            ui.close();
        }
        if chat.is_group && ui.button("Leave chat").clicked() {
            actions.push(Action::LeaveChat(chat.id.clone()));
            ui.close();
        }
    });
    rect
}

#[cfg(test)]
mod tests {
    use super::*;
    use ost::api::ChatInfo;

    fn palette() -> Palette {
        Palette::dark()
    }

    fn test_chat(id: &str, name: &str) -> ChatInfo {
        ChatInfo {
            id: id.into(),
            name: name.into(),
            is_group: false,
            last_message_time: None,
            last_message_sender: None,
            last_message_preview: Some("preview".into()),
        }
    }

    /// Clicking a chat row must send `Command::OpenChat` with its id.
    #[test]
    fn clicking_a_chat_row_opens_the_chat() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _guard = rt.enter(); // UnboundedSender needs no runtime, but keep one anyway

        let (cmd, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
        let chat = test_chat("19:chat_a@thread.v2", "Ada Lovelace");
        let chats = vec![test_chat("19:other@thread.v2", "Other"), chat];

        let ctx = SidebarCtx {
            chats: &chats,
            selected: None,
            teams: &[],
            view: SideView::Chats,
            cmd: &cmd,
            pal: &palette(),
            unread: &std::collections::HashMap::new(),
            show_badges: true,
            pinned: &std::collections::HashSet::new(),
            muted: &std::collections::HashSet::new(),
            notify_levels: &std::collections::HashMap::new(),
        };

        let egui_ctx = egui::Context::default();
        let mut row_rect: Option<egui::Rect> = None;
        let actions_out = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Action>::new()));
        let actions_capture = actions_out.clone();

        // Frame 1: lay out two rows.
        let mut out = egui_ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(300.0, 800.0),
                )),
                ..Default::default()
            },
            |ui| {
                let mut actions = Vec::new();
                let r = chat_row(ui, &ctx, &chats[1], &mut actions); // "Ada Lovelace"
                *actions_out.borrow_mut() = actions;
                row_rect = Some(r);
            },
        );
        out.textures_delta.clear();
        let rect = row_rect.expect("row rect");
        assert!(rect.width() > 100.0 && rect.height() >= 40.0, "sane rect");

        // Frame 2: click the middle of the row.
        let center = rect.center();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(300.0, 800.0),
            )),
            events: vec![
                egui::Event::PointerButton {
                    pos: center,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos: center,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
            ..Default::default()
        };
        let mut out = egui_ctx.run_ui(input, |ui| {
            let mut actions = Vec::new();
            chat_row(ui, &ctx, &chats[1], &mut actions);
            assert_eq!(actions.len(), 1, "click must push OpenChat action");
            match &actions[0] {
                Action::OpenChat(id) => assert_eq!(id, "19:chat_a@thread.v2"),
                other => panic!("unexpected action: {other:?}"),
            }
        });
        out.textures_delta.clear();

        assert!(
            cmd_rx.try_recv().is_err(),
            "the raw command channel must NOT be used for opening chats"
        );
    }

    /// Clicking empty space next to/below the rows must NOT open a chat.
    #[test]
    fn clicking_off_a_row_does_nothing() {
        let (cmd, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
        let chats = vec![test_chat("19:a@thread.v2", "One")];
        let ctx = SidebarCtx {
            chats: &chats,
            selected: None,
            teams: &[],
            view: SideView::Chats,
            cmd: &cmd,
            pal: &palette(),
            unread: &std::collections::HashMap::new(),
            show_badges: true,
            pinned: &std::collections::HashSet::new(),
            muted: &std::collections::HashSet::new(),
            notify_levels: &std::collections::HashMap::new(),
        };
        let egui_ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(300.0, 800.0),
            )),
            events: vec![egui::Event::PointerButton {
                pos: egui::pos2(150.0, 700.0), // far below the single row
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
            ..Default::default()
        };
        let mut out = egui_ctx.run_ui(input, |ui| {
            let mut actions = Vec::new();
            chat_row(ui, &ctx, &chats[0], &mut actions);
            assert!(actions.is_empty(), "no action for stray click");
        });
        out.textures_delta.clear();
        assert!(cmd_rx.try_recv().is_err(), "no command for stray click");
    }

    /// A row with an unread entry renders the badge without breaking layout.
    #[test]
    fn unread_badge_renders_sane_row() {
        let (cmd, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
        let chats = vec![test_chat("19:badge@thread.v2", "Badge chat")];
        let mut unread = std::collections::HashMap::new();
        unread.insert("19:badge@thread.v2".to_string(), (true, 123));
        let ctx = SidebarCtx {
            chats: &chats,
            selected: None,
            teams: &[],
            view: SideView::Chats,
            cmd: &cmd,
            pal: &palette(),
            unread: &unread,
            show_badges: true,
            pinned: &std::collections::HashSet::new(),
            muted: &std::collections::HashSet::new(),
            notify_levels: &std::collections::HashMap::new(),
        };
        let egui_ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(300.0, 800.0),
            )),
            ..Default::default()
        };
        let mut out = egui_ctx.run_ui(input, |ui| {
            let mut actions = Vec::new();
            let rect = chat_row(ui, &ctx, &chats[0], &mut actions);
            assert!(rect.width() > 100.0 && rect.height() >= 40.0);
            assert!(actions.is_empty());
        });
        out.textures_delta.clear();
    }
}
