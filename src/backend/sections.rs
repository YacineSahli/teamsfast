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

fn dirs_downloads() -> std::path::PathBuf {
    std::env::var_os("XDG_DOWNLOAD_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Downloads"))
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}
