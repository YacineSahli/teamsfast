//! Headless debug entry points (never open a window).

use eframe::Result;
use std::future::Future;
use std::pin::Pin;

pub fn dispatch(args: &[String]) -> Option<Result<()>> {
    match args.get(1).map(|s| s.as_str()) {
        Some("--dump-chats") => Some(dump_chats()),
        Some("--probe-chat") => args.get(2).map(|id| probe_chat(id)),
        // AGENT 1: "--search <query>" and "--teams" headless verifiers go here.
        Some("--search") => args
            .get(2)
            .map(|q| search_probe(q)),
        Some("--teams") => Some(teams_probe()),
        Some("--probe-planner") => Some(planner_probe()),
        Some("--probe-shifts") => Some(shifts_probe()),
        _ => None,
    }
}

fn with_client(
    f: impl FnOnce(&ost::api::client::TeamsClient) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + '_>>,
) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async {
        let client = ost::api::client::TeamsClient::new()
            .await
            .expect("not signed in (run the GUI once and sign in)");
        if let Err(e) = f(&client).await {
            eprintln!("error: {e:#}");
            std::process::exit(1);
        }
    });
    Ok(())
}

fn dump_chats() -> Result<()> {
    with_client(|client| {
        Box::pin(async move {
            let chats = crate::backend::list_chats_headless(client, 60).await?;
            for c in &chats {
                println!(
                    "{} | group={:5} | read_ms={:?} | {}",
                    c.id,
                    c.is_group,
                    c.last_read_ms,
                    if c.name.is_empty() { "(no name)" } else { &c.name }
                );
            }
            Ok(())
        })
    })
}

fn probe_chat(chat_id: &str) -> Result<()> {
    let chat_id = chat_id.to_string();
    with_client(|client| {
        Box::pin(async move {
            let (_, members) = ost::api::list_chat_members_data(client, &chat_id).await?;
            for m in &members {
                println!(
                    "{} | uid={:?} | {:25} | {:?}",
                    m.mri,
                    m.user_id,
                    if m.display_name.is_empty() { "(no name)" } else { &m.display_name },
                    m.email
                );
            }
            Ok(())
        })
    })
}

/// Print search hits for `query` (id | sender | preview).
fn search_probe(query: &str) -> Result<()> {
    let query = query.to_string();
    with_client(|client| {
        Box::pin(async move {
            let page = ost::api::search_messages_data(client, &query, 0, 25).await?;
            for h in &page.hits {
                let target = if h.chat_id.is_empty() {
                    h.channel_id.clone().unwrap_or_default()
                } else {
                    h.chat_id.clone()
                };
                println!("{} | {} | {}", target, h.sender, h.preview);
            }
            println!("(more: {})", page.more);
            Ok(())
        })
    })
}

/// Print teams and their channels.
fn teams_probe() -> Result<()> {
    with_client(|client| {
        Box::pin(async move {
            let teams = ost::api::list_teams_data(client).await?;
            for t in &teams {
                println!("TEAM {} | {}", t.id, t.name);
                for ch in &t.channels {
                    println!("  CH {} | {}", ch.id, ch.name);
                }
            }
            Ok(())
        })
    })
}

/// Print each team's planner plans (with a couple of tasks).
fn planner_probe() -> Result<()> {
    with_client(|client| {
        Box::pin(async move {
            let teams = ost::api::list_teams_data(client).await?;
            for t in &teams {
                match ost::api::list_plans_data(client, &t.id).await {
                    Ok(plans) => {
                        for p in &plans {
                            println!("PLAN {} | {} | team {}", p.id, p.title, t.name);
                            match ost::api::list_tasks_data(client, &p.id, 3).await {
                                Ok(tasks) => {
                                    for task in &tasks {
                                        println!(
                                            "  TASK {} | done={} | {}",
                                            task.id, task.completed, task.title
                                        );
                                    }
                                }
                                Err(e) => println!("  TASKS err: {e:#}"),
                            }
                        }
                    }
                    Err(e) => println!("PLAN err for {}: {e:#}", t.name),
                }
            }
            Ok(())
        })
    })
}

/// Print the first schedule-enabled team's shifts for this week.
fn shifts_probe() -> Result<()> {
    with_client(|client| {
        Box::pin(async move {
            let teams = ost::api::list_teams_data(client).await?;
            for t in &teams {
                let sched = match ost::api::list_schedule_data(client, &t.id).await {
                    Ok(s) => s,
                    Err(e) => {
                        println!("SCHED err for {}: {e:#}", t.name);
                        continue;
                    }
                };
                println!("SCHED {} | enabled={}", t.name, sched.enabled);
            }
            Ok(())
        })
    })
}
