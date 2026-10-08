//! Section panels: Calendar, Files, To Do, Activity. Pure views — data
//! arrives via App; actions flow back through the shared Action enum.

use crate::model::format_message_time;
use crate::theme::Palette;
use crate::ui::conversation::Action;
use egui::{Color32, RichText, ScrollArea, Ui};
use ost::api::{MeetingInfo, SharedFile, TodoListInfo, TodoTaskInfo};
use std::collections::HashMap;

// ----------------------------------------------------------------- calendar

/// Upcoming meetings, soonest first. Day-grouped list; each row shows the
/// join link actions when the event has one.
/// Centered body note for loading/empty states. Drawn with the painter
/// (no layout): ScrollAreas and nested layouts collapse available rects,
/// which pinned every layout-based attempt to the top.
fn body_note(ui: &mut Ui, spinner: bool, text: &str) {
    let _ = spinner;
    let rect = ui.max_rect();
    let center = egui::pos2(
        rect.center().x,
        rect.top() + (rect.height() * 0.45).max(90.0),
    );
    ui.painter().text(
        center,
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(13.0),
        egui::Color32::from_rgb(0x9a, 0x9f, 0xa8),
    );
}

pub fn calendar_panel(
    ui: &mut Ui,
    meetings: &[MeetingInfo],
    loading: bool,
    pal: &Palette,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.heading(RichText::new("Calendar").strong().size(17.0));
        ui.label(
            RichText::new("next 7 days")
                .small()
                .weak(),
        );
        if loading {
            ui.spinner();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button(RichText::new("Meet now").small())
                .on_hover_text("Create an instant meeting and join it")
                .clicked()
            {
                actions.push(Action::MeetNow);
            }
            if ui
                .small_button(RichText::new("Join with link…").small())
                .on_hover_text("Paste a Teams meeting link or ID")
                .clicked()
            {
                actions.push(Action::ShowJoinDialog);
            }
            if ui.small_button(RichText::new("Refresh").small()).clicked() {
                actions.push(Action::ReloadSection);
            }
        });
    });
        if loading && meetings.is_empty() {
            body_note(ui, true, "Loading calendar…");
            return;
        }
        if meetings.is_empty() {
            body_note(ui, false, "No meetings in the coming week.");
            return;
        }

    ui.separator();
    ScrollArea::vertical().id_salt("cal_scroll").auto_shrink(false).show(ui, |ui| {

        let mut last_day = String::new();
        for m in meetings {
            let day = m
                .start
                .as_deref()
                .map(|s| s.get(0..10).unwrap_or("").to_string())
                .unwrap_or_default();
            if !day.is_empty() && day != last_day {
                last_day = day.clone();
                ui.add_space(6.0);
                crate::ui::widgets::day_separator(ui, &day);
                ui.add_space(6.0);
            }
            meeting_row(ui, m, pal, actions);
        }
    });
}

fn meeting_row(ui: &mut Ui, m: &MeetingInfo, pal: &Palette, actions: &mut Vec<Action>) {
    let time = m
        .start
        .as_deref()
        .map(|s| format_message_time(s))
        .unwrap_or_default();
    egui::Frame::default()
        .fill(pal.surface)
        .stroke(egui::Stroke::new(1.0, pal.outline))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(time).small().color(pal.link).strong());
                ui.label(
                    RichText::new(if m.subject.is_empty() {
                        "(no title)"
                    } else {
                        &m.subject
                    })
                    .strong(),
                );
            });
            ui.horizontal(|ui| {
                if let Some(who) = &m.organizer {
                    ui.label(RichText::new(format!("by {who}")).small().weak());
                }
                if m.is_organizer {
                    ui.label(RichText::new("organizer").small().weak());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(url) = &m.join_url {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Join").small().color(pal.on_accent),
                                )
                                .fill(pal.accent)
                                .min_size(egui::vec2(54.0, 20.0)),
                            )
                            .on_hover_text("Join this meeting in TeamsFast")
                            .clicked()
                        {
                            actions.push(Action::JoinMeeting {
                                source: url.clone(),
                                label: Some(if m.subject.is_empty() {
                                    "Meeting".to_string()
                                } else {
                                    m.subject.clone()
                                }),
                            });
                        }
                        if ui
                            .small_button(RichText::new("Copy link").small())
                            .clicked()
                        {
                            actions.push(Action::CopyText(url.clone()));
                        }
                    }
                });
            });
        });
    ui.add_space(4.0);
}

// ------------------------------------------------------------------- files

pub fn files_panel(
    ui: &mut Ui,
    files: &[SharedFile],
    loading: bool,
    pal: &Palette,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.heading(RichText::new("Files").strong().size(17.0));
        ui.label(RichText::new("recent in OneDrive").small().weak());
        if loading {
            ui.spinner();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(RichText::new("Refresh").small()).clicked() {
                actions.push(Action::ReloadSection);
            }
        });
    });
        if loading && files.is_empty() {
            body_note(ui, true, "Loading files…");
            return;
        }
        if files.is_empty() {
            body_note(ui, false, "No recent files.");
            return;
        }

    ui.separator();
    ScrollArea::vertical().id_salt("files_scroll").auto_shrink(false).show(ui, |ui| {

        for f in files {
            file_row(ui, f, pal, actions);
        }
    });
}

fn file_row(ui: &mut Ui, f: &SharedFile, pal: &Palette, actions: &mut Vec<Action>) {
    let size = if f.size >= 1_048_576 {
        format!("{:.1} MB", f.size as f64 / 1_048_576.0)
    } else if f.size >= 1024 {
        format!("{:.0} KB", f.size as f64 / 1024.0)
    } else if f.size > 0 {
        format!("{} B", f.size)
    } else {
        "—".to_string()
    };
    let modified = f
        .modified
        .as_deref()
        .map(format_message_time)
        .unwrap_or_default();
    let is_folder = f.is_folder;
    let (rect, mut response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width() - 4.0, 40.0),
        egui::Sense::click(),
    );
    let hovered = response.hovered();
    if hovered {
        ui.painter().rect_filled(rect, 6, pal.surface_hover);
    }
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(8.0, 2.0))));
    {
        let ui = &mut child;
        ui.horizontal(|ui| {
            let icon = if is_folder {
                crate::theme::Icon::Archive
            } else {
                crate::theme::Icon::FileText
            };
            ui.add(
                egui::Image::from_bytes(icon.uri(), icon.bytes())
                    .tint(pal.secondary)
                    .fit_to_exact_size(egui::Vec2::splat(16.0)),
            );
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 150.0);
                ui.add(
                    egui::Label::new(RichText::new(&f.name).strong())
                        .truncate()
                        .selectable(false),
                );
                ui.label(
                    RichText::new(if modified.is_empty() {
                        size.clone()
                    } else {
                        format!("{size} · {modified}")
                    })
                    .small()
                    .weak(),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !is_folder {
                    if let (Some(drive), Some(item)) = (&f.drive_id, Some(&f.id)) {
                        let (drive, item, name) =
                            (drive.clone(), item.clone(), f.name.clone());
                        if ui
                            .add(egui::Button::new(
                                crate::theme::Icon::Download.image(pal.secondary, 14.0),
                            ))
                            .on_hover_text("Download and open")
                            .clicked()
                        {
                            actions.push(Action::DownloadDriveFile {
                                drive_id: drive,
                                item_id: item,
                                name,
                            });
                        }
                    }
                    if let Some(web) = &f.web_url {
                        if ui
                            .add(egui::Button::new(
                                crate::theme::Icon::ExternalLink.image(pal.secondary, 14.0),
                            ))
                            .on_hover_text("Open online")
                            .clicked()
                        {
                            actions.push(Action::OpenLink(web.clone()));
                        }
                    }
                }
            });
        });
    }
    response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.clicked() {
        // Click a folder/file: open online when possible.
        if let Some(web) = &f.web_url {
            actions.push(Action::OpenLink(web.clone()));
        }
    }
    ui.separator();
}

/// Files shared within one conversation (shown above the drive recents).
pub fn chat_files_panel(
    ui: &mut Ui,
    _chat_id: &str,
    chat_name: &str,
    files: &[SharedFile],
    loading: bool,
    pal: &Palette,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.heading(
            RichText::new(format!("Files · {chat_name}"))
                .strong()
                .size(15.0),
        );
        if loading {
            ui.spinner();
        }
    });
    ui.separator();
    ScrollArea::vertical()
        .id_salt(("chat_files", _chat_id))
        .max_height(260.0)
        .show(ui, |ui| {
            if files.is_empty() && !loading {
                ui.label(RichText::new("No files shared in this conversation.").weak());
            }
            for f in files {
                file_row(ui, f, pal, actions);
            }
        });
}

// ------------------------------------------------------------------- to-do

pub struct TodoPanelState {
    pub selected_list: Option<String>,
    pub quick_add: String,
    pub show_completed: bool,
}

impl Default for TodoPanelState {
    fn default() -> Self {
        Self {
            selected_list: None,
            quick_add: String::new(),
            show_completed: false,
        }
    }
}

/// Two-pane: lists on the left, tasks of the selected list on the right.
pub fn todo_panel(
    ui: &mut Ui,
    lists: &[TodoListInfo],
    tasks: &HashMap<String, Vec<TodoTaskInfo>>,
    st: &mut TodoPanelState,
    loading: bool,
    pal: &Palette,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.heading(RichText::new("To Do").strong().size(17.0));
        if loading {
            ui.spinner();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button(RichText::new(if st.show_completed {
                    "Hide completed"
                } else {
                    "Show completed"
                })
                .small())
                .clicked()
            {
                st.show_completed = !st.show_completed;
            }
            if ui.small_button(RichText::new("Refresh").small()).clicked() {
                actions.push(Action::ReloadSection);
            }
        });
    });
    ui.separator();
    if lists.is_empty() && !loading {
        ui.label(RichText::new("No To Do lists found.").weak());
        return;
    }
    let body_h = ui.available_height();
    ui.horizontal(|ui| {
        // Lists column.
        ui.allocate_ui(egui::vec2(200.0, body_h), |ui| {
            ScrollArea::vertical().id_salt("todo_lists_scroll").auto_shrink(false).show(ui, |ui| {
                // Parent is a horizontal row — force the column layout.
                ui.vertical(|ui| {
                for l in lists {
                    let sel = st.selected_list.as_deref() == Some(l.id.as_str());
                    if ui
                        .selectable_label(sel, RichText::new(&l.name).strong())
                        .clicked()
                    {
                        st.selected_list = Some(l.id.clone());
                        actions.push(Action::OpenTodoList(l.id.clone()));
                    }
                }
                })
            });
        });
        // Full-height divider between the columns (the stock vertical
        // Separator only spans the row's content height).
        {
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(1.0, body_h),
                egui::Sense::hover(),
            );
            ui.painter().vline(
                rect.center().x,
                rect.top()..=rect.bottom(),
                egui::Stroke::new(1.0, pal.outline),
            );
        }
        // Tasks column.
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            let Some(list) = st.selected_list.clone().or_else(|| lists.first().map(|l| l.id.clone())) else {
                ui.label(RichText::new("Pick a list.").weak());
                return;
            };
            if st.selected_list.is_none() {
                st.selected_list = Some(list.clone());
                actions.push(Action::OpenTodoList(list.clone()));
            }
            let empty: Vec<TodoTaskInfo> = Vec::new();
            let items = tasks.get(&list).unwrap_or(&empty);
            // Quick add.
            ui.horizontal(|ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut st.quick_add)
                        .hint_text("Add a task…")
                        .desired_width(ui.available_width() - 64.0),
                );
                let add = ui.button("Add").clicked()
                    || (field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                if add {
                    let title = st.quick_add.trim().to_string();
                    if !title.is_empty() {
                        actions.push(Action::AddTodoTask {
                            list_id: list.clone(),
                            title,
                        });
                        st.quick_add.clear();
                    }
                }
            });
            ui.add_space(4.0);
            ScrollArea::vertical().id_salt("todo_tasks_scroll").auto_shrink(false).show(ui, |ui| {
                let mut shown = 0;
                for t in items {
                    let done = t.status == "completed";
                    if done && !st.show_completed {
                        continue;
                    }
                    shown += 1;
                    ui.horizontal(|ui| {
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(18.0, 18.0),
                            egui::Sense::click(),
                        );
                        let stroke_col = if done { pal.ok } else { pal.outline };
                        ui.painter().rect_stroke(
                            rect,
                            4,
                            egui::Stroke::new(1.4, stroke_col),
                            egui::StrokeKind::Inside,
                        );
                        if done {
                            ui.painter().rect_filled(
                                rect.shrink(4.0),
                                3,
                                pal.ok,
                            );
                        }
                        if resp.clicked() {
                            actions.push(Action::SetTodoDone {
                                list_id: list.clone(),
                                task_id: t.id.clone(),
                                done: !done,
                            });
                        }
                        ui.add(
                            egui::Label::new(RichText::new(&t.title).color(if done {
                                pal.dim
                            } else {
                                pal.text
                            }))
                            .truncate()
                            .selectable(false),
                        )
                        .on_hover_text(format!(
                            "status: {}  importance: {}",
                            t.status, t.importance
                        ));
                        if let Some(due) = &t.due {
                            ui.label(
                                RichText::new(format_message_time(due))
                                    .small()
                                    .weak(),
                            );
                        }
                    });
                }
                if shown == 0 {
                    ui.label(RichText::new("Nothing here.").weak());
                }
            });
        });
    });
}

// ----------------------------------------------------------------- activity

/// One activity entry (mentions / reactions / replies) mined from live
/// events or notifications.
#[derive(Clone, Debug)]
pub struct ActivityEntry {
    pub kind: &'static str, // "mention" | "reaction" | "reply" | "message"
    pub chat_id: String,
    pub chat_name: String,
    pub who: String,
    pub preview: String,
    pub at_ms: u64,
}

pub fn activity_panel(
    ui: &mut Ui,
    entries: &[ActivityEntry],
    pal: &Palette,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.heading(RichText::new("Activity").strong().size(17.0));
        ui.label(RichText::new("mentions, reactions, replies").small().weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(
                    !entries.is_empty(),
                    egui::Button::new(RichText::new("Clear").small()),
                )
                .on_hover_text("Clear the activity feed")
                .clicked()
            {
                actions.push(Action::ClearActivity);
            }
        });
    });
    ui.separator();
    ScrollArea::vertical().id_salt("activity_scroll").auto_shrink(false).show(ui, |ui| {
        if entries.is_empty() {
            body_note(ui, false, "Nothing yet — mentions and reactions land here.");
        }
        for e in entries {
            let (icon, tint) = match e.kind {
                "mention" => (crate::theme::Icon::AtSign, pal.warning),
                "reaction" => (crate::theme::Icon::Smile, pal.ok),
                "reply" => (crate::theme::Icon::Reply, pal.link),
                _ => (crate::theme::Icon::MessageSquare, pal.secondary),
            };
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(ui.available_width() - 4.0, 44.0),
                egui::Sense::click(),
            );
            if response.hovered() {
                ui.painter().rect_filled(rect, 6, pal.surface_hover);
            }
            let mut child = ui.new_child(
                egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(8.0, 3.0))),
            );
            {
                let ui = &mut child;
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Image::from_bytes(icon.uri(), icon.bytes())
                            .tint(tint)
                            .fit_to_exact_size(egui::Vec2::splat(15.0)),
                    );
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - 10.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(&e.chat_name)
                                    .strong()
                                    .color(Color32::WHITE),
                            );
                            ui.label(
                                RichText::new(format!("· {}", e.kind))
                                    .small()
                                    .weak(),
                            );
                        });
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("{}: {}", e.who, e.preview))
                                    .small()
                                    .weak(),
                            )
                            .truncate()
                            .selectable(false),
                        );
                    });
                });
            }
            if response.clicked() {
                actions.push(Action::OpenChat(e.chat_id.clone()));
            }
            ui.separator();
        }
    });
}

// ----------------------------------------------------------------- planner

/// One planner board: a plan's buckets and tasks.
#[derive(Clone, Debug)]
pub struct PlannerBoard {
    pub team: String,
    pub plan: String,
    pub buckets: Vec<(String, String)>, // (id, name)
    pub tasks: Vec<ost::api::PlannerTaskInfo>,
}

/// Planner boards grouped by bucket columns.
pub fn planner_panel(
    ui: &mut Ui,
    boards: &[PlannerBoard],
    loading: bool,
    pal: &Palette,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.heading(RichText::new("Planner").strong().size(17.0));
        ui.label(RichText::new("boards from your teams").small().weak());
        if loading {
            ui.spinner();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(RichText::new("Refresh").small()).clicked() {
                actions.push(Action::ReloadSection);
            }
        });
    });
        if loading && boards.is_empty() {
            body_note(ui, true, "Loading planner…");
            return;
        }
        if boards.is_empty() {
            body_note(ui, false, "No planner tasks found in your teams.");
            return;
        }

    ui.separator();
    ScrollArea::vertical()
        .id_salt("planner_scroll")
        .auto_shrink(false)
        .show(ui, |ui| {
            for board in boards {
                ui.horizontal(|ui| {
                    ui.strong(&board.plan);
                    ui.label(
                        RichText::new(format!("· {}", board.team))
                            .small()
                            .weak(),
                    );
                });
                ui.add_space(2.0);
                let bucket_name = |id: &str| -> String {
                    board
                        .buckets
                        .iter()
                        .find(|(bid, _)| bid == id)
                        .map(|(_, n)| n.clone())
                        .unwrap_or_else(|| "No bucket".into())
                };
                egui::Grid::new(("planner-grid", &board.plan, &board.team))
                    .num_columns(2)
                    .spacing([10.0, 2.0])
                    .show(ui, |ui| {
                        for t in &board.tasks {
                            ui.horizontal(|ui| {
                                let (rect, resp) = ui.allocate_exact_size(
                                    egui::vec2(16.0, 16.0),
                                    egui::Sense::click(),
                                );
                                let col = if t.completed { pal.ok } else { pal.outline };
                                ui.painter().rect_stroke(
                                    rect,
                                    4,
                                    egui::Stroke::new(1.3, col),
                                    egui::StrokeKind::Inside,
                                );
                                if t.completed {
                                    ui.painter().rect_filled(rect.shrink(4.0), 3, pal.ok);
                                }
                                if resp.clicked() {
                                    actions.push(Action::SetPlannerDone {
                                        task_id: t.id.clone(),
                                        etag: t.etag.clone(),
                                        done: !t.completed,
                                    });
                                }
                            });
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&t.title).color(if t.completed {
                                            pal.dim
                                        } else {
                                            pal.text
                                        }),
                                    );
                                    if t.percent_complete > 0 && !t.completed {
                                        ui.label(
                                            RichText::new(format!(
                                                "{}%",
                                                t.percent_complete
                                            ))
                                            .small()
                                            .weak(),
                                        );
                                    }
                                    if let Some(due) = &t.due {
                                        ui.label(
                                            RichText::new(format_message_time(due))
                                                .small()
                                                .weak(),
                                        );
                                    }
                                });
                                ui.label(
                                    RichText::new(bucket_name(&t.bucket_id))
                                        .small()
                                        .weak(),
                                );
                            });
                            ui.end_row();
                        }
                    });
                ui.add_space(8.0);
                ui.separator();
            }
        });
}

// ----------------------------------------------------------------- shifts

/// This week's shifts, grouped per person.
pub fn shifts_panel(
    ui: &mut Ui,
    shifts: &[ost::api::ShiftInfo],
    loading: bool,
    pal: &Palette,
) {
    ui.horizontal(|ui| {
        ui.heading(RichText::new("Shifts").strong().size(17.0));
        ui.label(RichText::new("this week").small().weak());
        if loading {
            ui.spinner();
        }
    });
        if loading && shifts.is_empty() {
            body_note(ui, true, "Loading shifts…");
            return;
        }
        if shifts.is_empty() {
            body_note(ui, false, "No shifts this week (no team schedule or none shared).");
            return;
        }

    ui.separator();
    ScrollArea::vertical()
        .id_salt("shifts_scroll")
        .auto_shrink(false)
        .show(ui, |ui| {
            for sh in shifts {
                egui::Frame::default()
                    .fill(pal.surface)
                    .stroke(egui::Stroke::new(1.0, pal.outline))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&sh.display_name).strong());
                            if sh.is_draft {
                                ui.label(
                                    RichText::new("draft").small().weak(),
                                );
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let start = sh
                                        .start
                                        .as_deref()
                                        .map(format_message_time)
                                        .unwrap_or_default();
                                    let end = sh
                                        .end
                                        .as_deref()
                                        .map(format_message_time)
                                        .unwrap_or_default();
                                    if !start.is_empty() {
                                        ui.label(
                                            RichText::new(format!("{start} – {end}"))
                                                .small()
                                                .color(pal.link),
                                        );
                                    }
                                },
                            );
                        });
                        if let Some(notes) = &sh.notes {
                            if !notes.trim().is_empty() {
                                ui.label(RichText::new(notes).small().weak());
                            }
                        }
                    });
                ui.add_space(4.0);
            }
        });
}

// ----------------------------------------------------------------- onenote

/// Notebook → sections → pages tree (app-side; core returns flat pieces).
#[derive(Clone, Debug)]
pub struct NotebookTree {
    pub name: String,
    pub sections: Vec<ost::api::SectionInfo>,
}

pub struct NotesPanelState {
    pub open_page: Option<(String, String)>, // (page_id, title)
    pub append_text: String,
}

impl Default for NotesPanelState {
    fn default() -> Self {
        Self {
            open_page: None,
            append_text: String::new(),
        }
    }
}

/// OneNote browser: notebooks → sections → pages; a selected page renders
/// its text with an append box.
pub fn notes_panel(
    ui: &mut Ui,
    notebooks: &[NotebookTree],
    page: Option<&ost::api::NotePage>,
    st: &mut NotesPanelState,
    loading: bool,
    pal: &Palette,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.heading(RichText::new("OneNote").strong().size(17.0));
        if loading {
            ui.spinner();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(RichText::new("Refresh").small()).clicked() {
                actions.push(Action::ReloadSection);
            }
        });
    });
    ui.separator();
    let body_h = ui.available_height();
    ui.horizontal(|ui| {
        // Nav column.
        ui.allocate_ui(egui::vec2(240.0, body_h), |ui| {
            ScrollArea::vertical()
                .id_salt("notes_nav")
                .auto_shrink(false)
                .show(ui, |ui| {
                    // A ScrollArea inherits the parent layout — this one
                    // lives in a horizontal row, so force a column.
                    ui.vertical(|ui| {
                    if notebooks.is_empty() && !loading {
                        ui.label(RichText::new("No notebooks found.").weak());
                    }
                    for nb in notebooks {
                        ui.strong(&nb.name);
                        for sec in &nb.sections {
                            ui.label(
                                RichText::new(format!("  {}", sec.name)).small().weak(),
                            );
                            for p in &sec.pages {
                                let sel = st
                                    .open_page
                                    .as_ref()
                                    .map(|(id, _)| id == &p.id)
                                    .unwrap_or(false);
                                if ui
                                    .selectable_label(sel, RichText::new(p.title.clone()).small())
                                    .clicked()
                                {
                                    st.open_page = Some((p.id.clone(), p.title.clone()));
                                    actions.push(Action::ReadNotePage(p.id.clone()));
                                }
                            }
                        }
                        ui.add_space(4.0);
                    }
                    });
                });
        });
        // Divider.
        {
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(1.0, body_h),
                egui::Sense::hover(),
            );
            ui.painter().vline(
                rect.center().x,
                rect.top()..=rect.bottom(),
                egui::Stroke::new(1.0, pal.outline),
            );
        }
        // Page column.
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            match page {
                Some(p) => {
                    ui.strong(&p.title);
                    ui.separator();
                    ScrollArea::vertical()
                        .id_salt("notes_page")
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            // Render the page HTML as plain segments.
                            let segs =
                                crate::ui::widgets::parse_html(&p.html);
                            crate::ui::widgets::render_segments(
                                ui,
                                &segs,
                                |_, _| false,
                                |_, _, _| {},
                            );
                        });
                    ui.add_space(4.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        let field = ui.add(
                            egui::TextEdit::singleline(&mut st.append_text)
                                .hint_text("Append to this page…")
                                .desired_width(ui.available_width() - 66.0),
                        );
                        let send = ui.button("Append").clicked()
                            || (field.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                        if send && !st.append_text.trim().is_empty() {
                            if let Some((id, _)) = st.open_page.clone() {
                                actions.push(Action::AppendNote {
                                    page_id: id,
                                    text: st.append_text.trim().to_string(),
                                });
                                st.append_text.clear();
                            }
                        }
                    });
                }
                None => {
                    ui.label(RichText::new("Pick a page on the left.").weak());
                }
            }
        });
    });
}
