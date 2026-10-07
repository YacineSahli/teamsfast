//! Section domains: calendar, files, to-do. Each handler is best-effort;
//! failures surface as `Event::Error` with the section still rendering.

use super::{Event, Session};
use ost::api::client::TeamsClient;
use std::sync::mpsc::Sender;

fn client<'a>(ses: &'a Session) -> Option<&'a TeamsClient> {
    ses.client.as_ref()
}

/// `Command::LoadCalendar` — meetings for the coming week, soonest first.
pub async fn calendar(ses: &Session, tx: &Sender<Event>) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::list_upcoming_meetings_data(c, 30).await {
        Ok(meetings) => {
            let _ = tx.send(Event::Calendar(meetings));
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("calendar: {e:#}")));
            let _ = tx.send(Event::Calendar(Vec::new()));
        }
    }
}

/// `Command::LoadFiles` — recent OneDrive files.
pub async fn files(ses: &Session, tx: &Sender<Event>) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::list_drive_recents_data(c, 50).await {
        Ok(files) => {
            let _ = tx.send(Event::Files(files));
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("files: {e:#}")));
            let _ = tx.send(Event::Files(Vec::new()));
        }
    }
}

/// `Command::LoadTodo` — our To Do lists.
pub async fn todo_lists(ses: &Session, tx: &Sender<Event>) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::list_todo_lists_data(c).await {
        Ok(lists) => {
            let _ = tx.send(Event::TodoLists(lists));
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("to-do lists: {e:#}")));
            let _ = tx.send(Event::TodoLists(Vec::new()));
        }
    }
}

/// `Command::LoadTodoTasks` — tasks of one list.
pub async fn todo_tasks(ses: &Session, tx: &Sender<Event>, list_id: &str) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::list_todo_tasks_data(c, list_id, 50).await {
        Ok(tasks) => {
            let _ = tx.send(Event::TodoTasks {
                list_id: list_id.to_string(),
                tasks,
            });
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("to-do tasks: {e:#}")));
        }
    }
}

/// `Command::SetTodoDone` — complete / reopen one task.
pub async fn todo_set_done(
    ses: &Session,
    tx: &Sender<Event>,
    list_id: &str,
    task_id: &str,
    done: bool,
) {
    let Some(c) = client(ses) else {
        return;
    };
    let res = if done {
        ost::api::complete_todo_task_data(c, list_id, task_id).await
    } else {
        ost::api::reopen_todo_task_data(c, list_id, task_id).await
    };
    match res {
        Ok(_) => {
            let _ = tx.send(Event::Status("task updated ✓".into()));
            todo_tasks(ses, tx, list_id).await;
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("task update: {e:#}")));
        }
    }
}

/// `Command::AddTodoTask` — quick add into a list.
pub async fn todo_add(ses: &Session, tx: &Sender<Event>, list_id: &str, title: &str) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::create_todo_task_data(c, list_id, title).await {
        Ok(_) => {
            let _ = tx.send(Event::Status("task added ✓".into()));
            todo_tasks(ses, tx, list_id).await;
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("task add: {e:#}")));
        }
    }
}

/// `Command::DownloadDriveFile` — a OneDrive item to ~/Downloads, then open.
pub async fn download_drive_file(
    ses: &Session,
    tx: &Sender<Event>,
    drive_id: &str,
    item_id: &str,
    name: &str,
) {
    let Some(c) = client(ses) else {
        return;
    };
    let safe: String = name.chars().filter(|c| !"/\0".contains(*c)).take(120).collect();
    let dest = dirs_downloads().join(if safe.is_empty() { "download".into() } else { safe });
    match ost::api::download_file_data(c, drive_id, item_id, &dest.to_string_lossy()).await {
        Ok(bytes) => {
            let _ = tx.send(Event::DownloadDone {
                name: name.to_string(),
                path: dest.clone(),
            });
            if let Err(e) = open::that_detached(&dest) {
                log::debug!("open {}: {e:#}", dest.display());
            }
            let _ = bytes;
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("download: {e:#}")));
        }
    }
}

// ----------------------------------------------------------------- teams

/// `Command::CreateChannel` — add a standard channel to a team.
pub async fn create_channel(ses: &Session, tx: &Sender<Event>, team_id: &str, name: &str) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::create_channel_data(c, team_id, name, None).await {
        Ok(_) => {
            let _ = tx.send(Event::Status(format!("channel #{name} created ✓")));
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("create channel: {e:#}")));
        }
    }
}

/// `Command::SearchPublicTeams` — public teams matching a query.
pub async fn search_public_teams(ses: &Session, tx: &Sender<Event>, query: &str) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::search_public_teams_data(c, query, 20).await {
        Ok((_, teams)) => {
            let _ = tx.send(Event::PublicTeams(teams));
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("team search: {e:#}")));
            let _ = tx.send(Event::PublicTeams(Vec::new()));
        }
    }
}

/// `Command::JoinTeam` — join a public team by id.
pub async fn join_team(ses: &Session, tx: &Sender<Event>, team_id: &str) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::join_team_data(c, team_id).await {
        Ok(msg) => {
            let _ = tx.send(Event::Status(msg));
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("join team: {e:#}")));
        }
    }
}

/// `Command::CreateTeam` — create a team (polls the async operation).
pub async fn create_team(ses: &Session, tx: &Sender<Event>, name: &str) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::create_team_data(c, name, None).await {
        Ok(_) => {
            let _ = tx.send(Event::Status(format!("team {name} created ✓")));
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("create team: {e:#}")));
        }
    }
}

/// `Command::RenameChannel` — rename a channel (permission-gated server-side).
pub async fn rename_channel(
    ses: &Session,
    tx: &Sender<Event>,
    team_id: &str,
    channel_id: &str,
    name: &str,
) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::update_channel_data(c, team_id, channel_id, Some(name), None).await {
        Ok(()) => {
            let _ = tx.send(Event::Status(format!("channel renamed to #{name} ✓")));
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("rename channel: {e:#}")));
        }
    }
}

/// `Command::DeleteChannel` — delete a channel.
pub async fn delete_channel(
    ses: &Session,
    tx: &Sender<Event>,
    team_id: &str,
    channel_id: &str,
) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::delete_channel_data(c, team_id, channel_id).await {
        Ok(()) => {
            let _ = tx.send(Event::Status("channel deleted".into()));
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("delete channel: {e:#}")));
        }
    }
}

/// `Command::ChatFiles` — files recently shared in one conversation.
pub async fn chat_files(ses: &Session, tx: &Sender<Event>, chat_id: &str) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::list_chat_files_data(c, chat_id, 30).await {
        Ok(files) => {
            let _ = tx.send(Event::ChatFiles {
                chat_id: chat_id.to_string(),
                files,
            });
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("chat files: {e:#}")));
            let _ = tx.send(Event::ChatFiles {
                chat_id: chat_id.to_string(),
                files: Vec::new(),
            });
        }
    }
}

// ----------------------------------------------------------------- notes

/// `Command::LoadNotes` — personal OneNote notebooks with sections+pages.
pub async fn load_notes(ses: &Session, tx: &Sender<Event>) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::list_notebooks_data(c, None).await {
        Ok(books) => {
            let mut tree = Vec::new();
            for b in &books {
                let sections = ost::api::list_notebook_sections_data(c, &b.id, None)
                    .await
                    .unwrap_or_default();
                tree.push(crate::ui::sections::NotebookTree {
                    name: b.name.clone(),
                    sections,
                });
            }
            let _ = tx.send(Event::Notes(tree));
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("notebooks: {e:#}")));
            let _ = tx.send(Event::Notes(Vec::new()));
        }
    }
}

/// `Command::ReadNotePage` — one page's HTML.
pub async fn read_note_page(ses: &Session, tx: &Sender<Event>, page_id: &str) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::read_note_page_data(c, page_id, None).await {
        Ok(page) => {
            let _ = tx.send(Event::NotePage(page));
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("note page: {e:#}")));
        }
    }
}

/// `Command::AppendNote` — append a paragraph to a page.
pub async fn append_note(ses: &Session, tx: &Sender<Event>, page_id: &str, text: &str) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::append_note_paragraph_data(c, page_id, text, None).await {
        Ok(()) => {
            let _ = tx.send(Event::Status("note updated ✓".into()));
            read_note_page(ses, tx, page_id).await;
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("note append: {e:#}")));
        }
    }
}

// --------------------------------------------------------------- meet now

/// `Command::MeetNow` — create an online meeting starting now (+30 min)
/// and hand back its join URL.
pub async fn meet_now(ses: &Session, tx: &Sender<Event>) {
    let Some(c) = client(ses) else {
        return;
    };
    let now = jiff::Zoned::now();
    let end = now.clone() + jiff::Span::new().minutes(30);
    let fmt = |z: &jiff::Zoned| z.strftime("%Y-%m-%dT%H:%M:%S").to_string();
    match ost::api::schedule_meeting_data(
        c,
        "TeamsFast meeting",
        &fmt(&now),
        &fmt(&end),
        "UTC",
        true,
    )
    .await
    {
        Ok(meeting) => {
            if let Some(url) = meeting.join_url {
                let _ = tx.send(Event::MeetNowReady { join_url: url });
            } else {
                let _ = tx.send(Event::CallFailed(
                    "meeting created without a join link".into(),
                ));
            }
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("meet now: {e:#}")));
        }
    }
}

// ------------------------------------------------------------- planner

/// `Command::LoadPlanner` — every team's plans with buckets + tasks.
/// Bounded: first 4 plans overall keep the load light.
pub async fn load_planner(ses: &Session, tx: &Sender<Event>) {
    let Some(c) = client(ses) else {
        return;
    };
    let teams = match ost::api::list_teams_data(c).await {
        Ok(t) => t,
        Err(e) => {
            let _ = tx.send(Event::Error(format!("planner teams: {e:#}")));
            let _ = tx.send(Event::Planner(Vec::new()));
            return;
        }
    };
    let mut boards = Vec::new();
    'teams: for team in &teams {
        let plans = match ost::api::list_plans_data(c, &team.id).await {
            Ok(p) => p,
            Err(_) => continue,
        };
        for plan in plans {
            if boards.len() >= 4 {
                break 'teams;
            }
            let buckets = ost::api::list_buckets_data(c, &plan.id)
                .await
                .unwrap_or_default();
            let tasks = ost::api::list_tasks_data(c, &plan.id, 40)
                .await
                .unwrap_or_default();
            if tasks.is_empty() {
                continue;
            }
            boards.push(crate::ui::sections::PlannerBoard {
                team: team.name.clone(),
                plan: plan.title,
                buckets: buckets.into_iter().map(|b| (b.id, b.name)).collect(),
                tasks,
            });
        }
    }
    let _ = tx.send(Event::Planner(boards));
}

/// `Command::SetPlannerDone` — tick / untick a task.
pub async fn planner_set_done(
    ses: &Session,
    tx: &Sender<Event>,
    task_id: &str,
    etag: &str,
    done: bool,
) {
    let Some(c) = client(ses) else {
        return;
    };
    match ost::api::set_task_complete_with_client(c, task_id, etag, done).await {
        Ok(_) => {
            let _ = tx.send(Event::Status("task updated ✓".into()));
            load_planner(ses, tx).await;
        }
        Err(e) => {
            let _ = tx.send(Event::Error(format!("planner task: {e:#}")));
        }
    }
}

// -------------------------------------------------------------- shifts

/// `Command::LoadShifts` — the first schedule-enabled team's week.
pub async fn load_shifts(ses: &Session, tx: &Sender<Event>) {
    let Some(c) = client(ses) else {
        return;
    };
    let teams = match ost::api::list_teams_data(c).await {
        Ok(t) => t,
        Err(e) => {
            let _ = tx.send(Event::Error(format!("shifts teams: {e:#}")));
            let _ = tx.send(Event::Shifts(Vec::new()));
            return;
        }
    };
    for team in &teams {
        let Ok(schedule) = ost::api::list_schedule_data(c, &team.id).await else {
            continue;
        };
        if !schedule.enabled {
            continue;
        }
        // This week, local time: Monday 00:00 → +7 days.
        let now = jiff::Zoned::now();
        let days_from_monday = now.date().weekday().to_monday_zero_offset() as i64;
        let midnight = now
            .with()
            .hour(0)
            .minute(0)
            .second(0)
            .subsec_nanosecond(0)
            .build()
            .unwrap_or_else(|_| now.clone());
        let monday = midnight - jiff::Span::new().days(days_from_monday);
        let sunday = monday.clone() + jiff::Span::new().days(7);
        let fmt = |z: &jiff::Zoned| z.timestamp().to_string();
        match ost::api::list_shifts_range_data(c, &team.id, &fmt(&monday), &fmt(&sunday)).await
        {
            Ok(mut shifts) => {
                shifts.sort_by(|a, b| a.start.cmp(&b.start));
                let _ = tx.send(Event::Shifts(shifts));
                return;
            }
            Err(e) => {
                let _ = tx.send(Event::Error(format!("shifts: {e:#}")));
                let _ = tx.send(Event::Shifts(Vec::new()));
                return;
            }
        }
    }
    let _ = tx.send(Event::Shifts(Vec::new()));
}

fn dirs_downloads() -> std::path::PathBuf {
    std::env::var_os("XDG_DOWNLOAD_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Downloads"))
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}
